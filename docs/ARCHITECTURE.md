# How kite is built

A short tour of the code for contributors. kite is about 8,000 lines of Rust
in one crate, built on [ratatui](https://ratatui.rs) (terminal drawing),
[ropey](https://docs.rs/ropey) (text storage) and
[tree-sitter](https://tree-sitter.github.io) (highlighting). It talks to
language servers directly over stdio.

## The files

| File | What lives there |
| --- | --- |
| `src/main.rs` | Terminal setup and teardown, and the event loop |
| `src/app.rs` | All state and behaviour: keys, mouse, commands, popups, LSP requests and responses |
| `src/ui.rs` | Drawing every frame; records where things were drawn so clicks can be mapped back |
| `src/buffer.rs` | One open file: text, cursor, selection, undo, and editing operations |
| `src/commands.rs` | The command table, default keys, the leader layout, and key parsing |
| `src/palette.rs` | File finder, command palette and prompts, with fuzzy matching |
| `src/tree.rs` | The sidebar file tree |
| `src/menu.rs` | Right-click context menus |
| `src/search.rs` | Project-wide search (respects `.gitignore`) |
| `src/fileops.rs` | Duplicate, delete and reveal-in-file-manager for the tree |
| `src/highlight.rs` | The language table and tree-sitter highlighting |
| `src/lsp.rs` | The language server client: spawning, JSON-RPC framing, request bookkeeping |
| `src/config.rs` | Loading and saving `config.toml`, and default language servers |
| `src/theme.rs` | Colours, themes, and the truecolor/256-colour fallback |

## The event loop

kite is single-threaded where it matters: **all state lives in one `App` and
is only changed on the main thread.** Anything that waits on the outside world
runs on its own thread and sends an `AppEvent` down one channel:

```
 terminal input thread ──┐
 language server threads ─┼──► channel ──► main loop: handle events → draw → wait
 (one per server)        ──┘
```

`AppEvent` has three kinds: `Input` (a key, mouse or paste event), `Lsp`
(a message from a server) and `LspExit` (a server stopped).

Each turn of the loop in `main.rs`:

1. `App::after_events` does housekeeping: sends changed text to language
   servers and refreshes find results.
2. `ui::draw` redraws the whole screen. ratatui diffs it against the previous
   frame, so only changed cells reach the terminal.
3. Wait for the next event, or until the next **timer** is due, whichever
   comes first. There are two timers: the cursor blink, and
   `App::next_deadline` / `App::tick` for app work such as fetching hover info
   once the mouse has rested on a word.
4. Handle that event and any others already queued, then loop.

## Handling a key

`App::on_key` offers each key to whatever is in front, in this order, and
stops at the first that takes it:

1. The **leader menu**, if open (`leader_key`).
2. The **shortcuts panel**, if open: arrows scroll it, anything else closes it.
3. The **context menu**, **palette**, **search panel** or **settings panel**,
   if open.
4. The **find bar**, if focused.
5. The **completion popup**, if open: arrows, Enter/Tab, Esc.
6. The **keymap**: if the key is bound to a command, `App::run` runs it.
7. Otherwise, the focused area: `tree_key` or `editor_key` (typing, movement,
   Enter, Backspace…).

Mouse events (`on_mouse`) follow a similar order. Pointer movement goes to
`mouse_moved`, which drives hover-on-rest and the Alt/Option link underline.

**Commands** are the shared vocabulary: keys, the leader menu, the palette
and context menus all end up calling `App::run(Command)`. Adding a command
in one place (`commands.rs`) makes it available everywhere.

## Drawing

`ui::draw` paints every frame from scratch straight into ratatui's cell
buffer, in layers: file tree, tabs, the editor (or a full-pane panel such as
search or settings), find bar, completion and hover popups, status bar, then
the overlays (shortcuts panel, leader menu, palette, context menu) on top.

While drawing, it saves the rectangles it used in `app.layout` (`Layout` in
`app.rs`): where the editor's text starts, which columns each tab covers,
where the popups are. Mouse handling uses those rectangles to work out what
was clicked, so drawing and hit-testing never disagree.

## Buffers and editing

Each open file is a `Buffer` (`buffer.rs`): a `ropey::Rope`, a cursor and an
optional selection anchor (both char indices), and an undo history.

- **`Buffer::edit` is the only place text changes.** Everything else (typing,
  paste, indent, completion) is built on it, which keeps undo and change
  tracking correct.
- **Undo** groups typing into word-sized steps, and multi-line operations
  (indent, comment) into single steps.
- **`version`** goes up on every change. The highlighter re-parses when it
  differs from the last parse, and the LSP code sends the new text when it
  differs from `lsp_version`, the last version the server saw.
- Typing helpers live here too: auto-closing brackets and quotes,
  `closing_tag_for` for HTML/JSX tags, smart indentation on Enter.

## Highlighting

`highlight.rs` holds the language table (`langs()`): each language's id, file
extensions, comment prefix and, optionally, a tree-sitter grammar and
highlight query. The editor re-parses a file when its text changes (files
over 2 MB aren't highlighted) and maps query captures to theme colours.

## Language servers

`lsp.rs` is a small client:

- **Spawning.** The first time a file of a language opens, `App` starts that
  language's server (from `config.lsp`) and sends `initialize`. Messages are
  queued until the server answers, then flushed.
- **Requests.** Each request records a `Pending` value (what it was for,
  such as completion, hover or definition) under its id. The response is
  matched back in `App::on_lsp` and handled accordingly.
- **Syncing.** kite sends the whole document on each change (`didOpen`, then
  `didChange`), from `sync_lsp`.
- **Positions.** LSP counts UTF-16 units; `lsp_pos` and `from_lsp_pos` in
  `app.rs` convert to and from char indices.
- **Failures.** A server that can't start or exits is recorded with its last
  line of stderr, shown as the red `✗` marker in the status bar. Each spawned
  server has a *generation* number, so late messages from a server that has
  since been restarted are ignored.
- **Stale replies.** Completion and hover replies are dropped if the cursor
  or mouse has moved on to a different word since the request.

Completion (`complete_here`) asks the server when it's ready, and otherwise
falls back to `word_completion`, which suggests words from the open files.

## Configuration

`Config::load` reads `config.toml` once at startup. The keymap is built from
the default bindings plus the `[keys]` and `[leader_keys]` overrides
(`Keymap::new`), collecting any mistakes to show in the status bar. The
Settings panel changes `App.cfg` directly and saves it with `Config::save`.

## Where to start

- **A good first read:** `main.rs` (short), then `App::run` and `App::on_key`
  in `app.rs`.
- **Small, self-contained areas** that are good for first contributions:
  `buffer.rs` (editing behaviour, well tested), `commands.rs` (keys and
  commands), and `palette.rs` (fuzzy matching).
- **Recipes** for common changes are in [CONTRIBUTING.md](../CONTRIBUTING.md#common-changes).
