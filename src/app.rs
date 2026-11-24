//! Application state and behaviour: what happens for each key, click,
//! command and language-server message. Drawing lives in ui.rs.

use crate::buffer::{Buffer, is_word};
use crate::commands::{Command, Keymap, LeaderEntry, normalize};
use crate::config::{self, Config};
use crate::fileops;
use crate::lsp::{self, Pending, Server};
use crate::menu::{ContextMenu, MenuAction};
use crate::palette::{self, Action, Palette, Prompt};
use crate::theme::Theme;
use crate::tree::Tree;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// How long the mouse must rest on a word before its hover info is fetched.
const HOVER_DELAY: Duration = Duration::from_millis(400);

pub enum AppEvent {
    Input(Event),
    /// A message from the language server for (language id, generation).
    Lsp(String, u64, Value),
    /// That server exited, with its last line of stderr if any.
    LspExit(String, u64, Option<String>),
}

#[derive(PartialEq, Clone, Copy)]
pub enum Focus {
    Editor,
    Tree,
}

/// A diagnostic from a language server. Positions are (line, utf-16 column).
pub struct Diag {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub severity: u8,
    pub message: String,
}

pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    filter: String,
    sort: String,
    insert: String,
    edit_start: Option<(usize, usize)>,
}

pub struct Completion {
    pub items: Vec<CompletionItem>,
    /// Indices into `items` that match what has been typed, best first.
    pub shown: Vec<usize>,
    pub sel: usize,
    /// Char index where the word being completed starts.
    pub start: usize,
}

pub struct Find {
    pub query: String,
    pub focused: bool,
    pub matches: Vec<(usize, usize)>,
    stamp: (usize, i32),
}

/// One row of the project-search results list.
pub enum Row {
    Header(String),
    Hit(usize),
}

pub struct ProjectSearch {
    pub query: String,
    pub hits: Vec<crate::search::Hit>,
    pub rows: Vec<Row>,
    pub sel: usize,
    pub scroll: usize,
}

fn build_rows(hits: &[crate::search::Hit], root: &Path) -> Vec<Row> {
    let mut rows = vec![];
    let mut last: Option<&Path> = None;
    for (i, h) in hits.iter().enumerate() {
        if last != Some(h.path.as_path()) {
            let rel = h.path.strip_prefix(root).unwrap_or(&h.path);
            rows.push(Row::Header(rel.to_string_lossy().into_owned()));
            last = Some(h.path.as_path());
        }
        rows.push(Row::Hit(i));
    }
    rows
}

impl ProjectSearch {
    fn new() -> Self {
        ProjectSearch {
            query: String::new(),
            hits: vec![],
            rows: vec![],
            sel: 0,
            scroll: 0,
        }
    }

    fn run(&mut self, root: &Path) {
        self.hits = crate::search::search(root, &self.query);
        self.rows = build_rows(&self.hits, root);
        self.sel = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Hit(_)))
            .unwrap_or(0);
        self.scroll = 0;
    }

    fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        let mut i = self.sel as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if matches!(self.rows[i as usize], Row::Hit(_)) {
                self.sel = i as usize;
                return;
            }
        }
    }

    pub fn selected_hit(&self) -> Option<&crate::search::Hit> {
        match self.rows.get(self.sel) {
            Some(Row::Hit(i)) => self.hits.get(*i),
            _ => None,
        }
    }
}

/// One setting shown in the Settings panel. Each maps to a real `Config`
/// field, so toggling it here takes effect immediately and is persisted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    Theme,
    LineNumbers,
    CursorBlink,
    CursorStyle,
    FormatOnSave,
    TabWidth,
    MouseSupport,
    SidebarWidth,
}

pub enum SettingsRow {
    Header(&'static str),
    Item(SettingId),
}

/// Fixed layout of the Settings panel, grouped by category.
pub const SETTINGS_ROWS: &[SettingsRow] = &[
    SettingsRow::Header("Appearance"),
    SettingsRow::Item(SettingId::Theme),
    SettingsRow::Header("Editor"),
    SettingsRow::Item(SettingId::LineNumbers),
    SettingsRow::Item(SettingId::CursorStyle),
    SettingsRow::Item(SettingId::CursorBlink),
    SettingsRow::Item(SettingId::FormatOnSave),
    SettingsRow::Item(SettingId::TabWidth),
    SettingsRow::Header("Interface"),
    SettingsRow::Item(SettingId::MouseSupport),
    SettingsRow::Item(SettingId::SidebarWidth),
];

const TAB_WIDTHS: &[usize] = &[2, 4, 8];
const SIDEBAR_WIDTHS: &[u16] = &[20, 24, 30, 36, 44];

impl SettingId {
    pub fn label(self) -> &'static str {
        match self {
            SettingId::Theme => "Theme",
            SettingId::LineNumbers => "Line Numbers",
            SettingId::CursorBlink => "Blinking Cursor",
            SettingId::CursorStyle => "Cursor Style",
            SettingId::FormatOnSave => "Format on Save",
            SettingId::TabWidth => "Tab Width",
            SettingId::MouseSupport => "Mouse Support",
            SettingId::SidebarWidth => "Sidebar Width",
        }
    }

    /// The current value, rendered: a checkbox state for bools, a number
    /// for the cycled numeric settings.
    pub fn value(self, cfg: &Config) -> String {
        match self {
            SettingId::Theme => crate::theme::ThemeId::from_id(&cfg.theme)
                .label()
                .to_string(),
            SettingId::LineNumbers => checkbox(cfg.show_line_numbers),
            SettingId::CursorBlink => checkbox(cfg.cursor_blink),
            SettingId::CursorStyle => if cfg.cursor_style == "bar" {
                "Bar"
            } else {
                "Block"
            }
            .to_string(),
            SettingId::FormatOnSave => checkbox(cfg.format_on_save),
            SettingId::TabWidth => cfg.tab_width.to_string(),
            SettingId::MouseSupport => checkbox(cfg.mouse_support),
            SettingId::SidebarWidth => cfg.sidebar_width.to_string(),
        }
    }
}

fn checkbox(v: bool) -> String {
    if v {
        "[x]".to_string()
    } else {
        "[ ]".to_string()
    }
}

fn cycle(options: &[usize], current: usize, delta: isize) -> usize {
    let i = options.iter().position(|v| *v == current).unwrap_or(0) as isize;
    let n = options.len() as isize;
    options[((i + delta).rem_euclid(n)) as usize]
}

fn cycle_u16(options: &[u16], current: u16, delta: isize) -> u16 {
    let i = options.iter().position(|v| *v == current).unwrap_or(0) as isize;
    let n = options.len() as isize;
    options[((i + delta).rem_euclid(n)) as usize]
}

pub struct SettingsPanel {
    pub sel: usize,
    pub scroll: usize,
}

impl SettingsPanel {
    fn new() -> Self {
        let sel = SETTINGS_ROWS
            .iter()
            .position(|r| matches!(r, SettingsRow::Item(_)))
            .unwrap_or(0);
        SettingsPanel { sel, scroll: 0 }
    }

    fn move_sel(&mut self, delta: isize) {
        let n = SETTINGS_ROWS.len() as isize;
        let mut i = self.sel as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if matches!(SETTINGS_ROWS[i as usize], SettingsRow::Item(_)) {
                self.sel = i as usize;
                return;
            }
        }
    }

    pub fn selected(&self) -> Option<SettingId> {
        match SETTINGS_ROWS.get(self.sel) {
            Some(SettingsRow::Item(id)) => Some(*id),
            _ => None,
        }
    }
}

/// Screen rectangles from the last draw, used to route mouse events.
#[derive(Default)]
pub struct Layout {
    pub width: u16,
    pub tree: Rect,
    pub editor: Rect,
    pub gutter: u16,
    pub tabs_y: u16,
    /// (first column, one past last column, buffer index)
    pub tabs: Vec<(u16, u16, usize)>,
    pub palette_list: Rect,
    pub completion: Rect,
    pub hover: Rect,
    /// The "Go to Definition" row at the foot of the hover popup.
    pub hover_link: Rect,
    pub completion_first: usize,
    pub search_list: Rect,
    pub settings_list: Rect,
    pub menu: Rect,
    pub shortcuts: Rect,
    /// The "✗ server" marker in the status bar; clicking it restarts.
    pub server_badge: Rect,
    pub cursor: Option<(u16, u16)>,
}

enum Drag {
    Select,
    Sidebar,
}

#[derive(PartialEq, Clone, Copy)]
enum Armed {
    Quit,
    Close(usize),
}

pub struct App {
    pub root: PathBuf,
    pub cfg: Config,
    pub theme: Theme,
    pub keymap: Keymap,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub tree: Tree,
    pub show_tree: bool,
    pub tree_width: u16,
    pub tree_follow: bool,
    pub focus: Focus,
    pub palette: Option<Palette>,
    pub find: Option<Find>,
    pub search_panel: Option<ProjectSearch>,
    pub search_follow: bool,
    pub settings_panel: Option<SettingsPanel>,
    /// The keyboard shortcuts panel, open at this scroll offset.
    pub shortcuts: Option<usize>,
    /// The leader key's menu is open, with these keys pressed after it.
    pub leader: Option<Vec<char>>,
    pub menu: Option<ContextMenu>,
    /// The path a pending Rename prompt applies to.
    prompt_target: Option<PathBuf>,
    pub completion: Option<Completion>,
    pub hover: Option<String>,
    /// Char index a mouse hover popup points at; None means at the cursor.
    pub hover_at: Option<usize>,
    /// The word under the resting mouse pointer, and when to ask about it.
    hover_dwell: Option<(Instant, (usize, usize))>,
    /// The word a mouse hover was last asked about (or is showing for).
    hover_word: Option<(usize, usize)>,
    /// The word under the pointer while Alt/Option is held, drawn as a link
    /// because Alt+Click jumps to its definition.
    pub link_word: Option<(usize, usize)>,
    pub diagnostics: HashMap<PathBuf, Vec<Diag>>,
    pub servers: HashMap<String, Server>,
    /// Servers that couldn't start or stopped, by language id: the server's
    /// name and why. Shown in the status bar until restarted.
    pub failed_servers: HashMap<String, (String, String)>,
    pub status: String,
    pub layout: Layout,
    /// Scroll the editor to the cursor on the next draw.
    pub follow: bool,
    /// The cursor is in the hidden half of its blink.
    pub blink_off: bool,
    pub quit: bool,
    drag: Option<Drag>,
    last_click: (Instant, u16, u16, u8),
    armed: Option<Armed>,
    was_armed: Option<Armed>,
    clipboard: Option<arboard::Clipboard>,
    clip: String,
    jumps: Vec<(PathBuf, usize)>,
    tx: Sender<AppEvent>,
}

