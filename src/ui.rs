//! Rendering the Changes view: tab bar, file list, diff, comment box, list, status.
//!
//! See `specs/tui.md`. The layout is a header tab bar, a body split into the diff
//! (left) and the file list (right), and a status bar. While composing, the comment
//! box is spliced inline into the diff under the selected line; the comments-list
//! overlay is drawn on top when open. Rendering reads `App` only; all state changes
//! live in `app.rs`.

use std::rc::Rc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, BranchRow, Focus, FooterAction, ListRow, Mode, Tab, Tier};
use crate::diff::{FileDiff, FileState, Row};
use crate::file_list::{Annotation, RowKind};
use crate::forge;
use crate::model::{Comment, Scope};
use crate::theme::Palette;

/// The embedded editor's per-frame paint input in nvim mode: the grid (behind its lock, taken
/// only during the blit), a startup placeholder, or the dead-editor panel with a reason.
#[derive(Debug)]
pub enum NvimView<'a> {
    Grid {
        engine: &'a crate::nvim::Nvim,
        /// The colorscheme's `Normal` has no background: paint the terminal default under the
        /// cells (matching how a transparent theme looks in a plain terminal nvim).
        transparent: bool,
    },
    Starting,
    Dead(Option<String>),
}

pub fn render(frame: &mut Frame, app: &App) {
    render_with_nvim(frame, app, None);
}

pub fn render_with_nvim(frame: &mut Frame, app: &App, nvim: Option<&NvimView<'_>>) {
    let area = frame.area();
    let p = panes(area, app.list_pct);

    if app.tab == Tab::Pr {
        render_pr_header(frame, app, p.tab);
        render_pr_read(frame, app, p.diff);
        render_pr_nav(frame, app, p.files);
    } else {
        render_tab_bar(frame, app, p.tab);
        // Embedded-nvim editor mode: the diff pane hosts the editor's cell grid; the file list
        // stays exactly as in the default mode. The markdown preview wins over both bases —
        // in nvim mode it must paint OVER the editor grid or the toggle would be invisible.
        if app.md_showing() {
            render_markdown_preview(frame, app, p.diff);
        } else if app.editor_nvim {
            render_nvim_view(frame, app, nvim, p.diff);
        } else {
            render_diff_view(frame, app, p.diff);
        }
        render_file_list(frame, app, p.files);
    }
    // One footer band on every tab, drawn after the per-tab base so it sits on both layouts;
    // then the comments-list modal on top when it is open.
    render_footer(frame, app, p.status);

    if app.mode == Mode::List {
        render_comments_list(frame, app, area);
    } else if app.mode == Mode::CommitPick {
        render_commit_picker(frame, app, area);
    } else if app.mode == Mode::BranchPick {
        render_branch_picker(frame, app, area);
    } else if app.mode == Mode::Help {
        render_help_panel(frame, app, area);
    } else if app.mode == Mode::ConfirmDelete {
        render_confirm_delete(frame, app, area);
    } else if app.mode == Mode::ConfirmQuit {
        render_confirm_quit(frame, app, area);
    }
}

/// The vertical bands: tab bar, body, footer. The comment input is inline in the diff, not a
/// band of its own. The footer action bar is one row — it fits by dropping the least-relevant
/// actions, not by wrapping.
fn vrows(area: Rect) -> Rc<[Rect]> {
    Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).split(area)
}

/// The frame's layout rects: the diff pane, the file pane, and the whole body band. One
/// place computes the vertical bands and the horizontal split, so every geometry helper and
/// the renderer agree by construction (a layout change can't desync hit-testing from paint).
struct Panes {
    tab: Rect,
    diff: Rect,
    files: Rect,
    body: Rect,
    status: Rect,
}

fn panes(area: Rect, list_pct: u16) -> Panes {
    let rows = vrows(area);
    let body = rows[1];
    let split = Layout::horizontal([
        Constraint::Percentage(100 - list_pct),
        Constraint::Percentage(list_pct),
    ])
    .split(body);
    Panes { tab: rows[0], diff: split[0], files: split[1], body, status: rows[2] }
}

/// The whole body band (between the tab bar and status bar), for divider hit-testing.
#[must_use]
pub fn body_rect(area: Rect) -> Rect {
    vrows(area)[1]
}

/// Whether `(col, row)` lands on the draggable divider between the two panes.
#[must_use]
pub fn hit_divider(area: Rect, list_pct: u16, col: u16, row: u16) -> bool {
    let p = panes(area, list_pct);
    let in_body = row >= p.body.y && row < p.body.y + p.body.height;
    // The grab zone is exactly the two abutting pane borders — the diff pane's right border at
    // `files.x - 1` and the file pane's left border at `files.x` — and no content column of
    // either pane. In particular it must stop at `files.x`: the file pane's content starts at
    // `files.x + 1`, where the stage-marker paints, and that has to stay a click-to-stage target
    // rather than start a resize.
    in_body && col + 1 >= p.files.x && col <= p.files.x
}

/// The file-row index a click at `(col, row)` lands on, or `None` if outside the list.
/// `file_scroll` is the top visible row, so a click maps to the scrolled-to row.
#[must_use]
pub fn hit_file(
    area: Rect,
    list_pct: u16,
    col: u16,
    row: u16,
    n_files: usize,
    file_scroll: usize,
) -> Option<usize> {
    let inner = inner_rect(panes(area, list_pct).files);
    if !contains(inner, col, row) {
        return None;
    }
    let idx = (row - inner.y) as usize + file_scroll;
    (idx < n_files).then_some(idx)
}

/// Whether `(col, row)` lands on a file row's change marker — the first cell of the row, where
/// the `?`/`A`/`M` glyph paints. Used to turn a marker click into a stage/unstage toggle.
#[must_use]
pub fn on_file_marker(area: Rect, list_pct: u16, col: u16, row: u16) -> bool {
    let inner = inner_rect(panes(area, list_pct).files);
    contains(inner, col, row) && col == inner.x
}

/// The number of file rows visible in the file pane, used to clamp the file-list scroll.
#[must_use]
pub fn file_viewport_height(area: Rect, list_pct: u16) -> usize {
    inner_rect(panes(area, list_pct).files).height as usize
}

/// Whether `(col, row)` falls in the file pane, so the wheel scrolls the list it is over.
#[must_use]
pub fn in_files_pane(area: Rect, list_pct: u16, col: u16, row: u16) -> bool {
    contains(panes(area, list_pct).files, col, row)
}

/// The logical diff-row index a click at `(col, row)` lands on, or `None` if outside the
/// diff pane. `heights` (display rows per logical row) and `diff_scroll` reproduce the
/// painted window, so a click on any display line of a wrapped row maps to that row.
#[must_use]
pub fn hit_diff(
    area: Rect,
    list_pct: u16,
    col: u16,
    row: u16,
    heights: &[usize],
    diff_scroll: usize,
) -> Option<usize> {
    let inner = inner_rect(panes(area, list_pct).diff);
    if !contains(inner, col, row) {
        return None;
    }
    let target = (row - inner.y) as usize;
    let mut acc = 0;
    for (li, h) in heights.iter().enumerate().skip(diff_scroll) {
        acc += h;
        if target < acc {
            return Some(li);
        }
    }
    None
}

/// The number of diff rows visible in the diff pane, used to clamp the scroll.
#[must_use]
pub fn diff_viewport_height(area: Rect, list_pct: u16) -> usize {
    inner_rect(panes(area, list_pct).diff).height as usize
}

/// The display height (rows on screen) of each visible logical diff row, honoring wrap.
#[must_use]
pub fn diff_row_heights(app: &App, area: Rect) -> Vec<usize> {
    let width = inner_rect(panes(area, app.list_pct).diff).width as usize;
    let gutter_w = gutter_for(&app.diff);
    let p = app.palette();
    // A row's display height is its wrapped code lines plus any inline comment cards under
    // it (excluding a card whose comment is being edited), so scroll-clamping and hit-testing
    // match what the renderer paints.
    let cards = app.comment_cards();
    let editing = editing_comment(app);
    app.visible
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let base = row_height(r, gutter_w, width, app.wrap);
            let card: usize = cards[i]
                .iter()
                .filter(|&&ci| Some(ci) != editing)
                .filter_map(|&ci| app.store.get(ci))
                .map(|c| comment_card_lines(c, width, p).len())
                .sum();
            base + card
        })
        .collect()
}

/// The store index of the comment currently being edited, whose inline card is hidden in
/// favor of its edit box; `None` when not editing.
fn editing_comment(app: &App) -> Option<usize> {
    match app.mode {
        Mode::Composing { editing } => editing,
        _ => None,
    }
}

/// Rows the inline comment box occupies at the diff pane's `width`: the wrapped body height
/// (so the box grows as text wraps, not only on explicit newlines) plus the two borders.
#[must_use]
pub fn composer_height(app: &App, width: usize) -> usize {
    box_rows(&app.input, composer_content_width(width)).len() + 2
}

/// The text width inside the comment box: the diff pane width minus its two borders.
#[must_use]
pub fn composer_content_width(width: usize) -> usize {
    width.saturating_sub(2).max(1)
}

/// The diff pane's inner content width for the full terminal `area`, so the event loop can
/// reserve the comment box without a `Frame` (mirrors [`diff_viewport_height`]).
#[must_use]
pub fn diff_inner_width(area: Rect, list_pct: u16) -> usize {
    inner_rect(panes(area, list_pct).diff).width as usize
}

/// The comment box's display lines at `content_w`: each input line word-wrapped, with the
/// caret drawn as a block at its mapped (row, column). An empty box shows a placeholder.
fn composer_lines(app: &App, content_w: usize) -> Vec<Line<'static>> {
    let p = app.palette();
    if app.input.is_empty() {
        return vec![Line::from(vec![
            Span::styled(" ", caret_style(p)),
            Span::styled("Leave a comment…", Style::default().fg(p.overlay0)),
        ])];
    }
    let rows = box_rows(&app.input, content_w);
    let (caret_row, caret_col) = caret_rowcol(&rows, app.caret);
    rows.iter()
        .enumerate()
        .map(|(i, (_, text))| {
            if i == caret_row {
                row_with_caret(text, caret_col, p)
            } else {
                Line::from(text.clone())
            }
        })
        .collect()
}

/// The block-cursor style: the character under the caret shown dark-on-peach.
fn caret_style(p: &Palette) -> Style {
    Style::default().fg(p.surface0).bg(p.peach)
}

/// One box row with the caret block over the character at `col` (a trailing block at the end).
fn row_with_caret(text: &str, col: usize, p: &Palette) -> Line<'static> {
    let chars: Vec<char> = text.chars().collect();
    let col = col.min(chars.len());
    let left: String = chars[..col].iter().collect();
    let mut spans = vec![Span::raw(left)];
    if col < chars.len() {
        spans.push(Span::styled(chars[col].to_string(), caret_style(p)));
        spans.push(Span::raw(chars[col + 1..].iter().collect::<String>()));
    } else {
        spans.push(Span::styled(" ".to_string(), caret_style(p)));
    }
    Line::from(spans)
}

/// Wrap one logical line's `chars` to `width` display columns, returning contiguous half-open
/// char ranges (every char is in exactly one row, so a char index maps cleanly to a row). A
/// greedy word wrap that keeps the break space on its row; an over-wide word hard-breaks.
fn box_wrap(chars: &[char], width: usize) -> Vec<(usize, usize)> {
    if chars.is_empty() {
        return vec![(0, 0)];
    }
    let w = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let (mut col, mut i, mut last_space) = (0usize, start, None);
        while i < chars.len() {
            let cw = UnicodeWidthChar::width(chars[i]).unwrap_or(0);
            if col + cw > w && i > start {
                break;
            }
            col += cw;
            if chars[i] == ' ' {
                last_space = Some(i);
            }
            i += 1;
        }
        // Break after the last space that fits (keeping it on this row), else hard-break.
        let end = if i < chars.len() {
            last_space.filter(|&s| s + 1 > start).map_or(i, |s| s + 1)
        } else {
            i
        };
        rows.push((start, end));
        start = end;
    }
    rows
}

/// The box's visual rows over the whole `input`: `(start_char_index, text)` per row, wrapping
/// each logical line (split on `\n`) with [`box_wrap`]. A trailing newline yields an empty row.
fn box_rows(input: &str, width: usize) -> Vec<(usize, String)> {
    let chars: Vec<char> = input.chars().collect();
    let mut rows = Vec::new();
    let mut i = 0;
    loop {
        let line_end = chars[i..].iter().position(|&c| c == '\n').map_or(chars.len(), |p| i + p);
        for (a, b) in box_wrap(&chars[i..line_end], width) {
            rows.push((i + a, chars[i + a..i + b].iter().collect::<String>()));
        }
        match chars[line_end..].first() {
            Some('\n') => {
                i = line_end + 1;
                if i == chars.len() {
                    rows.push((i, String::new())); // a trailing newline opens an empty row
                    break;
                }
            }
            _ => break,
        }
    }
    if rows.is_empty() {
        rows.push((0, String::new()));
    }
    rows
}

