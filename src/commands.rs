//! Every user-facing action is a `Command`. Commands show up in the command
//! palette automatically and can be bound to keys in the config file.
//!
//! To add one: add a line to the `commands!` table, handle it in
//! `App::run`, and (optionally) give it a default key below.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;

macro_rules! commands {
    ($($variant:ident, $id:literal, $title:literal;)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub enum Command { $($variant),* }
        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$variant),*];
            pub fn id(self) -> &'static str { match self { $(Command::$variant => $id),* } }
            pub fn title(self) -> &'static str { match self { $(Command::$variant => $title),* } }
            pub fn from_id(id: &str) -> Option<Command> { Self::ALL.iter().copied().find(|c| c.id() == id) }
        }
    };
}

commands! {
    Save, "save", "File: Save";
    NewFile, "new_file", "File: New File…";
    CloseTab, "close_tab", "File: Close Tab";
    Quit, "quit", "Quit";
    FileFinder, "file_finder", "Go to File…";
    CommandPalette, "command_palette", "Command Palette";
    SearchInFiles, "search_in_files", "Search in Files…";
    GotoLine, "goto_line", "Go to Line…";
    NextTab, "next_tab", "View: Next Tab";
    PrevTab, "prev_tab", "View: Previous Tab";
    ToggleSidebar, "toggle_sidebar", "View: Toggle Sidebar";
    FocusSidebar, "focus_sidebar", "View: Focus File Tree / Editor";
    RefreshTree, "refresh_tree", "View: Refresh File Tree";
    Settings, "settings", "Settings…";
    Find, "find", "Find in File";
    FindNext, "find_next", "Find Next";
    FindPrev, "find_prev", "Find Previous";
    Undo, "undo", "Edit: Undo";
    Redo, "redo", "Edit: Redo";
    Copy, "copy", "Edit: Copy";
    Cut, "cut", "Edit: Cut";
    Paste, "paste", "Edit: Paste";
    SelectAll, "select_all", "Edit: Select All";
    ToggleComment, "toggle_comment", "Edit: Toggle Line Comment";
    Complete, "complete", "Code: Trigger Completion";
    Hover, "hover", "Code: Show Hover Info";
    GotoDefinition, "goto_definition", "Code: Go to Definition";
    GoBack, "go_back", "Code: Go Back";
    Format, "format", "Code: Format Document";
    RestartServer, "restart_server", "Code: Restart Language Server";
    OpenConfig, "open_config", "Settings: Open Config File";
    Shortcuts, "shortcuts", "Help: Keyboard Shortcuts";
}

pub type Key = (KeyCode, KeyModifiers);

const DEFAULT_KEYS: &[(&str, Command)] = &[
    ("ctrl-s", Command::Save),
    ("ctrl-n", Command::NewFile),
    ("ctrl-w", Command::CloseTab),
    ("ctrl-q", Command::Quit),
    ("ctrl-p", Command::CommandPalette),
    ("ctrl-shift-p", Command::CommandPalette),
    ("f1", Command::CommandPalette),
    ("ctrl-f", Command::FileFinder),
    ("ctrl-shift-f", Command::SearchInFiles),
    ("ctrl-,", Command::Settings),
    ("ctrl-g", Command::GotoLine),
    ("ctrl-pagedown", Command::NextTab),
    ("alt-]", Command::NextTab),
    ("ctrl-pageup", Command::PrevTab),
    ("alt-[", Command::PrevTab),
    ("ctrl-b", Command::ToggleSidebar),
    ("ctrl-e", Command::FocusSidebar),
    ("alt-f", Command::Find),
    ("f3", Command::FindNext),
    ("shift-f3", Command::FindPrev),
    ("ctrl-z", Command::Undo),
    ("ctrl-y", Command::Redo),
    ("ctrl-shift-z", Command::Redo),
    ("ctrl-c", Command::Copy),
    ("ctrl-x", Command::Cut),
    ("ctrl-v", Command::Paste),
    ("ctrl-a", Command::SelectAll),
    // Terminals report Ctrl+/ in different ways; bind them all.
    ("ctrl-/", Command::ToggleComment),
    ("ctrl-7", Command::ToggleComment),
    ("ctrl-_", Command::ToggleComment),
    ("ctrl-space", Command::Complete),
    ("alt-k", Command::Hover),
    ("ctrl-k", Command::Shortcuts),
    ("f12", Command::GotoDefinition),
    ("ctrl-o", Command::GoBack),
    ("alt-shift-f", Command::Format),
];

/// Bring a terminal key event into one canonical shape so `ctrl-shift-p`
/// matches however the terminal chose to report it.
pub fn normalize(code: KeyCode, mods: KeyModifiers) -> Key {
    let mut mods = mods
        & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::SUPER);
    let code = match code {
        KeyCode::Char(c) if c.is_uppercase() => {
            mods |= KeyModifiers::SHIFT;
            KeyCode::Char(c.to_lowercase().next().unwrap_or(c))
        }
        KeyCode::BackTab => {
            mods |= KeyModifiers::SHIFT;
            KeyCode::Tab
        }
        c => c,
    };
    (code, mods)
}

