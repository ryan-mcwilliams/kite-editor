//! User configuration, read from `~/.config/kite/config.toml`.
//! Every field is optional; missing ones fall back to the defaults here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Debug)]
#[serde(default)]
pub struct Config {
    pub tab_width: usize,
    pub sidebar_width: u16,
    pub format_on_save: bool,
    pub show_line_numbers: bool,
    pub cursor_blink: bool,
    /// "block" (default) or "bar".
    pub cursor_style: String,
    pub mouse_support: bool,
    /// One of "one_dark" (default), "monokai", "catppuccin_mocha", "dracula".
    /// See `theme::ThemeId`.
    pub theme: String,
    /// Key overrides: `"ctrl-s" = "save"`. See commands.rs for command ids.
    pub keys: BTreeMap<String, String>,
    /// Language servers by language id: `rust = ["rust-analyzer"]`.
    /// An empty list disables the server for that language.
    pub lsp: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            tab_width: 4,
            sidebar_width: 30,
            format_on_save: false,
            show_line_numbers: true,
            cursor_blink: true,
            cursor_style: "block".to_string(),
            mouse_support: true,
            theme: "one_dark".to_string(),
            keys: BTreeMap::new(),
            lsp: BTreeMap::new(),
        }
    }
}

fn default_lsp() -> BTreeMap<String, Vec<String>> {
    let table: &[(&str, &[&str])] = &[
        ("rust", &["rust-analyzer"]),
        ("python", &["pyright-langserver", "--stdio"]),
        ("javascript", &["typescript-language-server", "--stdio"]),
        ("typescript", &["typescript-language-server", "--stdio"]),
        (
            "typescriptreact",
            &["typescript-language-server", "--stdio"],
        ),
        ("go", &["gopls"]),
        ("c", &["clangd"]),
        ("cpp", &["clangd"]),
        ("json", &["vscode-json-language-server", "--stdio"]),
        ("css", &["vscode-css-language-server", "--stdio"]),
        ("html", &["vscode-html-language-server", "--stdio"]),
        ("yaml", &["yaml-language-server", "--stdio"]),
        ("toml", &["taplo", "lsp", "stdio"]),
        ("shellscript", &["bash-language-server", "start"]),
    ];
    table
        .iter()
        .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
        .collect()
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("kite").join("config.toml")
}

impl Config {
    /// Returns the config plus an error message if the file didn't parse.
    pub fn load() -> (Config, Option<String>) {
        let (mut cfg, err) = match std::fs::read_to_string(config_path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => (Config::default(), None),
        };
        let mut lsp = default_lsp();
        lsp.extend(std::mem::take(&mut cfg.lsp));
        cfg.lsp = lsp;
        cfg.tab_width = cfg.tab_width.clamp(1, 16);
        (cfg, err)
    }

    fn parse(text: &str) -> (Config, Option<String>) {
        match toml::from_str::<Config>(text) {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!("config.toml: {}", e.message())),
            ),
        }
    }

    /// Write the config back to disk (used by the Settings panel). This
    /// regenerates the file from the current values, so it preserves every
    /// setting including `keys` and `lsp` overrides, but not comments or
    /// formatting a user hand-edited into the file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}

pub const TEMPLATE: &str = r#"# kite configuration. Restart kite to apply changes.

# tab_width = 4
# sidebar_width = 30
# format_on_save = false
# show_line_numbers = true
# cursor_blink = true
# cursor_style = "block"  # or "bar"
# mouse_support = true
# theme = "one_dark"  # or "monokai", "catppuccin_mocha", "dracula"

# Key bindings: "key" = "command id" ("none" removes a default binding).
# Open the command palette (F1) to see every command and its current key.
# Modifiers: ctrl, alt, shift, cmd (cmd only reaches terminals that use the
# kitty keyboard protocol and don't claim the shortcut themselves).
[keys]
# "ctrl-t" = "file_finder"
# "ctrl-d" = "goto_definition"

# Language servers, by language id. Use an empty list to turn one off.
[lsp]
# rust = ["rust-analyzer"]
# python = ["pylsp"]
# markdown = ["marksman", "server"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_and_partial_configs_parse() {
        let (c, err) = Config::parse(TEMPLATE);
        assert!(err.is_none());
        assert_eq!(c.tab_width, 4);
        let (c, err) = Config::parse(
            "tab_width = 2\n[keys]\n\"ctrl-t\" = \"file_finder\"\n[lsp]\npython = [\"pylsp\"]\n",
        );
        assert!(err.is_none());
        assert_eq!(
            (c.tab_width, c.keys.len(), c.lsp["python"][0].as_str()),
            (2, 1, "pylsp")
        );
        assert!(Config::parse("tab_width = [").1.is_some());
    }
}