/// Map a caret char index to its `(row, col)` in the box rows: the last row that starts at or
/// before the caret, with the column clamped to that row's length.
fn caret_rowcol(rows: &[(usize, String)], caret: usize) -> (usize, usize) {
    let row = rows.iter().rposition(|(start, _)| *start <= caret).unwrap_or(0);
    let (start, text) = &rows[row];
    (row, (caret - start).min(text.chars().count()))
}

/// The new caret char index after moving up (`down == false`) or down one wrapped row, keeping
/// the column where the target row allows. For `↑`/`↓` in the comment editor.
#[must_use]
pub fn caret_vertical(input: &str, caret: usize, content_w: usize, down: bool) -> usize {
    let rows = box_rows(input, content_w);
    let (row, col) = caret_rowcol(&rows, caret);
    let target = if down { (row + 1).min(rows.len() - 1) } else { row.saturating_sub(1) };
    let (start, text) = &rows[target];
    start + col.min(text.chars().count())
}

/// Word-wrap a plain string to `width` columns, reusing the diff's [`wrap_segments`] so the
/// break rule (last space, hard-break an over-wide word, width-aware) is identical.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let cells: Vec<Cell> = s
        .chars()
        .map(|ch| Cell {
            ch,
            w: UnicodeWidthChar::width(ch).unwrap_or(0),
            fg: Color::Reset,
            emph: false,
            modifier: Modifier::empty(),
        })
        .collect();
    wrap_segments(&cells, width)
        .into_iter()
        .map(|(a, b)| cells[a..b].iter().map(|c| c.ch).collect())
        .collect()
}

/// A clickable region in the header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeaderHit {
    Tab(Tab),
    Scope,
    Base,
    Commit,
    MdView,
    Send,
}

/// Which header control a click at `(col, row)` lands on, if any.
#[must_use]
pub fn hit_header(area: Rect, app: &App, col: u16, row: u16) -> Option<HeaderHit> {
    if row != area.y {
        return None;
    }
    for (tab, start, end) in tab_spans() {
        if (start as u16..end as u16).contains(&col) {
            return Some(HeaderHit::Tab(tab));
        }
    }
    let scope_start = header_prefix_len() as u16;
    let scope_end = scope_start + scope_chip(app).len() as u16;
    let base_end = scope_end + base_chip(app).len() as u16;
    let commit_end = base_end + commit_chip(app).len() as u16;
    let md_end = commit_end + md_chip(app).len() as u16;
    let button_start = send_button_col(app, area.width as usize) as u16;
    if (scope_start..scope_end).contains(&col) {
        Some(HeaderHit::Scope)
    } else if (scope_end..base_end).contains(&col) {
        Some(HeaderHit::Base)
    } else if (base_end..commit_end).contains(&col) {
        Some(HeaderHit::Commit)
    } else if (commit_end..md_end).contains(&col) {
        Some(HeaderHit::MdView)
    } else if col >= button_start && col < area.width {
        Some(HeaderHit::Send)
    } else {
        None
    }
}

/// The two tabs and their labels, left to right. All-ASCII labels keep the byte length equal
/// to the display width, so the header column math stays simple.
const TABS: [(Tab, &str); 3] =
    [(Tab::Changes, "1 Changes"), (Tab::AllFiles, "2 All files"), (Tab::Pr, "3 PR")];
const HEADER_LEAD: &str = " ";
const TAB_GAP: &str = "  ";
const HEADER_GAP: &str = "  ";

/// Each tab's `(tab, start_col, end_col)` in the header, the single source the bar paints and
/// the click hit-tests against.
fn tab_spans() -> Vec<(Tab, usize, usize)> {
    let mut col = HEADER_LEAD.len();
    let mut out = Vec::new();
    for (i, (tab, label)) in TABS.iter().enumerate() {
        if i > 0 {
            col += TAB_GAP.len();
        }
        out.push((*tab, col, col + label.len()));
        col += label.len();
    }
    out
}

/// The column where the scope chip starts: past the tab bar and its trailing gap.
fn header_prefix_len() -> usize {
    tab_spans().last().map_or(HEADER_LEAD.len(), |&(_, _, end)| end) + HEADER_GAP.len()
}

fn scope_chip(app: &App) -> String {
    format!("[{}]", app.scope.label())
}

fn base_chip(app: &App) -> String {
    if app.scope != Scope::Branch {
        return String::new();
    }
    if let Some(name) = app.base.as_deref() {
        format!(" [>{}]", truncate_width(name, 24))
    } else {
        let name = app.resolved_base.as_deref().unwrap_or("?");
        format!(" [>auto:{}]", truncate_width(name, 19))
    }
}

fn commit_chip(app: &App) -> String {
    if app.scope != Scope::Commit {
        return String::new();
    }
    if app.comparing_tip() {
        return " [uncommitted]".to_string();
    }
    match app
        .selected_commit
        .as_deref()
        .and_then(|sha| app.commit_choices.iter().find(|c| c.sha == sha))
    {
        Some(c) => format!(" [>{} {}]", c.short, truncate_width(&c.title, 24)),
        None => match app.selected_commit.as_deref() {
            Some(sha) => format!(" [>{}]", &sha[..sha.len().min(8)]),
            None if app.commit_choices.is_empty() => " [no commits]".to_string(),
            None => " [>pick commit]".to_string(),
        },
    }
}

/// The markdown view toggle, shown only when the file under the cursor is markdown. The label
/// names the view CURRENTLY showing (the user's ask — not the action); clicking toggles.
fn md_chip(app: &App) -> String {
    if !app.is_markdown_open() {
        return String::new();
    }
    if app.md_view { " [md view]".to_string() } else { " [raw]".to_string() }
}

fn send_button(app: &App) -> String {
    format!("[ Send ({}) ]", app.unsent_count())
}

/// The header suffix: the active scope's changed-file count. Shared so the painter and the
/// hit-test place the right-aligned `Send` button at the same column.
fn header_suffix(app: &App) -> String {
    let reviewed = app.reviewed_count();
    if reviewed > 0 {
        format!("  {} changed · {reviewed} reviewed", app.changed_count())
    } else {
        format!("  {} changed", app.changed_count())
    }
}

/// The column the `Send` button paints at, matching `render_tab_bar`'s layout: right-aligned
/// when the header fits, packed left right after the suffix when the bar overflows (`pad`
/// collapses to 0). `hit_header` must use this, not a bare right-alignment, or a `Send` click
/// mis-fires (and on a narrow sidebar lands in a tab span) when the header overflows.
fn send_button_col(app: &App, width: usize) -> usize {
    let before = header_prefix_len()
        + scope_chip(app).len()
        + base_chip(app).len()
        + commit_chip(app).len()
        + md_chip(app).len()
        + header_suffix(app).len();
    before + width.saturating_sub(before + send_button(app).len())
}

/// The header's shared left side, painted by both tab bars: the lead pad, the three tab labels
/// (the active one bright + underlined, the inactive ones at `SUBTEXT0`), and the trailing gap
/// before each header's own suffix. One source so the two headers can't drift.
fn tab_bar_spans(app: &App) -> Vec<Span<'static>> {
    let p = app.palette();
    let bar = Style::default().bg(p.surface0);
    let mut spans = vec![Span::styled(HEADER_LEAD, bar)];
    for (i, (tab, label)) in TABS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(TAB_GAP, bar));
        }
        let style = if *tab == app.tab {
            bar.fg(p.lavender).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            bar.fg(p.subtext0)
        };
        spans.push(Span::styled(*label, style));
    }
    spans.push(Span::styled(HEADER_GAP, bar));
    spans
}

