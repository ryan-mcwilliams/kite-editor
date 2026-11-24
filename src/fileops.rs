//! File-system operations behind the right-click menu. Kept free of UI
//! state so they can be unit tested.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `a.txt` -> `a copy.txt`, then `a copy 2.txt`, ... (directories and
/// extensionless files just get the suffix).
pub fn unique_copy_path(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new(""));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            " copy".to_string()
        } else {
            format!(" copy {n}")
        };
        let candidate = dir.join(format!("{stem}{suffix}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Copy a file or directory tree next to the original; returns the new path.
pub fn duplicate(path: &Path) -> io::Result<PathBuf> {
    let to = unique_copy_path(path);
    copy_recursive(path, &to)?;
    Ok(to)
}

fn copy_recursive(from: &Path, to: &Path) -> io::Result<()> {
    let ft = std::fs::symlink_metadata(from)?.file_type();
    if ft.is_symlink() {
        #[cfg(unix)]
        {
            return std::os::unix::fs::symlink(std::fs::read_link(from)?, to);
        }
        #[cfg(not(unix))]
        {
            return std::fs::copy(from, to).map(|_| ());
        }
    }
    if ft.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Permanently delete a file or directory tree. A symlink is removed
/// without touching what it points to.
pub fn delete(path: &Path) -> io::Result<()> {
    let ft = std::fs::symlink_metadata(path)?.file_type();
    if ft.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Reveal in File Manager"
    }
}

/// Show the item in the system file manager. Output is discarded so
/// nothing is printed over the TUI.
pub fn reveal(path: &Path) -> io::Result<()> {
    let mut cmd;
    if cfg!(target_os = "macos") {
        cmd = Command::new("open");
        cmd.arg("-R").arg(path);
    } else {
        cmd = Command::new("xdg-open");
        cmd.arg(path.parent().unwrap_or(path));
    }
    let status = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("file manager exited with an error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kite-fileops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copy_names_count_up_and_keep_extensions() {
        let dir = scratch("names");
        let a = dir.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        assert_eq!(unique_copy_path(&a), dir.join("a copy.txt"));
        std::fs::write(dir.join("a copy.txt"), "x").unwrap();
        assert_eq!(unique_copy_path(&a), dir.join("a copy 2.txt"));
        let dot = dir.join(".gitignore");
        assert_eq!(unique_copy_path(&dot), dir.join(".gitignore copy"));
        let sub = dir.join("src");
        assert_eq!(unique_copy_path(&sub), dir.join("src copy"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_copies_files_and_whole_directories() {
        let dir = scratch("dup");
        std::fs::create_dir_all(dir.join("src/inner")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "a").unwrap();
        std::fs::write(dir.join("src/inner/b.rs"), "b").unwrap();
        let copy = duplicate(&dir.join("src")).unwrap();
        assert_eq!(copy, dir.join("src copy"));
        assert_eq!(
            std::fs::read_to_string(copy.join("inner/b.rs")).unwrap(),
            "b"
        );
        let file_copy = duplicate(&dir.join("src/a.rs")).unwrap();
        assert_eq!(std::fs::read_to_string(file_copy).unwrap(), "a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_trees_and_symlinks_not_their_targets() {
        let dir = scratch("del");
        std::fs::create_dir_all(dir.join("keep")).unwrap();
        std::fs::write(dir.join("keep/f.txt"), "f").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("keep"), dir.join("link")).unwrap();
            delete(&dir.join("link")).unwrap();
            assert!(dir.join("keep/f.txt").exists());
        }
        delete(&dir.join("keep")).unwrap();
        assert!(!dir.join("keep").exists());
        assert!(delete(&dir.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
