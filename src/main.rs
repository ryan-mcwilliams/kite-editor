//! kite — a terminal IDE that behaves like a desktop one.
//!
//! Layout of the code:
//!   main.rs       terminal setup and the event loop
//!   app.rs        state + behaviour (keys, mouse, commands, LSP responses)
//!   ui.rs         drawing
//!   buffer.rs     text, cursor, selection, undo
//!   commands.rs   the command list and key bindings
//!   palette.rs    file finder / command palette / prompts
//!   tree.rs       sidebar file tree
//!   highlight.rs  languages and tree-sitter highlighting
//!   lsp.rs        language server client
//!   config.rs     ~/.config/kite/config.toml
//!   theme.rs      colours

mod app;
mod buffer;
mod commands;
mod config;
mod fileops;
mod highlight;
mod lsp;
mod menu;
mod palette;
mod search;
mod theme;
mod tree;
mod ui;

use app::{App, AppEvent};
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::{execute, terminal::supports_keyboard_enhancement};
use std::io::{self, stdout, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long the cursor stays shown, then hidden, while blinking.
const BLINK: Duration = Duration::from_millis(530);

fn restore_terminal(enhanced: bool) {
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    // OSC 112 resets the cursor colour we set with OSC 12.
    let _ = stdout().write_all(b"\x1b]112\x07");
    let _ = stdout().flush();
    let _ = execute!(
        stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        SetCursorStyle::DefaultUserShape
    );
    ratatui::restore();
}

/// Keys, pastes and clicks restart the blink; mouse movement and language
/// server messages don't.
fn restarts_blink(ev: &AppEvent) -> bool {
    use event::{Event, MouseEventKind};
    match ev {
        AppEvent::Input(Event::Mouse(m)) => m.kind != MouseEventKind::Moved,
        AppEvent::Input(Event::Key(_) | Event::Paste(_)) => true,
        _ => false,
    }
}

fn main() -> io::Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("-h" | "--help") => {
            println!(
                "kite {}\n\nUsage: kite [DIRECTORY | FILE]\n\nOpens the directory (default: current) as a project.\nPress F1 inside kite for the command palette.",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("kite {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    // `kite some/dir` opens the directory; `kite some/file.rs` opens the
    // file with its parent directory as the project.
    let target = PathBuf::from(arg.unwrap_or_else(|| ".".into()));
    let (root, file) = if target.is_dir() {
        (target, None)
    } else {
        let parent = target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into());
        let parent = std::fs::canonicalize(&parent).unwrap_or(parent);
        // Prefer the enclosing project (nearest ancestor with a marker file).
        const MARKERS: &[&str] = &[
            ".git",
            "Cargo.toml",
            "package.json",
            "go.mod",
            "pyproject.toml",
        ];
        let project = parent
            .ancestors()
            .find(|d| MARKERS.iter().any(|m| d.join(m).exists()))
            .map(PathBuf::from);
        let parent = project.unwrap_or(parent);
        (parent, Some(target))
    };
    let root = std::fs::canonicalize(&root)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", root.display())))?;

    let (tx, rx) = mpsc::channel();
    let mut app = App::new(root, tx.clone());
    if let Some(file) = file {
        if !file.exists() {
            std::fs::write(&file, "")?;
            app.tree.refresh();
        }
        app.open_path(&file);
    }

    let mut terminal = ratatui::init();
    // The kitty keyboard protocol (Ghostty, kitty, WezTerm, iTerm2, …) lets
    // us tell Ctrl+Shift+P from Ctrl+P. Older terminals just skip this.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    execute!(stdout(), EnableBracketedPaste)?;
    app.set_mouse_capture(app.cfg.mouse_support);
    app.set_cursor_style();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal(enhanced);
        hook(info);
    }));

    // Terminal input and language servers each have their own thread; all
    // of them feed this one channel, so the app itself is single-threaded.
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if tx.send(AppEvent::Input(ev)).is_err() {
                break;
            }
        }
    });

    let mut cursor_colour = None;
    let mut blink_at = Instant::now() + BLINK;
    let result = loop {
        app.after_events();
        // The terminal draws its own cursor in its own colour, which can be
        // invisible on a black code panel. Match it to the theme's text colour.
        if let ratatui::style::Color::Rgb(r, g, b) = app.theme.fg
            && cursor_colour != Some((r, g, b))
        {
            cursor_colour = Some((r, g, b));
            let _ = write!(stdout(), "\x1b]12;#{r:02x}{g:02x}{b:02x}\x07");
            let _ = stdout().flush();
        }
        if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
            break Err(e);
        }
        // Wait for input, or until the cursor blinks or the app has timed
        // work to do (see `App::tick`), whichever comes first.
        let blink = app.cfg.cursor_blink.then_some(blink_at);
        let ev = match blink.into_iter().chain(app.next_deadline()).min() {
            Some(deadline) => {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(ev) => ev,
                    Err(RecvTimeoutError::Timeout) => {
                        let now = Instant::now();
                        if blink.is_some_and(|b| now >= b) {
                            app.blink_off = !app.blink_off;
                            blink_at = now + BLINK;
                        }
                        app.tick(now);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break Ok(()),
                }
            }
            None => {
                let Ok(ev) = rx.recv() else { break Ok(()) };
                ev
            }
        };
        let mut typed = restarts_blink(&ev);
        app.handle(ev);
        while let Ok(ev) = rx.try_recv() {
            typed |= restarts_blink(&ev);
            app.handle(ev);
        }
        // Like desktop editors, keep the cursor solid while you type or click.
        if typed {
            app.blink_off = false;
            blink_at = Instant::now() + BLINK;
        }
        if app.quit {
            break Ok(());
        }
    };

    app.shutdown();
    restore_terminal(enhanced);
    result
}