fn render_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let chip = scope_chip(app);
    let base = base_chip(app);
    let commit = commit_chip(app);
    let md = md_chip(app);
    let suffix = header_suffix(app);
    let button = send_button(app);
    let used = header_prefix_len()
        + chip.len()
        + base.len()
        + commit.len()
        + md.len()
        + suffix.len()
        + button.len();
    let pad = (area.width as usize).saturating_sub(used);

    // A quiet surface bar: the active tab in bright lavender, the inactive one dimmed, the
    let p = app.palette();
    let bar = Style::default().bg(p.surface0);
    let mut spans = tab_bar_spans(app);
    spans.push(Span::styled(chip, bar.fg(p.yellow).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(base, bar.fg(p.lavender).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(commit, bar.fg(p.lavender).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(md, bar.fg(p.blue).add_modifier(Modifier::BOLD)));
    spans.push(Span::styled(suffix, bar.fg(p.overlay0)));

    let send_fg = if app.store.is_empty() { p.overlay0 } else { p.green };
    spans.push(Span::styled(" ".repeat(pad), bar));
    spans.push(Span::styled(button, bar.fg(send_fg).add_modifier(Modifier::BOLD)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let title = if app.mode == Mode::ExtExpand {
        format!("Files  .{}▏", app.ext_query)
    } else if app.filter.is_empty() {
        "Files".to_string()
    } else if app.mode == Mode::Filter {
        format!("Files  /{}▏", app.filter)
    } else {
        format!("Files  /{}", app.filter)
    };
    let block = bordered(&title, app.focus == Focus::Files, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.file_rows.is_empty() {
        let msg = if app.filter.is_empty() {
            match app.tab {
                Tab::AllFiles => "no files",
                Tab::Changes if app.awaiting_turn() => "waiting for the agent's next turn",
                _ => "no changes",
            }
        } else {
            "no matches"
        };
        frame.render_widget(dim_paragraph(msg, p), inner);
        return;
    }

    let width = inner.width as usize;
    // Window the rows to the scrolled-to viewport; `file_scroll` keeps the cursor on screen.
    let items: Vec<ListItem> = app
        .file_rows
        .iter()
        .enumerate()
        .skip(app.file_scroll)
        .take(inner.height as usize)
        .map(|(i, row)| {
            // The selected row fills with the cursor color, dimmed when the list is unfocused.
            let fill = (i == app.file_cursor).then(|| p.cursor_bg(app.focus == Focus::Files));
            let indent = "  ".repeat(row.depth);
            match &row.kind {
                RowKind::Dir { expanded, path } => {
                    let changed = app.dir_has_changes(path);
                    let name_style = if row.ignored {
                        Style::default().fg(p.overlay0)
                    } else if changed {
                        Style::default().fg(p.blue).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(p.subtext0).add_modifier(Modifier::BOLD)
                    };
                    let mut spans = gutter_spans(' ', p.text, p);
                    if app.icons {
                        let (glyph, folder_color) = crate::icons::folder_icon(*expanded, p);
                        let color = if changed { p.blue } else { folder_color };
                        spans.push(Span::styled(indent.clone(), Style::default().fg(p.overlay0)));
                        spans.push(Span::styled(format!("{glyph} "), Style::default().fg(color)));
                    } else {
                        let arrow = if *expanded { "▾ " } else { "▸ " };
                        spans.push(Span::styled(
                            format!("{indent}{arrow}"),
                            Style::default().fg(p.overlay0),
                        ));
                    }
                    spans.push(Span::styled(format!("{}/", row.name), name_style));
                    selectable_row(spans, width, fill)
                }
                RowKind::File { annotation, index } => file_row_item(FileRow {
                    indent: &indent,
                    annotation: annotation.as_ref(),
                    status: app.file_status(&app.entries[*index].path),
                    name: &row.name,
                    width,
                    fill,
                    ignored: row.ignored,
                    reviewed: app.is_reviewed(&app.entries[*index].path),
                    icons: app.icons,
                    p,
                }),
            }
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

const GUTTER_WIDTH: usize = 2;

fn gutter_spans(marker: char, marker_color: Color, p: &Palette) -> Vec<Span<'static>> {
    vec![
        Span::styled(marker.to_string(), Style::default().fg(marker_color)),
        Span::styled("│", Style::default().fg(p.overlay0)),
    ]
}

#[derive(Clone, Copy)]
struct FileRow<'a> {
    indent: &'a str,
    annotation: Option<&'a Annotation>,
    status: Option<crate::model::FileStatus>,
    name: &'a str,
    width: usize,
    fill: Option<Color>,
    ignored: bool,
    reviewed: bool,
    icons: bool,
    p: &'a Palette,
}

/// The marker colour for a file's `git status`: the change kind's colour when staged (A green,
/// M/R amber, D red), greyed out when unstaged/untracked.
fn stage_color(p: &Palette, s: crate::model::FileStatus) -> Color {
    if s.staged { kind_color(p, s.marker) } else { p.overlay1 }
}

fn file_row_item(row: FileRow) -> ListItem<'static> {
    let FileRow { indent, annotation, status, name, width, fill, ignored, reviewed, icons, p } =
        row;
    // The marker column is the file's git-staging state. A reviewed file swaps the letter for a
    // ✓ but keeps the staging colour (green staged, grey not), so the status is never lost.
    let (marker, marker_color) = match status {
        Some(s) if reviewed => ('✓', stage_color(p, s)),
        Some(s) => (s.marker, stage_color(p, s)),
        None if reviewed => ('✓', p.overlay1),
        None => (' ', p.text),
    };
    let (additions, deletions) = annotation.map_or((0, 0), |a| (a.additions, a.deletions));
    let stats = stats_str(additions, deletions);
    let gap = if stats.is_empty() { 0 } else { 2 };
    let icon = icons.then(|| crate::icons::file_icon(name, p));
    let icon_w = icon.map_or(0, |(g, _)| format!("{g} ").width());
    let fixed = GUTTER_WIDTH + indent.width() + icon_w + stats.width() + gap;
    let shown = truncate_width(name, width.saturating_sub(fixed).max(1));
    // Dim the parent directories of a collapsed-chain name; keep the basename bright.
    let (dim, base) = match shown.rfind('/') {
        Some(s) => (&shown[..=s], &shown[s + 1..]),
        None => ("", shown.as_str()),
    };

    let mut spans = gutter_spans(marker, marker_color, p);
    spans.push(Span::styled(indent.to_string(), text_style(p)));
    if let Some((glyph, color)) = icon {
        spans.push(Span::styled(format!("{glyph} "), Style::default().fg(color)));
    }
    if !dim.is_empty() {
        spans.push(Span::styled(dim.to_string(), Style::default().fg(p.overlay0)));
    }
    // A git-ignored file recedes into a dim basename; its change marker and stats keep their
    // color so a kept ignored file still reads as a change (file-list.md). A file the SCOPE
    // deletes (committed or not) greys out and strikes through — the list-level "this is gone"
    // idiom. Deliberately keyed on the scope annotation, not git status: the marker column
    // stays a pure staging affordance, and a committed branch-deletion has nothing to stage.
    let deleted = annotation.is_some_and(|a| a.change == crate::model::ChangeKind::Deleted);
    let mut base_style = if ignored || reviewed || deleted {
        Style::default().fg(p.overlay0)
    } else {
        text_style(p)
    };
    if deleted {
        base_style = base_style.add_modifier(Modifier::CROSSED_OUT);
    }
    spans.push(Span::styled(base.to_string(), base_style));
    if !stats.is_empty() {
        let used: usize = spans.iter().map(Span::width).sum();
        let pad = width.saturating_sub(used + stats.width());
        spans.push(Span::raw(" ".repeat(pad)));
        spans.extend(stats_spans(additions, deletions, p));
    }
    selectable_row(spans, width, fill)
}

/// The `+a −d` stats text, dropping a side that is zero (`+210`, `−4`, or empty); used to
/// measure the stats column. [`stats_spans`] paints the same text in green/red.
fn stats_str(additions: u32, deletions: u32) -> String {
    match (additions, deletions) {
        (0, 0) => String::new(),
        (a, 0) => format!("+{a}"),
        (0, d) => format!("−{d}"),
        (a, d) => format!("+{a} −{d}"),
    }
}

/// The `+a −d` stats as colored spans: additions in green, deletions in red, matching the
/// diff's add/remove hues. Same glyphs (and width) as [`stats_str`].
fn stats_spans(additions: u32, deletions: u32, p: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if additions > 0 {
        spans.push(Span::styled(format!("+{additions}"), Style::default().fg(p.green)));
    }
    if additions > 0 && deletions > 0 {
        spans.push(Span::raw(" "));
    }
    if deletions > 0 {
        spans.push(Span::styled(format!("−{deletions}"), Style::default().fg(p.red)));
    }
    spans
}

/// Shorten `name` to `max` columns by eliding its head behind a leading `…`, preferring to
/// cut at a path separator so a partial directory name never shows.
fn elide_head(name: &str, max: usize) -> String {
    if name.width() <= max {
        return name.to_string();
    }
    let budget = max.saturating_sub(1); // a column for the `…`
    let mut tail = String::new();
    let mut w = 0;
    for ch in name.chars().rev() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > budget {
            break;
        }
        tail.insert(0, ch);
        w += cw;
    }
    if let Some(slash) = tail.find('/') {
        tail = tail[slash..].to_string();
    }
    format!("…{tail}")
}

/// A saved comment as inline display lines: a quiet box titled with the comment's location
/// (in the comment-yellow accent) holding its wrapped text. Spliced read-only under the
/// commented line so a submitted comment stays visible while reviewing.
fn comment_card_lines(c: &Comment, width: usize, p: &Palette) -> Vec<Line<'static>> {
    const INDENT: usize = 2;
    let box_w = width.saturating_sub(INDENT).max(10);
    let text_w = box_w.saturating_sub(4).max(1); // inside "│ " … " │"
    let border = Style::default().fg(p.overlay0);
    let title = Style::default().fg(p.peach).add_modifier(Modifier::BOLD);
    let body_style = Style::default().fg(p.text);
    let pad = || Span::raw(" ".repeat(INDENT));

    let label = truncate_width(&format!(" comment · {} ", c.location()), box_w.saturating_sub(3));
    let fill = box_w.saturating_sub(3 + label.width());
    let mut lines = vec![Line::from(vec![
        pad(),
        Span::styled("╭─", border),
        Span::styled(label, title),
        Span::styled(format!("{}╮", "─".repeat(fill)), border),
    ])];

    for logical in c.text.split('\n') {
        for piece in wrap_text(logical, text_w) {
            let gap = " ".repeat(text_w.saturating_sub(piece.width()));
            lines.push(Line::from(vec![
                pad(),
                Span::styled("│ ", border),
                Span::styled(piece, body_style),
                Span::styled(format!("{gap} │"), border),
            ]));
        }
    }

    lines.push(Line::from(vec![
        pad(),
        Span::styled(format!("╰{}╯", "─".repeat(box_w.saturating_sub(2))), border),
    ]));
    lines
}

/// Truncate `s` to `max` display columns, marking a cut with a trailing `…`.
fn truncate_width(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn pad_width(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(s.width());
    format!("{s}{}", " ".repeat(pad))
}

fn render_markdown_preview(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let path = app.diff_path.clone().unwrap_or_default();
    let block = bordered(&format!("preview · {path}"), true, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let src = app.preview_markdown().unwrap_or_default();
    let lines = render_markdown_lines(&src, inner.width, p);
    let scroll = app.preview_scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(Paragraph::new(Text::from(lines)).scroll((scroll, 0)), inner);
}

fn markdown_skin(p: &Palette) -> ratskin::MadSkin {
    use ratatui::crossterm::style::Color as Mad;
    let conv = |c: Color| match c {
        Color::Rgb(r, g, b) => Mad::Rgb { r, g, b },
        _ => Mad::Reset,
    };
    let mut skin = ratskin::MadSkin::default();
    skin.set_headers_fg(conv(p.mauve));
    skin.bold.set_fg(conv(p.peach));
    skin.italic.set_fg(conv(p.lavender));
    skin.inline_code.set_fg(conv(p.green));
    skin
}

fn render_markdown_lines(src: &str, width: u16, p: &Palette) -> Vec<Line<'static>> {
    let parsed = ratskin::RatSkin::parse_text(src);
    ratskin::RatSkin { skin: markdown_skin(p) }
        .parse(parsed, width.max(1))
        .into_iter()
        .map(|l| {
            Line::from(
                l.spans
                    .into_iter()
                    .map(|s| Span::styled(s.content.into_owned(), s.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

pub fn preview_metrics(app: &App, area: Rect, list_pct: u16) -> (usize, usize) {
    let diff = panes(area, list_pct).diff;
    let width = diff.width.saturating_sub(2);
    let viewport = diff.height.saturating_sub(2) as usize;
    let lines = match app.preview_markdown() {
        Some(src) => render_markdown_lines(&src, width, app.palette()).len(),
        None => 0,
    };
    (lines, viewport)
}

/// The embedded editor's blit target: the diff pane's interior (1-cell border inset). One
/// helper shared by sizing, painting and mouse hit-testing so they can never disagree.
#[must_use]
pub fn nvim_grid_rect(area: Rect, list_pct: u16) -> Rect {
    inner_rect(panes(area, list_pct).diff)
}

/// Whether a point is inside the embedded editor's grid (nvim mode's diff-pane interior).
#[must_use]
pub fn in_nvim_grid(area: Rect, list_pct: u16, col: u16, row: u16) -> bool {
    contains(nvim_grid_rect(area, list_pct), col, row)
}

/// The embedded-nvim pane: the shared bordered chrome, then the editor's own cells.
fn render_nvim_view(frame: &mut Frame, app: &App, nvim: Option<&NvimView<'_>>, area: Rect) {
    let p = app.palette();
    let block = bordered(&diff_title(app), app.focus == Focus::Diff, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match nvim {
        Some(NvimView::Grid { engine, transparent }) => {
            blit_nvim_grid(frame, app, engine, *transparent, inner);
        }
        Some(NvimView::Starting) | None => {
            let msg = Paragraph::new(Line::from(Span::styled(
                "starting nvim…",
                Style::default().fg(p.overlay1),
            )));
            frame.render_widget(msg, inner);
        }
        Some(NvimView::Dead(reason)) => {
            let mut lines: Vec<Line> = Vec::new();
            // Center the panel vertically, leaving room for its three content lines.
            for _ in 0..(inner.height / 2).saturating_sub(2) {
                lines.push(Line::default());
            }
            lines.push(
                Line::from(Span::styled(
                    "the editor (nvim) exited",
                    Style::default().fg(p.red).add_modifier(Modifier::BOLD),
                ))
                .centered(),
            );
            if let Some(reason) = reason {
                lines.push(
                    Line::from(Span::styled(reason.clone(), Style::default().fg(p.subtext0)))
                        .centered(),
                );
            }
            lines.push(Line::default());
            lines.push(
                Line::from(vec![
                    Span::styled("r", Style::default().fg(p.lavender)),
                    Span::styled(" restart · ", Style::default().fg(p.subtext0)),
                    Span::styled("tab", Style::default().fg(p.lavender)),
                    Span::styled(" files · ", Style::default().fg(p.subtext0)),
                    Span::styled("q", Style::default().fg(p.lavender)),
                    Span::styled(" quit", Style::default().fg(p.subtext0)),
                ])
                .centered(),
            );
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
    // Composing (nvim mode): the same inline input box the built-in pane uses, anchored to
    // the pane's bottom over the grid — the code stays visible above while the note is typed.
    // Same width as the pane interior, so the caret math in the key handler holds.
    if app.composing() {
        let box_h = composer_height(app, inner.width as usize).min(inner.height as usize).max(1);
        let box_rect = Rect {
            x: inner.x,
            y: inner.y + inner.height - box_h as u16,
            width: inner.width,
            height: box_h as u16,
        };
        frame.render_widget(Clear, box_rect);
        render_composer(frame, app, box_rect);
    }
}

/// Copy the editor's cell grid into the frame buffer. The grid is the truth for widths: a
/// [`CellText::WideTail`] means the glyph to its left spans two columns, so the tail cell is
/// marked skip. nvim's own default colors fill the rect (no reviewer bg underneath — the
/// border row is the themed seam), which also covers resize transients where grid and rect
/// briefly disagree.
fn blit_nvim_grid(
    frame: &mut Frame,
    app: &App,
    engine: &crate::nvim::Nvim,
    transparent: bool,
    inner: Rect,
) {
    use crate::nvim::CellText;
    let grid = engine.grid();
    let fg_def = rgb(grid.default_fg);
    // A transparent theme (Normal without a bg) means "the terminal's background shows
    // through" — nvim reports black in that case, so substitute the terminal default and the
    // embedded editor blends like a plain terminal nvim.
    let bg_def = if transparent { Color::Reset } else { rgb(grid.default_bg) };
    let buf = frame.buffer_mut();
    for y in inner.top()..inner.bottom() {
        for x in inner.left()..inner.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
                cell.set_style(Style::default().fg(fg_def).bg(bg_def));
            }
        }
    }
    let rows = grid.rows.min(inner.height);
    for row in 0..rows {
        let cols = grid.cols.min(inner.width);
        let cells = grid.row(row);
        for col in 0..cols {
            let gc = &cells[col as usize];
            let Some(cell) = buf.cell_mut((inner.x + col, inner.y + row)) else { continue };
            let attr = grid.attr(gc.hl);
            let mut style =
                Style::default().fg(attr.fg.map_or(fg_def, rgb)).bg(attr.bg.map_or(bg_def, rgb));
            if attr.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if attr.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if attr.underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if attr.strikethrough {
                style = style.add_modifier(Modifier::CROSSED_OUT);
            }
            if attr.reverse {
                style = style.add_modifier(Modifier::REVERSED);
            }
            match &gc.text {
                CellText::Char(c) => {
                    cell.set_char(*c);
                    cell.set_style(style);
                }
                CellText::Cluster(s) => {
                    cell.set_symbol(s);
                    cell.set_style(style);
                }
                CellText::WideTail => {
                    cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
                    cell.set_style(style);
                }
            }
        }
    }
    // nvim never paints its cursor into cells — draw it here, but only when the keys actually
    // route to the editor (diff focus, no reviewer modal capturing input) and it isn't busy.
    // XOR of REVERSED keeps it visible on already-reversed cells (e.g. a visual selection).
    let (cur_row, cur_col) = grid.cursor;
    if app.focus == Focus::Diff
        && app.mode == Mode::Normal
        && grid.cursor_visible
        && cur_row < rows
        && cur_col < grid.cols.min(inner.width)
        && let Some(cell) = buf.cell_mut((inner.x + cur_col, inner.y + cur_row))
    {
        let style = cell.style();
        let toggled = if style.add_modifier.contains(Modifier::REVERSED) {
            style.remove_modifier(Modifier::REVERSED)
        } else {
            style.add_modifier(Modifier::REVERSED)
        };
        cell.set_style(toggled);
    }
}

/// nvim mode's quit guard: the editor holds unsaved buffers and `q` was pressed.
fn render_confirm_quit(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let popup = centered(area, 60, 30);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.red))
        .title("Quit");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let lines = vec![
        Line::default(),
        Line::from(Span::styled(
            "Unsaved changes in the editor — quit anyway?",
            Style::default().fg(p.text),
        ))
        .centered(),
        Line::from(Span::styled(
            "Unsaved edits in nvim buffers will be lost.",
            Style::default().fg(p.subtext0),
        ))
        .centered(),
        Line::default(),
        Line::from(vec![
            Span::styled("y / enter", Style::default().fg(p.red).add_modifier(Modifier::BOLD)),
            Span::styled(" quit   ", Style::default().fg(p.subtext0)),
            Span::styled("n / esc", Style::default().fg(p.lavender)),
            Span::styled(" cancel", Style::default().fg(p.subtext0)),
        ])
        .centered(),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The left pane's title: the open file (with its old name on a rename), else the tab's noun.
/// Shared by the diff view and the embedded-nvim view.
fn diff_title(app: &App) -> String {
    // In embedded-nvim mode the editor can jump anywhere — a tag jump / go-to-definition can land
    // on a file outside the changeset, which the host tracks in `nvim_buf` without moving
    // `diff_path`. The title must name the file actually on screen, so there it follows the
    // editor's real buffer (mirroring `current_entry`'s precedence). The built-in diff can only
    // show `diff_path`, so it is unchanged.
    let shown = if app.editor_nvim {
        app.nvim_buf.as_deref().or(app.diff_path.as_deref())
    } else {
        app.diff_path.as_deref()
    };
    match shown {
        // The rename arrow belongs to the changeset entry itself; a jumped-to file that is not
        // `diff_path` carries no rename, so show its bare path.
        Some(new) if app.diff_path.as_deref() == Some(new) => match &app.diff.previous_path {
            Some(old) => format!("{old} → {new}"),
            None => new.to_string(),
        },
        Some(new) => new.to_string(),
        None => match app.tab {
            Tab::AllFiles => "File",
            _ => "Diff",
        }
        .to_string(),
    }
}

fn render_diff_view(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let title = diff_title(app);
    // While searching, the title carries the query and the match position (or "no match").
    let title = if app.mode == Mode::Search {
        match app.search_status() {
            Some((pos, total)) => format!("{title}  /{}▏  {pos}/{total}", app.search),
            None if app.search.is_empty() => format!("{title}  /▏"),
            None => format!("{title}  /{}▏  no match", app.search),
        }
    } else {
        title
    };
    let block = bordered(&title, app.focus == Focus::Diff, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.visible.is_empty() {
        // `All files` is a content browser, not a diff, so its empty/notice copy avoids diff
        // vocabulary and never shows the last-turn "waiting" state.
        let msg = match app.tab {
            Tab::AllFiles => match app.diff.state {
                FileState::Binary => "binary — no line comments",
                FileState::TooLarge => "file too large",
                FileState::Normal if app.diff_path.is_some() => "empty file",
                FileState::Normal => "select a file to read",
            },
            Tab::Changes if app.awaiting_turn() => "waiting for the agent's next turn",
            _ => match app.diff.state {
                FileState::Binary => "binary — no line comments",
                FileState::TooLarge => "file too large to diff",
                FileState::Normal => "no diff",
            },
        };
        frame.render_widget(dim_paragraph(msg, p), inner);
        return;
    }

    let height = inner.height as usize;
    if height == 0 {
        return;
    }
    let width = inner.width as usize;
    let gutter_w = gutter_for(&app.diff);
    let layout = RowLayout {
        gutter_w,
        width,
        h_scroll: app.h_scroll,
        wrap: app.wrap,
        focused: app.focus == Focus::Diff,
        pal: p,
    };
    let commented = app.commented_lines();
    let cards = app.comment_cards();
    let editing = editing_comment(app);
    let (lo, hi) = app.selection_range();
    let selecting = app.focus == Focus::Diff && app.select_anchor.is_some();

    // One logical row → its 1+ wrapped display lines, then any saved-comment cards anchored
    // to it. The cursor/selection apply to the code line's display rows, not the cards. The
    // card of a comment being edited is hidden — its edit box stands in for it.
    let row_lines = |i: usize| -> Vec<Line> {
        let state = RowState {
            commented: commented.contains(&i),
            cursor: app.focus == Focus::Diff && i == app.diff_cursor,
            selected: selecting && i >= lo && i <= hi,
        };
        let mut lines = render_row(&app.visible[i], layout, state);
        for &ci in &cards[i] {
            if Some(ci) != editing
                && let Some(c) = app.store.get(ci)
            {
                lines.extend(comment_card_lines(c, width, p));
            }
        }
        lines
    };
    // Display lines for the logical rows in `range`, in order.
    let display = |range: std::ops::Range<usize>| -> Vec<Line> {
        range.flat_map(&row_lines).collect::<Vec<_>>()
    };

    let rows = app.visible.len();
    if !app.composing() {
        // Fill the pane from `diff_scroll`'s first display line; clamp keeps the cursor in.
        let mut out = display(app.diff_scroll..rows);
        out.truncate(height);
        frame.render_widget(Paragraph::new(out), inner);
        return;
    }

    // Composing: splice the input box under the last selected line, in display rows.
    // Cap the box at height-1 so a comment taller than the viewport can't hide its anchor.
    let box_h = composer_height(app, width).min(height.saturating_sub(1)).max(1);
    let diff_budget = height - box_h;
    let anchor = hi.clamp(app.diff_scroll, rows.saturating_sub(1));
    let above = display(app.diff_scroll..anchor + 1);
    // Keep the anchor's last display line just above the box when `above` overflows.
    let above: Vec<Line> =
        if above.len() > diff_budget { above[above.len() - diff_budget..].to_vec() } else { above };
    let remaining = diff_budget - above.len();
    let mut below = display(anchor + 1..rows);
    below.truncate(remaining);

    let slots = Layout::vertical([
        Constraint::Length(above.len() as u16),
        Constraint::Length(box_h as u16),
        Constraint::Length(below.len() as u16),
    ])
    .split(inner);
    if !above.is_empty() {
        frame.render_widget(Paragraph::new(above), slots[0]);
    }
    render_composer(frame, app, slots[1]);
    if !below.is_empty() {
        frame.render_widget(Paragraph::new(below), slots[2]);
    }
}

/// The line-number column width for a diff of `rows` lines.
fn gutter_width(rows: usize) -> usize {
    rows.to_string().len().max(3)
}

/// The gutter width for a whole `FileDiff`, sized to its largest line number so it does not
/// resize when a fold toggles (folds hide lines but keep the numbering). One definition,
/// shared by `diff_row_heights` (measuring) and `render_diff_view` (painting), so the
/// measured and painted geometry can never disagree.
fn gutter_for(diff: &FileDiff) -> usize {
    let total_lines: usize =
        diff.rows.iter().map(|r| if r.is_content() { 1 } else { r.hidden() }).sum();
    gutter_width(total_lines)
}

/// The gutter prefix width: the change bar plus the right-aligned line number and a space.
fn gutter_prefix_width(gutter_w: usize) -> usize {
    1 + gutter_w + 1
}

/// How many display rows a row needs: 1 for a fold or with wrap off, else the number of
/// word-wrapped segments its (tab-expanded) content fills. Shares [`wrap_segments`] with
/// the renderer so per-row geometry stays aligned with what gets painted.
fn row_height(row: &Row, gutter_w: usize, width: usize, wrap: bool) -> usize {
    if !wrap || matches!(row, Row::Fold { .. }) {
        return 1;
    }
    let code_width = width.saturating_sub(gutter_prefix_width(gutter_w)).max(1);
    wrap_segments(&code_cells(row, false), code_width).len()
}

/// The diff-pane layout: constant for a frame.
#[derive(Clone, Copy)]
struct RowLayout<'a> {
    gutter_w: usize,
    width: usize,
    h_scroll: usize,
    wrap: bool,
    /// Whether the diff pane is focused — dims the cursor row when it is not.
    focused: bool,
    /// The active palette for the change bars, row tints, and fills.
    pal: &'a Palette,
}

/// A row's per-row highlight state.
#[derive(Clone, Copy)]
struct RowState {
    commented: bool,
    cursor: bool,
    selected: bool,
}

/// A diff row as one or more full-width display lines: a left change bar, the line
/// number, then syntax-colored code tinted red/green. With wrap on, a long line breaks
/// into `code_width`-wide rows; a continuation row carries a blank gutter so numbers
/// stay aligned. With wrap off, the line is one row scrolled by `h_scroll`.
fn render_row(row: &Row, layout: RowLayout<'_>, state: RowState) -> Vec<Line<'static>> {
    let RowLayout { gutter_w, width, h_scroll, wrap, focused, pal } = layout;
    let RowState { commented, cursor, selected } = state;
    if let Row::Fold { .. } = row {
        let label = if cursor {
            format!("  ⋯  {} unmodified lines — → expand", row.hidden())
        } else {
            format!("  ⋯  {} unmodified lines", row.hidden())
        };
        let mut line = Line::from(Span::styled(label, Style::default().fg(pal.subtext0)));
        if let Some(pad) = width.checked_sub(line.width()).filter(|p| *p > 0) {
            line.push_span(Span::raw(" ".repeat(pad)));
        }
        let bg = if cursor { pal.cursor_bg(focused) } else { pal.surface0 };
        return vec![line.style(Style::default().bg(bg).add_modifier(Modifier::BOLD))];
    }
    let num = row.new_no().or_else(|| row.old_no()).map_or(String::new(), |n| n.to_string());
    // A commented line's number takes the peach comment accent; others sit a step brighter
    // than the dim chrome so they stay legible while read.
    let num_color = if commented { pal.peach } else { pal.overlay1 };
    let (bar, bar_color) = match row.marker() {
        '-' => ("▌", pal.red),
        '+' => ("▌", pal.green),
        _ => (" ", pal.overlay0),
    };
    let row_bg = if cursor {
        Some(pal.cursor_bg(focused))
    } else if selected {
        Some(pal.surface1)
    } else {
        match row.marker() {
            '-' => Some(pal.del_bg),
            '+' => Some(pal.ins_bg),
            _ => None,
        }
    };

    // Word emphasis brightens the changed words, unless the row's fill is a cursor or
    // selection bg, which wins for readability.
    let emph_on = !cursor && !selected;
    let emph_bg = match row.marker() {
        '-' => pal.emph_del_bg,
        '+' => pal.emph_ins_bg,
        _ => pal.ins_bg,
    };
    let cells = code_cells(row, emph_on);

    let prefix_w = gutter_prefix_width(gutter_w);
    let code_width = width.saturating_sub(prefix_w).max(1);
    // Without wrap the line is one chunk scrolled by `h_scroll`; with wrap, word-wrapped
    // segments, the first numbered and the rest blank-gutter.
    let chunks: Vec<&[Cell]> = if wrap {
        wrap_segments(&cells, code_width).into_iter().map(|(s, e)| &cells[s..e]).collect()
    } else {
        vec![cells.get(skip_columns(&cells, h_scroll)..).unwrap_or(&[])]
    };

    chunks
        .into_iter()
        .enumerate()
        .map(|(k, chunk)| {
            let gutter = if k == 0 {
                vec![
                    Span::styled(bar, Style::default().fg(bar_color)),
                    Span::styled(format!("{num:>gutter_w$} "), Style::default().fg(num_color)),
                ]
            } else {
                // A continuation row keeps the change bar but blanks the number column.
                vec![
                    Span::styled(bar, Style::default().fg(bar_color)),
                    Span::raw(" ".repeat(prefix_w - 1)),
                ]
            };
            let mut spans = gutter;
            spans.extend(cells_to_spans(chunk, emph_bg));
            let mut line = Line::from(spans);
            if let Some(pad) = width.checked_sub(line.width()).filter(|p| *p > 0) {
                line.push_span(Span::raw(" ".repeat(pad)));
            }
            match row_bg {
                Some(bg) => line.style(Style::default().bg(bg)),
                None => line,
            }
        })
        .collect()
}

fn rgb(c: crate::diff::Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

/// Tabs expand to this many columns.
const TAB: usize = 4;

/// Greedy word wrap over display cells into half-open ranges, one per display row.
///
/// Breaks at the last space that fits within `width`, falling back to a hard break when a
/// single word is wider than the column. Leading spaces on a continuation are dropped so a
/// break landing just before a space doesn't leave an almost-empty row. An empty line still
/// yields one (empty) range so it occupies a row. The renderer and [`row_height`] share this
/// so what's measured matches what's painted.
fn wrap_segments(cells: &[Cell], width: usize) -> Vec<(usize, usize)> {
    if cells.is_empty() {
        return vec![(0, 0)];
    }
    let mut segs = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        // Take as many cells as fit within `width` columns, always at least one (so a glyph
        // wider than the column still gets its own row rather than stalling).
        let mut col = 0;
        let mut limit = start;
        while limit < cells.len() {
            let cw = cells[limit].w;
            if col + cw > width && limit > start {
                break;
            }
            col += cw;
            limit += 1;
        }
        if limit == cells.len() {
            segs.push((start, cells.len()));
            break;
        }
        // More cells follow; prefer breaking just after the last space that fits.
        let brk = (start..limit).rev().find(|&i| cells[i].ch == ' ').map(|i| i + 1);
        let end = brk.filter(|&e| e > start).unwrap_or(limit);
        segs.push((start, end));
        start = end;
        while start < cells.len() && cells[start].ch == ' ' {
            start += 1;
        }
    }
    segs
}

/// The first cell index lying at or past `cols` display columns — the no-wrap horizontal
/// scroll offset, snapping past a wide glyph that straddles the boundary rather than
/// splitting it.
fn skip_columns(cells: &[Cell], cols: usize) -> usize {
    let mut col = 0;
    let mut i = 0;
    while i < cells.len() && col < cols {
        col += cells[i].w;
        i += 1;
    }
    i
}

/// One display cell of a code line: a glyph, its terminal width in columns (1 for most
/// text, 2 for wide CJK/emoji, 0 for a combining mark), its syntax color, and whether it
/// falls in a word-emphasis range.
struct Cell {
    ch: char,
    w: usize,
    fg: Color,
    emph: bool,
    modifier: Modifier,
}

/// Expand a row's spans into display cells: tabs become spaces to the next tab stop, and
/// each char carries its column width, color, and (when `emph_on`) its word-emphasis flag.
/// Width comes from `unicode-width` so wide glyphs measure as the two columns they paint.
fn code_cells(row: &Row, emph_on: bool) -> Vec<Cell> {
    let emphasis = if emph_on { row.emphasis() } else { &[] };
    let in_emph = |i: u32| emphasis.iter().any(|&(a, b)| i >= a && i < b);
    let mut cells = Vec::new();
    let mut idx = 0u32;
    let mut col = 0usize; // display column, so tab stops land right after wide glyphs too
    for s in row.spans() {
        let fg = rgb(s.color);
        let modifier = font_modifier(s);
        for ch in s.text.chars() {
            let emph = in_emph(idx);
            if ch == '\t' {
                for _ in 0..(TAB - col % TAB) {
                    cells.push(Cell { ch: ' ', w: 1, fg, emph, modifier });
                    col += 1;
                }
            } else {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                cells.push(Cell { ch, w, fg, emph, modifier });
                col += w;
            }
            idx += 1;
        }
    }
    cells
}

fn font_modifier(s: &crate::diff::Span) -> Modifier {
    let mut m = Modifier::empty();
    if s.bold {
        m |= Modifier::BOLD;
    }
    if s.italic {
        m |= Modifier::ITALIC;
    }
    if s.underline {
        m |= Modifier::UNDERLINED;
    }
    m
}

/// Build spans from display cells, merging runs of equal color/emphasis; an emphasized
/// run takes `emph_bg` as its background.
fn cells_to_spans(cells: &[Cell], emph_bg: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<(Color, bool, Modifier)> = None;
    for c in cells {
        let key = (c.fg, c.emph, c.modifier);
        if cur != Some(key) {
            if let Some((fg, emph, modifier)) = cur {
                spans.push(cell_span(std::mem::take(&mut buf), fg, emph, modifier, emph_bg));
            }
            cur = Some(key);
        }
        buf.push(c.ch);
    }
    if let Some((fg, emph, modifier)) = cur {
        spans.push(cell_span(buf, fg, emph, modifier, emph_bg));
    }
    spans
}

fn cell_span(
    text: String,
    fg: Color,
    emph: bool,
    modifier: Modifier,
    emph_bg: Color,
) -> Span<'static> {
    let style = Style::default().fg(fg).add_modifier(modifier);
    Span::styled(text, if emph { style.bg(emph_bg) } else { style })
}

/// The inline comment input box, drawn at `area` (under the selection in the diff).
fn render_composer(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let loc = app.pending_location().unwrap_or_else(|| "comment".to_string());
    let editing = matches!(app.mode, Mode::Composing { editing: Some(_) });
    let title = if editing { format!("edit · {loc}") } else { format!("comment · {loc}") };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.peach))
        .title(title);
    let content_w = composer_content_width(area.width as usize);
    let body = Paragraph::new(composer_lines(app, content_w)).block(block);
    frame.render_widget(body, area);
}

/// The key glyph and label for a footer action; an empty label renders the glyph alone. The
/// `TogglePane` and `Send` labels depend on `app` (the destination pane, the comment count).
fn action_key_label(app: &App, action: FooterAction) -> (String, String) {
    use FooterAction as A;
    let (k, l): (&str, &str) = match action {
        A::Comment => ("c", "comment"),
        A::Select => ("v", "select"),
        A::ClearSelection | A::ClearFilter => ("esc", "clear"),
        A::EditComment => ("e", "edit"),
        A::OpenEditor => ("e", "editor"),
        A::DeleteComment | A::DeleteFile => ("d", "delete"),
        A::Resolve | A::ResolveSelected => ("r", "resolve"),
        A::SelectAll => ("a", "select all"),
        A::ConfirmDelete => ("y/↵", "delete"),
        A::ResetFile => ("r", "reset"),
        A::Review => {
            // nvim mode retired Space: Enter marks from the list and walks in the editor.
            if app.editor_nvim {
                return ("enter".into(), "review".into());
            }
            return (
                "space".into(),
                if app.focus == Focus::Diff { "next block" } else { "review" }.into(),
            );
        }
        A::JumpComment => ("n/N", "jump"),
        A::ExpandFold => ("→", "expand fold"),
        A::ExpandDir => ("→", "expand"),
        A::CollapseDir => ("←", "collapse"),
        A::ExpandTree => ("enter", "expand tree"),
        A::CollapseTree => ("enter", "collapse tree"),
        A::Preview => ("p", "preview"),
        A::SendPath => ("+", "→ chat"),
        A::TogglePane => {
            let dest = if app.focus == Focus::Files {
                if app.editor_nvim { "editor" } else { "diff" }
            } else {
                "files"
            };
            return ("⇥".into(), dest.into());
        }
        A::NvimHint => ("nvim", "keys go to the editor"),
        A::RestartEditor => ("r", "restart editor"),
        A::QuitAnyway => ("y/↵", "quit"),
        A::Scope => ("b/t/C", "scope"),
        A::Base => ("B", "base"),
        A::Filter => ("/", "filter"),
        A::Search => ("/", "search"),
        A::SearchNext => ("enter", "next match"),
        A::ApplyFilter => ("enter", "apply"),
        A::RevealExt => ("enter", "reveal"),
        A::PickCommit | A::PickBranch => ("enter", "compare"),
        A::Send => return ("s".into(), format!("send {}", app.unsent_count())),
        A::List => ("l", "list"),
        A::Copy => ("y", "copy"),
        A::Save => ("enter", "save"),
        A::Newline => ("⇧⏎", "newline"),
        A::Cancel => ("esc", "cancel"),
        A::CloseList | A::ClosePicker | A::CloseHelp => ("esc", "close"),
        A::OpenPr => ("o", "open ↗"),
        A::Refresh => ("r", "refresh"),
        A::Tabs => ("1·2·3", ""),
        A::Help => ("?", "help"),
        A::Quit => ("q", ""),
    };
    (k.into(), l.into())
}

/// A tier's `(key, label)` styles: the primary bright and bold, normal actions readable, the
/// orientation cluster dim so the eye lands on what to do, not on the always-there anchors.
fn tier_styles(tier: Tier, p: &Palette) -> (Style, Style) {
    match tier {
        Tier::Primary => (Style::default().fg(p.peach).add_modifier(Modifier::BOLD), text_style(p)),
        Tier::Normal => (Style::default().fg(p.lavender), Style::default().fg(p.subtext0)),
        Tier::Orientation => (Style::default().fg(p.overlay0), Style::default().fg(p.overlay0)),
    }
}

/// Render a run of actions as ` · `-separated `key label` spans, styled per tier.
fn action_spans(app: &App, acts: &[(FooterAction, Tier)]) -> Vec<Span<'static>> {
    let p = app.palette();
    let mut spans = Vec::new();
    for (i, &(action, tier)) in acts.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(p.overlay0)));
        }
        let (key, label) = action_key_label(app, action);
        let (key_style, label_style) = tier_styles(tier, p);
        spans.push(Span::styled(key, key_style));
        if !label.is_empty() {
            spans.push(Span::styled(format!(" {label}"), label_style));
        }
    }
    spans
}

/// The footer action bar: the context's actions (primary highlighted) packed left, the dim
/// orientation cluster packed right, fitting one line — orientation dropped first, then trailing
/// `Normal` actions, with a trailing `…` marking anything clipped (`specs/tui.md`).
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let w = area.width as usize;
    let all = app.footer_actions();
    let (mut left_acts, orient_acts): (Vec<_>, Vec<_>) =
        all.into_iter().partition(|&(_, t)| t != Tier::Orientation);

    // The read-only PR tab leads with the PR's state summary; the transient status sits among
    // the actions and never displaces them. The state line is capped so a long one never crowds
    // the primary action (and the `…`) off the line — leaving room for the actions plus the marker.
    let actions_w: usize = action_spans(app, &left_acts).iter().map(Span::width).sum();
    let pr_info = (app.tab == Tab::Pr).then(|| app.pr_snapshot()).flatten().map(|s| {
        let budget = w.saturating_sub(actions_w + 4).max(8);
        let text = truncate_width(&format!("{}   ", pr_state_line(s)), budget);
        Span::styled(text, Style::default().fg(p.subtext0))
    });
    let status = (!app.status.is_empty())
        .then(|| Span::styled(format!("  · {} ", app.status), Style::default().fg(p.peach)));

    let build_left = |acts: &[(FooterAction, Tier)]| -> Vec<Span<'static>> {
        let mut spans = vec![Span::raw(" ")];
        if let Some(info) = &pr_info {
            spans.push(info.clone());
        }
        spans.extend(action_spans(app, acts));
        if let Some(st) = &status {
            spans.push(st.clone());
        }
        spans
    };
    let orient: Vec<Span> = if orient_acts.is_empty() {
        Vec::new()
    } else {
        let mut spans = vec![Span::styled("│ ", Style::default().fg(p.overlay0))];
        spans.extend(action_spans(app, &orient_acts));
        spans
    };
    let orient_w: usize = orient.iter().map(Span::width).sum();

    let mut left = build_left(&left_acts);
    let line_width = |s: &[Span]| -> usize { s.iter().map(Span::width).sum() };
    let fits_with_orient = !orient.is_empty() && line_width(&left) + 1 + orient_w <= w;

    let spans = if fits_with_orient {
        // Leave one trailing cell so the last hint (`q`) doesn't butt against the edge.
        let pad = w.saturating_sub(line_width(&left) + orient_w + 1);
        left.push(Span::raw(" ".repeat(pad)));
        left.extend(orient);
        left
    } else {
        // Orientation is dropped; trim trailing `Normal` actions until the line fits, leaving
        // room for the `…` that marks the drop. The primary action is never trimmed.
        let dropped_orient = !orient.is_empty();
        let mut popped = false;
        while line_width(&left) + 2 > w
            && left_acts.len() > 1
            && left_acts.last().is_some_and(|&(_, t)| t == Tier::Normal)
        {
            left_acts.pop();
            popped = true;
            left = build_left(&left_acts);
        }
        // `…` whenever anything was clipped: the orientation cluster, a trimmed action, or a
        // primary still too wide to fit.
        if dropped_orient || popped || line_width(&left) + 2 > w {
            left.push(Span::styled(" …", Style::default().fg(p.overlay0)));
        }
        left
    };

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(p.surface0)),
        area,
    );
}

