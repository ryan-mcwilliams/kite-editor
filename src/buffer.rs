//! Text buffer: a rope plus cursor, selection, undo history and the editing
//! operations a non-modal editor needs. Nothing in here knows about the UI.

use crate::highlight::{Lang, Syntax};
use ropey::Rope;
use std::io;
use std::path::PathBuf;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    removed: String,
    inserted: String,
    before: usize,
    after: usize,
    group: u64,
}

pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    /// Cursor position as a char index into the rope.
    pub cursor: usize,
    /// Other end of the selection, if any.
    pub anchor: Option<usize>,
    goal: Option<usize>,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub dirty: bool,
    /// Bumped on every text change. Used by highlighting and LSP sync.
    pub version: i32,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    group: u64,
    in_tx: bool,
    coalesce: bool,
    /// Undo depth at the last save, so undoing back to it clears `dirty`.
    saved_at: Option<usize>,
    pub indent: String,
    pub lang: Option<&'static Lang>,
    pub syntax: Option<Syntax>,
    /// Last version sent to the language server (None = not opened there).
    pub lsp_version: Option<i32>,
}

pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn class(c: char) -> u8 {
    if is_word(c) {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
}

/// HTML elements that never take a closing tag.
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

fn pair_close(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        _ => return None,
    })
}

impl Buffer {
    pub fn from_text(text: &str, path: Option<PathBuf>) -> Self {
        let lang = path.as_deref().and_then(crate::highlight::lang_for_path);
        let mut indent = "    ".to_string();
        for line in text.lines().take(400) {
            if line.starts_with('\t') {
                indent = "\t".into();
                break;
            }
            let n = line.len() - line.trim_start_matches(' ').len();
            if n == 2 && line.len() > 2 {
                indent = "  ".into();
                break;
            }
            if n == 4 {
                break;
            }
        }
        Buffer {
            rope: Rope::from_str(text),
            path,
            cursor: 0,
            anchor: None,
            goal: None,
            scroll_row: 0,
            scroll_col: 0,
            dirty: false,
            version: 0,
            undo: vec![],
            redo: vec![],
            group: 0,
            in_tx: false,
            coalesce: false,
            saved_at: Some(0),
            indent,
            lang,
            syntax: None,
            lsp_version: None,
        }
    }