fn lsp_pos(b: &Buffer, idx: usize) -> Value {
    let line = b.rope.char_to_line(idx);
    let col = b
        .rope
        .line(line)
        .char_to_utf16_cu(idx - b.rope.line_to_char(line));
    json!({ "line": line, "character": col })
}

pub fn from_lsp_pos(b: &Buffer, line: usize, character: usize) -> usize {
    if line >= b.lines() {
        return b.len();
    }
    let l = b.rope.line(line);
    b.idx(line, l.utf16_cu_to_char(character.min(l.len_utf16_cu())))
}

fn pos_of(v: &Value) -> Option<(usize, usize)> {
    Some((
        v["line"].as_u64()? as usize,
        v["character"].as_u64()? as usize,
    ))
}

fn strip_snippet(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let (mut out, mut i, mut depth) = (String::new(), 0, 0);
    while i < cs.len() {
        match cs[i] {
            '\\' if i + 1 < cs.len() => {
                out.push(cs[i + 1]);
                i += 1;
            }
            '$' if cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                while cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                    i += 1;
                }
            }
            '$' if cs.get(i + 1) == Some(&'{') => {
                i += 1;
                while cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                    i += 1;
                }
                if cs.get(i + 1) == Some(&':') {
                    i += 1;
                }
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            c => out.push(c),
        }
        i += 1;
    }
    out
}

