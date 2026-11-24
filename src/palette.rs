//! The palette: one popup that is the file finder, the command palette
//! (input starting with `>`), go-to-line (`:`), and a generic text prompt.

use crate::commands::{Command, Keymap};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prompt {
    NewFile,
    NewFolder,
    Rename,
    SaveAs,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Open(PathBuf),
    Run(Command),
    Line(usize),
    Prompt(Prompt, String),
}

pub struct Item {
    pub label: String,
    pub detail: String,
    pub action: Action,
}

pub struct Palette {
    pub input: String,
    pub prompt: Option<Prompt>,
    pub items: Vec<Item>,
    pub sel: usize,
    pub scroll: usize,
    files: Vec<String>,
    commands: Vec<(Command, String)>,
    root: PathBuf,
}

pub const MAX_ROWS: usize = 12;

/// Subsequence fuzzy match. Higher is better; None means no match.
/// Rewards consecutive characters and matches at word starts.
pub fn fuzzy(pattern: &str, text: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let mut score = 0;
    let mut ti = 0;
    let mut last: Option<usize> = None;
    for pc in pattern.chars().flat_map(char::to_lowercase) {
        let mut found = None;
        while ti < t.len() {
            if t[ti].to_lowercase().next() == Some(pc) {
                found = Some(ti);
                break;
            }
            ti += 1;
        }
        let i = found?;
        let boundary = i == 0
            || !t[i - 1].is_alphanumeric()
            || (t[i].is_uppercase() && t[i - 1].is_lowercase());
        score += 1;
        if last.is_some_and(|l| l + 1 == i) {
            score += 6;
        }
        if boundary {
            score += 8;
        }
        last = Some(i);
        ti = i + 1;
    }
    Some(score - (t.len() as i32) / 8)
}

/// Score a path, strongly preferring matches within the file name.
fn fuzzy_path(pattern: &str, path: &str) -> Option<i32> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match fuzzy(pattern, name) {
        Some(s) => Some(s + 40),
        None => fuzzy(pattern, path),
    }
}

pub fn list_files(root: &Path) -> Vec<String> {
    // require_git(false): honor .gitignore even in a plain folder that
    // hasn't been `git init`'d yet.
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut files: Vec<String> = walker
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| {
            Some(
                e.path()
                    .strip_prefix(root)
                    .ok()?
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .take(50_000)
        .collect();
    files.sort();
    files
}

impl Palette {
    pub fn new(root: &Path, keymap: &Keymap, input: &str) -> Palette {
        let commands = Command::ALL
            .iter()
            .filter(|c| **c != Command::CommandPalette)
            .map(|c| (*c, keymap.binding(*c).unwrap_or_default()))
            .collect();
        let files = if input.is_empty() {
            list_files(root)
        } else {
            vec![]
        };
        let mut p = Palette {
            input: input.into(),
            prompt: None,
            items: vec![],
            sel: 0,
            scroll: 0,
            files,
            commands,
            root: root.into(),
        };
        p.refresh();
        p
    }

    pub fn prompt(root: &Path, kind: Prompt, initial: &str) -> Palette {
        Palette {
            input: initial.into(),
            prompt: Some(kind),
            items: vec![],
            sel: 0,
            scroll: 0,
            files: vec![],
            commands: vec![],
            root: root.into(),
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match self.prompt {
            Some(Prompt::NewFile) => "New file path, relative to the project",
            Some(Prompt::NewFolder) => "New folder path, relative to the project",
            Some(Prompt::Rename) => "New name or path, relative to the project",
            Some(Prompt::SaveAs) => "Save as (path relative to the project)",
            None => "Go to file…   > commands   : line",
        }
    }

    pub fn refresh(&mut self) {
        self.sel = 0;
        self.scroll = 0;
        self.items.clear();
        if self.prompt.is_some() || self.input.starts_with(':') {
            return;
        }
        if let Some(q) = self.input.strip_prefix('>') {
            let q = q.trim();
            let mut scored: Vec<(i32, usize)> = self
                .commands
                .iter()
                .enumerate()
                .filter_map(|(i, (c, _))| Some((fuzzy(q, c.title())?, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.items = scored
                .into_iter()
                .map(|(_, i)| {
                    let (c, key) = &self.commands[i];
                    Item {
                        label: c.title().into(),
                        detail: key.clone(),
                        action: Action::Run(*c),
                    }
                })
                .collect();
            return;
        }
        if self.files.is_empty() {
            self.files = list_files(&self.root);
        }
        let q = self.input.trim();
        let mut scored: Vec<(i32, &String)> = self
            .files
            .iter()
            .filter_map(|f| Some((fuzzy_path(q, f)?, f)))
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(a.1.len().cmp(&b.1.len()))
                .then(a.1.cmp(b.1))
        });
        self.items = scored
            .into_iter()
            .take(200)
            .map(|(_, f)| {
                let (dir, name) = f.rsplit_once('/').unwrap_or(("", f.as_str()));
                Item {
                    label: name.into(),
                    detail: dir.into(),
                    action: Action::Open(self.root.join(f)),
                }
            })
            .collect();
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + MAX_ROWS {
            self.scroll = self.sel + 1 - MAX_ROWS;
        }
    }

    /// What pressing Enter should do right now.
    pub fn accept(&self) -> Option<Action> {
        if let Some(kind) = self.prompt {
            let text = self.input.trim();
            return (!text.is_empty()).then(|| Action::Prompt(kind, text.to_string()));
        }
        if let Some(n) = self.input.strip_prefix(':') {
            return n.trim().parse().ok().map(Action::Line);
        }
        self.items.get(self.sel).map(|i| i.action.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_word_starts_and_runs() {
        assert!(fuzzy("xyz", "main.rs").is_none());
        assert!(fuzzy("mr", "main.rs").is_some());
        assert!(fuzzy("buf", "buffer.rs") > fuzzy("buf", "subfolder_ui.rs"));
        assert!(fuzzy_path("main", "src/main.rs") > fuzzy_path("main", "domain/x.rs"));
        assert!(fuzzy("GD", "Code: Go to Definition").is_some());
    }

    #[test]
    fn modes_follow_the_input_prefix() {
        let (km, _) = Keymap::new(&Default::default(), &Default::default());
        let mut p = Palette::new(Path::new("/nonexistent-kite"), &km, ">");
        assert_eq!(p.items.len(), Command::ALL.len() - 1);
        p.input = ">save".into();
        p.refresh();
        assert_eq!(p.accept(), Some(Action::Run(Command::Save)));
        assert_eq!(p.items[0].detail, "Ctrl+S");
        p.input = ":42".into();
        p.refresh();
        assert_eq!(p.accept(), Some(Action::Line(42)));
    }
}
