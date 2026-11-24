# kite-editor

A terminal IDE that behaves like a desktop one: open a folder, file tree on
the left, tabs, mouse everywhere, a command palette, and language servers for
diagnostics, completion, hover, go-to-definition and formatting. No modes.

Written from scratch in Rust (ratatui + tree-sitter), about 5,000 lines, so
the whole thing fits in your head and is yours to change.

## Run it

You need Rust (https://rustup.rs) and a C compiler (on macOS:
`xcode-select --install`), which the tree-sitter grammars use.

```sh
cargo run --release -- ~/some/project     # try it
cargo install --path .                    # put `kite` on your PATH
kite .                                    # open the current directory
kite src/main.rs                          # open a file (and its project)
```

The first build takes a few minutes; later ones are quick.

## Keys

Terminals don't pass Cmd through to programs, so the defaults use Ctrl.
Everything below can be rebound (see Configuration).

| Key | Action |
| --- | --- |
| Ctrl+F | Go to file (fuzzy). Type `>` for commands, `:42` for a line |
| Ctrl+P, F1, or Ctrl+Shift+P | Command palette (lists every command and its key) |
| Ctrl+B / Ctrl+E | Toggle sidebar / move focus between tree and editor |
| Ctrl+S, Ctrl+N, Ctrl+W, Ctrl+Q | Save, new file, close tab, quit |
| Alt+] / Alt+[ (or Ctrl+PageDown/Up) | Next / previous tab |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+A | Copy, cut, paste, select all (no selection = whole line) |
| Alt+F, F3, Shift+F3 | Find in file, next, previous |
| Ctrl+Shift+F | Search in files (results panel; needs the kitty keyboard protocol — use F1 → "Search in Files…" otherwise) |
| Ctrl+G | Go to line |
| Ctrl+/ | Toggle line comment |
| `>` ending an opening tag | Adds the closing tag (HTML, JSX/TSX); Enter between the tags splits them onto their own lines |
| Tab / Shift+Tab | Indent / outdent selected lines |
| Shift+arrows, Ctrl or Alt+arrows | Select; move by word |
| Ctrl+Space | Completions (they also appear as you type) |
| Ctrl+K | Keyboard shortcuts panel (Esc or Ctrl+K closes it) |
| Alt+K | Hover info |
| F12 or Alt+Click | Go to definition; Ctrl+O goes back. Holding Alt/Option underlines what the pointer is on, and the hover popup has a "Go to Definition" link |
| Alt+Shift+F | Format document |
| Right-click | Context menu: on a file or folder in the tree (open, new file/folder, rename, duplicate, delete with confirmation, copy path, reveal in Finder), on empty tree space, or on code (go to definition, hover, format, cut/copy/paste, comment, copy path). Navigate with the mouse or ↑/↓ + Enter, Esc closes |
| Ctrl+, (or F1 → "Settings…") | Settings panel — toggle/cycle options by category, saved to config.toml |
| ↳ Appearance → Theme | Cycle One Dark (black editor background, default), Monokai, Catppuccin Mocha, Dracula |

Mouse: click to place the cursor, drag to select, double-click a word,
triple-click a line, wheel to scroll, click tabs and their ×, click the tree,
drag the sidebar edge to resize it, rest the pointer on a symbol to
see its type and docs (needs its language server).

## Language servers

kite starts a server the first time you open a file of that language, if the
server is installed. Defaults:

| Language | Server | Install |
| --- | --- | --- |
| Rust | rust-analyzer | `rustup component add rust-analyzer` |
| Python | pyright-langserver | `npm i -g pyright` |
| TypeScript / JavaScript | typescript-language-server | `npm i -g typescript typescript-language-server` |
| Go | gopls | `go install golang.org/x/tools/gopls@latest` |
| C / C++ | clangd | comes with Xcode tools / LLVM |
| JSON, CSS, HTML | vscode-*-language-server | `npm i -g vscode-langservers-extracted` |
| YAML, TOML, shell | yaml-language-server, taplo, bash-language-server | |

If a server is missing or stops, the status bar shows a red `✗ server`
marker; click it (or run "Code: Restart Language Server") to see why and try
again. Without a running server, completion suggests words from your open
files instead.

Syntax highlighting is built in for Rust, Python, JS/TS/TSX, Go, C, JSON,
TOML, YAML, shell, CSS, HTML and Markdown and needs nothing installed.

## Configuration

Run "Settings: Open Config File" from the palette. It creates and opens
`~/.config/kite/config.toml`:

```toml
tab_width = 4
format_on_save = true

[keys]
"ctrl-t" = "file_finder"     # command ids are in src/commands.rs
"ctrl-q" = "none"            # remove a default

[lsp]
python = ["pylsp"]           # swap a server
markdown = ["marksman", "server"]
```

## Terminal notes (macOS)

- Use a terminal with 24-bit colour and the kitty keyboard protocol for the
  best experience: Ghostty, kitty, WezTerm or iTerm2. Others work, with
  256 colours and F1 instead of Ctrl+Shift+P.
- Option as a modifier: in Terminal.app enable "Use Option as Meta key"; in
  iTerm2 set Option to "Esc+". Needed for Alt+arrows, Alt+] and Alt+Click.
- Because kite handles the mouse, the terminal's own text selection is off.
  Hold Option (iTerm2/Terminal) or Shift (most others) while dragging to use
  it. Cmd+V pastes as usual; Ctrl+C / Ctrl+V use the system clipboard.
- The file tree shows a coloured icon per file/folder type (via
  [`devicons`](https://crates.io/crates/devicons)). This needs a
  [Nerd Font](https://www.nerdfonts.com/) set as the terminal's font — without
  one, icons show as boxes or blanks. Ghostty, kitty, WezTerm and iTerm2 all
  support setting one.

## How the code is organised

| File | What lives there |
| --- | --- |
| `src/main.rs` | Terminal setup and the event loop |
| `src/app.rs` | All state and behaviour: keys, mouse, commands, LSP responses |
| `src/ui.rs` | Drawing; saves rectangles so clicks can be mapped back |
| `src/buffer.rs` | Text (rope), cursor, selection, undo, editing operations |
| `src/commands.rs` | The command table and default key bindings |
| `src/palette.rs` | File finder, command palette, prompts, fuzzy matching |
| `src/tree.rs` | Sidebar file tree |
| `src/highlight.rs` | Language table and tree-sitter highlighting |
| `src/lsp.rs` | Language server client (JSON-RPC over stdio) |
| `src/config.rs`, `src/theme.rs` | Config file and colours |

The app is single-threaded. One thread reads terminal input and one per
language server reads its output; all of them send events down one channel,
and the main loop handles an event, then redraws.

Common changes:

- **Add a command**: one line in the `commands!` table in `commands.rs`, a
  match arm in `App::run` in `app.rs`, optionally a default key. It appears
  in the palette automatically.
- **Add a language**: one entry in `langs()` in `highlight.rs` (and a
  tree-sitter grammar crate if you want highlighting), plus a default server
  in `config.rs`.
- **Change colours**: `Theme::one_dark()` in `theme.rs`.

`cargo test` runs the unit tests (buffer editing, key parsing, fuzzy
matching, highlighting queries, LSP framing).

## Not built yet

Roughly in the order I'd do them:

1. Find and replace
2. Rename / delete / move in the file tree; reload files changed on disk
3. Split panes
4. Integrated terminal panel
5. LSP extras: signature help, rename symbol, find references, code actions,
   auto-import on completion, incremental document sync
6. Git gutter and branch in the status bar
7. Multiple cursors
8. Soft wrap
