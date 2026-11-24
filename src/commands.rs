//! Every user-facing action is a `Command`. Commands show up in the command
//! palette automatically and can be bound to keys in the config file.
//!
//! To add one: add a line to the `commands!` table, handle it in
//! `App::run`, and (optionally) give it a default key below.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::collections::{BTreeMap, HashMap};

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
    Leader, "leader", "Leader Key…";
    ToggleLineNumbers, "toggle_line_numbers", "View: Toggle Line Numbers";
    ToggleCursorBlink, "toggle_cursor_blink", "View: Toggle Cursor Blink";
    ToggleMouse, "toggle_mouse", "View: Toggle Mouse Support";
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
    ("ctrl-t", Command::Leader),
    ("f12", Command::GotoDefinition),
    ("ctrl-o", Command::GoBack),
    ("alt-shift-f", Command::Format),
];

/// Names of the leader key's groups, by the keys that open them.
const LEADER_GROUPS: &[(&str, &str)] = &[
    ("g", "Go"),
    ("f", "File"),
    ("s", "Search"),
    ("c", "Code"),
    ("t", "Toggle"),
    ("b", "Tabs"),
];

/// What the keys after the leader do: `"g l"` is leader, then g, then l.
const DEFAULT_LEADER: &[(&str, Command)] = &[
    ("l", Command::GotoLine),
    ("p", Command::CommandPalette),
    ("?", Command::Shortcuts),
    ("g l", Command::GotoLine),
    ("g d", Command::GotoDefinition),
    ("g b", Command::GoBack),
    ("g f", Command::FileFinder),
    ("f s", Command::Save),
    ("f n", Command::NewFile),
    ("f w", Command::CloseTab),
    ("f c", Command::OpenConfig),
    ("s f", Command::Find),
    ("s p", Command::SearchInFiles),
    ("s n", Command::FindNext),
    ("s N", Command::FindPrev),
    ("c f", Command::Format),
    ("c h", Command::Hover),
    ("c a", Command::Complete),
    ("c r", Command::RestartServer),
    ("t s", Command::ToggleSidebar),
    ("t n", Command::ToggleLineNumbers),
    ("t b", Command::ToggleCursorBlink),
    ("t m", Command::ToggleMouse),
    ("b n", Command::NextTab),
    ("b p", Command::PrevTab),
    ("b w", Command::CloseTab),
];

pub enum LeaderEntry {
    Group(LeaderGroup),
    Run(Command),
}

/// One level of the leader key's menu: what each next key does.
#[derive(Default)]
pub struct LeaderGroup {
    pub name: String,
    pub items: BTreeMap<char, LeaderEntry>,
}

/// `"g l"` → `['g', 'l']`. Each step is one character.
fn parse_leader_seq(s: &str) -> Option<Vec<char>> {
    let seq: Vec<char> = s
        .split_whitespace()
        .map(|k| {
            let mut cs = k.chars();
            match (cs.next(), cs.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            }
        })
        .collect::<Option<_>>()?;
    (!seq.is_empty()).then_some(seq)
}

impl LeaderGroup {
    /// The group reached by pressing `path` after the leader.
    pub fn at(&self, path: &[char]) -> Option<&LeaderGroup> {
        match path.split_first() {
            None => Some(self),
            Some((c, rest)) => match self.items.get(c)? {
                LeaderEntry::Group(g) => g.at(rest),
                LeaderEntry::Run(_) => None,
            },
        }
    }

    fn insert(&mut self, seq: &[char], cmd: Command, prefix: &str) -> Result<(), String> {
        let (&c, rest) = seq.split_first().expect("non-empty sequence");
        if rest.is_empty() {
            if matches!(self.items.get(&c), Some(LeaderEntry::Group(_))) {
                return Err(format!("leader \"{prefix}{c}\" is a group of keys"));
            }
            self.items.insert(c, LeaderEntry::Run(cmd));
            return Ok(());
        }
        let path = format!("{prefix}{c}");
        let entry = self.items.entry(c).or_insert_with(|| {
            let name = LEADER_GROUPS
                .iter()
                .find(|(k, _)| *k == path)
                .map_or_else(|| path.clone(), |(_, n)| n.to_string());
            LeaderEntry::Group(LeaderGroup {
                name,
                items: BTreeMap::new(),
            })
        });
        match entry {
            LeaderEntry::Group(g) => g.insert(rest, cmd, &format!("{path} ")),
            LeaderEntry::Run(_) => Err(format!("leader \"{path}\" already runs a command")),
        }
    }