pub fn parse_key(s: &str) -> Option<Key> {
    let s = s.trim().to_lowercase();
    let (mod_part, key) = if s.len() > 1 && s.ends_with('-') {
        (&s[..s.len() - 1], "-")
    } else {
        match s.rfind('-') {
            Some(i) => (&s[..i + 1], &s[i + 1..]),
            None => ("", s.as_str()),
        }
    };
    let mut mods = KeyModifiers::NONE;
    for m in mod_part.split('-').filter(|m| !m.is_empty()) {
        mods |= match m {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "option" | "opt" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            "cmd" | "super" => KeyModifiers::SUPER,
            _ => return None,
        };
    }
    let code = match key {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        k if k.len() > 1 && k.starts_with('f') => KeyCode::F(k[1..].parse().ok()?),
        k if k.chars().count() == 1 => KeyCode::Char(k.chars().next()?),
        _ => return None,
    };
    Some((code, mods))
}

pub fn format_key((code, mods): Key) -> String {
    let mut out = String::new();
    for (m, name) in [
        (KeyModifiers::CONTROL, "Ctrl+"),
        (KeyModifiers::ALT, "Alt+"),
        (KeyModifiers::SHIFT, "Shift+"),
        (KeyModifiers::SUPER, "Cmd+"),
    ] {
        if mods.contains(m) {
            out.push_str(name);
        }
    }
    match code {
        KeyCode::Char(' ') => out.push_str("Space"),
        KeyCode::Char(c) => out.extend(c.to_uppercase()),
        KeyCode::F(n) => out.push_str(&format!("F{n}")),
        other => out.push_str(&format!("{other:?}")),
    }
    out
}

pub struct Keymap {
    map: HashMap<Key, Command>,
}

impl Keymap {
    /// Defaults, overridden by `[keys]` in the config file
    /// (`"ctrl-s" = "save"`, or `"ctrl-s" = "none"` to unbind).
    pub fn new(overrides: &std::collections::BTreeMap<String, String>) -> (Keymap, Vec<String>) {
        let mut map = HashMap::new();
        let mut errors = vec![];
        for (k, c) in DEFAULT_KEYS {
            map.insert(parse_key(k).expect("default key parses"), *c);
        }
        for (k, id) in overrides {
            match (parse_key(k), Command::from_id(id)) {
                (Some(key), Some(cmd)) => {
                    map.insert(key, cmd);
                }
                (Some(key), None) if id == "none" => {
                    map.remove(&key);
                }
                (None, _) => errors.push(format!("unknown key \"{k}\"")),
                (_, None) => errors.push(format!("unknown command \"{id}\"")),
            }
        }
        (Keymap { map }, errors)
    }

    pub fn get(&self, key: Key) -> Option<Command> {
        self.map.get(&key).copied()
    }

    /// Every binding for a command, shortest first, for the shortcuts panel.
    /// The terminal-specific spellings of Ctrl+/ are left out.
    pub fn bindings(&self, cmd: Command) -> Vec<String> {
        let aliases = [parse_key("ctrl-7"), parse_key("ctrl-_")];
        let mut keys: Vec<String> = self
            .map
            .iter()
            .filter(|(k, c)| **c == cmd && !aliases.contains(&Some(**k)))
            .map(|(k, _)| format_key(*k))
            .collect();
        keys.sort_by_key(|s| (s.len(), s.clone()));
        keys
    }

    /// The shortest binding for a command, for display in the palette.
    pub fn binding(&self, cmd: Command) -> Option<String> {
        self.map
            .iter()
            .filter(|(_, c)| **c == cmd)
            .map(|(k, _)| format_key(*k))
            .min_by_key(|s| (s.len(), s.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_keys() {
        assert_eq!(
            parse_key("ctrl-shift-p"),
            Some((
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ))
        );
        assert_eq!(
            parse_key("alt--"),
            Some((KeyCode::Char('-'), KeyModifiers::ALT))
        );
        assert_eq!(parse_key("f12"), Some((KeyCode::F(12), KeyModifiers::NONE)));
        assert_eq!(parse_key("hyper-x"), None);
        assert_eq!(
            format_key(parse_key("ctrl-shift-p").unwrap()),
            "Ctrl+Shift+P"
        );
    }

    #[test]
    fn normalizes_uppercase_and_backtab() {
        assert_eq!(
            normalize(KeyCode::Char('P'), KeyModifiers::CONTROL),
            parse_key("ctrl-shift-p").unwrap()
        );
        assert_eq!(
            normalize(KeyCode::BackTab, KeyModifiers::NONE),
            parse_key("shift-tab").unwrap()
        );
    }

    #[test]
    fn config_overrides_and_unbinds() {
        let mut o = std::collections::BTreeMap::new();
        o.insert("ctrl-s".to_string(), "none".to_string());
        o.insert("ctrl-t".to_string(), "file_finder".to_string());
        o.insert("ctrl-u".to_string(), "nope".to_string());
        let (km, errors) = Keymap::new(&o);
        assert_eq!(km.get(parse_key("ctrl-s").unwrap()), None);
        assert_eq!(
            km.get(parse_key("ctrl-t").unwrap()),
            Some(Command::FileFinder)
        );
        assert_eq!(errors.len(), 1);
    }
}
