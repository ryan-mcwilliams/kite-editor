//! The right-click context menu: what it can contain, and which entries to
//! offer for each place you can click (a tree row, the empty tree area, the
//! editor). Drawing is in ui.rs and the actions are run by app.rs.

use crate::commands::{Command, Keymap};
use crate::fileops::reveal_label;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    Cmd(Command),
    OpenFile(PathBuf),
    NewFileIn(PathBuf),
    NewFolderIn(PathBuf),
    Rename(PathBuf),
    Duplicate(PathBuf),
    /// Ask for confirmation first (opens a second, two-item menu).
    AskDelete(PathBuf),
    Delete(PathBuf),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    Reveal(PathBuf),
    Cancel,
}

pub enum MenuEntry {
    Item {
        label: String,
        /// Right-aligned key binding, if the action has one.
        hint: String,
        action: MenuAction,
        danger: bool,
    },
    Separator,
}

pub struct ContextMenu {
    pub entries: Vec<MenuEntry>,
    pub sel: Option<usize>,
    /// Where the click happened; the menu is placed here, clamped to the screen.
    pub anchor: (u16, u16),
}

fn item(label: &str, action: MenuAction) -> MenuEntry {
    MenuEntry::Item {
        label: label.to_string(),
        hint: String::new(),
        action,
        danger: false,
    }
}

fn cmd(keys: &Keymap, label: &str, c: Command) -> MenuEntry {
    MenuEntry::Item {
        label: label.to_string(),
        hint: keys.binding(c).unwrap_or_default(),
        action: MenuAction::Cmd(c),
        danger: false,
    }
}

impl ContextMenu {
    fn new(entries: Vec<MenuEntry>, anchor: (u16, u16)) -> ContextMenu {
        ContextMenu {
            entries,
            sel: None,
            anchor,
        }
    }

    /// A file or folder row in the sidebar.
    pub fn for_tree_node(path: &Path, is_dir: bool, anchor: (u16, u16)) -> ContextMenu {
        let p = path.to_path_buf();
        let target_dir = if is_dir {
            p.clone()
        } else {
            p.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        let mut e = vec![];
        if !is_dir {
            e.push(item("Open", MenuAction::OpenFile(p.clone())));
            e.push(MenuEntry::Separator);
        }
        e.push(item("New File…", MenuAction::NewFileIn(target_dir.clone())));
        e.push(item("New Folder…", MenuAction::NewFolderIn(target_dir)));
        e.push(MenuEntry::Separator);
        e.push(item("Rename…", MenuAction::Rename(p.clone())));
        e.push(item("Duplicate", MenuAction::Duplicate(p.clone())));
        e.push(item(
            if is_dir {
                "Delete Folder…"
            } else {
                "Delete File…"
            },
            MenuAction::AskDelete(p.clone()),
        ));
        e.push(MenuEntry::Separator);
        e.push(item("Copy Path", MenuAction::CopyPath(p.clone())));
        e.push(item(
            "Copy Relative Path",
            MenuAction::CopyRelativePath(p.clone()),
        ));
        e.push(item(reveal_label(), MenuAction::Reveal(p)));
        ContextMenu::new(e, anchor)
    }

    /// Empty space in the sidebar: acts on the project root.
    pub fn for_tree_blank(root: &Path, keys: &Keymap, anchor: (u16, u16)) -> ContextMenu {
        let r = root.to_path_buf();
        ContextMenu::new(
            vec![
                item("New File…", MenuAction::NewFileIn(r.clone())),
                item("New Folder…", MenuAction::NewFolderIn(r.clone())),
                MenuEntry::Separator,
                cmd(keys, "Refresh File Tree", Command::RefreshTree),
                item(reveal_label(), MenuAction::Reveal(r)),
            ],
            anchor,
        )
    }

    /// Code in the editor. `path` is the open file, if it has one on disk.
    pub fn for_editor(path: Option<&Path>, keys: &Keymap, anchor: (u16, u16)) -> ContextMenu {
        let mut e = vec![
            cmd(keys, "Go to Definition", Command::GotoDefinition),
            cmd(keys, "Show Hover Info", Command::Hover),
            cmd(keys, "Format Document", Command::Format),
            MenuEntry::Separator,
            cmd(keys, "Cut", Command::Cut),
            cmd(keys, "Copy", Command::Copy),
            cmd(keys, "Paste", Command::Paste),
            cmd(keys, "Select All", Command::SelectAll),
            MenuEntry::Separator,
            cmd(keys, "Toggle Comment", Command::ToggleComment),
        ];
        if let Some(p) = path {
            e.push(MenuEntry::Separator);
            e.push(item("Copy Path", MenuAction::CopyPath(p.to_path_buf())));
            e.push(item(
                "Copy Relative Path",
                MenuAction::CopyRelativePath(p.to_path_buf()),
            ));
            e.push(item(reveal_label(), MenuAction::Reveal(p.to_path_buf())));
        }
        ContextMenu::new(e, anchor)
    }

    /// The second step of Delete. "Cancel" is preselected so a stray Enter
    /// doesn't destroy anything.
    pub fn confirm_delete(path: &Path, anchor: (u16, u16)) -> ContextMenu {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut m = ContextMenu::new(
            vec![
                MenuEntry::Item {
                    label: format!("Delete '{name}' permanently"),
                    hint: String::new(),
                    action: MenuAction::Delete(path.to_path_buf()),
                    danger: true,
                },
                item("Cancel", MenuAction::Cancel),
            ],
            anchor,
        );
        m.sel = Some(1);
        m
    }

    pub fn is_selectable(&self, i: usize) -> bool {
        matches!(self.entries.get(i), Some(MenuEntry::Item { .. }))
    }

    /// Move the highlight, skipping separators and wrapping around.
    pub fn move_sel(&mut self, delta: isize) {
        let n = self.entries.len() as isize;
        if n == 0 {
            return;
        }
        let mut i = match self.sel {
            Some(s) => s as isize,
            None if delta > 0 => -1,
            None => n,
        };
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if self.is_selectable(i as usize) {
                self.sel = Some(i as usize);
                return;
            }
        }
    }