    pub fn open(path: PathBuf) -> io::Result<Self> {
        let bytes = std::fs::read(&path)?;
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "binary file"));
        }
        Ok(Self::from_text(
            &String::from_utf8_lossy(&bytes),
            Some(path),
        ))
    }

    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::new(io::ErrorKind::Other, "no file name"));
        };
        let mut out = io::BufWriter::new(std::fs::File::create(path)?);
        self.rope.write_to(&mut out)?;
        io::Write::flush(&mut out)?;
        self.dirty = false;
        self.saved_at = Some(self.undo.len());
        self.coalesce = false;
        Ok(())
    }

    pub fn name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => "untitled".into(),
        }
    }

    pub fn len(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Length of a line in chars, excluding its line ending.
    pub fn line_len(&self, line: usize) -> usize {
        let l = self.rope.line(line);
        let mut n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n -= 1;
            if n > 0 && l.char(n - 1) == '\r' {
                n -= 1;
            }
        }
        n
    }

    pub fn pos(&self, idx: usize) -> (usize, usize) {
        let idx = idx.min(self.len());
        let line = self.rope.char_to_line(idx);
        (line, idx - self.rope.line_to_char(line))
    }

    pub fn idx(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.lines() - 1);
        self.rope.line_to_char(line) + col.min(self.line_len(line))
    }

    fn char_at(&self, idx: usize) -> Option<char> {
        (idx < self.len()).then(|| self.rope.char(idx))
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        match self.anchor {
            Some(a) if a != self.cursor => Some((a.min(self.cursor), a.max(self.cursor))),
            _ => None,
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|(s, e)| self.rope.slice(s..e).to_string())
    }

    pub fn select(&mut self, start: usize, end: usize) {
        self.anchor = Some(start.min(self.len()));
        self.cursor = end.min(self.len());
        self.coalesce = false;
    }

    // ---- display columns (tabs and wide characters) ----

    pub fn col_to_x(&self, line: usize, col: usize, tabw: usize) -> usize {
        let mut x = 0;
        for c in self.rope.line(line).chars().take(col) {
            x += char_width(c, x, tabw);
        }
        x
    }

    pub fn x_to_col(&self, line: usize, x: usize, tabw: usize) -> usize {
        let mut acc = 0;
        let max = self.line_len(line);
        for (i, c) in self.rope.line(line).chars().enumerate() {
            if i >= max {
                break;
            }
            let w = char_width(c, acc, tabw);
            if x < acc + w {
                return if w > 1 && x - acc >= w.div_ceil(2) {
                    i + 1
                } else {
                    i
                };
            }
            acc += w;
        }
        max
    }

    // ---- editing ----

    /// The one place text changes. Everything else is built on this.
    pub fn edit(&mut self, start: usize, end: usize, text: &str) {
        let removed = self.rope.slice(start..end).to_string();
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let before = self.cursor;
        self.rope.remove(start..end);
        self.rope.insert(start, text);
        let n = text.chars().count();
        self.cursor = start + n;
        self.anchor = None;
        self.goal = None;
        if !self.in_tx {
            let typing = removed.is_empty() && n == 1 && text != "\n";
            let contiguous = self.undo.last().is_some_and(|e| {
                e.removed.is_empty() && e.at + e.inserted.chars().count() == start
            });
            if !(self.coalesce && typing && contiguous) {
                self.group += 1;
            }
            self.coalesce = typing && !text.starts_with(char::is_whitespace);
        }
        if self.saved_at.is_some_and(|d| self.undo.len() < d) {
            self.saved_at = None;
        }
        self.undo.push(Edit {
            at: start,
            removed,
            inserted: text.to_string(),
            before,
            after: self.cursor,
            group: self.group,
        });
        self.redo.clear();
        self.dirty = true;
        self.version += 1;
    }

    /// Group several edits into one undo step.
    pub fn transaction(&mut self, f: impl FnOnce(&mut Self)) {
        self.group += 1;
        self.in_tx = true;
        f(self);
        self.in_tx = false;
        self.coalesce = false;
    }

    pub fn undo(&mut self) -> bool {
        let Some(g) = self.undo.last().map(|e| e.group) else {
            return false;
        };
        while self.undo.last().is_some_and(|e| e.group == g) {
            let e = self.undo.pop().unwrap();
            let n = e.inserted.chars().count();
            self.rope.remove(e.at..e.at + n);
            self.rope.insert(e.at, &e.removed);
            self.cursor = e.before;
            self.redo.push(e);
        }
        self.after_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(g) = self.redo.last().map(|e| e.group) else {
            return false;
        };
        while self.redo.last().is_some_and(|e| e.group == g) {
            let e = self.redo.pop().unwrap();
            let n = e.removed.chars().count();
            self.rope.remove(e.at..e.at + n);
            self.rope.insert(e.at, &e.inserted);
            self.cursor = e.after;
            self.undo.push(e);
        }
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        self.cursor = self.cursor.min(self.len());
        self.anchor = None;
        self.goal = None;
        self.coalesce = false;
        self.dirty = self.saved_at != Some(self.undo.len());
        self.version += 1;
    }

    /// Insert text at the cursor, replacing the selection.
    pub fn insert(&mut self, text: &str) {
        let (s, e) = self.selection().unwrap_or((self.cursor, self.cursor));
        self.edit(s, e, text);
    }

    /// A typed character, with bracket/quote auto-pairing.
    pub fn type_char(&mut self, c: char) {
        let quote_ok = !(c == '\'' && self.lang.is_some_and(|l| l.id == "rust"));
        if let Some((s, e)) = self.selection() {
            if let (Some(close), true) = (pair_close(c), quote_ok) {
                let inner = self.rope.slice(s..e).to_string();
                self.edit(s, e, &format!("{c}{inner}{close}"));
                self.select(s + 1, e + 1);
            } else {
                self.edit(s, e, c.encode_utf8(&mut [0; 4]));
            }
            return;
        }
        let cur = self.cursor;
        let next = self.char_at(cur);
        let prev = if cur > 0 { self.char_at(cur - 1) } else { None };
        if matches!(c, ')' | ']' | '}' | '"' | '\'' | '`') && next == Some(c) {
            self.cursor += 1;
            return;
        }
        if c == '>' {
            if let Some(close) = self.closing_tag_for(cur) {
                self.edit(cur, cur, &format!(">{close}"));
                self.cursor = cur + 1;
                return;
            }
        }
        if let (Some(close), true) = (pair_close(c), quote_ok) {
            let is_quote = c == close;
            let next_ok =
                next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';'));
            let prev_ok = !is_quote || prev.is_none_or(|p| !is_word(p) && p != c);
            if next_ok && prev_ok {
                self.edit(cur, cur, &format!("{c}{close}"));
                self.cursor -= 1;
                return;
            }
        }
        self.edit(cur, cur, c.encode_utf8(&mut [0; 4]));
    }

    /// Typing `>` at `end`: if that finishes an opening tag such as `<h1`
    /// or `<div className="x"`, the tag that closes it ("</h1>"). Only in
    /// HTML and JSX; plain TypeScript's `<...>` is usually a generic.
    fn closing_tag_for(&self, end: usize) -> Option<String> {
        let html = match self.lang.map(|l| l.id) {
            Some("html") => true,
            Some("javascript" | "typescriptreact") => false,
            _ => return None,
        };
        // Find the `<` that opens this tag, not looking past another `>`
        // (except inside `{...}`, as in `onClick={() => go()}`).
        let floor = end.saturating_sub(2000);
        let (mut open, mut depth) = (end, 0);
        loop {
            if open == floor {
                return None;
            }
            open -= 1;
            match self.rope.char(open) {
                '}' => depth += 1,
                '{' if depth > 0 => depth -= 1,
                '<' if depth == 0 => break,
                '>' if depth == 0 => return None,
                _ => {}
            }
        }
        // `a<b` is a comparison or a generic, not a tag.
        if open > 0 && is_word(self.rope.char(open - 1)) {
            return None;
        }
        let inner = self.rope.slice(open + 1..end).to_string();
        let name: String = inner
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            .collect();
        if name.is_empty() {
            // `<>` opens a JSX fragment.
            return (!html && inner.is_empty()).then(|| "</>".to_string());
        }
        if !name.starts_with(char::is_alphabetic) {
            return None;
        }
        // The rest must be whole attributes: nothing left open, and nothing
        // that reads like code (`a <b && c >`).
        let rest = &inner[name.len()..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let (mut quote, mut depth) = (None, 0i32);
        for c in rest.chars() {
            match (quote, c) {
                (Some(q), _) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'' | '`') => quote = Some(c),
                (None, '{') => depth += 1,
                (None, '}') => depth -= 1,
                (None, _) if depth > 0 => {}
                (None, c) if c.is_alphanumeric() || c.is_whitespace() => {}
                (None, '=' | '-' | '_' | ':' | '.' | '@') => {}
                _ => return None,
            }
        }
        if quote.is_some() || depth != 0 || inner.trim_end().ends_with(['/', '=']) {
            return None;
        }
        if html && VOID_TAGS.contains(&name.to_lowercase().as_str()) {
            return None;
        }
        let close = format!("</{name}>");
        let after = self
            .rope
            .slice(end..(end + close.chars().count()).min(self.len()));
        (after != close.as_str()).then_some(close)
    }

    pub fn newline(&mut self) {
        let (s, e) = self.selection().unwrap_or((self.cursor, self.cursor));
        let (line, col) = self.pos(s);
        let ws: String = self
            .rope
            .line(line)
            .chars()
            .take(col)
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let prev = if s > 0 { self.char_at(s - 1) } else { None };
        let next = self.char_at(e);
        let python = self.lang.is_some_and(|l| l.id == "python");
        // Between a tag pair: `<div>|</div>`.
        let tags = prev == Some('>') && next == Some('<') && self.char_at(e + 1) == Some('/');
        let opens = matches!(prev, Some('{' | '[' | '(')) || (python && prev == Some(':')) || tags;
        let mut text = format!("\n{ws}");
        if opens {
            text.push_str(&self.indent);
        }
        let cursor_after = s + text.chars().count();
        if tags
            || matches!(
                (prev, next),
                (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
            )
        {
            text.push_str(&format!("\n{ws}"));
        }
        self.edit(s, e, &text);
        self.cursor = cursor_after;
    }

    pub fn backspace(&mut self) {
        if let Some((s, e)) = self.selection() {
            return self.edit(s, e, "");
        }
        let cur = self.cursor;
        if cur == 0 {
            return;
        }
        let prev = self.rope.char(cur - 1);
        if pair_close(prev).is_some() && self.char_at(cur) == pair_close(prev) {
            return self.edit(cur - 1, cur + 1, "");
        }
        let (line, col) = self.pos(cur);
        let unit = self.indent.len();
        if prev == ' '
            && col >= unit
            && unit > 1
            && self.rope.line(line).chars().take(col).all(|c| c == ' ')
        {
            let back = if col % unit == 0 { unit } else { col % unit };
            return self.edit(cur - back, cur, "");
        }
        let start = if prev == '\n' && cur >= 2 && self.rope.char(cur - 2) == '\r' {
            cur - 2
        } else {
            cur - 1
        };
        self.edit(start, cur, "");
    }

    pub fn delete(&mut self) {
        if let Some((s, e)) = self.selection() {
            return self.edit(s, e, "");
        }
        let cur = self.cursor;
        if cur >= self.len() {
            return;
        }
        let end = if self.rope.char(cur) == '\r' && self.char_at(cur + 1) == Some('\n') {
            cur + 2
        } else {
            cur + 1
        };
        self.edit(cur, end, "");
        self.cursor = cur;
    }

    pub fn delete_word_back(&mut self) {
        if self.selection().is_none() {
            let end = self.cursor;
            self.word_left(false);
            let start = self.cursor;
            self.edit(start, end, "");
        } else {
            self.backspace();
        }
    }

    /// First and last line touched by the selection (or the cursor line).
    pub fn selected_lines(&self) -> (usize, usize) {
        match self.selection() {
            Some((s, e)) => {
                let (l0, _) = self.pos(s);
                let (l1, c1) = self.pos(e);
                (l0, if c1 == 0 && l1 > l0 { l1 - 1 } else { l1 })
            }
            None => {
                let l = self.pos(self.cursor).0;
                (l, l)
            }
        }
    }

    /// Edit inside a transaction while keeping cursor/anchor attached to the
    /// text they were next to.
    fn edit_keeping(&mut self, start: usize, end: usize, text: &str, marks: &mut [usize; 2]) {
        let n = text.chars().count();
        for m in marks.iter_mut() {
            // A mark sitting exactly where text is inserted stays in front of it.
            if *m >= end && (*m > start || end > start) {
                *m = *m + n - (end - start);
            } else if *m > start {
                *m = start + n;
            }
        }
        self.edit(start, end, text);
    }

    fn with_marks(&mut self, f: impl FnOnce(&mut Self, &mut [usize; 2])) {
        let had_sel = self.selection().is_some();
        let mut marks = [self.anchor.unwrap_or(self.cursor), self.cursor];
        self.transaction(|b| f(b, &mut marks));
        self.cursor = marks[1].min(self.len());
        self.anchor = had_sel.then_some(marks[0].min(self.len()));
    }

    pub fn indent_lines(&mut self) {
        let (l0, l1) = self.selected_lines();
        let unit = self.indent.clone();
        self.with_marks(|b, marks| {
            for line in l0..=l1 {
                if b.line_len(line) > 0 {
                    let at = b.rope.line_to_char(line);
                    b.edit_keeping(at, at, &unit, marks);
                }
            }
        });
    }

    pub fn dedent_lines(&mut self) {
        let (l0, l1) = self.selected_lines();
        let unit = self.indent.chars().count();
        self.with_marks(|b, marks| {
            for line in l0..=l1 {
                let at = b.rope.line_to_char(line);
                let mut n = 0;
                for c in b.rope.line(line).chars() {
                    if c == '\t' && n == 0 {
                        n = 1;
                        break;
                    }
                    if c == ' ' && n < unit {
                        n += 1;
                    } else {
                        break;
                    }
                }
                if n > 0 {
                    b.edit_keeping(at, at + n, "", marks);
                }
            }
        });
    }

    /// Tab key: indent the selected lines, or insert indentation at the cursor.
    pub fn tab(&mut self) {
        let (l0, l1) = self.selected_lines();
        if self.selection().is_some() && l0 != l1 {
            return self.indent_lines();
        }
        if self.indent == "\t" {
            return self.insert("\t");
        }
        let unit = self.indent.len();
        let (_, col) = self.pos(self.selection().map_or(self.cursor, |s| s.0));
        self.insert(&" ".repeat(unit - col % unit));
    }

    pub fn toggle_comment(&mut self) {
        let Some(token) = self.lang.map(|l| l.comment).filter(|c| !c.is_empty()) else {
            return;
        };
        let (l0, l1) = self.selected_lines();
        let text_of = |b: &Buffer, l: usize| b.rope.line(l).to_string();
        let all_commented = (l0..=l1)
            .map(|l| text_of(self, l))
            .filter(|t| !t.trim().is_empty())
            .all(|t| t.trim_start().starts_with(token));
        let min_indent = (l0..=l1)
            .map(|l| text_of(self, l))
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.chars().take_while(|c| *c == ' ' || *c == '\t').count())
            .min()
            .unwrap_or(0);
        let tlen = token.chars().count();
        self.with_marks(|b, marks| {
            for line in l0..=l1 {
                let t = text_of(b, line);
                if t.trim().is_empty() {
                    continue;
                }
                let ls = b.rope.line_to_char(line);
                if all_commented {
                    let ws = t.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                    let space = t.chars().nth(ws + tlen) == Some(' ');
                    b.edit_keeping(ls + ws, ls + ws + tlen + space as usize, "", marks);
                } else {
                    b.edit_keeping(
                        ls + min_indent,
                        ls + min_indent,
                        &format!("{token} "),
                        marks,
                    );
                }
            }
        });
    }

    // ---- movement ----

    fn go(&mut self, to: usize, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = to.min(self.len());
        self.coalesce = false;
    }

    pub fn move_to(&mut self, to: usize, select: bool) {
        self.goal = None;
        self.go(to, select);
    }

    pub fn left(&mut self, select: bool) {
        self.goal = None;
        if let (false, Some((s, _))) = (select, self.selection()) {
            return self.go(s, false);
        }
        let (l, c) = self.pos(self.cursor);
        if c > 0 {
            self.go(self.idx(l, c - 1), select);
        } else if l > 0 {
            self.go(self.idx(l - 1, usize::MAX), select);
        } else {
            self.go(0, select);
        }
    }

    pub fn right(&mut self, select: bool) {
        self.goal = None;
        if let (false, Some((_, e))) = (select, self.selection()) {
            return self.go(e, false);
        }
        let (l, c) = self.pos(self.cursor);
        if c < self.line_len(l) {
            self.go(self.idx(l, c + 1), select);
        } else if l + 1 < self.lines() {
            self.go(self.idx(l + 1, 0), select);
        } else {
            self.go(self.cursor, select);
        }
    }

    pub fn vertical(&mut self, delta: isize, select: bool) {
        let (l, c) = self.pos(self.cursor);
        let goal = *self.goal.get_or_insert(c);
        let target = l as isize + delta;
        if target < 0 {
            self.go(0, select);
        } else if target as usize >= self.lines() {
            self.go(self.len(), select);
        } else {
            self.go(self.idx(target as usize, goal), select);
        }
    }

    pub fn word_left(&mut self, select: bool) {
        self.goal = None;
        let mut i = self.cursor;
        while i > 0 && self.rope.char(i - 1).is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let k = class(self.rope.char(i - 1));
            while i > 0 && class(self.rope.char(i - 1)) == k {
                i -= 1;
            }
        }
        self.go(i, select);
    }

    pub fn word_right(&mut self, select: bool) {
        self.goal = None;
        let n = self.len();
        let mut i = self.cursor;
        while i < n && self.rope.char(i).is_whitespace() {
            i += 1;
        }
        if i < n {
            let k = class(self.rope.char(i));
            while i < n && class(self.rope.char(i)) == k {
                i += 1;
            }
        }
        self.go(i, select);
    }

    /// Home: first non-blank character, or column 0 if already there.
    pub fn home(&mut self, select: bool) {
        self.goal = None;
        let (l, c) = self.pos(self.cursor);
        let ws = self
            .rope
            .line(l)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count()
            .min(self.line_len(l));
        self.go(self.idx(l, if c == ws { 0 } else { ws }), select);
    }

    pub fn end(&mut self, select: bool) {
        self.goal = None;
        let (l, _) = self.pos(self.cursor);
        self.go(self.idx(l, usize::MAX), select);
    }

    pub fn select_all(&mut self) {
        self.select(0, self.len());
    }

    /// Start of the identifier ending at `idx`.
    pub fn word_start(&self, idx: usize) -> usize {
        let mut i = idx;
        while i > 0 && is_word(self.rope.char(i - 1)) {
            i -= 1;
        }
        i
    }

    pub fn select_word_at(&mut self, idx: usize) {
        let n = self.len();
        if n == 0 {
            return;
        }
        let i = idx.min(n - 1);
        let k = class(self.rope.char(i));
        let (mut s, mut e) = (i, i);
        while s > 0 && class(self.rope.char(s - 1)) == k && self.rope.char(s - 1) != '\n' {
            s -= 1;
        }
        while e < n && class(self.rope.char(e)) == k && self.rope.char(e) != '\n' {
            e += 1;
        }
        self.select(s, e);
    }

    pub fn select_line_at(&mut self, idx: usize) {
        let (l, _) = self.pos(idx);
        let s = self.rope.line_to_char(l);
        let e = if l + 1 < self.lines() {
            self.rope.line_to_char(l + 1)
        } else {
            self.len()
        };
        self.select(s, e);
    }
}