fn render_comments_list(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let popup = centered(area, 80, 60);
    frame.render_widget(Clear, popup);
    let selected = app.list_selected.len();
    let title = if selected > 0 {
        format!("Comments ({}) — {selected} selected", app.store.len())
    } else {
        format!("Comments ({})", app.store.len())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.mauve))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let width = inner.width as usize;
    // Reserve the last inner row for a command status bar; the rest scrolls.
    let content_h = inner.height.saturating_sub(1) as usize;
    let display = app.list_display();
    let cursor_row = app.list_cursor_row();
    let scroll = commit_scroll(cursor_row, content_h);
    let mut items: Vec<ListItem> = Vec::new();
    for (d, row) in display.iter().enumerate().skip(scroll).take(content_h) {
        match row {
            ListRow::Header(label) => items.push(ListItem::new(Line::from(Span::styled(
                format!("── {label} ──"),
                Style::default().fg(p.overlay1).add_modifier(Modifier::BOLD),
            )))),
            ListRow::Item(i) => {
                let Some(c) = app.store.get(*i) else { continue };
                let checked = app.is_list_selected(*i);
                let checkbox = Span::styled(
                    if checked { "[x] " } else { "[ ] " },
                    Style::default().fg(if checked { p.green } else { p.overlay1 }),
                );
                // Un-sent ("fresh") comments read in pale green so what the next Send will carry
                // stands out; sent comments are the usual bold mauve.
                let loc_style = if c.sent {
                    Style::default().fg(p.mauve).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.green).add_modifier(Modifier::BOLD)
                };
                let mut spans = vec![
                    checkbox,
                    Span::styled(c.location(), loc_style),
                    Span::styled(format!("  {}", c.text), text_style(p)),
                ];
                if app.is_stale(c) {
                    spans.push(Span::styled("  (stale)", Style::default().fg(p.red)));
                }
                items.push(selectable_row(spans, width, (d == cursor_row).then_some(p.surface2)));
            }
        }
    }
    let content_area = Rect { height: content_h as u16, ..inner };
    frame.render_widget(List::new(items), content_area);

    // The command status bar pinned to the overlay's last row.
    let bar = Rect { y: inner.y + content_h as u16, height: 1, ..inner };
    let hints = "space check · a all · r resolve · enter open · s send · y copy · e edit · d delete · esc close";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_width(hints, width),
            Style::default().fg(p.overlay1),
        )))
        .style(Style::default().bg(p.surface0)),
        bar,
    );
}

