//! The file tree in the sidebar: a flat list of the currently visible rows.
//! Directories are read lazily when expanded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

pub struct Tree {
    pub root: PathBuf,
    pub nodes: Vec<Node>,
    pub sel: usize,
    pub scroll: usize,
}

const HIDDEN: &[&str] = &[".git", ".DS_Store"];

fn read_dir(dir: &Path, depth: usize) -> Vec<Node> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut nodes: Vec<Node> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let is_dir = e.path().is_dir();
            Some(Node {
                path: e.path(),
                name,
                is_dir,
                depth,
                expanded: false,
            })
        })
        .collect();
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

impl Tree {
    pub fn new(root: PathBuf) -> Tree {
        let nodes = read_dir(&root, 0);
        Tree {
            root,
            nodes,
            sel: 0,
            scroll: 0,
        }
    }

    fn expand(&mut self, i: usize) {
        if self.nodes[i].is_dir && !self.nodes[i].expanded {
            self.nodes[i].expanded = true;
            let children = read_dir(&self.nodes[i].path, self.nodes[i].depth + 1);
            self.nodes.splice(i + 1..i + 1, children);
        }
    }

    fn collapse(&mut self, i: usize) {
        if self.nodes[i].expanded {
            self.nodes[i].expanded = false;
            let depth = self.nodes[i].depth;
            let end = self.nodes[i + 1..]
                .iter()
                .position(|n| n.depth <= depth)
                .map_or(self.nodes.len(), |p| i + 1 + p);
            self.nodes.drain(i + 1..end);
        }
    }

    pub fn toggle(&mut self, i: usize) {
        if i < self.nodes.len() {
            if self.nodes[i].expanded {
                self.collapse(i)
            } else {
                self.expand(i)
            }
        }
    }

    /// Left arrow: collapse the directory, or jump to the parent.
    pub fn collapse_or_parent(&mut self) {
        let Some(n) = self.nodes.get(self.sel) else {
            return;
        };
        if n.expanded {
            self.collapse(self.sel);
        } else if n.depth > 0 {
            let depth = n.depth;
            if let Some(p) = self.nodes[..self.sel].iter().rposition(|n| n.depth < depth) {
                self.sel = p;
            }
        }
    }

    pub fn expand_selected(&mut self) {
        if self.sel < self.nodes.len() {
            self.expand(self.sel);
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        if !self.nodes.is_empty() {
            self.sel = (self.sel as isize + delta).clamp(0, self.nodes.len() as isize - 1) as usize;
        }
    }

    /// Re-read from disk, keeping expanded directories open.
    pub fn refresh(&mut self) {
        let open: HashSet<PathBuf> = self
            .nodes
            .iter()
            .filter(|n| n.expanded)
            .map(|n| n.path.clone())
            .collect();
        let selected = self.nodes.get(self.sel).map(|n| n.path.clone());
        self.nodes = read_dir(&self.root, 0);
        let mut i = 0;
        while i < self.nodes.len() {
            if open.contains(&self.nodes[i].path) {
                self.expand(i);
            }
            i += 1;
        }
        self.sel = selected
            .and_then(|p| self.nodes.iter().position(|n| n.path == p))
            .unwrap_or(0);
    }

    /// Expand the ancestors of `path` and select it.
    pub fn reveal(&mut self, path: &Path) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut cur = self.root.clone();
        for part in rel.components() {
            cur.push(part);
            let Some(i) = self.nodes.iter().position(|n| n.path == cur) else {
                return;
            };
            if cur == path {
                self.sel = i;
            } else {
                self.expand(i);
            }
        }
    }

    /// Keep the selection inside a viewport of `height` rows.
    pub fn scroll_into_view(&mut self, height: usize) {
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if height > 0 && self.sel >= self.scroll + height {
            self.scroll = self.sel + 1 - height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_collapse_reveal_refresh() {
        let dir = std::env::temp_dir().join(format!("kite-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("src/deep/x.rs"), "").unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        std::fs::write(dir.join("a.txt"), "").unwrap();

        let mut t = Tree::new(dir.clone());
        let names = |t: &Tree| t.nodes.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&t), ["src", "a.txt"]);
        t.reveal(&dir.join("src/deep/x.rs"));
        assert_eq!(names(&t), ["src", "deep", "x.rs", "main.rs", "a.txt"]);
        assert_eq!(t.nodes[t.sel].name, "x.rs");
        std::fs::write(dir.join("src/new.rs"), "").unwrap();
        t.refresh();
        assert_eq!(
            names(&t),
            ["src", "deep", "x.rs", "main.rs", "new.rs", "a.txt"]
        );
        assert_eq!(t.nodes[t.sel].name, "x.rs");
        t.toggle(0);
        assert_eq!(names(&t), ["src", "a.txt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