    fn remove(&mut self, seq: &[char]) {
        match seq {
            [] => {}
            [c] => {
                self.items.remove(c);
            }
            [c, rest @ ..] => {
                if let Some(LeaderEntry::Group(g)) = self.items.get_mut(c) {
                    g.remove(rest);
                    if g.items.is_empty() {
                        self.items.remove(c);
                    }
                }
            }
        }
    }

    /// Every sequence that runs a command, as ("g l", command), in order.
    pub fn sequences(&self) -> Vec<(String, Command)> {
        let mut out = vec![];
        for (c, entry) in &self.items {
            match entry {
                LeaderEntry::Run(cmd) => out.push((c.to_string(), *cmd)),
                LeaderEntry::Group(g) => {
                    for (rest, cmd) in g.sequences() {
                        out.push((format!("{c} {rest}"), cmd));
                    }
                }
            }
        }
        out
    }
}

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
    /// What follows the leader key.
    pub leader: LeaderGroup,
}

impl Keymap {
    /// Defaults, overridden by `[keys]` in the config file
    /// (`"ctrl-s" = "save"`, or `"ctrl-s" = "none"` to unbind), and the
    /// leader's keys, overridden by `[leader_keys]` (`"g l" = "goto_line"`).
    pub fn new(
        overrides: &BTreeMap<String, String>,
        leader_overrides: &BTreeMap<String, String>,
    ) -> (Keymap, Vec<String>) {
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
        let mut leader = LeaderGroup::default();
        for (seq, cmd) in DEFAULT_LEADER {
            let seq = parse_leader_seq(seq).expect("default leader sequence parses");
            leader
                .insert(&seq, *cmd, "")
                .expect("default leader keys don't clash");
        }
        for (seq, id) in leader_overrides {
            let Some(keys) = parse_leader_seq(seq) else {
                errors.push(format!(
                    "leader keys \"{seq}\": use single characters, like \"g l\""
                ));
                continue;
            };
            if id == "none" {
                leader.remove(&keys);
                continue;
            }
            match Command::from_id(id) {
                Some(cmd) => {
                    if let Err(e) = leader.insert(&keys, cmd, "") {
                        errors.push(e);
                    }
                }
                None => errors.push(format!("unknown command \"{id}\"")),
            }
        }
        (Keymap { map, leader }, errors)
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

    /// The shortest binding for a command, for display in the palette; or,
    /// if it has no key of its own, its leader sequence ("Ctrl+T s p").
    pub fn binding(&self, cmd: Command) -> Option<String> {
        self.direct_binding(cmd).or_else(|| {
            let (seq, _) = self
                .leader
                .sequences()
                .into_iter()
                .filter(|(_, c)| *c == cmd)
                .min_by_key(|(s, _)| s.len())?;
            Some(format!("{} {seq}", self.direct_binding(Command::Leader)?))
        })
    }

    fn direct_binding(&self, cmd: Command) -> Option<String> {
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
        let (km, errors) = Keymap::new(&o, &BTreeMap::new());
        assert_eq!(km.get(parse_key("ctrl-s").unwrap()), None);
        assert_eq!(
            km.get(parse_key("ctrl-t").unwrap()),
            Some(Command::FileFinder)
        );
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn leader_defaults_overrides_and_fallback_binding() {
        let (km, errors) = Keymap::new(&BTreeMap::new(), &BTreeMap::new());
        assert!(errors.is_empty());
        assert!(matches!(
            km.leader.at(&['g']).unwrap().items.get(&'l'),
            Some(LeaderEntry::Run(Command::GotoLine))
        ));
        assert_eq!(km.leader.at(&['g']).unwrap().name, "Go");
        // No key of its own, so the palette shows its leader sequence.
        assert_eq!(
            km.binding(Command::ToggleLineNumbers).as_deref(),
            Some("Ctrl+T t n")
        );

        let mut o = BTreeMap::new();
        o.insert("x".to_string(), "toggle_sidebar".to_string());
        o.insert("f c".to_string(), "none".to_string());
        o.insert("g".to_string(), "save".to_string()); // clashes with the Go group
        o.insert("gg".to_string(), "save".to_string()); // not single characters
        let (km, errors) = Keymap::new(&BTreeMap::new(), &o);
        assert!(matches!(
            km.leader.items.get(&'x'),
            Some(LeaderEntry::Run(Command::ToggleSidebar))
        ));
        assert!(km.leader.at(&['f']).unwrap().items.get(&'c').is_none());
        assert_eq!(errors.len(), 2, "{errors:?}");
    }
}