/// The store index of the comment row a click at `(col, row)` lands on, skipping group headers
/// and the status bar; `None` otherwise. Uses the same grouped layout and scroll as the renderer.
#[must_use]
pub fn hit_comments_list(area: Rect, app: &App, col: u16, row: u16) -> Option<usize> {
    let inner = Block::default().borders(Borders::ALL).inner(centered(area, 80, 60));
    if !contains(inner, col, row) {
        return None;
    }
    let content_h = inner.height.saturating_sub(1) as usize;
    let clicked = (row - inner.y) as usize;
    if clicked >= content_h {
        return None; // the status bar
    }
    let scroll = commit_scroll(app.list_cursor_row(), content_h);
    match app.list_display().get(clicked + scroll) {
        Some(ListRow::Item(i)) => Some(*i),
        _ => None,
    }
}

/// The `?` help content as titled groups of `(keys, description)`. One source for both the
/// rendered panel and its line count, hand-maintained from the key handler in `lib.rs`.
fn help_groups(nvim: bool) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    if nvim {
        return vec![
            (
                "Navigate (files pane)",
                vec![
                    ("j / k  ↑ / ↓", "move the cursor"),
                    ("PgUp/PgDn · Ctrl-u/d", "page / half-page"),
                    ("Tab", "switch files ⇄ editor (editor: normal/visual mode only)"),
                    ("← / →", "collapse / expand a folder"),
                    ("x", "expand every folder with changes · again collapses back"),
                    (
                        ".",
                        "reveal folders holding a file of an extension (.rs⏎) · empty ⏎ collapses back",
                    ),
                    (
                        "backspace",
                        "delete the file/folder under the cursor · a changed file also offers reset-to-base (confirms first)",
                    ),
                    ("[ / ]", "narrow / widen the file list"),
                    ("/", "filter the file list"),
                    (
                        "enter",
                        "file row: mark reviewed → next unreviewed · folder: expand/collapse",
                    ),
                    ("p / view chip", "markdown: rendered ⇄ raw — sticky, follows the selection"),
                ],
            ),
            (
                "Editor (nvim)",
                vec![
                    ("tab", "back to the files pane · every other key goes to nvim"),
                    ("enter", "walk the review: hunk to hunk, then mark reviewed → next file"),
                    ("backspace", "walk backward (enters the previous file on its last hunk)"),
                    ("]c / [c", "next / previous change in the file"),
                    (
                        "i a o … / paste",
                        "Changes is read-only — these flip to All files at the same spot",
                    ),
                    ("ctrl+i", "in All files: back to the Changes review, edit included"),
                    ("space rd", "side-by-side diff vs the base (dp/do editable)"),
                    ("space rh", "revert the hunk under the cursor (last hunk → next file)"),
                ],
            ),
            (
                "Comments",
                vec![
                    ("space rc", "comment on the line / visual selection"),
                    ("space re / rx / rr", "edit / delete / resolve the comment under the cursor"),
                    ("space rl / rs / ry", "comments list · send to the agent · copy all"),
                    (
                        "s / l / +",
                        "send un-sent · comments list · send the file's path to the agent",
                    ),
                ],
            ),
            (
                "Scope & base",
                vec![
                    ("1 / 2 / 3", "Changes / All files / PR tab"),
                    ("b / t / C", "branch / last-turn / commit scope"),
                    ("click base chip", "pick the base branch (Branch scope)"),
                    ("click commit chip", "pick a commit to compare against (Commit scope)"),
                ],
            ),
            (
                "Global",
                vec![
                    ("esc", "close any overlay · else clear selection / filter"),
                    ("r", "files pane: reload · crashed editor: restart it"),
                    ("q", "files pane: quit (asks if the editor has unsaved changes)"),
                    ("mouse", "click/drag/wheel work in both panes · drag the divider"),
                ],
            ),
            (
                "Status markers (git)",
                vec![
                    ("A M D R", "added · modified · deleted · renamed"),
                    ("?", "new, untracked file"),
                    (
                        "colour",
                        "in the change kind's colour = staged · grey = not staged · blank = unchanged vs HEAD",
                    ),
                    (
                        "click marker",
                        "stage a grey file (git add) or unstage a coloured one (git reset)",
                    ),
                ],
            ),
        ];
    }
    vec![
        (
            "Navigate",
            vec![
                ("j / k  ↑ / ↓", "move the cursor"),
                ("PgUp/PgDn", "page the focused pane"),
                ("Ctrl-u / Ctrl-d", "half-page"),
                ("Tab", "switch files ⇄ diff"),
                ("← / →", "collapse/expand dir · expand fold · scroll diff"),
                ("Enter", "expand/collapse the tree under a folder"),
                ("x", "expand every folder with changes · again collapses back"),
                (
                    ".",
                    "reveal folders holding a file of an extension (.rs⏎) · empty ⏎ collapses back",
                ),
                (
                    "backspace",
                    "delete the file/folder under the cursor · a changed file also offers reset-to-base (confirms first)",
                ),
                ("w", "toggle line wrap"),
                ("[ / ]", "narrow / widen the file list"),
            ],
        ),
        (
            "Review",
            vec![
                ("Space", "diff: next change block, then mark file → next file"),
                ("Space", "file list: mark the whole file reviewed → next"),
                ("n / N", "next / previous comment (across files · header button too)"),
            ],
        ),
        (
            "Comments",
            vec![
                ("c", "comment on the selection"),
                ("v", "start / extend a selection"),
                ("e", "edit the comment / open $EDITOR"),
                ("r", "resolve the comment under the cursor"),
                ("d", "delete the comment"),
                ("l", "comments list (grouped by base; green = un-sent)"),
                ("(list) space/a", "check row / all · r resolve · enter jump"),
                ("(list) s/y/e/d", "send · copy · edit · delete"),
            ],
        ),
        (
            "Scope & base",
            vec![
                ("1 / 2 / 3", "Changes / All files / PR tab"),
                ("b / t / C", "branch / last-turn / commit scope"),
                ("click base chip", "pick the base branch (Branch scope)"),
                ("click commit chip", "pick a commit to compare against (Commit scope)"),
            ],
        ),
        (
            "Panels",
            vec![
                ("p", "markdown preview (markdown files)"),
                ("?", "this help"),
                ("/", "search the diff (when focused) · else filter the file list"),
            ],
        ),
        (
            "Send",
            vec![
                ("s / S", "send comments to the agent"),
                ("y / Y", "copy comments to the clipboard"),
                ("+", "send the highlighted file's path to the agent"),
            ],
        ),
        (
            "Global",
            vec![
                ("esc", "close any overlay · else clear the selection / filter"),
                ("r", "reload"),
                ("q", "quit"),
                ("mouse", "click file/diff/header · wheel scroll · drag divider/select"),
            ],
        ),
        (
            "Status markers (git)",
            vec![
                ("A M D R", "added · modified · deleted · renamed"),
                ("?", "a new, untracked file"),
                (
                    "colour",
                    "in the change kind's colour = staged · grey = not staged · blank = unchanged vs HEAD",
                ),
                (
                    "click marker",
                    "stage a grey file (git add) or unstage a coloured one (git reset)",
                ),
            ],
        ),
    ]
}

