# Contributing to kite-editor

Thanks for your interest! kite is a small, readable codebase, and
contributions of every size are welcome: bug reports, docs fixes, small
improvements and new features.

- **Found a bug or have an idea?** [Open an issue](https://github.com/ryan-mcwilliams/kite-editor/issues/new/choose).
- **Want to fix something?** Comment on the issue first for anything bigger
  than a small fix, so we can agree on the approach before you spend time on it.
- **New to the code?** Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for
  a tour; it's short.

## Setting up

You need a recent stable Rust (from [rustup.rs](https://rustup.rs)) and a C
compiler for the tree-sitter grammars (`xcode-select --install` on macOS,
`build-essential` on Debian/Ubuntu).

```sh
git clone https://github.com/ryan-mcwilliams/kite-editor
cd kite-editor
cargo build          # first build takes a few minutes
cargo run -- .       # open kite on its own source
```

For language-server features while you work, install `rust-analyzer`
(`rustup component add rust-analyzer`).

### Keep your own config out of the way

kite reads `~/.config/kite/config.toml`. To test with a clean config (or
without touching yours), point it somewhere else:

```sh
mkdir -p /tmp/kite-dev/kite
XDG_CONFIG_HOME=/tmp/kite-dev cargo run -- .
```

### Debugging output

kite owns the terminal while it runs, so printing to stdout or stderr
scrambles the screen. Send stderr to a file and watch it from another
terminal:

```sh
cargo run -- . 2>/tmp/kite.log      # then: tail -f /tmp/kite.log
```

`eprintln!` from anywhere in the code then lands in that file. Please remove
debug prints before opening a pull request.

## Making a change

1. **Branch** from `main`: `git switch -c fix-tab-scrolling`.
2. **Make the change**, keeping it focused on one thing.
3. **Add or update tests** where the logic can be tested without a terminal
   (see below).
4. **Check it** before pushing:

   ```sh
   cargo fmt            # formatting (CI-style check: cargo fmt --check)
   cargo clippy         # don't add new warnings
   cargo test           # all tests pass
   ```

   `cargo clippy` currently reports some existing style warnings; please
   don't add new ones (fixing old ones in a separate PR is welcome).
5. **Try it for real** in a terminal: the behaviour you changed, and nearby
   things it could affect.
6. **Update the docs** if you changed keys, commands, options or behaviour
   users will notice: [README.md](README.md) and
   [docs/CONFIGURATION.md](docs/CONFIGURATION.md).
7. **Open a pull request** describing what changed and why, and how you
   tested it. For anything visual, a before/after screenshot or terminal
   capture helps a lot.

Commit messages: a short summary in the imperative ("Add leader key menu",
"Fix hover popup placement near the bottom edge"), then a blank line and any
detail worth keeping.

## Tests

`cargo test` runs the unit tests, which live at the bottom of each file in a
`#[cfg(test)] mod tests`. They cover the logic that doesn't need a terminal:
buffer editing and undo, auto-closing brackets and tags, key parsing and the
leader layout, fuzzy matching, highlighting queries, LSP message framing,
project search, file operations and config parsing.

Good candidates for new tests are anything in `buffer.rs`, `commands.rs`,
`palette.rs`, `search.rs` and `config.rs`. Tests that touch the filesystem
make a temporary directory named after the test, so they can run in
parallel without clashing.

### Testing the UI with tmux

Drawing and input are easiest to check by running kite inside a detached
tmux session, sending it keys, and reading the screen back:

```sh
cargo build
tmux new-session -d -s kite -x 120 -y 30 "target/debug/kite ."
tmux send-keys -t kite C-t            # press Ctrl+T
tmux send-keys -t kite g              # then g
tmux capture-pane -t kite -p          # print the screen as text
tmux capture-pane -t kite -p -e       # …with colour escape codes
tmux kill-session -t kite
```

Mouse input can be sent the same way, as the escape sequences a terminal
produces (SGR mouse encoding, 1-based column and row):

```sh
# move the pointer to column 50, row 6 (button code 35 = move, 43 = move with Alt)
tmux send-keys -t kite -l "$(printf '\033[<35;50;6M')"
# left click at column 50, row 6: press, then release
tmux send-keys -t kite -l "$(printf '\033[<0;50;6M\033[<0;50;6m')"
```

Give kite a moment between steps (and a few seconds for a language server to
start) before capturing.

## Code style

- **Match the surrounding code.** Naming, comment density and structure
  should look like they belong.
- **Comments explain why**, not what. Doc comments (`///`) on functions and
  fields whose purpose isn't obvious from the name.
- **Keep the event loop single-threaded.** Background work (like a language
  server) runs on its own thread and sends an `AppEvent` back; state is only
  changed on the main thread.
- **No new dependencies without discussion.** Open an issue first; kite stays
  small on purpose.
- **Nothing should print to the terminal** while kite is running, including
  from spawned processes (pipe or discard their output).

## Common changes

### Add a command

1. Add a line to the `commands!` table in `src/commands.rs`: the variant, an
   id (used in config files) and a palette title. A `"Group: "` prefix puts
   it in that group in the shortcuts panel.
2. Handle it in `App::run` in `src/app.rs`.
3. Optionally give it a default key in `DEFAULT_KEYS` and/or a leader
   sequence in `DEFAULT_LEADER` (both in `commands.rs`).
4. Add it to the command table in `docs/CONFIGURATION.md`.

It appears in the command palette and shortcuts panel automatically.

### Add a language

1. Add an entry to `langs()` in `src/highlight.rs`: its id, file extensions,
   comment prefix, and, for highlighting, a tree-sitter grammar crate and its
   highlight query.
2. Add a default language server to `default_lsp()` in `src/config.rs`, if
   there's a common one.
3. Update the language lists in the README and `docs/CONFIGURATION.md`.

`highlight::tests::every_grammar_and_query_loads` checks the grammar and
query load.

### Add a setting

1. Add the field and its default to `Config` in `src/config.rs`, and a
   commented line to `TEMPLATE`.
2. To show it in the Settings panel: add a `SettingId` variant in
   `src/app.rs`, its `label()` and `value()`, a row in `SETTINGS_ROWS`, and
   its behaviour in `apply_setting`.
3. Document it in the options table in `docs/CONFIGURATION.md`.

### Add or change a theme

Themes are constructors in `src/theme.rs` (`Theme::one_dark()` and friends).
Add a constructor and a `ThemeId` variant (with its `id()` and `label()`),
and list it in `ALL_THEMES`; it then appears in the Settings panel.

## Reporting bugs

The issue templates ask for this, but in short: what you did, what you
expected, what happened instead, and your kite version (`kite --version`),
OS and terminal (and its version). Terminal differences explain a lot of
odd behaviour, so the terminal matters.

## Questions

Open an issue; there's no question too small.
