# Configuring kite

kite reads `~/.config/kite/config.toml` (or `$XDG_CONFIG_HOME/kite/config.toml`)
when it starts. Every setting is optional; anything you leave out uses the
default below.

- **Create it** with the palette command *Settings: Open Config File*
  (Ctrl+T f c). It starts from a commented template.
- **Restart kite** after editing the file by hand.
- **The Settings panel** (Ctrl+,) changes the common options immediately and
  saves them for you. Note that it rewrites the file from the current values:
  your settings, `[keys]`, `[leader_keys]` and `[lsp]` are kept, but comments
  and formatting you added by hand are not.
- **Mistakes** don't stop kite starting. A file that doesn't parse falls back
  to the defaults, and the first problem is shown in the status bar.

## Options

| Option | Default | Values |
| --- | --- | --- |
| `theme` | `"one_dark"` | `"one_dark"`, `"monokai"`, `"catppuccin_mocha"`, `"dracula"` |
| `tab_width` | `4` | 1–16. The Settings panel cycles 2, 4, 8 |
| `format_on_save` | `false` | Format with the language server when saving |
| `show_line_numbers` | `true` | |
| `cursor_style` | `"block"` | `"block"` or `"bar"` |
| `cursor_blink` | `true` | kite blinks the cursor itself, so this works even if your terminal's own blink setting is off |
| `mouse_support` | `true` | Turn off to use your terminal's own mouse selection |
| `sidebar_width` | `30` | Columns. You can also drag the sidebar's edge |

```toml
theme = "dracula"
tab_width = 2
format_on_save = true
cursor_style = "bar"
```

## Key bindings: `[keys]`