/// The total rendered height of the help content, matching `help_lines`' layout (a blank
/// spacer before every group but the first, a header, then one row per binding).
fn help_total_lines(nvim: bool) -> usize {
    help_groups(nvim)
        .iter()
        .enumerate()
        .map(|(g, (_, rows))| usize::from(g > 0) + 1 + rows.len())
        .sum()
}

/// The help content as styled lines: bold section headers and dim-key rows.
fn help_lines(p: &Palette, nvim: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (g, (title, rows)) in help_groups(nvim).into_iter().enumerate() {
        if g > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            title,
            Style::default().fg(p.mauve).add_modifier(Modifier::BOLD),
        )));
        for (keys, desc) in rows {
            lines.push(Line::from(vec![
                Span::styled(format!("  {keys:<16}"), Style::default().fg(p.lavender)),
                Span::styled(desc, text_style(p)),
            ]));
        }
    }
    lines
}

/// The help overlay's `(total lines, viewport height)`, for scroll clamping (`lib.rs`).
#[must_use]
pub fn help_metrics(area: Rect, nvim: bool) -> (usize, usize) {
    let inner = inner_rect(centered(area, 80, 70));
    (help_total_lines(nvim), inner.height as usize)
}

fn render_help_panel(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let popup = centered(area, 80, 70);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.mauve))
        .title("Keys");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let scroll = app.help_scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(Text::from(help_lines(p, app.editor_nvim))).scroll((scroll, 0)),
        inner,
    );
}

