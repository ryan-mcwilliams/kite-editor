# kite-editor

```
╭────────────────────────────────╮
│                                │
│  ██╗  ██╗██╗████████╗███████╗  │
│  ██║ ██╔╝██║╚══██╔══╝██╔════╝  │
│  █████╔╝ ██║   ██║   █████╗    │
│  ██╔═██╗ ██║   ██║   ██╔══╝    │
│  ██║  ██╗██║   ██║   ███████╗  │
│  ╚═╝  ╚═╝╚═╝   ╚═╝   ╚══════╝  │
│                                │
╰────────────────────────────────╯
```

**A light and nimble terminal text editor** that behaves like a desktop one:
open a folder, file tree on the left, tabs, the mouse works everywhere, a
command palette, and language servers for completion, hover, diagnostics,
go-to-definition and formatting. No modes to learn: you type, and text
appears.

The command is `kite`. It's written from scratch in Rust (ratatui +
tree-sitter), about 8,000 lines, small enough to read end to end and make
your own.

> **Status:** early and actively developed. It's used day to day on macOS;
> expect rough edges, and please [report them](#contributing).

## Features

- **Project view**: a file tree with icons, tabs, and a fuzzy file finder.
- **Command palette** listing every command and its key.
- **Leader key** (Ctrl+T): a which-key menu for quick, mnemonic shortcuts.
- **Language servers**: completion as you type, hover info (including on
  mouse-over), go to definition (F12 or Alt/Option+Click), formatting and
  inline diagnostics. Falls back to word completion without a server.
- **Syntax highlighting** built in with tree-sitter, nothing to install.
- **Editing comforts**: auto-closing brackets, quotes and HTML/JSX tags,
  smart indentation, toggle comment, word-sized undo.
- **Search** in the current file or across the project (respects `.gitignore`).
- **Mouse everywhere**: click, drag-select, double/triple-click, scroll,
  right-click context menus, a resizable sidebar.
- **Themes**: One Dark (default), Monokai, Catppuccin Mocha, Dracula.
- **Settings panel** (Ctrl+,) and a plain `config.toml` for everything else.

## Install

You need:

- **Rust**, a recent stable toolchain (from [rustup.rs](https://rustup.rs)).
  kite uses the 2024 edition and is developed on Rust 1.100.
- **A C compiler**, which the tree-sitter grammars need. On macOS:
  `xcode-select --install`. On Debian/Ubuntu: `sudo apt install build-essential`.

Then install straight from GitHub:

```sh
cargo install --git https://github.com/ryan-mcwilliams/kite-editor
```

Or from a clone:

```sh
git clone https://github.com/ryan-mcwilliams/kite-editor
cd kite-editor
cargo install --path .
```

Either puts `kite` in `~/.cargo/bin`. The first build takes a few minutes
(the tree-sitter grammars are compiled); later builds are quick. To update,
run the same command again.

## Quick start

```sh
kite .                # open the current directory as a project
kite ~/code/my-app    # open another directory
kite src/main.rs      # open a file, with its enclosing project
```

When you open a file, kite looks upward for a project marker (`.git`,
`Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`) and opens that folder
as the project.

Then:

1. **Ctrl+F** to find a file, **Ctrl+P** for every command.
2. **Ctrl+K** shows all keyboard shortcuts.
3. **Ctrl+T** opens the leader menu.
4. **Ctrl+S** saves, **Ctrl+W** closes the tab, **Ctrl+Q** quits.

For completion and hover, install the language server for your language
(see [Language servers](#language-servers)).

## Keys

Terminals don't pass Cmd through to programs, so the defaults use Ctrl.
Every key can be rebound; see [Configuration](docs/CONFIGURATION.md).

| Key | Action |
| --- | --- |
| Ctrl+F | Go to file (fuzzy). Type `>` for commands, `:42` for a line |
| Ctrl+P, F1, or Ctrl+Shift+P | Command palette (lists every command and its key) |
| Ctrl+K | Keyboard shortcuts panel |
| Ctrl+T | Leader key (see below) |
| Ctrl+B / Ctrl+E | Toggle sidebar / move focus between tree and editor |
| Ctrl+S, Ctrl+N, Ctrl+W, Ctrl+Q | Save, new file, close tab, quit |
| Alt+] / Alt+[ (or Ctrl+PageDown/Up) | Next / previous tab |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+A | Copy, cut, paste, select all (no selection = whole line) |
| Alt+F, F3, Shift+F3 | Find in file, next, previous |
| Ctrl+Shift+F | Search in files (needs the kitty keyboard protocol; otherwise use Ctrl+T s p) |
| Ctrl+G | Go to line |
| Ctrl+/ | Toggle line comment |
| Tab / Shift+Tab | Indent / outdent selected lines |
| Shift+arrows, Ctrl or Alt+arrows | Select; move by word |
| Ctrl+Space | Completions (they also appear as you type) |
| Alt+K | Hover info for the symbol at the cursor |
| F12 or Alt+Click | Go to definition; Ctrl+O goes back |
| Alt+Shift+F | Format document |
| Ctrl+, | Settings panel |
| `>` ending an opening tag | Adds the closing tag (HTML, JSX/TSX) |

### Leader key

Press **Ctrl+T** and a menu slides up showing what each next key does.
Keys are grouped by meaning:

| Keys | Does |
| --- | --- |
| `l` | Go to line |
| `1`–`9` | Jump to tab 1–9 |
| `p` / `?` | Command palette / shortcuts panel |
| `g` Go | `l` line · `d` definition · `b` back · `f` file |
| `f` File | `s` save · `n` new · `w` close tab · `c` open config |
| `s` Search | `f` in file · `p` in project · `n` / `N` next / previous match |
| `c` Code | `f` format · `h` hover · `a` complete · `r` restart language server |
| `t` Toggle | `s` sidebar · `n` line numbers · `b` cursor blink · `m` mouse |
| `b` Tabs | `n` next · `p` previous · `w` close |

So **Ctrl+T l** jumps to a line and **Ctrl+T g d** goes to a definition.
Backspace steps back a level; Esc cancels. Add your own under
`[leader_keys]` in the config.

### Mouse

Click to place the cursor, drag to select, double-click a word, triple-click
a line, scroll with the wheel. Click tabs and their ×, and drag the sidebar's
edge to resize it. Rest the pointer on a symbol to see its type and docs;
hold Alt/Option to underline it as a link, and Alt/Option+Click to jump to
its definition. Right-click files, folders or code for a context menu (new
file/folder, rename, duplicate, delete, copy path, reveal in Finder; on code:
go to definition, hover, format, cut/copy/paste, comment).

## Language servers

kite starts a language server the first time you open a file of that
language, if the server is installed. Defaults:

| Language | Server | Install |
| --- | --- | --- |
| Rust | rust-analyzer | `rustup component add rust-analyzer` |
| Python | pyright-langserver | `npm i -g pyright` |
| TypeScript / JavaScript | typescript-language-server | `npm i -g typescript typescript-language-server` |
| Go | gopls | `go install golang.org/x/tools/gopls@latest` |
| C / C++ | clangd | Xcode command line tools, or LLVM |
| JSON, CSS, HTML | vscode-*-language-server | `npm i -g vscode-langservers-extracted` |
| YAML | yaml-language-server | `npm i -g yaml-language-server` |
| TOML | taplo | `cargo install taplo-cli --features lsp` |
| Shell | bash-language-server | `npm i -g bash-language-server` |

The status bar shows the server's name and whether it's still starting or
indexing. If a server is missing or stops, a red `✗ server` marker appears;
click it (or Ctrl+T c r) to see why and try again. You can swap or add
servers in the config.

Syntax highlighting is built in for Rust, Python, JavaScript, TypeScript/TSX,
Go, C, JSON, TOML, YAML, shell, CSS, HTML and Markdown.

## Configuration

Run **Settings: Open Config File** (Ctrl+T f c) to create and open
`~/.config/kite/config.toml` (or `$XDG_CONFIG_HOME/kite/config.toml`). The
Settings panel (Ctrl+,) changes the common options for you.

```toml
tab_width = 4
theme = "catppuccin_mocha"
format_on_save = true

[keys]
"ctrl-d" = "goto_definition"  # bind a key to a command id
"ctrl-q" = "none"             # remove a default

[leader_keys]
"x" = "toggle_sidebar"        # Ctrl+T x

[lsp]
python = ["pylsp"]            # use a different server
```

Every option, every command id and the full leader layout are in
**[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**.

## Terminal setup

kite works in any modern terminal; these make it better:

- **24-bit colour and the kitty keyboard protocol.** Ghostty, kitty, WezTerm
  and iTerm2 have both. In other terminals colours fall back to 256, and
  shortcuts like Ctrl+Shift+F can't be told apart from Ctrl+F (use the
  leader key or F1 instead).
- **A [Nerd Font](https://www.nerdfonts.com/)** as the terminal font, for the
  file tree's icons. Without one they show as boxes or blanks.
- **Option as Alt on macOS**, for Alt+arrows, Alt+] and Alt+K: in
  Terminal.app enable "Use Option as Meta key"; in iTerm2 set Option to
  "Esc+"; in Ghostty set `macos-option-as-alt = true`.

Because kite handles the mouse, your terminal's own text selection is off
while it runs. Hold Shift (Option in iTerm2 and Terminal.app) while dragging
to use it.

## Platform support

| Platform | Status |
| --- | --- |
| macOS | Used daily; the primary platform |
| Linux | Expected to work (file manager via `xdg-open`); lightly tested |
| Windows | Untested; contributions welcome |

## Troubleshooting

- **No completions or hover.** Check the status bar for a red `✗` marker and
  click it for the reason. Language servers also need the project's own file
  to work: a `Cargo.toml` for Rust, a `package.json` (and installed
  `node_modules`) for TypeScript, and so on.
- **A shortcut does nothing.** Your terminal may be using it, or not sending
  it. Every command is also in the palette (Ctrl+P) and most are on the
  leader key (Ctrl+T).
- **Alt/Option shortcuts don't work.** See *Option as Alt* in
  [Terminal setup](#terminal-setup).
- **Icons show as boxes.** Set a Nerd Font as your terminal's font.
- **`config.toml` errors.** kite still starts with defaults and shows the
  first problem in the status bar.

## Roadmap

Roughly in order:

1. Find and replace
2. Move files in the tree; reload files changed on disk
3. Find references, rename symbol, signature help, code actions
4. Split panes
5. Integrated terminal panel
6. Git gutter and branch in the status bar
7. Keyboard macros (record and replay)
8. Multiple cursors, soft wrap

Ideas and help with any of these are welcome; open an issue to discuss.

## Contributing

Contributions of all sizes are welcome: bug reports, docs, small fixes and
features. Start with **[CONTRIBUTING.md](CONTRIBUTING.md)**, which covers
setting up, testing, code style and how changes are reviewed, and
**[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** for a tour of the code.

## License

[MIT](LICENSE). Contributions are accepted under the same license.