    pub fn selected_action(&self) -> Option<MenuAction> {
        match self.entries.get(self.sel?) {
            Some(MenuEntry::Item { action, .. }) => Some(action.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Keymap {
        Keymap::new(&Default::default(), &Default::default()).0
    }

    fn labels(m: &ContextMenu) -> Vec<&str> {
        m.entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn file_and_folder_menus_differ() {
        let file = ContextMenu::for_tree_node(Path::new("/p/src/a.rs"), false, (0, 0));
        let dir = ContextMenu::for_tree_node(Path::new("/p/src"), true, (0, 0));
        assert!(labels(&file).contains(&"Open"));
        assert!(labels(&file).contains(&"Delete File…"));
        assert!(!labels(&dir).contains(&"Open"));
        assert!(labels(&dir).contains(&"Delete Folder…"));
        // New File on a file targets its folder; on a folder, the folder itself.
        let new_in = |m: &ContextMenu| {
            m.entries.iter().find_map(|e| match e {
                MenuEntry::Item {
                    action: MenuAction::NewFileIn(d),
                    ..
                } => Some(d.clone()),
                _ => None,
            })
        };
        assert_eq!(new_in(&file), Some(PathBuf::from("/p/src")));
        assert_eq!(new_in(&dir), Some(PathBuf::from("/p/src")));
    }

    #[test]
    fn editor_menu_shows_key_hints_and_file_actions_only_with_a_path() {
        let k = keys();
        let with = ContextMenu::for_editor(Some(Path::new("/p/a.rs")), &k, (0, 0));
        let without = ContextMenu::for_editor(None, &k, (0, 0));
        assert!(labels(&with).contains(&"Copy Path"));
        assert!(!labels(&without).contains(&"Copy Path"));
        let hint = with.entries.iter().find_map(|e| match e {
            MenuEntry::Item { label, hint, .. } if label == "Go to Definition" => Some(hint),
            _ => None,
        });
        assert_eq!(hint.map(String::as_str), Some("F12"));
    }

    #[test]
    fn selection_skips_separators_and_wraps() {
        let mut m = ContextMenu::for_tree_blank(Path::new("/p"), &keys(), (0, 0));
        assert_eq!(m.sel, None);
        m.move_sel(1);
        assert_eq!(m.sel, Some(0));
        m.move_sel(1);
        m.move_sel(1); // index 2 is a separator, so this lands on 3
        assert_eq!(m.sel, Some(3));
        m.move_sel(-1);
        m.move_sel(-1);
        m.move_sel(-1); // wraps past the top to the last item
        assert_eq!(m.sel, Some(4));
        let mut empty = ContextMenu::for_tree_blank(Path::new("/p"), &keys(), (0, 0));
        empty.move_sel(-1);
        assert_eq!(empty.sel, Some(4));
    }

    #[test]
    fn delete_confirmation_preselects_cancel() {
        let m = ContextMenu::confirm_delete(Path::new("/p/a.rs"), (3, 4));
        assert_eq!(m.selected_action(), Some(MenuAction::Cancel));
        assert!(labels(&m)[0].contains("'a.rs'"));
    }
}