fn render_confirm_delete(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let Some(pd) = app.pending_delete() else { return };
    let popup = centered(area, 60, 30);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.red))
        .title("Delete");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let kind = if pd.is_dir { "folder" } else { "file" };
    let path = truncate_width(&pd.path, (inner.width as usize).saturating_sub(kind.len() + 10));
    let path_span = Span::styled(path, Style::default().fg(p.mauve).add_modifier(Modifier::BOLD));
    let lines = if pd.resettable {
        // Three-way: delete the file outright, or reset it to the review base (discard the
        // reviewed changes). `Enter` is deliberately unbound so a stray keypress fires neither.
        vec![
            Line::from(vec![Span::styled(format!("{kind} "), text_style(p)), path_span]),
            Line::default(),
            Line::from(Span::styled(
                "delete removes the file · reset restores the review base (discards changes)",
                Style::default().fg(p.subtext0),
            )),
            Line::default(),
            Line::from(vec![
                Span::styled("d", Style::default().fg(p.red).add_modifier(Modifier::BOLD)),
                Span::styled(" delete    ", text_style(p)),
                Span::styled("r", Style::default().fg(p.green).add_modifier(Modifier::BOLD)),
                Span::styled(" reset    ", text_style(p)),
                Span::styled("n / esc", Style::default().fg(p.lavender)),
                Span::styled(" cancel", Style::default().fg(p.subtext0)),
            ]),
        ]
    } else {
        let tail = if pd.is_dir { " and everything inside it?" } else { "?" };
        vec![
            Line::from(vec![
                Span::styled(format!("Delete {kind} "), text_style(p)),
                path_span,
                Span::styled(tail.to_string(), text_style(p)),
            ]),
            Line::default(),
            Line::from(Span::styled(
                "This removes it from the working tree.",
                Style::default().fg(p.subtext0),
            )),
            Line::default(),
            Line::from(vec![
                Span::styled("y / enter", Style::default().fg(p.red).add_modifier(Modifier::BOLD)),
                Span::styled(" delete    ", text_style(p)),
                Span::styled("n / esc", Style::default().fg(p.lavender)),
                Span::styled(" cancel", Style::default().fg(p.subtext0)),
            ]),
        ]
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn commit_picker_rect(area: Rect) -> Rect {
    centered(area, 80, 60)
}

fn commit_scroll(cursor: usize, height: usize) -> usize {
    if height == 0 || cursor < height { 0 } else { cursor - height + 1 }
}

fn render_commit_picker(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let popup = commit_picker_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.mauve))
        .title(format!("Compare with commit ({})", app.commit_choices.len()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let width = inner.width as usize;
    let height = inner.height as usize;
    // Row 0 is the synthetic "Uncommitted" base; the real commits follow, one-indexed.
    let total = app.commit_choices.len() + 1;
    let scroll = commit_scroll(app.commit_cursor, height);
    let end = (scroll + height).min(total);
    let items: Vec<ListItem> = (scroll..end)
        .map(|i| {
            let selected = (i == app.commit_cursor).then_some(p.surface2);
            if i == 0 {
                let label = Span::styled(
                    "uncommitted  ".to_string(),
                    Style::default().fg(p.peach).add_modifier(Modifier::BOLD),
                );
                let hint = Span::styled(
                    "working tree vs the latest commit — follows new commits".to_string(),
                    Style::default().fg(p.overlay1),
                );
                return selectable_row(vec![label, hint], width, selected);
            }
            let c = &app.commit_choices[i - 1];
            let date = Span::styled(format!("{}  ", c.date), Style::default().fg(p.overlay1));
            let hash = Span::styled(
                format!("{}  ", c.short),
                Style::default().fg(p.mauve).add_modifier(Modifier::BOLD),
            );
            let author_w = 20.min(width / 4);
            let author_txt = pad_width(&truncate_width(&c.author, author_w), author_w);
            let author = Span::styled(format!("{author_txt}  "), Style::default().fg(p.blue));
            let used = c.date.len() + 2 + c.short.len() + 2 + author_w + 2;
            let title = Span::styled(
                truncate_width(&c.title, width.saturating_sub(used).max(1)),
                text_style(p),
            );
            selectable_row(
                vec![date, hash, author, title],
                width,
                (i == app.commit_cursor).then_some(p.surface2),
            )
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

#[must_use]
pub fn hit_commit_pick(area: Rect, app: &App, col: u16, row: u16) -> Option<usize> {
    let inner = Block::default().borders(Borders::ALL).inner(commit_picker_rect(area));
    if col < inner.x
        || col >= inner.x + inner.width
        || row < inner.y
        || row >= inner.y + inner.height
    {
        return None;
    }
    let scroll = commit_scroll(app.commit_cursor, inner.height as usize);
    let idx = scroll + (row - inner.y) as usize;
    // +1 for the synthetic "Uncommitted" row at index 0.
    (idx < app.commit_choices.len() + 1).then_some(idx)
}

fn render_branch_picker(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let popup = commit_picker_rect(area);
    frame.render_widget(Clear, popup);
    let count = app.branch_choices.iter().filter(|r| matches!(r, BranchRow::Item(_))).count();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.mauve))
        .title(format!("Compare with branch ({count})"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let width = inner.width as usize;
    let height = inner.height as usize;
    let scroll = commit_scroll(app.branch_cursor, height);
    let end = (scroll + height).min(app.branch_choices.len());
    let items: Vec<ListItem> = app.branch_choices[scroll..end]
        .iter()
        .enumerate()
        .map(|(row, br)| match br {
            BranchRow::Item(name) => {
                let i = scroll + row;
                let span = Span::styled(truncate_width(name, width), Style::default().fg(p.mauve));
                selectable_row(vec![span], width, (i == app.branch_cursor).then_some(p.surface2))
            }
            BranchRow::Divider => {
                let rule = Span::styled("─".repeat(width), Style::default().fg(p.surface2));
                ListItem::new(Line::from(rule))
            }
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

#[must_use]
pub fn hit_branch_pick(area: Rect, app: &App, col: u16, row: u16) -> Option<usize> {
    let inner = Block::default().borders(Borders::ALL).inner(commit_picker_rect(area));
    if col < inner.x
        || col >= inner.x + inner.width
        || row < inner.y
        || row >= inner.y + inner.height
    {
        return None;
    }
    let scroll = commit_scroll(app.branch_cursor, inner.height as usize);
    let idx = scroll + (row - inner.y) as usize;
    match app.branch_choices.get(idx) {
        Some(BranchRow::Item(_)) => Some(idx),
        _ => None,
    }
}

#[must_use]
pub fn in_picker_popup(area: Rect, col: u16, row: u16) -> bool {
    let r = commit_picker_rect(area);
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

/// The default body text color.
fn text_style(p: &Palette) -> Style {
    Style::default().fg(p.text)
}

/// A list row, highlighted with the shared selection fill (`surface2` + bold, full
/// width) when `selected` — the same treatment the diff cursor uses, so every cursor
/// in the UI reads the same. The fill is applied per span (with a trailing pad) so it
/// spans the full width under the `List` widget, matching the diff's `Paragraph` rows.
fn selectable_row(
    mut spans: Vec<Span<'static>>,
    width: usize,
    fill: Option<Color>,
) -> ListItem<'static> {
    if let Some(bg) = fill {
        let used: usize = spans.iter().map(Span::width).sum();
        if width > used {
            spans.push(Span::raw(" ".repeat(width - used)));
        }
        for s in &mut spans {
            s.style = s.style.bg(bg).add_modifier(Modifier::BOLD);
        }
    }
    ListItem::new(Line::from(spans))
}

// --- PR tab (specs/forge-host.md, specs/tui.md) --------------------------------

/// The header for the read-only PR tab: the tab names, then a right-anchored, clickable
/// `status #number ↗` chip (status colored by lifecycle, the `↗` sharing the number's colour),
/// with the PR title right-aligned to its left. Merge/sync/checks live in the footer.
fn render_pr_header(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let bar = Style::default().bg(p.surface0);
    let mut spans = tab_bar_spans(app);
    let lead_tabs: usize = spans.iter().map(Span::width).sum();
    let w = area.width as usize;

    // A resolved PR shows its identity chip; with no PR the header carries nothing — the read
    // pane is the single home for the empty/degraded message, not repeated across all regions.
    if let forge::PrView::Pr(s) = &app.pr {
        let number = format!("#{}", s.number);
        let (status, color) = pr_status_chip(p, s);
        let chip_w = pr_chip_width(s);
        // The title fills the gap left of the chip, right-aligned against it (a leading pad).
        let name = truncate_width(&s.title, w.saturating_sub(lead_tabs + chip_w + 2).max(4));
        let pad = w.saturating_sub(lead_tabs + name.width() + 2 + chip_w);
        spans.push(Span::styled(" ".repeat(pad), bar));
        spans.push(Span::styled(name, bar.fg(p.subtext0)));
        spans.push(Span::styled("  ", bar));
        spans.push(Span::styled(status, bar.fg(color).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(" ", bar));
        spans.push(Span::styled(number, bar.fg(p.yellow).add_modifier(Modifier::BOLD)));
        // The arrow shares the PR number's colour, reading as part of the clickable chip.
        spans.push(Span::styled(" ↗", bar.fg(p.yellow)));
    }

    // Fill the rest of the bar (the Pr arm already reaches the right edge).
    let used: usize = spans.iter().map(Span::width).sum();
    if used < w {
        spans.push(Span::styled(" ".repeat(w - used), bar));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The status chip word for a PR's lifecycle; its accent comes from [`pr_status_chip`].
fn pr_status_word(s: &forge::PrSnapshot) -> &'static str {
    match s.state {
        forge::PrState::Merged => "merged",
        forge::PrState::Closed => "closed",
        forge::PrState::Open if s.is_draft => "draft",
        forge::PrState::Open => "open",
    }
}

/// The status chip word and its theme accent, by lifecycle.
fn pr_status_chip(p: &Palette, s: &forge::PrSnapshot) -> (&'static str, Color) {
    let color = match s.state {
        forge::PrState::Merged => p.mauve,
        forge::PrState::Closed => p.red,
        forge::PrState::Open if s.is_draft => p.yellow,
        forge::PrState::Open => p.green,
    };
    (pr_status_word(s), color)
}

/// The display width of the header's `status #number ↗` chip — shared by the painter and the
/// click hit-test so they agree on its right-anchored column range.
fn pr_chip_width(s: &forge::PrSnapshot) -> usize {
    pr_status_word(s).width() + " ".width() + format!("#{}", s.number).width() + " ↗".width()
}

/// The PR's merge, sync, and checks status for the footer, joined by `·`. Merge and sync show
/// only for an open PR — they are meaningless once it is merged or closed.
fn pr_state_line(s: &forge::PrSnapshot) -> String {
    let mut parts: Vec<String> = Vec::new();
    if s.state == forge::PrState::Open {
        match s.merge {
            forge::Merge::Conflicting => parts.push(format!("⚠ conflicts with {}", s.base_ref)),
            forge::Merge::Blocked => parts.push("blocked".into()),
            forge::Merge::Clean => {}
        }
        match s.sync {
            forge::Sync::Unpushed(n) => parts.push(format!("⇡ {n} unpushed")),
            forge::Sync::Behind(n) => parts.push(format!("⇣ {n} behind")),
            forge::Sync::InSync => {}
        }
    }
    parts.push(checks_summary(s));
    parts.push(format!("{} comments", s.comments.len()));
    // A capped surface means the lists are a prefix; point at GitHub for the rest rather than
    // showing the partial counts as if complete (specs/forge-host.md).
    if s.truncated {
        parts.push("+more on GitHub ↗".into());
    }
    parts.join(" · ")
}

/// A one-token checks summary for the footer (`✓ checks` / `✗ N failing` / `● running`).
fn checks_summary(s: &forge::PrSnapshot) -> String {
    match s.checks_rollup() {
        None => "no checks".into(),
        Some(forge::CheckStatus::Failure) => format!("✗ {} failing", s.failing_checks()),
        Some(forge::CheckStatus::Running) => "● checks running".into(),
        Some(_) => "✓ checks".into(),
    }
}

/// The right navigator: the checks list above the newest-first comments list, with the cursor
/// row filled and the view windowed to keep it on screen.
fn render_pr_nav(frame: &mut Frame, app: &App, area: Rect) {
    // The navigator over the PR's checks and comments. Identity lives in the header; the left
    // pane reads the selected comment — so this pane names its contents, not "PR" again.
    let p = app.palette();
    let block = bordered("Checks & comments", true, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(s) = app.pr_snapshot() else {
        // The empty/degraded message lives once, in the read pane; this navigator stays blank.
        return;
    };
    let width = inner.width as usize;
    let dim = Style::default().fg(p.overlay0);
    let now = std::time::SystemTime::now();

    // Every check and comment row is a cursor stop; only the section headers and the blank
    // separator are display-only. The painter and the click hit-test share `pr_nav_layout` +
    // `pr_nav_scroll`, so the painted rows and the hit math cannot drift.
    let layout = pr_nav_layout(s);
    let rows: Vec<(Vec<Span<'static>>, bool)> = layout
        .iter()
        .map(|nav| match *nav {
            PrNavRow::ChecksHeader => (vec![Span::styled(pr_checks_header(s), dim)], false),
            PrNavRow::Blank => (Vec::new(), false),
            PrNavRow::CommentsHeader => {
                (vec![Span::styled(format!("comments · {}", s.comments.len()), dim)], false)
            }
            PrNavRow::Select(i) => {
                let spans = if let Some(c) = s.checks.get(i) {
                    let (glyph, color) = check_glyph(p, c.status);
                    vec![
                        Span::styled(format!(" {glyph} "), Style::default().fg(color)),
                        Span::styled(c.name.clone(), text_style(p)),
                    ]
                } else {
                    pr_comment_row(&s.comments[i - s.checks.len()], width, now, p)
                };
                (spans, app.pr_cursor == i)
            }
        })
        .collect();

    let viewport = inner.height as usize;
    let selected = rows.iter().position(|(_, sel)| *sel).unwrap_or(0);
    let scroll = pr_nav_scroll(rows.len(), selected, viewport);
    let items: Vec<ListItem> = rows
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .map(|(spans, sel)| selectable_row(spans, width, sel.then(|| p.cursor_bg(true))))
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// One display row of the PR navigator. `Select(i)` is a cursor stop — `i` indexes checks then
/// comments, the same space as `App::pr_cursor`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PrNavRow {
    ChecksHeader,
    Blank,
    CommentsHeader,
    Select(usize),
}

/// The navigator's display rows in paint order — the single home of the layout, shared by the
/// painter and the click hit-test.
fn pr_nav_layout(s: &forge::PrSnapshot) -> Vec<PrNavRow> {
    let mut rows = vec![PrNavRow::ChecksHeader];
    rows.extend((0..s.checks.len()).map(PrNavRow::Select));
    rows.push(PrNavRow::Blank);
    rows.push(PrNavRow::CommentsHeader);
    rows.extend((s.checks.len()..s.checks.len() + s.comments.len()).map(PrNavRow::Select));
    rows
}

/// The navigator's scroll: center the selected display row so its neighbors on BOTH sides stay
/// visible — a bottom-pinned selection on a checks-heavy PR hid every comment below it.
fn pr_nav_scroll(rows: usize, selected: usize, viewport: usize) -> usize {
    selected.saturating_sub(viewport / 2).min(rows.saturating_sub(viewport))
}

/// The `checks` section header with its rollup (`✗ 1 failing` / `✓ N passed` / `running`).
fn pr_checks_header(s: &forge::PrSnapshot) -> String {
    match s.checks_rollup() {
        None => "checks  none".into(),
        Some(forge::CheckStatus::Failure) => format!("checks  ✗ {} failing", s.failing_checks()),
        Some(forge::CheckStatus::Running) => "checks  running".into(),
        Some(_) => format!("checks  ✓ {} passed", s.checks.len()),
    }
}

/// One comment row: `@author anchor`, then a trailing `resolved`/`outdated` marker or the age.
fn pr_comment_row(
    cm: &forge::Comment,
    width: usize,
    now: std::time::SystemTime,
    p: &Palette,
) -> Vec<Span<'static>> {
    let author_color = if cm.author_is_bot { p.overlay1 } else { p.peach };
    let trailing = if cm.is_resolved {
        "resolved".to_string()
    } else if cm.is_outdated {
        "outdated".to_string()
    } else {
        forge::relative_age(&cm.created_at, now)
    };
    let author = format!("@{} ", cm.author);
    let budget = width.saturating_sub(author.width() + trailing.width() + 3).max(1);
    let anchor = elide_head(&cm.anchor, budget);
    vec![
        Span::styled(author, Style::default().fg(author_color)),
        Span::styled(anchor, text_style(p)),
        Span::styled(format!("  {trailing}"), Style::default().fg(p.overlay0)),
    ]
}

/// The left read pane: the selected comment's hunk (for a finding) then its body, a check's
/// open hint, or the loading/degraded message.
fn render_pr_read(frame: &mut Frame, app: &App, area: Rect) {
    let p = app.palette();
    let selected = app.pr_selected_comment();
    let check = app.pr_selected_check();
    let title = match (selected, check) {
        (Some(cm), _) => format!("@{} · {}", cm.author, cm.anchor),
        (None, Some(c)) => format!("check · {}", c.name),
        (None, None) => "PR".to_string(),
    };
    let block = bordered(&title, false, p);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(c) = check {
        // A selected check: its status, where its page lives, and how to get there.
        let (glyph, color) = check_glyph(p, c.status);
        lines.push(Line::from(vec![
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::styled(check_status_word(c.status), Style::default().fg(color)),
        ]));
        lines.push(Line::raw(""));
        match &c.url {
            Some(url) => {
                for piece in wrap_text(url, width.max(1)) {
                    lines.push(Line::from(Span::styled(piece, Style::default().fg(p.overlay1))));
                }
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    "o opens this check ↗",
                    Style::default().fg(p.overlay0),
                )));
            }
            None => lines.push(Line::from(Span::styled(
                "this check publishes no details page — o opens the PR ↗",
                Style::default().fg(p.overlay0),
            ))),
        }
    } else if let Some(cm) = selected {
        if let Some(hunk) = &cm.snippet {
            for raw in hunk.lines() {
                let color = match raw.bytes().next() {
                    Some(b'+') => p.green,
                    Some(b'-') => p.red,
                    _ => p.overlay0,
                };
                lines.push(Line::from(Span::styled(raw.to_string(), Style::default().fg(color))));
            }
            lines.push(Line::raw(""));
        }
        for logical in cm.body.split('\n') {
            for piece in wrap_text(logical, width.max(1)) {
                lines.push(Line::from(Span::styled(piece, text_style(p))));
            }
        }
        if cm.reply_count > 0 {
            let plural = if cm.reply_count == 1 { "reply" } else { "replies" };
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                format!("↳ {} {plural} — open on GitHub to read", cm.reply_count),
                Style::default().fg(p.overlay0),
            )));
        }
    } else {
        lines
            .push(Line::from(Span::styled(pr_empty_msg(&app.pr), Style::default().fg(p.overlay0))));
    }

    // Clamp in `usize` before the `u16` cast — `pr_read_scroll` grows unbounded via the wheel,
    // so casting first could wrap a large value below the clamp.
    let scroll = app.pr_read_scroll.min(lines.len().saturating_sub(1)) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// The one-line message for a loading or degraded PR view, each naming what unblocks it.
fn pr_empty_msg(view: &forge::PrView) -> &'static str {
    match view {
        forge::PrView::Loading => "loading…",
        forge::PrView::Pr(_) => "",
        forge::PrView::NoPr => "no PR for this branch yet — push and open one, then press r",
        forge::PrView::Ambiguous(_) => "this branch backs several open PRs — open one on GitHub",
        forge::PrView::NoGh => "gh not found — install gh, then press r",
        forge::PrView::NotAuthed => "not signed in — run `gh auth login`, then press r",
        forge::PrView::NotGitHub => "not a GitHub remote — the PR tab needs a github.com origin",
        forge::PrView::Error(_) => "github unavailable — retrying; press r to retry now",
    }
}

/// Whether a click at `(col, row)` lands on the header's right-anchored `status #number ↗`
/// chip — the whole chip opens the PR.
#[must_use]
pub fn hit_pr_open(area: Rect, app: &App, col: u16, row: u16) -> bool {
    let Some(s) = app.pr_snapshot() else {
        return false;
    };
    if row != area.y {
        return false;
    }
    let chip_w = pr_chip_width(s) as u16;
    // The chip occupies the last `chip_w` columns; `saturating_sub` keeps the bound overflow-free.
    col >= area.width.saturating_sub(chip_w) && col < area.width
}

/// The comment index a click at `(col, row)` lands on, or `None` (a check, header, or blank).
/// Mirrors `render_pr_nav`'s row layout and cursor-windowed scroll; only comments are selectable.
#[must_use]
pub fn pr_nav_hit(area: Rect, app: &App, col: u16, row: u16) -> Option<usize> {
    let inner = inner_rect(panes(area, app.list_pct).files);
    if !contains(inner, col, row) {
        return None;
    }
    let s = app.pr_snapshot()?;
    // Reconstruct the painted window from the shared layout, then map the clicked display row
    // back to its cursor stop (checks and comments alike; headers and the blank are inert).
    let layout = pr_nav_layout(s);
    let selected = layout.iter().position(|r| *r == PrNavRow::Select(app.pr_cursor)).unwrap_or(0);
    let viewport = inner.height as usize;
    let scroll = pr_nav_scroll(layout.len(), selected, viewport);
    let d = (row - inner.y) as usize + scroll;
    match layout.get(d) {
        Some(PrNavRow::Select(i)) => Some(*i),
        _ => None,
    }
}

/// The status glyph and Catppuccin accent for a check.
fn check_glyph(p: &Palette, status: forge::CheckStatus) -> (&'static str, Color) {
    match status {
        forge::CheckStatus::Success => ("✓", p.green),
        forge::CheckStatus::Failure => ("✗", p.red),
        forge::CheckStatus::Running => ("●", p.yellow),
        forge::CheckStatus::Pending => ("○", p.overlay0),
        forge::CheckStatus::Skipped => ("⊘", p.overlay0),
    }
}

/// The read pane's word for a check's outcome.
fn check_status_word(status: forge::CheckStatus) -> &'static str {
    match status {
        forge::CheckStatus::Success => "passed",
        forge::CheckStatus::Failure => "failed",
        forge::CheckStatus::Running => "running",
        forge::CheckStatus::Pending => "pending",
        forge::CheckStatus::Skipped => "skipped",
    }
}

// --- helpers -------------------------------------------------------------------

fn bordered(title: &str, focused: bool, p: &Palette) -> Block<'static> {
    // A focused pane gets a lavender border; an unfocused one recedes to a surface tone.
    let color = if focused { p.lavender } else { p.surface2 };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(title.to_string())
}

fn dim_paragraph<'a>(text: &'a str, p: &Palette) -> Paragraph<'a> {
    Paragraph::new(text).style(Style::default().fg(p.overlay0))
}