fn hover_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(hover_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(_) => v["value"].as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn search(b: &Buffer, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return vec![];
    }
    // Smart case: all-lowercase queries match case-insensitively.
    let fold = !query.chars().any(char::is_uppercase);
    let norm = |c: char| {
        if fold {
            c.to_lowercase().next().unwrap_or(c)
        } else {
            c
        }
    };
    let q: Vec<char> = query.chars().map(norm).collect();
    let text: Vec<char> = b.rope.chars().map(norm).collect();
    let mut out = vec![];
    let mut i = 0;
    while i + q.len() <= text.len() && out.len() < 20_000 {
        if text[i..i + q.len()] == q[..] {
            out.push((i, i + q.len()));
            i += q.len();
        } else {
            i += 1;
        }
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() {
                T[(n >> shift & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

impl App {
    pub fn new(root: PathBuf, tx: Sender<AppEvent>) -> App {
        let (cfg, cfg_err) = Config::load();
        let (keymap, key_errs) = Keymap::new(&cfg.keys, &cfg.leader_keys);
        let status = cfg_err
            .or_else(|| key_errs.first().map(|e| format!("config.toml: {e}")))
            .unwrap_or_default();
        App {
            tree: Tree::new(root.clone()),
            tree_width: cfg.sidebar_width.max(12),
            root,
            theme: crate::theme::ThemeId::from_id(&cfg.theme).theme(),
            cfg,
            keymap,
            buffers: vec![],
            active: 0,
            show_tree: true,
            tree_follow: false,
            focus: Focus::Tree,
            palette: None,
            find: None,
            search_panel: None,
            search_follow: true,
            settings_panel: None,
            shortcuts: None,
            leader: None,
            menu: None,
            prompt_target: None,
            completion: None,
            hover: None,
            hover_at: None,
            hover_dwell: None,
            hover_word: None,
            link_word: None,
            diagnostics: HashMap::new(),
            servers: HashMap::new(),
            failed_servers: HashMap::new(),
            status,
            layout: Layout::default(),
            follow: true,
            blink_off: false,
            quit: false,
            drag: None,
            last_click: (Instant::now(), 0, 0, 0),
            armed: None,
            was_armed: None,
            clipboard: arboard::Clipboard::new().ok(),
            clip: String::new(),
            jumps: vec![],
            tx,
        }
    }

    pub fn buf(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    fn buf_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    fn close_popups(&mut self) {
        self.completion = None;
        self.hover = None;
    }

    // ---- files and tabs ----

    pub fn open_path(&mut self, path: &Path) -> bool {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self
            .buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(&path))
        {
            self.active = i;
        } else {
            match Buffer::open(path.clone()) {
                Ok(b) => {
                    self.buffers.push(b);
                    self.active = self.buffers.len() - 1;
                }
                Err(e) => {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.set_status(format!("Can't open {name}: {e}"));
                    return false;
                }
            }
        }
        self.focus = Focus::Editor;
        self.follow = true;
        self.find = None;
        self.close_popups();
        self.tree.reveal(&path);
        self.tree_follow = true;
        self.ensure_server();
        true
    }

    fn switch_tab(&mut self, i: usize) {
        if i < self.buffers.len() {
            self.active = i;
            self.focus = Focus::Editor;
            self.follow = true;
            self.find = None;
            self.close_popups();
            if let Some(p) = self.buffers[i].path.clone() {
                self.tree.reveal(&p);
                self.tree_follow = true;
            }
        }
    }

    fn close_tab(&mut self, i: usize) {
        let Some(b) = self.buffers.get(i) else { return };
        if b.dirty && self.was_armed != Some(Armed::Close(i)) {
            self.armed = Some(Armed::Close(i));
            self.set_status(format!(
                "{} has unsaved changes — close again to discard them, or Ctrl+S to save",
                b.name()
            ));
            return;
        }
        self.remove_buffer(i);
    }

    /// Close a tab without the unsaved-changes check.
    fn remove_buffer(&mut self, i: usize) {
        let Some(b) = self.buffers.get(i) else { return };
        if let (Some(lang), Some(path), Some(_)) = (b.lang, &b.path, b.lsp_version) {
            if let Some(srv) = self.servers.get_mut(lang.id) {
                srv.notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": lsp::path_to_uri(path) } }),
                );
            }
        }
        self.buffers.remove(i);
        if self.active > i || self.active >= self.buffers.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.find = None;
        self.close_popups();
        self.follow = true;
        if self.buffers.is_empty() {
            self.focus = Focus::Tree;
        }
    }

    fn save(&mut self) {
        let Some(b) = self.buf() else { return };
        if b.path.is_none() {
            self.palette = Some(Palette::prompt(&self.root, Prompt::SaveAs, ""));
            return;
        }
        if self.cfg.format_on_save && self.request_format(true) {
            return;
        }
        self.write_active();
    }

    fn write_active(&mut self) {
        let Some(b) = self.buffers.get_mut(self.active) else {
            return;
        };
        match b.save() {
            Ok(()) => {
                let (name, path, lang) = (b.name(), b.path.clone().unwrap(), b.lang);
                self.sync_lsp();
                if let Some(srv) = lang.and_then(|l| self.servers.get_mut(l.id)) {
                    srv.notify(
                        "textDocument/didSave",
                        json!({ "textDocument": { "uri": lsp::path_to_uri(&path) } }),
                    );
                }
                let note = if path == config::config_path() {
                    " — restart kite to apply"
                } else {
                    ""
                };
                self.set_status(format!("Saved {name}{note}"));
            }
            Err(e) => self.set_status(format!("Save failed: {e}")),
        }
    }

    // ---- commands ----

    pub fn run(&mut self, cmd: Command) {
        use Command::*;
        match cmd {
            Save => self.save(),
            NewFile => self.palette = Some(Palette::prompt(&self.root, Prompt::NewFile, "")),
            CloseTab => self.close_tab(self.active),
            Shortcuts => self.shortcuts = Some(0),
            Leader => {
                self.close_popups();
                self.leader = Some(vec![]);
            }
            ToggleLineNumbers => self.toggle_setting(SettingId::LineNumbers),
            ToggleCursorBlink => self.toggle_setting(SettingId::CursorBlink),
            ToggleMouse => self.toggle_setting(SettingId::MouseSupport),
            Quit => {
                let dirty = self.buffers.iter().filter(|b| b.dirty).count();
                if dirty > 0 && self.was_armed != Some(Armed::Quit) {
                    self.armed = Some(Armed::Quit);
                    self.set_status(format!(
                        "{dirty} unsaved file(s) — press quit again to discard changes"
                    ));
                } else {
                    self.quit = true;
                }
            }
            FileFinder => self.palette = Some(Palette::new(&self.root, &self.keymap, "")),
            CommandPalette => self.palette = Some(Palette::new(&self.root, &self.keymap, ">")),
            GotoLine => self.palette = Some(Palette::new(&self.root, &self.keymap, ":")),
            SearchInFiles => {
                self.close_popups();
                self.find = None;
                self.settings_panel = None;
                self.search_panel = Some(ProjectSearch::new());
                self.search_follow = true;
            }
            Settings => {
                self.close_popups();
                self.find = None;
                self.search_panel = None;
                self.settings_panel = Some(SettingsPanel::new());
            }
            NextTab | PrevTab if !self.buffers.is_empty() => {
                let n = self.buffers.len();
                self.switch_tab((self.active + if cmd == NextTab { 1 } else { n - 1 }) % n);
            }
            NextTab | PrevTab => {}
            ToggleSidebar => {
                self.show_tree = !self.show_tree;
                if !self.show_tree && !self.buffers.is_empty() {
                    self.focus = Focus::Editor;
                }
            }
            FocusSidebar => {
                if self.focus == Focus::Tree && !self.buffers.is_empty() {
                    self.focus = Focus::Editor;
                } else {
                    self.show_tree = true;
                    self.focus = Focus::Tree;
                    self.tree_follow = true;
                }
            }
            RefreshTree => self.tree.refresh(),
            Find => {
                let Some(b) = self.buf() else { return };
                let seed = b.selected_text().filter(|t| !t.contains('\n'));
                let query = seed
                    .or_else(|| self.find.as_ref().map(|f| f.query.clone()))
                    .unwrap_or_default();
                self.find = Some(self::Find {
                    query,
                    focused: true,
                    matches: vec![],
                    stamp: (usize::MAX, 0),
                });
                self.focus = Focus::Editor;
                self.close_popups();
                self.refresh_find();
            }
            FindNext => self.find_step(true, false),
            FindPrev => self.find_step(false, false),
            Undo | Redo => {
                if let Some(b) = self.buf_mut() {
                    let done = if cmd == Undo { b.undo() } else { b.redo() };
                    self.follow = true;
                    if !done {
                        self.set_status(if cmd == Undo {
                            "Nothing to undo"
                        } else {
                            "Nothing to redo"
                        });
                    }
                }
            }
            Copy | Cut => {
                let Some(b) = self.buf_mut() else { return };
                if b.selection().is_none() {
                    // No selection: act on the whole line, like VS Code and Zed.
                    b.select_line_at(b.cursor);
                    if cmd == Copy {
                        let text = b.selected_text().unwrap_or_default();
                        b.anchor = None;
                        return self.copy(text);
                    }
                }
                let text = b.selected_text().unwrap_or_default();
                if cmd == Cut {
                    b.insert("");
                    self.follow = true;
                }
                self.copy(text);
            }
            Paste => {
                let text = self.clipboard_text();
                self.on_paste(&text);
            }
            SelectAll => {
                if let Some(b) = self.buf_mut() {
                    b.select_all();
                }
            }
            ToggleComment => {
                if let Some(b) = self.buf_mut() {
                    b.toggle_comment();
                }
            }
            Complete => self.complete_here(true),
            RestartServer => self.restart_server(),
            Hover => {
                if !self.request_at_cursor("textDocument/hover", |_| Pending::Hover { mouse: None })
                {
                    self.no_server();
                }
            }
            GotoDefinition => {
                if !self.request_at_cursor("textDocument/definition", |_| Pending::Definition) {
                    self.no_server();
                }
            }
            GoBack => match self.jumps.pop() {
                Some((path, cursor)) => {
                    if self.open_path(&path) {
                        if let Some(b) = self.buf_mut() {
                            b.move_to(cursor, false);
                        }
                    }
                }
                None => self.set_status("No earlier location"),
            },
            Format => {
                if !self.request_format(false) {
                    self.no_server();
                }
            }
            OpenConfig => {
                let path = config::config_path();
                if !path.exists() {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(&path, config::TEMPLATE);
                }
                self.open_path(&path);
            }
        }
    }

    fn no_server(&mut self) {
        if self.buf().is_some() {
            self.set_status("No language server running for this file");
        }
    }

    fn do_action(&mut self, action: Action) {
        match action {
            Action::Open(path) => {
                self.open_path(&path);
            }
            Action::Run(cmd) => self.run(cmd),
            Action::Line(n) => {
                if let Some(b) = self.buf_mut() {
                    let idx = b.idx(n.saturating_sub(1), 0);
                    b.move_to(idx, false);
                    self.follow = true;
                }
            }
            Action::Prompt(kind, text) => {
                let path = self.root.join(text.trim_start_matches('/'));
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match kind {
                    Prompt::NewFile => {
                        if !path.exists() {
                            if let Err(e) = std::fs::write(&path, "") {
                                return self.set_status(format!("Can't create file: {e}"));
                            }
                        }
                        self.tree.refresh();
                        self.open_path(&path);
                    }
                    Prompt::NewFolder => {
                        if let Err(e) = std::fs::create_dir_all(&path) {
                            return self.set_status(format!("Can't create folder: {e}"));
                        }
                        self.tree.refresh();
                        self.tree.reveal(&path);
                        self.tree_follow = true;
                    }
                    Prompt::Rename => {
                        let Some(old) = self.prompt_target.take() else {
                            return;
                        };
                        self.rename_path(&old, &path);
                    }
                    Prompt::SaveAs => {
                        let Some(b) = self.buffers.get_mut(self.active) else {
                            return;
                        };
                        b.lang = crate::highlight::lang_for_path(&path);
                        b.path = Some(path.clone());
                        b.syntax = None;
                        self.write_active();
                        self.tree.refresh();
                        self.tree.reveal(&path);
                        self.ensure_server();
                    }
                }
            }
        }
    }

    // ---- clipboard ----

    fn copy(&mut self, text: String) {
        let ok = self
            .clipboard
            .as_mut()
            .is_some_and(|c| c.set_text(text.clone()).is_ok());
        if !ok {
            // No system clipboard (e.g. over SSH): ask the terminal via OSC 52.
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
            let _ = out.flush();
        }
        self.clip = text;
    }

    fn clipboard_text(&mut self) -> String {
        self.clipboard
            .as_mut()
            .and_then(|c| c.get_text().ok())
            .unwrap_or_else(|| self.clip.clone())
    }

    fn on_paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(p) = &mut self.palette {
            p.input.push_str(text.lines().next().unwrap_or(""));
            p.refresh();
        } else if let Some(s) = &mut self.search_panel {
            s.query.push_str(text.lines().next().unwrap_or(""));
        } else if let Some(f) = self.find.as_mut().filter(|f| f.focused) {
            f.query.push_str(text.lines().next().unwrap_or(""));
            self.refresh_find();
            self.find_step(true, true);
        } else if self.focus == Focus::Editor {
            if let Some(b) = self.buf_mut() {
                b.insert(&text);
                self.follow = true;
                self.close_popups();
            }
        }
    }

    // ---- find ----

    fn refresh_find(&mut self) {
        let (Some(f), Some(b)) = (&mut self.find, self.buffers.get(self.active)) else {
            return;
        };
        f.matches = search(b, &f.query);
        f.stamp = (self.active, b.version);
    }

    fn find_step(&mut self, forward: bool, inclusive: bool) {
        let stale = match (&self.find, self.buf()) {
            (Some(f), Some(b)) => f.stamp != (self.active, b.version),
            _ => return self.run(Command::Find),
        };
        if stale {
            self.refresh_find();
        }
        let (Some(f), Some(b)) = (&self.find, self.buffers.get_mut(self.active)) else {
            return;
        };
        if f.matches.is_empty() {
            let msg = if f.query.is_empty() {
                String::new()
            } else {
                format!("No matches for \"{}\"", f.query)
            };
            return self.set_status(msg);
        }
        let (sel_start, sel_end) = b.selection().unwrap_or((b.cursor, b.cursor));
        let i = if forward {
            let from = if inclusive {
                sel_start
            } else {
                sel_end.max(sel_start + 1)
            };
            f.matches.iter().position(|m| m.0 >= from).unwrap_or(0)
        } else {
            f.matches
                .iter()
                .rposition(|m| m.0 < sel_start)
                .unwrap_or(f.matches.len() - 1)
        };
        let (s, e) = f.matches[i];
        b.select(s, e);
        self.follow = true;
        let total = f.matches.len();
        self.set_status(format!("{} of {total}", i + 1));
    }

    // ---- input ----

    pub fn handle(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Input(Event::Key(k)) if k.kind != KeyEventKind::Release => self.on_key(k),
            AppEvent::Input(Event::Mouse(m)) => self.on_mouse(m),
            AppEvent::Input(Event::Paste(text)) => self.on_paste(&text),
            AppEvent::Input(_) => {}
            AppEvent::Lsp(key, generation, msg) => {
                if self
                    .servers
                    .get(&key)
                    .is_some_and(|s| s.generation == generation)
                {
                    self.on_lsp(&key, msg);
                }
            }
            AppEvent::LspExit(key, generation, last_err) => {
                if self
                    .servers
                    .get(&key)
                    .is_some_and(|s| s.generation == generation)
                {
                    let mut srv = self.servers.remove(&key).unwrap();
                    srv.shutdown();
                    let why = match last_err {
                        Some(e) => format!("{} stopped: {e}", srv.name),
                        None => format!("{} stopped", srv.name),
                    };
                    self.set_status(why.clone());
                    self.failed_servers.insert(key.clone(), (srv.name, why));
                    for b in &mut self.buffers {
                        if b.lang.is_some_and(|l| l.id == key) {
                            b.lsp_version = None;
                        }
                    }
                }
            }
        }
    }

    /// Housekeeping after a batch of events, before drawing.
    pub fn after_events(&mut self) {
        self.sync_lsp();
        let stale = match (&self.find, self.buf()) {
            (Some(f), Some(b)) => f.stamp != (self.active, b.version),
            _ => false,
        };
        if stale {
            self.refresh_find();
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        self.status.clear();
        self.was_armed = self.armed.take();
        self.hover_dwell = None;
        self.hover_word = None;
        self.link_word = None;
        let key = normalize(k.code, k.modifiers);
        let plain = (k.modifiers - KeyModifiers::SHIFT).is_empty();

        if let Some(path) = self.leader.take() {
            return self.leader_key(path, k.code, k.modifiers);
        }
        // The shortcuts panel scrolls with the arrows; Esc or its own key
        // closes it, and any other key closes it and then does its usual job.
        if let Some(scroll) = &mut self.shortcuts {
            match k.code {
                KeyCode::Up => return *scroll = scroll.saturating_sub(1),
                KeyCode::Down => return *scroll += 1,
                KeyCode::PageUp => return *scroll = scroll.saturating_sub(10),
                KeyCode::PageDown => return *scroll += 10,
                _ => {}
            }
            self.shortcuts = None;
            if k.code == KeyCode::Esc || self.keymap.get(key) == Some(Command::Shortcuts) {
                return;
            }
        }
        if self.menu.is_some() {
            return self.menu_key(k.code);
        }
        if self.palette.is_some() {
            return self.palette_key(k.code, k.modifiers);
        }
        if self.search_panel.is_some() {
            return self.search_panel_key(k.code, k.modifiers);
        }
        if self.settings_panel.is_some() {
            return self.settings_panel_key(k.code, k.modifiers);
        }
        if self.find.as_ref().is_some_and(|f| f.focused) && self.find_key(k.code, k.modifiers) {
            return;
        }
        self.hover = None;
        if let Some(c) = &mut self.completion {
            match k.code {
                KeyCode::Up => return c.sel = c.sel.checked_sub(1).unwrap_or(c.shown.len() - 1),
                KeyCode::Down => return c.sel = (c.sel + 1) % c.shown.len(),
                KeyCode::Enter | KeyCode::Tab if plain => return self.accept_completion(),
                KeyCode::Esc => return self.completion = None,
                _ => {}
            }
        }
        if let Some(cmd) = self.keymap.get(key) {
            if !matches!(cmd, Command::Complete) {
                self.completion = None;
            }
            return self.run(cmd);
        }
        match self.focus {
            Focus::Tree => self.tree_key(k.code),
            Focus::Editor => self.editor_key(k.code, k.modifiers),
        }
    }

    fn palette_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let Some(p) = &mut self.palette else { return };
        match code {
            KeyCode::Esc => self.palette = None,
            KeyCode::Enter => {
                let action = p.accept();
                self.palette = None;
                if let Some(a) = action {
                    self.do_action(a);
                }
            }
            KeyCode::Up => p.move_sel(-1),
            KeyCode::Down | KeyCode::Tab => p.move_sel(1),
            KeyCode::Char('p') if ctrl => p.move_sel(-1),
            KeyCode::Char('n') if ctrl => p.move_sel(1),
            KeyCode::PageUp => p.move_sel(-(palette::MAX_ROWS as isize)),
            KeyCode::PageDown => p.move_sel(palette::MAX_ROWS as isize),
            KeyCode::Backspace => {
                if p.input.is_empty() {
                    self.palette = None;
                } else {
                    if ctrl || mods.contains(KeyModifiers::ALT) {
                        while p.input.pop().is_some_and(|c| c.is_alphanumeric())
                            && p.input.ends_with(char::is_alphanumeric)
                        {}
                    } else {
                        p.input.pop();
                    }
                    p.refresh();
                }
            }
            KeyCode::Char('v') if ctrl => {
                let text = self.clipboard_text();
                self.on_paste(&text);
            }
            KeyCode::Char(c) if !ctrl && !mods.contains(KeyModifiers::ALT) => {
                p.input.push(c);
                p.refresh();
            }
            _ => {}
        }
    }

    fn search_panel_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let Some(s) = &mut self.search_panel else {
            return;
        };
        match code {
            KeyCode::Esc => self.search_panel = None,
            KeyCode::Enter => {
                if s.selected_hit().is_some() {
                    self.search_panel_activate();
                } else {
                    s.run(&self.root);
                }
            }
            KeyCode::Up => s.move_sel(-1),
            KeyCode::Down => s.move_sel(1),
            KeyCode::PageUp => s.move_sel(-10),
            KeyCode::PageDown => s.move_sel(10),
            KeyCode::Backspace => {
                s.query.pop();
            }
            KeyCode::Char('v') if ctrl => {
                let text = self.clipboard_text();
                self.on_paste(&text);
            }
            KeyCode::Char(c) if !ctrl && !mods.contains(KeyModifiers::ALT) => s.query.push(c),
            _ => {}
        }
        self.search_follow = true;
    }

    fn search_panel_activate(&mut self) {
        let Some(hit) = self.search_panel.as_ref().and_then(|s| s.selected_hit()) else {
            return;
        };
        let (path, line, col) = (hit.path.clone(), hit.line, hit.col);
        self.search_panel = None;
        self.jump_to_plain(&path, line, col);
    }

    fn search_panel_mouse(&mut self, x: u16, y: u16, kind: MouseEventKind) {
        let list = self.layout.search_list;
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !list.contains(Position::new(x, y)) {
                    self.search_panel = None;
                    return;
                }
                let row =
                    self.search_panel.as_ref().map_or(0, |s| s.scroll) + (y - list.y) as usize;
                if let Some(s) = &mut self.search_panel {
                    if matches!(s.rows.get(row), Some(Row::Hit(_))) {
                        s.sel = row;
                    } else {
                        return;
                    }
                }
                self.search_panel_activate();
            }
            MouseEventKind::ScrollDown => {
                if let Some(s) = &mut self.search_panel {
                    s.move_sel(1);
                    self.search_follow = true;
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(s) = &mut self.search_panel {
                    s.move_sel(-1);
                    self.search_follow = true;
                }
            }
            _ => {}
        }
    }

    // ---- context menu ----

    /// Right-click: build the menu for whatever is under the pointer.
    fn open_context_menu(&mut self, x: u16, y: u16) {
        let at = Position::new(x, y);
        let anchor = (x, y);
        self.hover = None;
        self.completion = None;
        let tree = self.layout.tree;
        if self.show_tree && tree.contains(at) {
            if x + 1 == tree.right() {
                return; // the draggable border
            }
            self.focus = Focus::Tree;
            let node = (y > tree.y)
                .then(|| self.tree.scroll + (y - tree.y - 1) as usize)
                .and_then(|i| {
                    self.tree
                        .nodes
                        .get(i)
                        .map(|n| (i, n.path.clone(), n.is_dir))
                });
            self.menu = Some(match node {
                Some((i, path, is_dir)) => {
                    self.tree.sel = i;
                    ContextMenu::for_tree_node(&path, is_dir, anchor)
                }
                None => ContextMenu::for_tree_blank(&self.root, &self.keymap, anchor),
            });
        } else if self.layout.editor.contains(at) && !self.buffers.is_empty() {
            let Some(idx) = self.editor_idx_at(x, y) else {
                return;
            };
            self.focus = Focus::Editor;
            if let Some(f) = &mut self.find {
                f.focused = false;
            }
            // Like most editors: right-clicking outside the selection moves
            // the cursor there, so "Go to Definition" acts on what you clicked.
            let b = &mut self.buffers[self.active];
            if !b.selection().is_some_and(|(s, e)| idx >= s && idx <= e) {
                b.move_to(idx, false);
            }
            let path = b.path.clone();
            self.menu = Some(ContextMenu::for_editor(
                path.as_deref(),
                &self.keymap,
                anchor,
            ));
        }
    }

    fn menu_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Left => self.menu = None,
            KeyCode::Up => {
                if let Some(m) = &mut self.menu {
                    m.move_sel(-1);
                }
            }
            KeyCode::Down => {
                if let Some(m) = &mut self.menu {
                    m.move_sel(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.menu_activate(),
            _ => {}
        }
    }

    /// Returns true if the event was consumed. A right-click elsewhere closes
    /// the menu and is passed on so it can open a new one.
    fn menu_mouse(&mut self, m: MouseEvent) -> bool {
        let r = self.layout.menu;
        let at = Position::new(m.column, m.row);
        let Some(menu) = &mut self.menu else {
            return false;
        };
        let row =
            (m.column > r.x && m.column + 1 < r.right() && m.row > r.y && m.row + 1 < r.bottom())
                .then(|| (m.row - r.y - 1) as usize)
                .filter(|i| menu.is_selectable(*i));
        match m.kind {
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                if let Some(i) = row {
                    menu.sel = Some(i);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = row {
                    menu.sel = Some(i);
                    self.menu_activate();
                } else if !r.contains(at) {
                    self.menu = None;
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.menu = None;
                return false;
            }
            MouseEventKind::Up(_) => {}
            _ => self.menu = None,
        }
        true
    }

    fn menu_activate(&mut self) {
        let Some(menu) = self.menu.take() else { return };
        if let Some(action) = menu.selected_action() {
            self.run_menu_action(action, menu.anchor);
        }
    }

    /// Path relative to the project, as shown in prompts.
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    fn run_menu_action(&mut self, action: MenuAction, anchor: (u16, u16)) {
        match action {
            MenuAction::Cmd(c) => self.run(c),
            MenuAction::OpenFile(p) => {
                self.open_path(&p);
            }
            MenuAction::NewFileIn(dir) => self.prompt_in_dir(Prompt::NewFile, &dir),
            MenuAction::NewFolderIn(dir) => self.prompt_in_dir(Prompt::NewFolder, &dir),
            MenuAction::Rename(p) => {
                let text = self.rel(&p);
                self.prompt_target = Some(p);
                self.palette = Some(Palette::prompt(&self.root, Prompt::Rename, &text));
            }
            MenuAction::Duplicate(p) => match fileops::duplicate(&p) {
                Ok(to) => {
                    self.tree.refresh();
                    self.tree.reveal(&to);
                    self.tree_follow = true;
                    self.set_status(format!("Duplicated as {}", self.rel(&to)));
                }
                Err(e) => self.set_status(format!("Can't duplicate: {e}")),
            },
            MenuAction::AskDelete(p) => self.menu = Some(ContextMenu::confirm_delete(&p, anchor)),
            MenuAction::Delete(p) => self.delete_path(&p),
            MenuAction::CopyPath(p) => {
                self.copy(p.to_string_lossy().into_owned());
                self.set_status("Copied path");
            }
            MenuAction::CopyRelativePath(p) => {
                let rel = self.rel(&p);
                self.copy(rel);
                self.set_status("Copied relative path");
            }
            MenuAction::Reveal(p) => {
                if let Err(e) = fileops::reveal(&p) {
                    self.set_status(format!("Can't open the file manager: {e}"));
                }
            }
            MenuAction::Cancel => {}
        }
    }

    /// Open a path prompt pre-filled with `dir/`.
    fn prompt_in_dir(&mut self, kind: Prompt, dir: &Path) {
        let mut text = self.rel(dir);
        if !text.is_empty() {
            text.push('/');
        }
        self.palette = Some(Palette::prompt(&self.root, kind, &text));
    }

    /// Delete a file or folder for good, closing any tabs that were open in it.
    fn delete_path(&mut self, path: &Path) {
        let old_sel = self.tree.sel;
        let result = fileops::delete(path);
        if result.is_ok() {
            while let Some(i) = self
                .buffers
                .iter()
                .position(|b| b.path.as_deref().is_some_and(|p| p.starts_with(path)))
            {
                self.remove_buffer(i);
            }
        }
        self.tree.refresh();
        self.tree.sel = old_sel.min(self.tree.nodes.len().saturating_sub(1));
        self.tree_follow = true;
        match result {
            Ok(()) => self.set_status(format!("Deleted {}", self.rel(path))),
            Err(e) => self.set_status(format!("Can't delete: {e}")),
        }
    }

    /// Rename or move a file/folder. Tabs open inside it are re-opened at
    /// their new paths so language servers see the move.
    fn rename_path(&mut self, old: &Path, new: &Path) {
        if new == old {
            return;
        }
        if new.exists() {
            return self.set_status(format!("{} already exists", self.rel(new)));
        }
        if let Some(b) = self
            .buffers
            .iter()
            .find(|b| b.dirty && b.path.as_deref().is_some_and(|p| p.starts_with(old)))
        {
            return self.set_status(format!("Save or close {} before renaming it", b.name()));
        }
        if let Err(e) = std::fs::rename(old, new) {
            return self.set_status(format!("Can't rename: {e}"));
        }
        let remap = |p: &Path| {
            let rest = p.strip_prefix(old).ok()?;
            // `join("")` would add a trailing slash to a plain file path.
            Some(if rest.as_os_str().is_empty() {
                new.to_path_buf()
            } else {
                new.join(rest)
            })
        };
        let focus = self.focus;
        let active_path = self.buffers.get(self.active).and_then(|b| {
            b.path
                .as_deref()
                .map(|p| remap(p).unwrap_or(p.to_path_buf()))
        });
        let moved: Vec<(usize, PathBuf)> = self
            .buffers
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Some((i, remap(b.path.as_deref()?)?)))
            .collect();
        for (i, _) in moved.iter().rev() {
            self.remove_buffer(*i);
        }
        for (_, p) in &moved {
            self.open_path(p);
        }
        if let Some(i) = active_path.and_then(|w| {
            self.buffers
                .iter()
                .position(|b| b.path.as_deref() == Some(w.as_path()))
        }) {
            self.active = i;
        }
        self.focus = focus;
        self.tree.refresh();
        self.tree.reveal(new);
        self.tree_follow = true;
        self.set_status(format!("Renamed to {}", self.rel(new)));
    }

    fn settings_panel_key(&mut self, code: KeyCode, _mods: KeyModifiers) {
        if self.settings_panel.is_none() {
            return;
        }
        match code {
            KeyCode::Esc => self.settings_panel = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.settings_panel {
                    p.move_sel(-1);
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.settings_panel {
                    p.move_sel(1);
                }
            }
            KeyCode::Left => self.apply_selected_setting(-1),
            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => self.apply_selected_setting(1),
            _ => {}
        }
    }

    fn settings_panel_mouse(&mut self, x: u16, y: u16, kind: MouseEventKind) {
        let list = self.layout.settings_list;
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !list.contains(Position::new(x, y)) {
                    self.settings_panel = None;
                    return;
                }
                let row =
                    self.settings_panel.as_ref().map_or(0, |p| p.scroll) + (y - list.y) as usize;
                if matches!(SETTINGS_ROWS.get(row), Some(SettingsRow::Item(_))) {
                    if let Some(p) = &mut self.settings_panel {
                        p.sel = row;
                    }
                    self.apply_selected_setting(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(p) = &mut self.settings_panel {
                    p.move_sel(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(p) = &mut self.settings_panel {
                    p.move_sel(-1);
                }
            }
            _ => {}
        }
    }

    /// Toggle or cycle the currently-selected setting, apply its side effect
    /// immediately, and persist the change to `config.toml`.
    fn apply_selected_setting(&mut self, delta: isize) {
        if let Some(id) = self.settings_panel.as_ref().and_then(|p| p.selected()) {
            self.apply_setting(id, delta);
        }
    }

    /// Flip an on/off setting from a command, and say which way it went.
    fn toggle_setting(&mut self, id: SettingId) {
        self.apply_setting(id, 1);
        let on = id.value(&self.cfg) == checkbox(true);
        self.set_status(format!("{}: {}", id.label(), if on { "on" } else { "off" }));
    }

    /// A key pressed while the leader menu is open, `path` keys deep:
    /// a letter opens a group or runs a command, Backspace steps back out,
    /// 1-9 at the top jump to that tab, and anything else closes the menu.
    fn leader_key(&mut self, mut path: Vec<char>, code: KeyCode, mods: KeyModifiers) {
        let c = match code {
            KeyCode::Char(c) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => c,
            KeyCode::Backspace if !path.is_empty() => {
                path.pop();
                self.leader = Some(path);
                return;
            }
            _ => return,
        };
        let entry = self
            .keymap
            .leader
            .at(&path)
            .and_then(|g| match g.items.get(&c)? {
                LeaderEntry::Group(_) => Some(None),
                LeaderEntry::Run(cmd) => Some(Some(*cmd)),
            });
        match entry {
            Some(None) => {
                path.push(c);
                self.leader = Some(path);
            }
            Some(Some(cmd)) => self.run(cmd),
            None if path.is_empty() && c.is_ascii_digit() && c != '0' => {
                let i = c as usize - '1' as usize;
                if i < self.buffers.len() {
                    self.switch_tab(i);
                } else {
                    self.set_status(format!("There's no tab {c}"));
                }
            }
            None => {
                let keys: String = path.iter().chain([&c]).map(|k| format!(" {k}")).collect();
                self.set_status(format!("Nothing is on leader{keys}"));
            }
        }
    }

    /// Change a setting as the Settings panel would, apply it immediately,
    /// and persist it to `config.toml`.
    fn apply_setting(&mut self, id: SettingId, delta: isize) {
        match id {
            SettingId::Theme => {
                use crate::theme::{ALL_THEMES, ThemeId};
                let current = ThemeId::from_id(&self.cfg.theme);
                let i = ALL_THEMES.iter().position(|t| *t == current).unwrap_or(0) as isize;
                let n = ALL_THEMES.len() as isize;
                let next = ALL_THEMES[((i + delta).rem_euclid(n)) as usize];
                self.cfg.theme = next.id().to_string();
                self.theme = next.theme();
            }
            SettingId::LineNumbers => self.cfg.show_line_numbers = !self.cfg.show_line_numbers,
            SettingId::CursorBlink => self.cfg.cursor_blink = !self.cfg.cursor_blink,
            SettingId::CursorStyle => {
                self.cfg.cursor_style = if self.cfg.cursor_style == "bar" {
                    "block"
                } else {
                    "bar"
                }
                .to_string();
                self.set_cursor_style();
            }
            SettingId::FormatOnSave => self.cfg.format_on_save = !self.cfg.format_on_save,
            SettingId::MouseSupport => {
                self.cfg.mouse_support = !self.cfg.mouse_support;
                self.set_mouse_capture(self.cfg.mouse_support);
            }
            SettingId::TabWidth => {
                self.cfg.tab_width = cycle(TAB_WIDTHS, self.cfg.tab_width, delta);
            }
            SettingId::SidebarWidth => {
                self.cfg.sidebar_width = cycle_u16(SIDEBAR_WIDTHS, self.cfg.sidebar_width, delta);
                self.tree_width = self.cfg.sidebar_width;
            }
        }
        if let Err(e) = self.cfg.save() {
            self.set_status(format!("Couldn't save settings: {e}"));
        }
    }

    pub fn set_mouse_capture(&self, on: bool) {
        use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
        use ratatui::crossterm::execute;
        let mut out = std::io::stdout();
        let _ = if on {
            execute!(out, EnableMouseCapture)
        } else {
            execute!(out, DisableMouseCapture)
        };
    }

    /// Block or bar cursor per `cursor_style`. Always the steady variant:
    /// kite blinks the cursor itself (see `blink_off`), because terminals
    /// are free to ignore a program's request for a blinking one.
    pub fn set_cursor_style(&self) {
        use ratatui::crossterm::cursor::SetCursorStyle;
        use ratatui::crossterm::execute;
        let _ = execute!(
            std::io::stdout(),
            if self.cfg.cursor_style == "bar" {
                SetCursorStyle::SteadyBar
            } else {
                SetCursorStyle::SteadyBlock
            }
        );
    }

    /// Jump to a plain (non-LSP) char-column location, same bookkeeping as
    /// `jump_to` (used for go-to-definition) but without utf-16 translation.
    fn jump_to_plain(&mut self, path: &Path, line: usize, col: usize) {
        if let Some(b) = self.buf() {
            if let Some(p) = &b.path {
                self.jumps.push((p.clone(), b.cursor));
            }
        }
        if self.open_path(path) {
            let rows = self.layout.editor.height as usize;
            if let Some(b) = self.buf_mut() {
                let idx = b.idx(line, col);
                b.move_to(idx, false);
                if line < b.scroll_row || line >= b.scroll_row + rows {
                    b.scroll_row = line.saturating_sub(rows / 3);
                }
            }
        }
    }

    /// Keys while the find bar has focus. Returns false to let the key fall
    /// through to normal handling (so Ctrl+S etc. still work).
    fn find_key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        match code {
            KeyCode::Esc => self.find = None,
            KeyCode::Enter => self.find_step(!mods.contains(KeyModifiers::SHIFT), false),
            KeyCode::Down => self.find_step(true, false),
            KeyCode::Up => self.find_step(false, false),
            KeyCode::Backspace => {
                f.query.pop();
                self.refresh_find();
                self.find_step(true, true);
            }
            KeyCode::Char(c) if (mods - KeyModifiers::SHIFT).is_empty() => {
                f.query.push(c);
                self.refresh_find();
                self.find_step(true, true);
            }
            _ => return false,
        }
        true
    }

    fn tree_key(&mut self, code: KeyCode) {
        self.tree_follow = true;
        match code {
            KeyCode::Up => self.tree.move_sel(-1),
            KeyCode::Down => self.tree.move_sel(1),
            KeyCode::PageUp => self.tree.move_sel(-10),
            KeyCode::PageDown => self.tree.move_sel(10),
            KeyCode::Left => self.tree.collapse_or_parent(),
            KeyCode::Right => self.tree.expand_selected(),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_tree_row(self.tree.sel),
            KeyCode::Esc if !self.buffers.is_empty() => self.focus = Focus::Editor,
            _ => {}
        }
    }

    fn activate_tree_row(&mut self, i: usize) {
        let Some(node) = self.tree.nodes.get(i) else {
            return;
        };
        self.tree.sel = i;
        if node.is_dir {
            self.tree.toggle(i);
        } else {
            let path = node.path.clone();
            self.open_path(&path);
        }
    }

    fn editor_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let shift = mods.contains(KeyModifiers::SHIFT);
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);
        let word = ctrl || alt;
        let page = (self.layout.editor.height as isize - 1).max(1);
        let Some(b) = self.buffers.get_mut(self.active) else {
            return;
        };
        let mut typed = None;
        match code {
            KeyCode::Left if word => b.word_left(shift),
            KeyCode::Right if word => b.word_right(shift),
            KeyCode::Left => b.left(shift),
            KeyCode::Right => b.right(shift),
            KeyCode::Up => b.vertical(-1, shift),
            KeyCode::Down => b.vertical(1, shift),
            KeyCode::PageUp => b.vertical(-page, shift),
            KeyCode::PageDown => b.vertical(page, shift),
            KeyCode::Home if ctrl => b.move_to(0, shift),
            KeyCode::End if ctrl => b.move_to(b.len(), shift),
            KeyCode::Home => b.home(shift),
            KeyCode::End => b.end(shift),
            // macOS terminals send Option+Left/Right as Alt+b / Alt+f.
            KeyCode::Char('b') if alt && !ctrl => b.word_left(shift),
            KeyCode::Char('f') if alt && !ctrl => b.word_right(shift),
            KeyCode::Enter => b.newline(),
            KeyCode::BackTab => b.dedent_lines(),
            KeyCode::Tab if shift => b.dedent_lines(),
            KeyCode::Tab => b.tab(),
            KeyCode::Backspace if word => b.delete_word_back(),
            KeyCode::Backspace => b.backspace(),
            KeyCode::Delete => b.delete(),
            KeyCode::Esc => {
                b.anchor = None;
                self.find = None;
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                b.type_char(c);
                typed = Some(c);
            }
            _ => return,
        }
        self.follow = true;
        match (typed, code) {
            (Some(c), _) if is_word(c) => {
                if self.completion.is_some() {
                    self.filter_completion();
                } else {
                    self.complete_here(false);
                }
            }
            (Some(c), _) => {
                self.completion = None;
                let lang = self.buf().and_then(|b| b.lang);
                // Only member/path access pops completions unprompted; other
                // server trigger characters such as `(` are too noisy.
                let b = &self.buffers[self.active];
                let path_sep = c == ':' && b.cursor >= 2 && b.rope.char(b.cursor - 2) == ':';
                let auto = (c == '.' || path_sep)
                    && lang
                        .and_then(|l| self.servers.get(l.id))
                        .is_some_and(|s| s.triggers.contains(&c));
                if auto {
                    self.request_completion();
                }
            }
            (None, KeyCode::Backspace) if self.completion.is_some() => self.filter_completion(),
            _ => self.completion = None,
        }
    }

    // ---- mouse ----

    /// Buffer char index under a screen cell in the editor.
    fn editor_idx_at(&self, x: u16, y: u16) -> Option<usize> {
        let b = self.buf()?;
        let e = self.layout.editor;
        let row = y.saturating_sub(e.y) as usize + b.scroll_row;
        if row >= b.lines() {
            return Some(b.len());
        }
        let tx = (x as usize).saturating_sub((e.x + self.layout.gutter) as usize) + b.scroll_col;
        Some(b.idx(row, b.x_to_col(row, tx, self.cfg.tab_width)))
    }

    /// The identifier (char range) under a screen cell in the editor, if the
    /// pointer is really on it rather than past the end of the line.
    fn word_under_mouse(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let e = self.layout.editor;
        if !e.contains(Position::new(x, y)) || x < e.x + self.layout.gutter {
            return None;
        }
        let b = self.buf()?;
        let idx = self.editor_idx_at(x, y)?;
        if idx >= b.len() || !is_word(b.rope.char(idx)) {
            return None;
        }
        let mut end = idx;
        while end < b.len() && is_word(b.rope.char(end)) {
            end += 1;
        }
        Some((b.word_start(idx), end))
    }

    /// The pointer moved: wait for it to rest on a word before asking the
    /// server about it, and close a mouse hover once it moves elsewhere.
    fn mouse_moved(&mut self, x: u16, y: u16, alt: bool) {
        self.link_word = self.word_under_mouse(x, y).filter(|_| alt);
        if self.hover_at.is_some() && self.layout.hover.contains(Position::new(x, y)) {
            return; // reading the popup
        }
        let busy = self.menu.is_some() || self.completion.is_some() || self.drag.is_some();
        let word = self.word_under_mouse(x, y).filter(|_| !busy);
        if word.is_some()
            && (word == self.hover_word || self.hover_dwell.is_some_and(|(_, w)| Some(w) == word))
        {
            return;
        }
        if self.hover_at.is_some() {
            self.hover = None;
            self.hover_at = None;
        }
        self.hover_word = None;
        self.hover_dwell = word.map(|w| (Instant::now() + HOVER_DELAY, w));
    }

    /// The hover popup's "Go to Definition": jump to the definition of what
    /// the popup describes, leaving the cursor put so Go Back returns here.
    fn definition_from_hover(&mut self) {
        let Some(idx) = self.hover_at.or(self.buf().map(|b| b.cursor)) else {
            return;
        };
        self.hover = None;
        self.hover_at = None;
        if !self.request_at("textDocument/definition", idx, |_| Pending::Definition) {
            self.no_server();
        }
    }

    /// When `tick` next has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.hover_dwell.map(|(due, _)| due)
    }

    /// Timed work: fetch hover info once the mouse has rested long enough.
    pub fn tick(&mut self, now: Instant) {
        let Some((due, word)) = self.hover_dwell else {
            return;
        };
        if now < due {
            return;
        }
        self.hover_dwell = None;
        self.hover_word = Some(word);
        let ready = self
            .buf()
            .and_then(|b| b.lang)
            .and_then(|l| self.servers.get(l.id))
            .is_some_and(|s| s.ready);
        if ready {
            self.request_at("textDocument/hover", word.0, |_| Pending::Hover {
                mouse: Some(word),
            });
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let (x, y) = (m.column, m.row);
        let at = Position::new(x, y);
        if m.kind == MouseEventKind::Moved {
            return self.mouse_moved(x, y, m.modifiers.contains(KeyModifiers::ALT));
        }
        self.hover_dwell = None;
        self.hover_word = None;
        self.link_word = None;
        // A click anywhere dismisses the leader menu, and does nothing else.
        if self.leader.take().is_some() && matches!(m.kind, MouseEventKind::Down(_)) {
            return;
        }
        let in_tree = self.show_tree && self.layout.tree.contains(at);
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.was_armed = self.armed.take();
        }

        if self.menu.is_some() && self.menu_mouse(m) {
            return;
        }

        if let Some(scroll) = &mut self.shortcuts {
            let inside = self.layout.shortcuts.contains(at);
            match m.kind {
                MouseEventKind::ScrollUp if inside => return *scroll = scroll.saturating_sub(3),
                MouseEventKind::ScrollDown if inside => return *scroll += 3,
                MouseEventKind::Down(_) if !inside => self.shortcuts = None,
                _ if inside => return,
                _ => {}
            }
        }

        if let Some(p) = &mut self.palette {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let list = self.layout.palette_list;
                    if list.contains(at) {
                        p.sel = p.scroll + (y - list.y) as usize;
                        let action = p.accept();
                        self.palette = None;
                        if let Some(a) = action {
                            self.do_action(a);
                        }
                    } else {
                        self.palette = None;
                    }
                }
                MouseEventKind::ScrollDown => p.move_sel(1),
                MouseEventKind::ScrollUp => p.move_sel(-1),
                _ => {}
            }
            return;
        }

        if m.kind == MouseEventKind::Down(MouseButton::Left)
            && self.layout.server_badge.contains(at)
        {
            return self.restart_server();
        }
        if self.search_panel.is_some() {
            return self.search_panel_mouse(x, y, m.kind);
        }
        if self.settings_panel.is_some() {
            return self.settings_panel_mouse(x, y, m.kind);
        }

        if self.hover.is_some() && self.layout.hover.contains(at) {
            if m.kind == MouseEventKind::Down(MouseButton::Left)
                && self.layout.hover_link.contains(at)
            {
                self.definition_from_hover();
            }
            return; // clicks and scrolls on the popup stay on it
        }

        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.hover = None;
                if let Some(c) = &mut self.completion {
                    if self.layout.completion.contains(at) {
                        let row =
                            self.layout.completion_first + (y - self.layout.completion.y) as usize;
                        c.sel = row.min(c.shown.len() - 1);
                        return self.accept_completion();
                    }
                    self.completion = None;
                }
                if self.show_tree && x + 1 == self.layout.tree.right() {
                    self.drag = Some(Drag::Sidebar);
                } else if in_tree {
                    self.focus = Focus::Tree;
                    if y > self.layout.tree.y {
                        self.activate_tree_row(
                            self.tree.scroll + (y - self.layout.tree.y - 1) as usize,
                        );
                    }
                } else if y == self.layout.tabs_y {
                    if let Some(&(x0, x1, i)) =
                        self.layout.tabs.iter().find(|t| x >= t.0 && x < t.1)
                    {
                        // The last three cells of a tab (" × ") are its close button.
                        if x + 3 >= x1 && x1 - x0 > 4 {
                            self.close_tab(i)
                        } else {
                            self.switch_tab(i)
                        }
                    }
                } else if self.layout.editor.contains(at) {
                    let Some(idx) = self.editor_idx_at(x, y) else {
                        return;
                    };
                    self.focus = Focus::Editor;
                    if let Some(f) = &mut self.find {
                        f.focused = false;
                    }
                    let (t, lx, ly, n) = self.last_click;
                    let count = if t.elapsed().as_millis() < 400 && (lx, ly) == (x, y) {
                        n % 3 + 1
                    } else {
                        1
                    };
                    self.last_click = (Instant::now(), x, y, count);
                    let b = &mut self.buffers[self.active];
                    match count {
                        2 => b.select_word_at(idx),
                        3 => b.select_line_at(idx),
                        _ => {
                            b.move_to(idx, m.modifiers.contains(KeyModifiers::SHIFT));
                            self.drag = Some(Drag::Select);
                        }
                    }
                    if m.modifiers.contains(KeyModifiers::ALT) {
                        self.run(Command::GotoDefinition);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => match self.drag {
                Some(Drag::Sidebar) => {
                    self.tree_width =
                        (x + 1).clamp(12, self.layout.width.saturating_sub(20).max(12));
                }
                Some(Drag::Select) => {
                    let e = self.layout.editor;
                    // Dragging past the top or bottom edge scrolls.
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        if y < e.y {
                            b.scroll_row = b.scroll_row.saturating_sub(1);
                        } else if y >= e.bottom() {
                            b.scroll_row = (b.scroll_row + 1).min(b.lines() - 1);
                        }
                    }
                    let cy = y.clamp(e.y, e.bottom().saturating_sub(1));
                    if let Some(idx) = self.editor_idx_at(x, cy) {
                        self.buffers[self.active].move_to(idx, true);
                    }
                }
                None => {}
            },
            MouseEventKind::Down(MouseButton::Right) => self.open_context_menu(x, y),
            MouseEventKind::Up(_) => self.drag = None,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                if in_tree {
                    let max = self.tree.nodes.len().saturating_sub(1);
                    self.tree.scroll = if down {
                        (self.tree.scroll + 3).min(max)
                    } else {
                        self.tree.scroll.saturating_sub(3)
                    };
                } else if let Some(c) = self
                    .completion
                    .as_mut()
                    .filter(|_| self.layout.completion.contains(at))
                {
                    c.sel = if down {
                        (c.sel + 1).min(c.shown.len() - 1)
                    } else {
                        c.sel.saturating_sub(1)
                    };
                } else if let Some(b) = self.buffers.get_mut(self.active) {
                    self.hover = None;
                    self.completion = None;
                    b.scroll_row = if down {
                        (b.scroll_row + 3).min(b.lines() - 1)
                    } else {
                        b.scroll_row.saturating_sub(3)
                    };
                }
            }
            MouseEventKind::ScrollRight | MouseEventKind::ScrollLeft => {
                if let Some(b) = self.buffers.get_mut(self.active) {
                    b.scroll_col = if m.kind == MouseEventKind::ScrollRight {
                        b.scroll_col + 4
                    } else {
                        b.scroll_col.saturating_sub(4)
                    };
                }
            }
            _ => {}
        }
    }

    // ---- language servers ----

    fn ensure_server(&mut self) {
        let Some(id) = self.buf().and_then(|b| b.lang).map(|l| l.id) else {
            return;
        };
        if self.servers.contains_key(id) || self.failed_servers.contains_key(id) {
            return;
        }
        // No server configured for this language isn't a problem to report.
        let Some(cmd) = self.cfg.lsp.get(id).filter(|c| !c.is_empty()).cloned() else {
            return;
        };
        match Server::spawn(id, &cmd, &self.root, self.tx.clone()) {
            Ok(s) => {
                self.servers.insert(id.into(), s);
            }
            Err(e) => {
                let why = if e.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "{} isn't installed — install it for {id} completions and diagnostics",
                        cmd[0]
                    )
                } else {
                    format!("{} couldn't start: {e}", cmd[0])
                };
                self.set_status(why.clone());
                self.failed_servers.insert(id.into(), (cmd[0].clone(), why));
            }
        }
    }

    /// Stop the active file's language server if it's running, forget any
    /// earlier failure, and start it again.
    fn restart_server(&mut self) {
        let Some(id) = self.buf().and_then(|b| b.lang).map(|l| l.id) else {
            return self.no_server();
        };
        self.failed_servers.remove(id);
        if let Some(mut srv) = self.servers.remove(id) {
            srv.shutdown();
        }
        for b in &mut self.buffers {
            if b.lang.is_some_and(|l| l.id == id) {
                b.lsp_version = None;
            }
        }
        self.ensure_server();
        match self.servers.get(id) {
            Some(srv) => self.set_status(format!("Starting {}…", srv.name)),
            None if !self.failed_servers.contains_key(id) => self.no_server(),
            None => {}
        }
    }

    /// Ask the language server for completions if it's up; otherwise (or if
    /// `explicit` and it has none) offer words from the open files.
    fn complete_here(&mut self, explicit: bool) {
        let ready = self
            .buf()
            .and_then(|b| b.lang)
            .and_then(|l| self.servers.get(l.id))
            .is_some_and(|s| s.ready);
        if ready && self.request_completion() {
            return;
        }
        self.word_completion(explicit);
        if explicit && self.completion.is_none() {
            self.set_status("No suggestions");
        }
    }

    /// Suggestions made of the words in the open files, for when there's no
    /// language server to ask. Unless `explicit`, needs a typed prefix.
    fn word_completion(&mut self, explicit: bool) {
        let Some(b) = self.buffers.get(self.active) else {
            return;
        };
        let start = b.word_start(b.cursor);
        if start == b.cursor && !explicit {
            return self.completion = None;
        }
        let mut seen = HashSet::new();
        let mut items = vec![];
        // The active file first, so its words win ties.
        let order = std::iter::once(self.active)
            .chain((0..self.buffers.len()).filter(|i| *i != self.active));
        'files: for i in order {
            let buf = &self.buffers[i];
            if buf.len() > 2_000_000 {
                continue;
            }
            let mut word = String::new();
            let mut word_at = 0;
            for (idx, ch) in buf.rope.chars().chain(std::iter::once(' ')).enumerate() {
                if is_word(ch) {
                    if word.is_empty() {
                        word_at = idx;
                    }
                    word.push(ch);
                    continue;
                }
                // Skip the word being typed itself, short words and numbers.
                let typing = i == self.active && word_at == start;
                if !typing
                    && word.chars().count() >= 3
                    && !word.starts_with(|c: char| c.is_ascii_digit())
                    && seen.insert(word.clone())
                {
                    items.push(CompletionItem {
                        label: word.clone(),
                        detail: String::new(),
                        filter: word.clone(),
                        sort: format!("{:06}", items.len()),
                        insert: word.clone(),
                        edit_start: None,
                    });
                    if items.len() >= 5000 {
                        break 'files;
                    }
                }
                word.clear();
            }
        }
        if items.is_empty() {
            return self.completion = None;
        }
        self.completion = Some(Completion {
            items,
            shown: vec![],
            sel: 0,
            start,
        });
        self.filter_completion();
    }

    /// Tell the server about the active buffer's current text.
    fn sync_lsp(&mut self) {
        let Some(b) = self.buffers.get_mut(self.active) else {
            return;
        };
        let (Some(lang), Some(path)) = (b.lang, &b.path) else {
            return;
        };
        let Some(srv) = self.servers.get_mut(lang.id) else {
            return;
        };
        let uri = lsp::path_to_uri(path);
        match b.lsp_version {
            None => srv.notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": uri, "languageId": lang.id, "version": b.version, "text": b.rope.to_string() } }),
            ),
            Some(v) if v != b.version => srv.notify(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": uri, "version": b.version }, "contentChanges": [{ "text": b.rope.to_string() }] }),
            ),
            _ => return,
        }
        b.lsp_version = Some(b.version);
    }

    /// Ask for completions, remembering where the word being completed
    /// starts so a reply that arrives after the cursor has left it is dropped.
    fn request_completion(&mut self) -> bool {
        let Some(start) = self.buf().map(|b| b.word_start(b.cursor)) else {
            return false;
        };
        self.request_at_cursor("textDocument/completion", |path| Pending::Completion {
            path,
            start,
        })
    }

    fn request_at_cursor(
        &mut self,
        method: &str,
        pending: impl FnOnce(PathBuf) -> Pending,
    ) -> bool {
        let Some(cursor) = self.buf().map(|b| b.cursor) else {
            return false;
        };
        self.request_at(method, cursor, pending)
    }

    /// Send a position request about char index `idx` in the active buffer.
    fn request_at(
        &mut self,
        method: &str,
        idx: usize,
        pending: impl FnOnce(PathBuf) -> Pending,
    ) -> bool {
        self.sync_lsp();
        let Some(b) = self.buffers.get(self.active) else {
            return false;
        };
        let (Some(lang), Some(path)) = (b.lang, b.path.clone()) else {
            return false;
        };
        let Some(srv) = self.servers.get_mut(lang.id) else {
            return false;
        };
        let params = json!({ "textDocument": { "uri": lsp::path_to_uri(&path) }, "position": lsp_pos(b, idx) });
        srv.request(method, params, pending(path));
        true
    }

    fn request_format(&mut self, save: bool) -> bool {
        self.sync_lsp();
        let Some(b) = self.buffers.get(self.active) else {
            return false;
        };
        let (Some(lang), Some(path)) = (b.lang, b.path.clone()) else {
            return false;
        };
        let Some(srv) = self.servers.get_mut(lang.id).filter(|s| s.ready) else {
            return false;
        };
        let params = json!({
            "textDocument": { "uri": lsp::path_to_uri(&path) },
            "options": { "tabSize": self.cfg.tab_width, "insertSpaces": b.indent != "\t" }
        });
        srv.request(
            "textDocument/formatting",
            params,
            Pending::Format {
                path,
                version: b.version,
                save,
            },
        );
        true
    }

    fn on_lsp(&mut self, key: &str, msg: Value) {
        let Some(srv) = self.servers.get_mut(key) else {
            return;
        };
        if let Some(method) = msg["method"].as_str() {
            if let Some(id) = msg.get("id") {
                // A request from the server; answer so it doesn't wait on us.
                let result = match method {
                    "workspace/configuration" => {
                        Value::Array(vec![
                            Value::Null;
                            msg["params"]["items"].as_array().map_or(0, Vec::len)
                        ])
                    }
                    _ => Value::Null,
                };
                return srv.respond(id, result);
            }
            let params = &msg["params"];
            match method {
                "textDocument/publishDiagnostics" => {
                    let Some(path) = params["uri"].as_str().and_then(lsp::uri_to_path) else {
                        return;
                    };
                    let diags = params["diagnostics"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|d| {
                                    Some(Diag {
                                        start: pos_of(&d["range"]["start"])?,
                                        end: pos_of(&d["range"]["end"])?,
                                        severity: d["severity"].as_u64().unwrap_or(1) as u8,
                                        message: d["message"].as_str().unwrap_or("").to_string(),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.diagnostics.insert(path, diags);
                }
                "window/showMessage" => {
                    if let Some(m) = params["message"].as_str() {
                        self.status = m.lines().next().unwrap_or("").to_string();
                    }
                }
                "$/progress" => match params["value"]["kind"].as_str() {
                    Some("begin") => {
                        srv.progress = params["value"]["title"].as_str().map(str::to_string)
                    }
                    Some("end") => srv.progress = None,
                    _ => {}
                },
                _ => {}
            }
            return;
        }
        let Some(pending) = msg["id"].as_i64().and_then(|id| srv.pending.remove(&id)) else {
            return;
        };
        let result = &msg["result"];
        match pending {
            Pending::Initialize => srv.initialized(result),
            Pending::Completion { path, start } => self.on_completion(&path, start, result),
            Pending::Hover { mouse } => {
                // A mouse hover only shows if the pointer is still on that word.
                if mouse.is_some() && mouse != self.hover_word {
                    return;
                }
                let text = hover_text(&result["contents"]);
                let text: Vec<&str> = text
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("```"))
                    .collect();
                let text = text.join("\n").trim().to_string();
                if text.is_empty() {
                    if mouse.is_none() {
                        self.set_status("No hover information here");
                    }
                } else {
                    self.hover = Some(text);
                    self.hover_at = mouse.map(|w| w.0);
                }
            }
            Pending::Definition => {
                let loc = if result.is_array() {
                    &result[0]
                } else {
                    result
                };
                let uri = loc["uri"].as_str().or(loc["targetUri"].as_str());
                let range = if loc["range"].is_object() {
                    &loc["range"]
                } else {
                    &loc["targetSelectionRange"]
                };
                match (uri.and_then(lsp::uri_to_path), pos_of(&range["start"])) {
                    (Some(path), Some((line, ch))) => self.jump_to(&path, line, ch),
                    _ => self.set_status("No definition found"),
                }
            }
            Pending::Format {
                path,
                version,
                save,
            } => {
                let Some(i) = self
                    .buffers
                    .iter()
                    .position(|b| b.path.as_deref() == Some(&path))
                else {
                    return;
                };
                let b = &mut self.buffers[i];
                if b.version != version {
                    return self.set_status("File changed while formatting — skipped");
                }
                let mut edits: Vec<(usize, usize, String)> = result
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| {
                                let (sl, sc) = pos_of(&e["range"]["start"])?;
                                let (el, ec) = pos_of(&e["range"]["end"])?;
                                Some((
                                    from_lsp_pos(b, sl, sc),
                                    from_lsp_pos(b, el, ec),
                                    e["newText"].as_str()?.to_string(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                edits.sort_by(|a, b| b.0.cmp(&a.0));
                let (line, col) = b.pos(b.cursor);
                let changed = !edits.is_empty();
                if changed {
                    b.transaction(|b| {
                        for (s, e, text) in &edits {
                            b.edit(*s, *e, text);
                        }
                    });
                    let idx = b.idx(line, col);
                    b.move_to(idx, false);
                }
                if save && i == self.active {
                    self.write_active();
                } else {
                    self.set_status(if changed {
                        "Formatted"
                    } else {
                        "Already formatted"
                    });
                }
            }
        }
    }

    fn jump_to(&mut self, path: &Path, line: usize, character: usize) {
        if let Some(b) = self.buf() {
            if let Some(p) = &b.path {
                self.jumps.push((p.clone(), b.cursor));
            }
        }
        if self.open_path(path) {
            let rows = self.layout.editor.height as usize;
            if let Some(b) = self.buf_mut() {
                let idx = from_lsp_pos(b, line, character);
                b.move_to(idx, false);
                if line < b.scroll_row || line >= b.scroll_row + rows {
                    b.scroll_row = line.saturating_sub(rows / 3);
                }
            }
        }
    }

    // ---- completion ----

    fn on_completion(&mut self, path: &Path, start: usize, result: &Value) {
        let Some(b) = self.buffers.get(self.active) else {
            return;
        };
        let moved_on = b.word_start(b.cursor) != start || b.cursor < start;
        if b.path.as_deref() != Some(path) || self.palette.is_some() || moved_on {
            return;
        }
        let Some(raw) = result
            .get("items")
            .and_then(Value::as_array)
            .or(result.as_array())
        else {
            return self.completion = None;
        };
        let items: Vec<CompletionItem> = raw
            .iter()
            .take(3000)
            .filter_map(|it| {
                let label = it["label"].as_str()?.to_string();
                let te = &it["textEdit"];
                let mut insert = te["newText"]
                    .as_str()
                    .or(it["insertText"].as_str())
                    .unwrap_or(&label)
                    .to_string();
                if it["insertTextFormat"] == 2 {
                    insert = strip_snippet(&insert);
                }
                let range = if te["range"].is_object() {
                    &te["range"]
                } else {
                    &te["replace"]
                };
                Some(CompletionItem {
                    detail: it["detail"]
                        .as_str()
                        .or(it["labelDetails"]["description"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    filter: it["filterText"].as_str().unwrap_or(&label).to_string(),
                    sort: it["sortText"].as_str().unwrap_or(&label).to_string(),
                    edit_start: pos_of(&range["start"]),
                    insert,
                    label,
                })
            })
            .collect();
        if items.is_empty() {
            return self.word_completion(false);
        }
        let start = b.word_start(b.cursor);
        self.completion = Some(Completion {
            items,
            shown: vec![],
            sel: 0,
            start,
        });
        self.filter_completion();
    }

    fn filter_completion(&mut self) {
        let (Some(c), Some(b)) = (&mut self.completion, self.buffers.get(self.active)) else {
            return;
        };
        if b.cursor < c.start {
            return self.completion = None;
        }
        let prefix = b.rope.slice(c.start..b.cursor).to_string();
        if !prefix.chars().all(is_word) {
            return self.completion = None;
        }
        let lower = prefix.to_lowercase();
        let mut scored: Vec<(i32, usize)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let mut s = palette::fuzzy(&prefix, &it.filter)?;
                if it.filter.starts_with(&prefix) {
                    s += 100;
                } else if it.filter.to_lowercase().starts_with(&lower) {
                    s += 60;
                }
                Some((if prefix.is_empty() { 0 } else { s }, i))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| c.items[a.1].sort.cmp(&c.items[b.1].sort))
        });
        c.shown = scored.into_iter().take(200).map(|(_, i)| i).collect();
        c.sel = 0;
        // Nothing to offer, or the only suggestion is exactly what's typed.
        if c.shown.is_empty() || (c.shown.len() == 1 && c.items[c.shown[0]].insert == prefix) {
            self.completion = None;
        }
    }

    fn accept_completion(&mut self) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let (Some(&i), Some(b)) = (c.shown.get(c.sel), self.buffers.get_mut(self.active)) else {
            return;
        };
        let item = &c.items[i];
        let start = item
            .edit_start
            .map(|(l, ch)| from_lsp_pos(b, l, ch))
            .filter(|s| *s <= b.cursor)
            .unwrap_or(c.start);
        let end = b.cursor;
        b.edit(start, end, &item.insert);
        self.follow = true;
    }

    pub fn shutdown(&mut self) {
        for srv in self.servers.values_mut() {
            srv.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_are_flattened_to_plain_text() {
        assert_eq!(strip_snippet("println!(\"$1\")$0"), "println!(\"\")");
        assert_eq!(
            strip_snippet("fn ${1:name}(${2:args}) {}"),
            "fn name(args) {}"
        );
        assert_eq!(strip_snippet("cost \\$5"), "cost $5");
    }

    #[test]
    fn smart_case_search() {
        let b = Buffer::from_text("Foo foo FOO", None);
        assert_eq!(search(&b, "foo").len(), 3);
        assert_eq!(search(&b, "Foo"), vec![(0, 3)]);
    }

    #[test]
    fn lsp_positions_count_utf16_units() {
        let b = Buffer::from_text("a😀b\nxy", None);
        assert_eq!(lsp_pos(&b, 2), json!({ "line": 0, "character": 3 }));
        assert_eq!(from_lsp_pos(&b, 0, 3), 2);
        assert_eq!(from_lsp_pos(&b, 1, 99), 6);
        assert_eq!(from_lsp_pos(&b, 9, 0), 6);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b"hi"), "aGk=");
        assert_eq!(base64(b"hey"), "aGV5");
        assert_eq!(base64(b"h"), "aA==");
    }
}