pub fn char_width(c: char, x: usize, tabw: usize) -> usize {
    if c == '\t' {
        tabw - x % tabw
    } else {
        c.width().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s, None)
    }

    #[test]
    fn typing_coalesces_into_word_sized_undo_steps() {
        let mut b = buf("");
        for c in "hello world".chars() {
            b.type_char(c);
        }
        assert_eq!(b.rope.to_string(), "hello world");
        b.undo();
        assert_eq!(b.rope.to_string(), "hello ");
        b.undo();
        assert_eq!(b.rope.to_string(), "");
        b.redo();
        b.redo();
        assert_eq!(b.rope.to_string(), "hello world");
    }

    #[test]
    fn auto_pairs_and_types_over_closer() {
        let mut b = buf("");
        b.type_char('(');
        assert_eq!(b.rope.to_string(), "()");
        b.type_char('x');
        b.type_char(')');
        assert_eq!(b.rope.to_string(), "(x)");
        assert_eq!(b.cursor, 3);
        let mut b = buf("");
        b.type_char('{');
        b.backspace();
        assert_eq!(b.rope.to_string(), "");
    }

    fn typed(lang: &str, text: &str) -> Buffer {
        let mut b = Buffer::from_text("", None);
        b.lang = crate::highlight::langs().iter().find(|l| l.id == lang);
        for c in text.chars() {
            b.type_char(c);
        }
        b
    }

    #[test]
    fn closes_tags_in_html_and_jsx() {
        let b = typed("html", "<h1>");
        assert_eq!(b.rope.to_string(), "<h1></h1>");
        assert_eq!(b.cursor, 4);
        let b = typed("typescriptreact", "<Foo.Bar id=\"x\" on={() => go(a > b)}>");
        assert!(b.rope.to_string().ends_with("></Foo.Bar>"), "{}", b.rope);
        assert_eq!(typed("javascript", "<>").rope.to_string(), "<></>");
        let mut b = typed("html", "<div>");
        b.newline();
        assert_eq!(b.rope.to_string(), "<div>\n    \n</div>");
        assert_eq!(b.pos(b.cursor), (1, 4));
    }

    #[test]
    fn leaves_non_tags_alone() {
        for (lang, text) in [
            ("html", "<br>"),
            ("html", "<img src=\"a.png\" />"),
            ("html", "</p>"),
            ("javascript", "a<b>"),
            ("javascript", "if (a <b && c >"),
            ("javascript", "x => y"),
            ("typescript", "<T>"),
            ("rust", "<T>"),
        ] {
            let b = typed(lang, text);
            assert!(
                !b.rope.to_string()[text.len()..].contains("</"),
                "{lang}: {}",
                b.rope
            );
        }
    }

    #[test]
    fn newline_keeps_indent_and_opens_blocks() {
        let mut b = buf("    foo");
        b.move_to(7, false);
        b.newline();
        assert_eq!(b.rope.to_string(), "    foo\n    ");
        let mut b = buf("fn a() {}");
        b.move_to(8, false);
        b.newline();
        assert_eq!(b.rope.to_string(), "fn a() {\n    \n}");
        assert_eq!(b.pos(b.cursor), (1, 4));
    }

    #[test]
    fn selection_movement_and_replace() {
        let mut b = buf("one two three");
        b.word_right(true);
        assert_eq!(b.selected_text().unwrap(), "one");
        b.insert("1");
        assert_eq!(b.rope.to_string(), "1 two three");
        b.end(true);
        b.backspace();
        assert_eq!(b.rope.to_string(), "1");
        b.undo();
        assert_eq!(b.rope.to_string(), "1 two three");
        assert!(b.dirty);
        b.undo();
        assert!(
            !b.dirty,
            "undoing back to the saved text clears the dirty flag"
        );
    }

    #[test]
    fn indent_dedent_is_one_undo_step() {
        let mut b = buf("a\nb\nc");
        b.select_all();
        b.tab();
        assert_eq!(b.rope.to_string(), "    a\n    b\n    c");
        assert_eq!(b.selected_text().unwrap(), "    a\n    b\n    c");
        b.dedent_lines();
        assert_eq!(b.rope.to_string(), "a\nb\nc");
        b.undo();
        assert_eq!(b.rope.to_string(), "    a\n    b\n    c");
    }

    #[test]
    fn toggle_comment_round_trips() {
        let mut b = Buffer::from_text("fn a() {\n    x();\n}\n", Some("t.rs".into()));
        b.select(0, b.len());
        b.toggle_comment();
        assert_eq!(b.rope.to_string(), "// fn a() {\n//     x();\n// }\n");
        b.toggle_comment();
        assert_eq!(b.rope.to_string(), "fn a() {\n    x();\n}\n");
    }

    #[test]
    fn display_columns_handle_tabs_and_wide_chars() {
        let b = buf("\tab\n日本x");
        assert_eq!(b.col_to_x(0, 1, 4), 4);
        assert_eq!(b.x_to_col(0, 5, 4), 2);
        assert_eq!(b.col_to_x(1, 2, 4), 4);
        assert_eq!(b.x_to_col(1, 4, 4), 2);
    }

    #[test]
    fn word_and_line_selection() {
        let mut b = buf("let foo_bar = 1;\nnext");
        b.select_word_at(6);
        assert_eq!(b.selected_text().unwrap(), "foo_bar");
        b.select_line_at(2);
        assert_eq!(b.selected_text().unwrap(), "let foo_bar = 1;\n");
    }
}