/// The theme accent for a change marker, matched to the diff's add/remove hues.
fn kind_color(p: &Palette, marker: char) -> Color {
    match marker {
        'A' => p.green,
        'M' | 'R' => p.peach,
        'D' | '?' => p.red,
        _ => p.text,
    }
}

/// Whether `(col, row)` falls inside `rect`.
fn contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x
        && col < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

/// The content area inside a one-cell border.
fn inner_rect(outer: Rect) -> Rect {
    Rect {
        x: outer.x.saturating_add(1),
        y: outer.y.saturating_add(1),
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    }
}

/// A `Rect` centered in `area` at `pct_x` × `pct_y` percent of its size.
fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - pct_y) / 2),
        Constraint::Percentage(pct_y),
        Constraint::Percentage((100 - pct_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pct_x) / 2),
        Constraint::Percentage(pct_x),
        Constraint::Percentage((100 - pct_x) / 2),
    ])
    .split(v[1])[1]
}

#[cfg(test)]
mod empty_pane_click_tests {
    use super::{Rect, hit_divider, hit_file, in_files_pane};

    // The contract the mouse dispatch's empty-pane fallback rests on: with no file rows,
    // `hit_file` misses everywhere, yet `in_files_pane` still owns the clicks — so a click
    // in an emptied list (e.g. a filter with no matches) can claim focus instead of falling
    // through to the diff pane.
    #[test]
    fn empty_list_clicks_stay_in_files_pane() {
        let area = Rect::new(0, 0, 100, 30);
        let list_pct = 30;
        let mut pane_points = 0;
        for row in 0..30 {
            for col in 0..100 {
                assert_eq!(hit_file(area, list_pct, col, row, 0, 0), None);
                if in_files_pane(area, list_pct, col, row) && !hit_divider(area, list_pct, col, row)
                {
                    pane_points += 1;
                }
            }
        }
        assert!(pane_points > 0, "the files pane should own some clickable area");
    }

    // With rows present, a click below the last row is still a pane click, not a file hit —
    // the same fallback focuses the pane rather than doing nothing.
    #[test]
    fn click_below_last_row_misses_files_but_stays_in_pane() {
        let area = Rect::new(0, 0, 100, 30);
        let list_pct = 30;
        // Files pane: right 30% of the body band → x=70..100; inner rows start at y=2.
        let (col, row) = (80, 10);
        assert!(in_files_pane(area, list_pct, col, row));
        assert_eq!(hit_file(area, list_pct, col, row, 3, 0), None, "row 10 is past 3 files");
        assert!(hit_file(area, list_pct, col, 2, 3, 0).is_some(), "top inner row hits file 0");
    }
}
