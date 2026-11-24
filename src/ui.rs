//! Drawing. Everything is painted straight into ratatui's cell buffer, and
//! the rectangles used are saved in `app.layout` so mouse clicks can be
//! mapped back to what was drawn there.

use crate::app::{App, Focus, Row, SETTINGS_ROWS, SettingsRow, from_lsp_pos};
use crate::buffer::char_width;
use crate::commands::Command;
use crate::highlight::{Syntax, Token};
use crate::menu::MenuEntry;
use crate::palette::MAX_ROWS;
use ratatui::Frame;
use ratatui::buffer::Buffer as Buf;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

fn fill(buf: &mut Buf, r: Rect, style: Style) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_style(style);
        }
    }
}

/// Draw `s` clipped to `max_w` cells; returns the x after the last cell.
fn text(buf: &mut Buf, x: u16, y: u16, s: &str, max_w: u16, style: Style) -> u16 {
    if max_w == 0 || y >= buf.area.bottom() || x >= buf.area.right() {
        return x;
    }
    let max_w = max_w.min(buf.area.right() - x);
    buf.set_stringn(x, y, s, max_w as usize, style).0
}

fn right_text(buf: &mut Buf, right: u16, y: u16, s: &str, min_x: u16, style: Style) {
    let w = s.width() as u16;
    if right >= min_x + w {
        text(buf, right - w, y, s, w, style);
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < 24 || area.height < 6 {
        return;
    }
    let buf = f.buffer_mut();
    fill(buf, area, Style::new().bg(app.theme.bg).fg(app.theme.fg));

    let main_h = area.height - 1;
    let tree_w = if app.show_tree {
        app.tree_width.min(area.width - 20)
    } else {
        0
    };
    let has_buf = !app.buffers.is_empty();
    let find_h = (app.find.is_some() && has_buf) as u16;
    app.layout.width = area.width;
    app.layout.tree = Rect::new(0, 0, tree_w, main_h);
    app.layout.tabs_y = 0;
    app.layout.editor = Rect::new(
        tree_w,
        1 + find_h,
        area.width - tree_w,
        main_h.saturating_sub(1 + find_h),
    );
    app.layout.completion = Rect::default();
    app.layout.palette_list = Rect::default();
    app.layout.shortcuts = Rect::default();

    if tree_w > 0 {
        draw_tree(buf, app);
    }
    draw_tabs(buf, app, Rect::new(tree_w, 0, area.width - tree_w, 1));
    let mut find_cursor = None;
    if app.settings_panel.is_some() {
        app.layout.cursor = None;
        draw_settings_panel(buf, app);
    } else if app.search_panel.is_some() {
        app.layout.cursor = draw_search_panel(buf, app);
    } else if has_buf {
        app.layout.cursor = draw_editor(buf, app);
        if find_h > 0 {
            find_cursor = draw_find(buf, app, Rect::new(tree_w, 1, area.width - tree_w, 1));
        }
        draw_completion(buf, app, area);
        draw_hover(buf, app, area);
    } else {
        app.layout.cursor = None;
        draw_welcome(buf, app);
    }
    draw_status(buf, app, Rect::new(0, main_h, area.width, 1));
    draw_shortcuts(buf, app, Rect::new(0, 0, area.width, main_h));
    let palette_cursor = draw_palette(buf, app, area);
    draw_context_menu(buf, app, area);

    let cursor = palette_cursor.or(find_cursor).or(
        if app.search_panel.is_some() || app.focus == Focus::Editor {
            app.layout.cursor
        } else {
            None
        },
    );
    let blinked_out = app.cfg.cursor_blink && app.blink_off;
    let covered = app.menu.is_some() || app.shortcuts.is_some();
    if let Some(pos) = cursor.filter(|_| !covered && !blinked_out) {
        f.set_cursor_position(pos);
    }
}

fn draw_tree(buf: &mut Buf, app: &mut App) {
    let r = app.layout.tree;
    let th = &app.theme;
    let w = r.width - 1;
    fill(buf, r, Style::new().bg(th.panel).fg(th.fg));
    for y in r.top()..r.bottom() {
        buf[(r.right() - 1, y)]
            .set_symbol("│")
            .set_style(Style::new().fg(th.border).bg(th.panel));
    }
    let title = app
        .root
        .file_name()
        .map(|n| n.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    text(
        buf,
        1,
        0,
        &title,
        w.saturating_sub(1),
        Style::new().fg(th.dim).add_modifier(Modifier::BOLD),
    );

    let rows = (r.height - 1) as usize;
    if app.tree_follow {
        app.tree.scroll_into_view(rows);
        app.tree_follow = false;
    }
    app.tree.scroll = app.tree.scroll.min(
        app.tree
            .nodes
            .len()
            .saturating_sub(rows.min(app.tree.nodes.len())),
    );
    let active = app.buffers.get(app.active).and_then(|b| b.path.as_ref());
    for (i, node) in app
        .tree
        .nodes
        .iter()
        .enumerate()
        .skip(app.tree.scroll)
        .take(rows)
    {
        let y = 1 + (i - app.tree.scroll) as u16;
        let selected = i == app.tree.sel;
        let is_active = Some(&node.path) == active;
        let bg = if selected && app.focus == Focus::Tree {
            th.selection
        } else if selected || is_active {
            th.line
        } else {
            th.panel
        };
        fill(buf, Rect::new(0, y, w, 1), Style::new().bg(bg));
        let worst = app
            .diagnostics
            .get(&node.path)
            .and_then(|d| d.iter().map(|d| d.severity).min());
        let fg = match worst {
            Some(1) => th.error,
            Some(2) => th.warning,
            _ if node.name.starts_with('.') => th.dim,
            _ => th.fg,
        };
        let mut style = Style::new().fg(fg).bg(bg);
        if is_active {
            style = style.add_modifier(Modifier::BOLD);
        }
        let x = 1 + (node.depth * 2) as u16;
        if x + 4 < w {
            let chevron = if !node.is_dir {
                "  "
            } else if node.expanded {
                "▾ "
            } else {
                "▸ "
            };
            text(buf, x, y, chevron, 2, Style::new().fg(th.dim).bg(bg));
            let devicon = devicons::icon_for_file(&node.path, &Some(devicons::Theme::Dark));
            let icon_fg = if selected && app.focus == Focus::Tree {
                fg
            } else {
                crate::theme::parse_hex(devicon.color)
            };
            let mut icon_buf = [0u8; 4];
            let icon_str = devicon.icon.encode_utf8(&mut icon_buf);
            text(buf, x + 2, y, icon_str, 1, Style::new().fg(icon_fg).bg(bg));
            text(buf, x + 4, y, &node.name, w - x - 4, style);
        }
    }
}

fn draw_tabs(buf: &mut Buf, app: &mut App, r: Rect) {
    let th = &app.theme;
    fill(buf, r, Style::new().bg(th.panel).fg(th.dim));
    app.layout.tabs.clear();
    let labels: Vec<String> = app
        .buffers
        .iter()
        .map(|b| format!(" {} {} ", b.name(), if b.dirty { "●" } else { "×" }))
        .collect();
    // Skip leading tabs if needed so the active one is always visible.
    let mut first = 0;
    while first < app.active
        && labels[first..=app.active]
            .iter()
            .map(|l| l.width())
            .sum::<usize>()
            > r.width as usize
    {
        first += 1;
    }
    let mut x = r.x;
    for (i, label) in labels.iter().enumerate().skip(first) {
        if x >= r.right() {
            break;
        }
        let active = i == app.active;
        let mut style = if active {
            Style::new().bg(th.bg).fg(th.fg)
        } else {
            Style::new().bg(th.panel).fg(th.dim)
        };
        if active && app.focus == Focus::Editor {
            style = style.add_modifier(Modifier::BOLD);
        }
        let end = text(buf, x, r.y, label, r.right() - x, style);
        // The label ends in "● " when unsaved; colour just the dot, if it fit.
        if app.buffers[i].dirty && end >= x + 2 && buf[(end - 2, r.y)].symbol() == "●" {
            buf[(end - 2, r.y)].set_fg(th.modified);
        }
        app.layout.tabs.push((x, end, i));
        x = end;
    }
}

fn draw_editor(buf: &mut Buf, app: &mut App) -> Option<(u16, u16)> {
    let area = app.layout.editor;
    if area.width < 8 || area.height == 0 {
        return None;
    }
    let tabw = app.cfg.tab_width;
    let focused = app.focus == Focus::Editor;
    let link = app.link_word;
    let App {
        buffers,
        theme: th,
        diagnostics,
        find,
        layout,
        follow,
        active,
        ..
    } = app;
    let b = &mut buffers[*active];

    // Re-parse for highlighting if the text changed since the last draw.
    let grammar = b.lang.and_then(|l| l.grammar());
    if let Some(g) = grammar {
        if b.syntax.as_ref().is_none_or(|s| s.version != b.version) {
            b.syntax = if b.rope.len_bytes() < 2_000_000 {
                Syntax::parse(g, b.rope.to_string(), b.version)
            } else {
                None
            };
        }
    }

    let lines = b.lines();
    let show_nums = app.cfg.show_line_numbers;
    let digits = if show_nums {
        lines.to_string().len().max(3) as u16
    } else {
        0
    };
    let gutter = if show_nums { digits + 3 } else { 2 };
    layout.gutter = gutter;
    let x0 = area.x + gutter;
    let tw = (area.width - gutter) as usize;
    let h = area.height as usize;
    let (cl, cc) = b.pos(b.cursor);
    let cx = b.col_to_x(cl, cc, tabw);

    if *follow {
        let margin = 3.min(h.saturating_sub(1) / 2);
        if cl < b.scroll_row + margin {
            b.scroll_row = cl.saturating_sub(margin);
        } else if cl + margin >= b.scroll_row + h {
            b.scroll_row = cl + margin + 1 - h;
        }
        if cx < b.scroll_col {
            b.scroll_col = cx.saturating_sub(8);
        } else if cx + 1 >= b.scroll_col + tw {
            b.scroll_col = cx + 9 - tw.min(cx + 9);
        }
        *follow = false;
    }
    b.scroll_row = b.scroll_row.min(lines - 1);
    let (top, left) = (b.scroll_row, b.scroll_col);
    let last = (top + h).min(lines);
    let vis_start = b.rope.line_to_char(top);
    let vis_end = if last >= lines {
        b.len()
    } else {
        b.rope.line_to_char(last)
    };

    // Per-character syntax tokens for the visible range.
    let mut tokens: Vec<Option<Token>> = vec![None; vis_end - vis_start];
    if let (Some(g), Some(syn)) = (
        grammar,
        b.syntax.as_ref().filter(|s| s.version == b.version),
    ) {
        let bytes = b.rope.char_to_byte(vis_start)..b.rope.char_to_byte(vis_end);
        for (s, e, tok) in syn.spans(g, bytes) {
            let s = b.rope.byte_to_char(s).max(vis_start);
            let e = b.rope.byte_to_char(e).min(vis_end);
            if s < e {
                tokens[s - vis_start..e - vis_start].fill(Some(tok));
            }
        }
    }

    // Diagnostics: underline ranges, plus the worst one per line for the gutter.
    let mut underline: Vec<u8> = vec![0; vis_end - vis_start];
    let mut line_diag: HashMap<usize, (u8, &str)> = HashMap::new();
    if let Some(diags) = b.path.as_ref().and_then(|p| diagnostics.get(p)) {
        for d in diags.iter().filter(|d| d.end.0 >= top && d.start.0 < last) {
            let s = from_lsp_pos(b, d.start.0, d.start.1);
            let e = from_lsp_pos(b, d.end.0, d.end.1).max(s + 1);
            for i in s.max(vis_start)..e.min(vis_end) {
                let u = &mut underline[i - vis_start];
                if *u == 0 || d.severity < *u {
                    *u = d.severity;
                }
            }
            let entry = line_diag
                .entry(d.start.0)
                .or_insert((d.severity, &d.message));
            if d.severity < entry.0 {
                *entry = (d.severity, &d.message);
            }
        }
    }

    let sel = b.selection();
    let matches: &[(usize, usize)] = find.as_ref().map_or(&[], |f| &f.matches);
    let mut mi = matches.partition_point(|m| m.1 <= vis_start);

    for row in 0..h {
        let line = top + row;
        if line >= lines {
            break;
        }
        let y = area.y + row as u16;
        let is_cur = line == cl;
        let line_bg = if is_cur && sel.is_none() && focused {
            th.line
        } else {
            th.bg
        };
        fill(
            buf,
            Rect::new(area.x, y, area.width, 1),
            Style::new().bg(line_bg).fg(th.fg),
        );

        // Gutter: diagnostic marker and line number.
        let diag = line_diag.get(&line);
        if let Some((sev, _)) = diag {
            text(
                buf,
                area.x,
                y,
                "●",
                1,
                Style::new().fg(th.severity(*sev)).bg(line_bg),
            );
        }
        if show_nums {
            let num_style = Style::new()
                .fg(if is_cur { th.fg } else { th.line_no })
                .bg(line_bg);
            text(
                buf,
                area.x + 1,
                y,
                &format!("{:>w$}", line + 1, w = digits as usize),
                digits,
                num_style,
            );
        }

        let ls = b.rope.line_to_char(line);
        let mut x = 0usize;
        let mut end_idx = ls;
        let mut tmp = [0u8; 4];
        for (i, ch) in b.rope.line(line).chars().enumerate() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            let idx = ls + i;
            end_idx = idx + 1;
            let w = char_width(ch, x, tabw);
            if w == 0 {
                continue;
            }
            if x + w > left && x < left + tw {
                while mi < matches.len() && matches[mi].1 <= idx {
                    mi += 1;
                }
                let in_match = mi < matches.len() && matches[mi].0 <= idx;
                let selected = sel.is_some_and(|(s, e)| idx >= s && idx < e);
                let bg = if selected {
                    th.selection
                } else if in_match {
                    th.find
                } else {
                    line_bg
                };
                let fg = tokens[idx - vis_start].map_or(th.fg, |t| th.token(t));
                let mut style = Style::new().fg(fg).bg(bg);
                let sev = underline[idx - vis_start];
                if sev > 0 {
                    style = style.add_modifier(Modifier::UNDERLINED);
                    if sev == 1 {
                        style = style.fg(th.error);
                    }
                }
                if tokens[idx - vis_start] == Some(Token::Comment) {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if link.is_some_and(|(s, e)| idx >= s && idx < e) {
                    style = style.fg(th.accent).add_modifier(Modifier::UNDERLINED);
                }
                let sx = x0 + (x.max(left) - left) as u16;
                let fully_visible = x >= left && x + w <= left + tw;
                if ch == '\t' || !fully_visible {
                    let cells = ((x + w).min(left + tw) - x.max(left)) as u16;
                    fill(buf, Rect::new(sx, y, cells, 1), Style::new().bg(bg));
                } else {
                    text(buf, sx, y, ch.encode_utf8(&mut tmp), w as u16, style);
                }
            }
            x += w;
            if x >= left + tw {
                break;
            }
        }
        // Show that the line break itself is selected.
        if x >= left && x < left + tw && sel.is_some_and(|(s, e)| end_idx >= s && end_idx < e) {
            fill(
                buf,
                Rect::new(x0 + (x - left) as u16, y, 1, 1),
                Style::new().bg(th.selection),
            );
        }
        // Inline diagnostic message after the end of the line.
        if let Some((sev, msg)) = diag {
            let start = x.max(left) - left + 3;
            if start + 8 < tw {
                let msg = msg.lines().next().unwrap_or("");
                let style = Style::new()
                    .fg(th.severity(*sev))
                    .bg(line_bg)
                    .add_modifier(Modifier::DIM);
                text(buf, x0 + start as u16, y, msg, (tw - start) as u16, style);
            }
        }
    }

    (cl >= top && cl < top + h && cx >= left && cx < left + tw)
        .then(|| (x0 + (cx - left) as u16, area.y + (cl - top) as u16))
}

fn draw_find(buf: &mut Buf, app: &App, r: Rect) -> Option<(u16, u16)> {
    let th = &app.theme;
    let f = app.find.as_ref()?;
    fill(buf, r, Style::new().bg(th.popup).fg(th.fg));
    let x = text(
        buf,
        r.x + 1,
        r.y,
        "Find ",
        r.width.saturating_sub(1),
        Style::new().fg(th.accent).bg(th.popup),
    );
    let end = text(
        buf,
        x,
        r.y,
        &f.query,
        r.right().saturating_sub(x + 1),
        Style::new().fg(th.fg).bg(th.popup),
    );
    let info = match f.matches.len() {
        _ if f.query.is_empty() => "type to search · Esc to close ".to_string(),
        0 => "no matches ".to_string(),
        n => format!("{n} matches · Enter next · Shift+Enter previous · Esc close "),
    };
    right_text(
        buf,
        r.right(),
        r.y,
        &info,
        end + 2,
        Style::new().fg(th.dim).bg(th.popup),
    );
    f.focused.then_some((end, r.y))
}

/// Draws the project-search panel in place of the editor. Returns the
/// query-input cursor position.
fn draw_search_panel(buf: &mut Buf, app: &mut App) -> Option<(u16, u16)> {
    let area = app.layout.editor;
    if area.height == 0 || area.width < 8 {
        return None;
    }
    let th = &app.theme;
    fill(buf, area, Style::new().bg(th.bg).fg(th.fg));

    let input_bg = Style::new().bg(th.popup_sel);
    fill(buf, Rect::new(area.x, area.y, area.width, 1), input_bg);
    let lx = text(
        buf,
        area.x + 1,
        area.y,
        "Search in files  ",
        area.width.saturating_sub(1),
        input_bg.fg(th.accent),
    );
    let end;
    {
        let Some(ps) = &app.search_panel else {
            return None;
        };
        end = if ps.query.is_empty() {
            text(
                buf,
                lx,
                area.y,
                "type a query, Enter to search",
                area.right().saturating_sub(lx + 1),
                input_bg.fg(th.dim),
            );
            lx
        } else {
            text(
                buf,
                lx,
                area.y,
                &ps.query,
                area.right().saturating_sub(lx + 1),
                input_bg.fg(th.fg),
            )
        };
        let info = if ps.rows.is_empty() {
            if ps.query.is_empty() {
                String::new()
            } else {
                "no matches".to_string()
            }
        } else {
            let files = ps
                .rows
                .iter()
                .filter(|r| matches!(r, Row::Header(_)))
                .count();
            format!(
                "{} match{} in {} file{}",
                ps.hits.len(),
                if ps.hits.len() == 1 { "" } else { "es" },
                files,
                if files == 1 { "" } else { "s" }
            )
        };
        right_text(
            buf,
            area.right(),
            area.y,
            &format!("{info}  "),
            end + 2,
            input_bg.fg(th.dim),
        );
    }

    let list = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    app.layout.search_list = list;
    let rows_h = list.height as usize;
    if app.search_follow {
        let Some(ps) = &mut app.search_panel else {
            return None;
        };
        if ps.sel < ps.scroll {
            ps.scroll = ps.sel;
        } else if rows_h > 0 && ps.sel >= ps.scroll + rows_h {
            ps.scroll = ps.sel + 1 - rows_h;
        }
        app.search_follow = false;
    }

    let Some(ps) = &app.search_panel else {
        return None;
    };
    for (i, row) in ps.rows.iter().enumerate().skip(ps.scroll).take(rows_h) {
        let y = list.y + (i - ps.scroll) as u16;
        match row {
            Row::Header(path) => {
                fill(
                    buf,
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(th.panel),
                );
                text(
                    buf,
                    list.x + 1,
                    y,
                    path,
                    list.width.saturating_sub(2),
                    Style::new()
                        .fg(th.accent)
                        .bg(th.panel)
                        .add_modifier(Modifier::BOLD),
                );
            }
            Row::Hit(hi) => {
                let hit = &ps.hits[*hi];
                let bg = if i == ps.sel { th.selection } else { th.bg };
                fill(
                    buf,
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(bg),
                );
                let prefix = format!("  {}:{}  ", hit.line + 1, hit.col + 1);
                let mut cx = text(
                    buf,
                    list.x,
                    y,
                    &prefix,
                    list.width,
                    Style::new().fg(th.dim).bg(bg),
                );
                for (ci, c) in hit.text.chars().enumerate() {
                    if cx >= list.right() {
                        break;
                    }
                    let hl = ci >= hit.col && ci < hit.end_col;
                    let mut st = Style::new().fg(if hl { th.accent } else { th.fg }).bg(bg);
                    if hl {
                        st = st.add_modifier(Modifier::BOLD);
                    }
                    cx = text(buf, cx, y, c.encode_utf8(&mut [0; 4]), 1, st);
                }
            }
        }
    }
    Some((end, area.y))
}

fn draw_settings_panel(buf: &mut Buf, app: &mut App) {
    let area = app.layout.editor;
    if area.height == 0 || area.width < 8 {
        return;
    }
    let th = &app.theme;
    fill(buf, area, Style::new().bg(th.bg).fg(th.fg));

    fill(
        buf,
        Rect::new(area.x, area.y, area.width, 1),
        Style::new().bg(th.popup_sel),
    );
    text(
        buf,
        area.x + 1,
        area.y,
        "Settings",
        area.width.saturating_sub(1),
        Style::new()
            .fg(th.accent)
            .bg(th.popup_sel)
            .add_modifier(Modifier::BOLD),
    );
    right_text(
        buf,
        area.right(),
        area.y,
        "↑↓ move · ←→/Enter change · Esc close  ",
        area.x + 10,
        Style::new().fg(th.dim).bg(th.popup_sel),
    );

    let list = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    app.layout.settings_list = list;
    let rows_h = list.height as usize;

    let Some(panel) = &mut app.settings_panel else {
        return;
    };
    if panel.sel < panel.scroll {
        panel.scroll = panel.sel;
    } else if rows_h > 0 && panel.sel >= panel.scroll + rows_h {
        panel.scroll = panel.sel + 1 - rows_h;
    }
    let (sel, scroll) = (panel.sel, panel.scroll);
    let cfg = &app.cfg;

    for (i, row) in SETTINGS_ROWS.iter().enumerate().skip(scroll).take(rows_h) {
        let y = list.y + (i - scroll) as u16;
        match row {
            SettingsRow::Header(name) => {
                fill(
                    buf,
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(th.panel),
                );
                text(
                    buf,
                    list.x + 1,
                    y,
                    name,
                    list.width.saturating_sub(2),
                    Style::new()
                        .fg(th.accent)
                        .bg(th.panel)
                        .add_modifier(Modifier::BOLD),
                );
            }
            SettingsRow::Item(id) => {
                let bg = if i == sel { th.selection } else { th.bg };
                fill(
                    buf,
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(bg),
                );
                text(
                    buf,
                    list.x + 3,
                    y,
                    id.label(),
                    list.width.saturating_sub(4),
                    Style::new().fg(th.fg).bg(bg),
                );
                let value = id.value(cfg);
                right_text(
                    buf,
                    list.right(),
                    y,
                    &format!("{value}  "),
                    list.x + 3,
                    Style::new()
                        .fg(th.accent)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
}

fn draw_completion(buf: &mut Buf, app: &mut App, area: Rect) {
    let (Some(c), Some((cx, cy))) = (&app.completion, app.layout.cursor) else {
        return;
    };
    let th = &app.theme;
    let rows = c.shown.len().min(10);
    if rows == 0 {
        return;
    }
    let first = if c.sel >= rows { c.sel + 1 - rows } else { 0 };
    let visible = || c.shown[first..first + rows].iter().map(|i| &c.items[*i]);
    let label_w = visible()
        .map(|i| i.label.width())
        .max()
        .unwrap_or(0)
        .min(40);
    let detail_w = visible()
        .map(|i| i.detail.width())
        .max()
        .unwrap_or(0)
        .min(30);
    let w = ((label_w + detail_w + 4).max(24) as u16).min(area.width);
    let typed = app.buf().map_or(0, |b| b.cursor.saturating_sub(c.start)) as u16;
    let x = cx.saturating_sub(typed + 1).min(area.width - w);
    let below = cy + 1 + rows as u16 <= area.height - 1;
    let y = if below {
        cy + 1
    } else {
        cy.saturating_sub(rows as u16)
    };
    let rect = Rect::new(x, y, w, rows as u16);
    fill(buf, rect, Style::new().bg(th.popup).fg(th.fg));
    for (row, item) in visible().enumerate() {
        let y = y + row as u16;
        let bg = if first + row == c.sel {
            th.popup_sel
        } else {
            th.popup
        };
        fill(buf, Rect::new(x, y, w, 1), Style::new().bg(bg));
        text(
            buf,
            x + 1,
            y,
            &item.label,
            (label_w as u16).min(w - 2),
            Style::new().fg(th.fg).bg(bg),
        );
        let dx = x + 1 + label_w as u16 + 2;
        if dx < x + w - 1 {
            text(
                buf,
                dx,
                y,
                &item.detail,
                x + w - 1 - dx,
                Style::new().fg(th.dim).bg(bg),
            );
        }
    }
    app.layout.completion = rect;
    app.layout.completion_first = first;
}

/// Greedy word wrap that also hard-splits words longer than the width.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = vec![];
    for line in s.lines() {
        let mut cur = String::new();
        for word in line.split_inclusive(' ') {
            if cur.width() + word.trim_end().width() > width && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            for ch in word.chars() {
                if cur.width() + 1 > width {
                    out.push(std::mem::take(&mut cur));
                    if ch == ' ' {
                        continue;
                    }
                }
                cur.push(ch);
            }
        }
        out.push(cur);
    }
    out
}

/// Screen cell of char index `idx` in the active buffer, if it's in view.
fn screen_pos(app: &App, idx: usize) -> Option<(u16, u16)> {
    let b = app.buf()?;
    let e = app.layout.editor;
    let (line, col) = b.pos(idx);
    let x = b.col_to_x(line, col, app.cfg.tab_width);
    let row = line.checked_sub(b.scroll_row)?;
    let dx = x.checked_sub(b.scroll_col)?;
    let tw = e.width.saturating_sub(app.layout.gutter) as usize;
    (row < e.height as usize && dx < tw)
        .then(|| (e.x + app.layout.gutter + dx as u16, e.y + row as u16))
}

fn draw_hover(buf: &mut Buf, app: &mut App, area: Rect) {
    app.layout.hover = Rect::default();
    app.layout.hover_link = Rect::default();
    let anchor = match app.hover_at {
        Some(idx) => screen_pos(app, idx),
        None => app.layout.cursor,
    };
    let (Some(hover), Some((cx, cy))) = (&app.hover, anchor) else {
        return;
    };
    let th = &app.theme;
    let max_w = (area.width as usize).saturating_sub(4).min(76);
    let mut lines = wrap(hover, max_w);
    let above = cy.saturating_sub(app.layout.editor.y) as usize;
    let below = (area.height - 1).saturating_sub(cy + 1) as usize;
    let place_above = above >= lines.len().min(8) + 1 || above > below;
    // Leave the last row for the "Go to Definition" link.
    let room = if place_above { above } else { below }.min(17);
    lines.truncate(room.saturating_sub(1));
    if lines.is_empty() {
        return;
    }
    let key = app
        .keymap
        .binding(Command::GotoDefinition)
        .map_or(String::new(), |k| format!("  {k}"));
    let link = "Go to Definition";
    let link_w = (link.width() + key.width()) as u16;
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16).max(link_w) + 2;
    let hgt = lines.len() as u16 + 1;
    let x = cx.saturating_sub(1).min(area.width.saturating_sub(w));
    let y = if place_above { cy - hgt } else { cy + 1 };
    let rect = Rect::new(x, y, w, hgt);
    fill(buf, rect, Style::new().bg(th.popup).fg(th.fg));
    for (i, l) in lines.iter().enumerate() {
        text(
            buf,
            x + 1,
            y + i as u16,
            l,
            w - 2,
            Style::new().fg(th.fg).bg(th.popup),
        );
    }
    let ly = y + hgt - 1;
    let end = text(
        buf,
        x + 1,
        ly,
        link,
        w - 2,
        Style::new()
            .fg(th.accent)
            .bg(th.popup)
            .add_modifier(Modifier::UNDERLINED),
    );
    text(
        buf,
        end,
        ly,
        &key,
        x + w - 1 - end.min(x + w - 1),
        Style::new().fg(th.dim).bg(th.popup),
    );
    app.layout.hover = rect;
    app.layout.hover_link = Rect::new(x + 1, ly, link.width() as u16, 1);
}

/// The welcome screen's logo: solid block letters with a thin outline
/// offset behind them, in a rounded frame, coloured with a left-to-right
/// gradient (see `theme::logo_gradient`). Drawn as-is, minus its common
/// indentation and trailing spaces, so it centres properly.
const LOGO: &str = r#"
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
"#;

fn draw_welcome(buf: &mut Buf, app: &App) {
    let th = &app.theme;
    let r = app.layout.editor;
    let hints = [
        (Command::FileFinder, "Go to file"),
        (Command::CommandPalette, "All commands"),
        (Command::SearchInFiles, "Search in files"),
        (Command::FocusSidebar, "Focus the file tree"),
        (Command::NewFile, "New file"),
        (Command::Quit, "Quit"),
    ];
    let lines: Vec<&str> = LOGO.lines().filter(|l| !l.trim().is_empty()).collect();
    let indent = lines
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let logo: Vec<&str> = lines.iter().map(|l| l[indent..].trim_end()).collect();
    let logo_w = logo.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let tagline = format!(
        "A light and nimble terminal text editor · v{}",
        env!("CARGO_PKG_VERSION")
    );
    // Logo, blank, tagline, blank, hints; or just the name if that won't fit.
    let big = r.width >= logo_w + 4 && r.height >= logo.len() as u16 + 4 + hints.len() as u16;
    let head_h = if big { logo.len() as u16 + 3 } else { 2 };
    let top = r.y + r.height.saturating_sub(head_h + hints.len() as u16) / 2;
    let mid = r.x + r.width / 2;
    if big {
        let x = mid.saturating_sub(logo_w / 2);
        // Coloured column by column, so the gradient runs left to right
        // across the frame and letters alike.
        let span = logo_w.saturating_sub(1).max(1) as f32;
        for (i, line) in logo.iter().enumerate() {
            for (dx, ch) in line.chars().enumerate() {
                let fg = crate::theme::logo_gradient(dx as f32 / span);
                let mut tmp = [0u8; 4];
                let s = ch.encode_utf8(&mut tmp);
                text(
                    buf,
                    x + dx as u16,
                    top + i as u16,
                    s,
                    1,
                    Style::new().fg(fg),
                );
            }
        }
        let tw = tagline.width() as u16;
        text(
            buf,
            mid.saturating_sub(tw / 2),
            top + logo.len() as u16 + 1,
            &tagline,
            r.width,
            Style::new().fg(th.dim),
        );
    } else {
        text(
            buf,
            mid.saturating_sub(2),
            top,
            "kite",
            4,
            Style::new().fg(th.accent).add_modifier(Modifier::BOLD),
        );
    }
    for (i, (cmd, label)) in hints.iter().enumerate() {
        let y = top + head_h + i as u16;
        if y >= r.bottom() {
            break;
        }
        let key = app.keymap.binding(*cmd).unwrap_or_default();
        right_text(buf, mid - 1, y, &key, r.x, Style::new().fg(th.fg));
        text(
            buf,
            mid + 1,
            y,
            label,
            r.right().saturating_sub(mid + 1),
            Style::new().fg(th.dim),
        );
    }
}

fn draw_status(buf: &mut Buf, app: &mut App, r: Rect) {
    let th = &app.theme;
    let base = Style::new().bg(th.panel).fg(th.dim);
    fill(buf, r, base);
    let mut right = String::new();
    let mut left = (app.status.clone(), base.fg(th.fg));
    if let Some(b) = app.buf() {
        let (line, col) = b.pos(b.cursor);
        let diags = b.path.as_ref().and_then(|p| app.diagnostics.get(p));
        if let Some(diags) = diags {
            let count = |sev: u8| diags.iter().filter(|d| d.severity == sev).count();
            if count(1) + count(2) > 0 {
                right.push_str(&format!("✕ {}  ▲ {}   ", count(1), count(2)));
            }
            // With nothing else to say, show the problem on the cursor's line.
            if left.0.is_empty() {
                if let Some(d) = diags
                    .iter()
                    .filter(|d| d.start.0 <= line && line <= d.end.0)
                    .min_by_key(|d| d.severity)
                {
                    left = (
                        d.message.lines().next().unwrap_or("").to_string(),
                        base.fg(th.severity(d.severity)),
                    );
                }
            }
        }
        if let Some(lang) = b.lang {
            if let Some(srv) = app.servers.get(lang.id) {
                let state = match (&srv.progress, srv.ready) {
                    (_, false) => " (starting)".to_string(),
                    (Some(p), _) => format!(" ({p})"),
                    _ => String::new(),
                };
                right.push_str(&format!("{}{state}   ", srv.name));
            }
            right.push_str(&format!("{}   ", lang.id));
        }
        if let Some((s, e)) = b.selection() {
            right.push_str(&format!("{} selected   ", e - s));
        }
        right.push_str(&format!("Ln {}, Col {} ", line + 1, col + 1));
    }
    let rw = right.width() as u16;
    right_text(buf, r.right(), r.y, &right, r.x, base);
    // A server that couldn't start or stopped stays flagged until restarted.
    let badge = app
        .buf()
        .and_then(|b| b.lang)
        .and_then(|l| app.failed_servers.get(l.id))
        .map(|(name, _)| format!("✗ {name}"));
    let mut badge_rect = Rect::default();
    let mut used = rw;
    if let Some(badge) = badge {
        let bw = badge.width() as u16;
        if r.width > rw + bw + 20 {
            let bx = r.right() - rw - bw - 3;
            text(buf, bx, r.y, &badge, bw, base.fg(th.error));
            badge_rect = Rect::new(bx, r.y, bw, 1);
            used += bw + 3;
        }
    }
    text(
        buf,
        r.x + 1,
        r.y,
        &left.0,
        r.width.saturating_sub(used + 3),
        left.1,
    );
    app.layout.server_badge = badge_rect;
}

/// The right-click menu: a bordered box at the click position, kept on screen.
fn draw_context_menu(buf: &mut Buf, app: &mut App, area: Rect) {
    app.layout.menu = Rect::default();
    let Some(menu) = &app.menu else { return };
    let th = &app.theme;
    let widths = |pick: fn(&MenuEntry) -> Option<&str>| {
        menu.entries
            .iter()
            .filter_map(pick)
            .map(|t| t.width())
            .max()
            .unwrap_or(0)
    };
    let label_w = widths(|e| match e {
        MenuEntry::Item { label, .. } => Some(label.as_str()),
        _ => None,
    });
    let hint_w = widths(|e| match e {
        MenuEntry::Item { hint, .. } => Some(hint.as_str()),
        _ => None,
    });
    let inner = label_w + if hint_w > 0 { 3 + hint_w } else { 0 };
    let w = ((inner + 4) as u16).min(area.width);
    let h = (menu.entries.len() as u16 + 2).min(area.height);
    let x = menu.anchor.0.min(area.width - w);
    let y = menu.anchor.1.min(area.height - h);
    let r = Rect::new(x, y, w, h);
    app.layout.menu = r;

    let bg = th.popup;
    let edge = Style::new().fg(th.border).bg(bg);
    fill(buf, r, Style::new().bg(bg).fg(th.fg));
    let rule = "─".repeat(w.saturating_sub(2) as usize);
    text(buf, x, y, &format!("╭{rule}╮"), w, edge);
    text(buf, x, y + h - 1, &format!("╰{rule}╯"), w, edge);
    for (i, entry) in menu.entries.iter().enumerate() {
        let ry = y + 1 + i as u16;
        if ry + 1 >= y + h {
            break;
        }
        text(buf, x, ry, "│", 1, edge);
        text(buf, x + w - 1, ry, "│", 1, edge);
        match entry {
            MenuEntry::Separator => {
                text(buf, x, ry, &format!("├{rule}┤"), w, edge);
            }
            MenuEntry::Item {
                label,
                hint,
                danger,
                ..
            } => {
                let selected = menu.sel == Some(i);
                let row_bg = if selected { th.selection } else { bg };
                fill(buf, Rect::new(x + 1, ry, w - 2, 1), Style::new().bg(row_bg));
                let mut style = Style::new()
                    .fg(if *danger { th.error } else { th.fg })
                    .bg(row_bg);
                if selected {
                    style = style.add_modifier(Modifier::BOLD);
                }
                text(buf, x + 2, ry, label, w.saturating_sub(4), style);
                if !hint.is_empty() {
                    right_text(
                        buf,
                        x + w - 2,
                        ry,
                        hint,
                        x + 2 + label_w as u16,
                        Style::new().fg(th.dim).bg(row_bg),
                    );
                }
            }
        }
    }
}

enum ShortcutLine {
    Header(&'static str),
    Item(String, &'static str),
    Blank,
}

/// Keys that aren't commands, so aren't in the keymap.
const EDITOR_KEYS: &[(&str, &str)] = &[
    ("Shift+Arrows", "Select"),
    ("Ctrl/Alt+Arrows", "Move by word"),
    ("Tab / Shift+Tab", "Indent / outdent"),
    ("Alt+Click", "Go to definition"),
    ("Right-click", "Context menu"),
    ("Esc", "Close popups"),
];

/// Every bound command, grouped by the prefix of its title ("File: Save").
fn shortcut_lines(app: &App) -> Vec<ShortcutLine> {
    const GROUPS: &[&str] = &["", "File", "Edit", "View", "Code", "Settings", "Help"];
    let mut lines = vec![];
    for group in GROUPS {
        let mut items = vec![];
        for cmd in Command::ALL {
            let (prefix, label) = cmd.title().split_once(": ").unwrap_or(("", cmd.title()));
            let keys = app.keymap.bindings(*cmd);
            if prefix == *group && !keys.is_empty() {
                items.push(ShortcutLine::Item(
                    keys[..keys.len().min(2)].join(", "),
                    label,
                ));
            }
        }
        if !items.is_empty() {
            lines.push(ShortcutLine::Header(if group.is_empty() {
                "General"
            } else {
                group
            }));
            lines.extend(items);
            lines.push(ShortcutLine::Blank);
        }
    }
    lines.push(ShortcutLine::Header("Editor"));
    for (keys, label) in EDITOR_KEYS {
        lines.push(ShortcutLine::Item(keys.to_string(), label));
    }
    lines
}

/// Flow `lines` down then across in columns of `rows`, never leaving a
/// header at the foot of a column or a blank at the top of one. Returns each
/// line's (column, row), or None if it's dropped, and the columns used.
fn flow(lines: &[ShortcutLine], rows: usize) -> (Vec<Option<(usize, usize)>>, usize) {
    let (mut col, mut row) = (0, 0);
    let mut pos = vec![];
    for line in lines {
        match line {
            ShortcutLine::Blank if row == 0 => {
                pos.push(None);
                continue;
            }
            ShortcutLine::Header(_) if row > 0 && row + 1 >= rows => (col, row) = (col + 1, 0),
            _ => {}
        }
        pos.push(Some((col, row)));
        row += 1;
        if row == rows {
            (col, row) = (col + 1, 0);
        }
    }
    (pos, col + (row > 0) as usize)
}

/// The keyboard shortcuts panel: a strip along the bottom of `area`, with
/// the shortcuts flowed down then across in as many columns as fit.
fn draw_shortcuts(buf: &mut Buf, app: &mut App, area: Rect) {
    let Some(scroll) = app.shortcuts else { return };
    const KEY_W: u16 = 22;
    const COL_W: u16 = 48;
    let lines = shortcut_lines(app);
    let cols = ((area.width.saturating_sub(2)) / COL_W).max(1) as usize;
    let mut rows = lines.len().div_ceil(cols);
    let mut placed = flow(&lines, rows);
    while placed.1 > cols {
        rows += 1;
        placed = flow(&lines, rows);
    }
    let h = (rows as u16 + 2)
        .min(area.height * 2 / 3)
        .max(4.min(area.height));
    let r = Rect::new(area.x, area.bottom() - h, area.width, h);
    app.layout.shortcuts = r;
    let visible = (h - 2) as usize;
    let scroll = scroll.min(rows.saturating_sub(visible));
    app.shortcuts = Some(scroll);

    let th = &app.theme;
    fill(buf, r, Style::new().bg(th.popup).fg(th.fg));
    let title = Style::new().bg(th.popup_sel);
    fill(buf, Rect::new(r.x, r.y, r.width, 1), title);
    text(
        buf,
        r.x + 1,
        r.y,
        "Keyboard Shortcuts",
        r.width - 1,
        title.fg(th.accent).add_modifier(Modifier::BOLD),
    );
    let hint = if rows > visible {
        "↑↓ scroll · Esc close  "
    } else {
        "Esc close  "
    };
    right_text(buf, r.right(), r.y, hint, r.x + 20, title.fg(th.dim));

    let col_w = COL_W.min(r.width - 2);
    for (line, pos) in lines.iter().zip(placed.0) {
        let Some((col, row)) = pos else { continue };
        if row < scroll || row >= scroll + visible {
            continue;
        }
        let x = r.x + 2 + col as u16 * COL_W;
        let y = r.y + 1 + (row - scroll) as u16;
        if x >= r.right() {
            break;
        }
        let w = col_w.min(r.right() - x);
        let bg = Style::new().bg(th.popup);
        match line {
            ShortcutLine::Header(name) => {
                text(
                    buf,
                    x,
                    y,
                    name,
                    w,
                    bg.fg(th.dim).add_modifier(Modifier::BOLD),
                );
            }
            ShortcutLine::Item(keys, label) => {
                text(buf, x, y, keys, KEY_W.min(w), bg.fg(th.accent));
                if w > KEY_W {
                    text(buf, x + KEY_W, y, label, w - KEY_W - 1, bg.fg(th.fg));
                }
            }
            ShortcutLine::Blank => {}
        }
    }
}

fn draw_palette(buf: &mut Buf, app: &mut App, area: Rect) -> Option<(u16, u16)> {
    let p = app.palette.as_ref()?;
    let th = &app.theme;
    let w = area.width.saturating_sub(4).min(76);
    let x = (area.width - w) / 2;
    let y = if area.height > 20 { 2 } else { 0 };
    let rows = p
        .items
        .len()
        .min(MAX_ROWS)
        .min(area.height.saturating_sub(y + 2) as usize) as u16;
    fill(
        buf,
        Rect::new(x, y, w, rows + 1),
        Style::new().bg(th.popup).fg(th.fg),
    );
    let input_bg = Style::new().bg(th.popup_sel);
    fill(buf, Rect::new(x, y, w, 1), input_bg);
    let end = if p.input.is_empty() {
        text(buf, x + 2, y, p.placeholder(), w - 3, input_bg.fg(th.dim));
        x + 2
    } else {
        text(buf, x + 2, y, &p.input, w - 3, input_bg.fg(th.fg))
    };
    for (row, item) in p
        .items
        .iter()
        .skip(p.scroll)
        .take(rows as usize)
        .enumerate()
    {
        let ry = y + 1 + row as u16;
        let selected = p.scroll + row == p.sel;
        let bg = if selected { th.selection } else { th.popup };
        fill(buf, Rect::new(x, ry, w, 1), Style::new().bg(bg));
        let mut style = Style::new().fg(th.fg).bg(bg);
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        let lx = text(buf, x + 2, ry, &item.label, w - 4, style);
        let detail_style = Style::new().fg(th.dim).bg(bg);
        if matches!(item.action, crate::palette::Action::Run(_)) {
            right_text(buf, x + w - 2, ry, &item.detail, lx + 2, detail_style);
        } else if lx + 2 < x + w - 2 {
            text(
                buf,
                lx + 2,
                ry,
                &item.detail,
                x + w - 2 - (lx + 2),
                detail_style,
            );
        }
    }
    app.layout.palette_list = Rect::new(x, y + 1, w, rows);
    Some((end, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_on_words_and_splits_long_ones() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\n\nb", 10), ["a", "", "b"]);
    }
}