Map a key to a [command id](#commands), or to `"none"` to remove a default.

```toml
[keys]
"ctrl-d" = "goto_definition"
"alt-up" = "prev_tab"
"ctrl-q" = "none"
```

**Writing keys:** modifiers, then the key, joined with `-`.

- Modifiers: `ctrl`, `alt` (or `option`), `shift`, `cmd` (or `super`).
- Keys: a single character (`a`, `/`, `,`, `]`), or `space`, `enter`, `tab`,
  `esc`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`,
  `pageup`, `pagedown`, `f1`–`f12`.
- Examples: `"ctrl-shift-p"`, `"alt-]"`, `"f12"`, `"alt--"` (Alt and minus).

**What your terminal can send:** terminals don't pass Cmd to programs unless
they support the kitty keyboard protocol and don't use the shortcut
themselves. Combinations like Ctrl+Shift+letter also need that protocol. If a
binding does nothing, try it in Ghostty, kitty or WezTerm, or pick another key.

## Leader keys: `[leader_keys]`

The leader key (Ctrl+T) opens a menu of follow-up keys. Each entry is a
sequence of single characters separated by spaces, mapped to a command id or
`"none"`.

```toml
[keys]
"ctrl-l" = "leader"            # use Ctrl+L as the leader…
"ctrl-t" = "none"              # …and free up Ctrl+T

[leader_keys]
"x" = "toggle_sidebar"         # Ctrl+T x
"g r" = "restart_server"       # Ctrl+T g r, inside the Go group
"t w" = "settings"             # Ctrl+T t w
"g d" = "none"                 # remove a default
```

A key can either run a command or open a group, not both, so `"g" = "save"`
is reported as an error (because `g` opens the Go group). Sequences under a
new first letter create a new group, named after its key.

The default layout:

| Keys | Command |
| --- | --- |
| `l` | Go to line |
| `p` | Command palette |
| `?` | Keyboard shortcuts |
| `1`–`9` | Jump to that tab (built in) |
| `g l` · `g d` · `g b` · `g f` | Go to line · definition · back · file |
| `f s` · `f n` · `f w` · `f c` | Save · new file · close tab · open config |
| `s f` · `s p` · `s n` · `s N` | Find in file · search in files · next · previous |
| `c f` · `c h` · `c a` · `c r` | Format · hover · complete · restart language server |
| `t s` · `t n` · `t b` · `t m` | Toggle sidebar · line numbers · cursor blink · mouse |
| `b n` · `b p` · `b w` | Next tab · previous tab · close tab |

## Language servers: `[lsp]`

Map a language id to the command that starts its server (program first, then
arguments). An empty list turns the server off.

```toml
[lsp]
python = ["pylsp"]
markdown = ["marksman", "server"]
c = []                          # no server for C
```

Language ids with a default server: `rust`, `python`, `javascript`,
`typescript`, `typescriptreact`, `go`, `c`, `cpp`, `json`, `css`, `html`,
`yaml`, `toml`, `shellscript`. `markdown` has highlighting but no default
server. The servers and how to install them are listed in the
[README](../README.md#language-servers).

## Commands

Every command, with its id (for `[keys]` and `[leader_keys]`), its title in
the command palette, and its default keys.

| Command id | Palette title | Default keys | Leader |
| --- | --- | --- | --- |
| `save` | File: Save | Ctrl+S | `f s` |
| `new_file` | File: New File… | Ctrl+N | `f n` |
| `close_tab` | File: Close Tab | Ctrl+W | `f w`, `b w` |
| `quit` | Quit | Ctrl+Q |  |
| `file_finder` | Go to File… | Ctrl+F | `g f` |
| `command_palette` | Command Palette | Ctrl+P, Ctrl+Shift+P, F1 | `p` |
| `search_in_files` | Search in Files… | Ctrl+Shift+F | `s p` |
| `goto_line` | Go to Line… | Ctrl+G | `l`, `g l` |
| `next_tab` | View: Next Tab | Ctrl+PageDown, Alt+] | `b n` |
| `prev_tab` | View: Previous Tab | Ctrl+PageUp, Alt+[ | `b p` |
| `toggle_sidebar` | View: Toggle Sidebar | Ctrl+B | `t s` |
| `focus_sidebar` | View: Focus File Tree / Editor | Ctrl+E |  |
| `refresh_tree` | View: Refresh File Tree |  |  |
| `settings` | Settings… | Ctrl+, |  |
| `find` | Find in File | Alt+F | `s f` |
| `find_next` | Find Next | F3 | `s n` |
| `find_prev` | Find Previous | Shift+F3 | `s N` |
| `undo` | Edit: Undo | Ctrl+Z |  |
| `redo` | Edit: Redo | Ctrl+Y, Ctrl+Shift+Z |  |
| `copy` | Edit: Copy | Ctrl+C |  |
| `cut` | Edit: Cut | Ctrl+X |  |
| `paste` | Edit: Paste | Ctrl+V |  |
| `select_all` | Edit: Select All | Ctrl+A |  |
| `toggle_comment` | Edit: Toggle Line Comment | Ctrl+/ |  |
| `complete` | Code: Trigger Completion | Ctrl+Space | `c a` |
| `hover` | Code: Show Hover Info | Alt+K | `c h` |
| `goto_definition` | Code: Go to Definition | F12 | `g d` |
| `go_back` | Code: Go Back | Ctrl+O | `g b` |
| `format` | Code: Format Document | Alt+Shift+F | `c f` |
| `restart_server` | Code: Restart Language Server |  | `c r` |
| `open_config` | Settings: Open Config File |  | `f c` |
| `shortcuts` | Help: Keyboard Shortcuts | Ctrl+K | `?` |
| `leader` | Leader Key… | Ctrl+T |  |
| `toggle_line_numbers` | View: Toggle Line Numbers |  | `t n` |
| `toggle_cursor_blink` | View: Toggle Cursor Blink |  | `t b` |
| `toggle_mouse` | View: Toggle Mouse Support |  | `t m` |

Some keys aren't commands and can't be rebound: Tab / Shift+Tab (indent),
Shift+arrows (select), Ctrl/Alt+arrows (word movement), Esc (close popups),
and mouse actions such as Alt+Click.

The command palette (Ctrl+P) and shortcuts panel (Ctrl+K) always show your
current bindings, so they're the quickest way to check a change worked.
