//! Project-wide text search: walk the project (respecting .gitignore, same
//! as the file finder) and collect per-line matches. Deliberately simple
//! and synchronous — capped so a big repo can't make it hang.

use std::path::{Path, PathBuf};

pub struct Hit {
    pub path: PathBuf,
    /// 0-based line and char-column range within that line.
    pub line: usize,
    pub col: usize,
    pub end_col: usize,
    pub text: String,
}

const MAX_FILES: usize = 20_000;
const MAX_HITS: usize = 2_000;
const MAX_PER_FILE: usize = 200;
const MAX_FILE_BYTES: u64 = 4_000_000;

/// Smart case: an all-lowercase query matches case-insensitively.
pub fn search(root: &Path, query: &str) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }
    let fold = !query.chars().any(char::is_uppercase);
    let norm = |c: char| {
        if fold {
            c.to_lowercase().next().unwrap_or(c)
        } else {
            c
        }
    };
    let q: Vec<char> = query.chars().map(norm).collect();

    // require_git(false): honor .gitignore even in a plain folder that
    // hasn't been `git init`'d yet.
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut hits = vec![];
    let mut scanned = 0;
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        scanned += 1;
        if scanned > MAX_FILES || hits.len() >= MAX_HITS {
            break;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue; // binary file
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut in_file = 0;
        'lines: for (line_no, line) in text.lines().enumerate() {
            let chars: Vec<char> = line.chars().map(norm).collect();
            let mut i = 0;
            while i + q.len() <= chars.len() {
                if chars[i..i + q.len()] == q[..] {
                    hits.push(Hit {
                        path: entry.path().to_path_buf(),
                        line: line_no,
                        col: i,
                        end_col: i + q.len(),
                        text: line.chars().take(500).collect(),
                    });
                    i += q.len();
                    in_file += 1;
                    if in_file >= MAX_PER_FILE || hits.len() >= MAX_HITS {
                        break 'lines;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp project unique to this process and test, since tests run in
    /// parallel and would otherwise clobber each other's files.
    fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kite-search-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, content) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn finds_matches_across_files_with_smart_case() {
        let dir = project(
            "smart_case",
            &[
                ("a.rs", "fn foo() {}\nlet Foo = 1;"),
                ("sub/b.rs", "// foo again"),
            ],
        );
        let hits = search(&dir, "foo");
        assert_eq!(hits.len(), 3);
        assert_eq!((hits[0].line, hits[0].col, hits[0].end_col), (0, 3, 6));
        let hits = search(&dir, "Foo");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, dir.join("a.rs"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skips_binary_files_and_respects_gitignore() {
        let dir = project(
            "gitignore",
            &[("keep.txt", "needle here"), (".gitignore", "ignored.txt")],
        );
        std::fs::write(dir.join("ignored.txt"), "needle too").unwrap();
        std::fs::write(
            dir.join("blob.bin"),
            [0u8, 1, 2, b'n', b'e', b'e', b'd', b'l', b'e'],
        )
        .unwrap();
        let hits = search(&dir, "needle");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, dir.join("keep.txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_query_matches_nothing() {
        let dir = project("empty_query", &[("a.txt", "x")]);
        assert!(search(&dir, "").is_empty());
        assert!(search(&dir, "   ").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
