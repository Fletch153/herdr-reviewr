mod common;

use common::Repo;
use herdr_reviewr::app::{App, Focus, Mode};
use herdr_reviewr::model::Scope;
use herdr_reviewr::ui::{self, HeaderHit};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn dump(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                out.push_str(cell.symbol());
            }
        }
        out.push('\n');
    }
    out
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    dump(terminal.backend().buffer())
}

fn render_buffer(app: &App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn render_at(app: &App, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    dump(terminal.backend().buffer())
}

const SELECTION_BG: ratatui::style::Color = ratatui::style::Color::Rgb(0x58, 0x5b, 0x70);
const PEACH: ratatui::style::Color = ratatui::style::Color::Rgb(0xfa, 0xb3, 0x87);

fn composing(app: &mut App) {
    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|r| r.marker() == '+').unwrap();
    app.start_comment();
}

#[test]
fn the_empty_comment_box_shows_a_placeholder() {
    let mut app = edited_app();
    composing(&mut app);
    assert!(render(&app).contains("Leave a comment…"), "an empty box shows the placeholder");
}

#[test]
fn the_caret_block_sits_on_the_character_at_the_caret() {
    let mut app = edited_app();
    composing(&mut app);
    app.input_push('a');
    app.input_push('b');
    app.caret_left();
    let buf = render_buffer(&app);
    let mut found = false;
    for y in 0..40 {
        for x in 0..140 {
            if buf.cell((x, y)).is_some_and(|c| c.bg == PEACH && c.symbol() == "b") {
                found = true;
            }
        }
    }
    assert!(found, "the caret block highlights the character at the caret");
}

#[test]
fn caret_vertical_moves_between_wrapped_rows() {
    assert_eq!(ui::caret_vertical("abcdef", 4, 3, false), 1);
    assert_eq!(ui::caret_vertical("abcdef", 1, 3, true), 4);
}

#[test]
fn the_fold_hint_names_the_arrow_key() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut body = String::new();
    for i in 0..30 {
        let _ = writeln!(body, "line {i}");
    }
    r.write("f.rs", &body);
    r.commit_all("init");
    r.write("f.rs", &body.replace("line 15", "LINE 15"));
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|row| row.hidden() > 0).expect("a fold row");

    let out = render(&app);
    assert!(out.contains("→ expand"), "the fold hint names the `→` key");
    assert!(!out.contains("⏎ expand"), "no stale enter hint remains");
}

#[test]
fn a_click_on_the_stage_marker_is_not_swallowed_by_the_pane_divider() {
    // Regression: the divider grab zone used to extend one column into the file pane, over the
    // stage-marker cell, so a marker click started a (no-op) resize instead of staging. The whole
    // mouse dispatch checks `hit_divider` before `hit_file`, so the marker must fall outside it.
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n"); // an unstaged modification -> a grey 'M' marker
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let (w, h) = (140u16, 40u16);
    let area = Rect::new(0, 0, w, h);
    let buf = render_buffer(&app);

    // Find the 'M' stage-marker cell in the rendered file pane.
    let mut marker = None;
    for y in 0..h {
        for x in 0..w {
            if buf.cell((x, y)).is_some_and(|c| c.symbol() == "M") {
                marker = Some((x, y));
            }
        }
    }
    let (mx, my) = marker.expect("the 'M' stage marker is rendered");

    assert!(ui::on_file_marker(area, app.list_pct, mx, my), "the marker cell is the stage target");
    assert!(
        !ui::hit_divider(area, app.list_pct, mx, my),
        "the divider must not swallow the marker click"
    );
    // The divider is still grabbable on its own border, one column left of the marker.
    assert!(
        ui::hit_divider(area, app.list_pct, mx - 1, my),
        "the pane border still starts a resize"
    );

    // And the click actually stages: dispatch order is divider (miss) -> file hit -> marker -> stage.
    assert_eq!(app.file_status("a.rs").map(|s| s.staged), Some(false), "starts unstaged");
    let row = app.file_rows.iter().position(|rw| rw.name.contains("a.rs")).expect("a.rs row");
    assert!(!ui::hit_divider(area, app.list_pct, mx, my) && app.stage_toggle(row), "marker stages");
    assert_eq!(app.file_status("a.rs").map(|s| s.staged), Some(true), "the click staged it");
}

#[test]
fn a_reviewed_tick_keeps_the_staging_colour() {
    // Regression: a reviewed file rendered a ✓ hardcoded green, discarding its staging status.
    // The ✓ must carry the same colour the git marker would (green staged, grey not).
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n"); // unstaged modification -> grey status
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.toggle_reviewed(); // mark a.rs reviewed -> the marker becomes ✓

    let tick_fg = |app: &App| {
        let buf = render_buffer(app);
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = buf.cell((x, y)).unwrap();
                if cell.symbol() == "✓" {
                    return cell.fg;
                }
            }
        }
        panic!("no ✓ was rendered");
    };

    let unstaged = tick_fg(&app);
    let row = app.file_rows.iter().position(|rr| rr.name.contains("a.rs")).expect("a.rs row");
    app.stage_toggle(row); // stage it; content is unchanged so the review tick survives
    assert!(app.is_reviewed("a.rs"), "staging keeps the tick");
    let staged = tick_fg(&app);

    assert_ne!(unstaged, staged, "the ✓ colour tracks staging instead of always being green");
}

fn edited_app() -> App {
    let r = Repo::init();
    r.write("hello.rs", "alpha\nbeta\n");
    r.commit_all("init");
    r.write("hello.rs", "alpha\nBETA\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app
}

#[test]
fn the_file_list_renders_as_a_directory_tree() {
    let r = Repo::init();
    r.write("src/app.rs", "x\n");
    r.write("src/ui.rs", "y\n");
    r.write("Cargo.toml", "[package]\n");
    r.commit_all("init");
    r.write("src/app.rs", "x2\n");
    r.write("src/ui.rs", "y2\n");
    r.write("Cargo.toml", "[package]\nname='z'\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let files_pane: String = render(&app)
        .lines()
        .map(|l| l.chars().skip(l.chars().count() * 70 / 100).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(files_pane.contains("src/"), "the directory groups its files: {files_pane:?}");
    assert!(files_pane.contains("app.rs") && files_pane.contains("ui.rs"), "files by basename");
    assert!(!files_pane.contains("src/app.rs"), "a grouped file is not shown by full path");
    assert!(files_pane.contains("Cargo.toml"), "the top-level file shows too");
}

#[test]
fn a_saved_comment_renders_inline_as_a_card() {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|row| row.marker() == '+').unwrap();
    app.start_comment();
    for ch in "memoize this".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    let out = render(&app);
    assert!(out.contains("memoize this"), "the saved comment stays visible inline: {out:?}");
    assert!(out.contains("comment ·"), "the inline card is titled with the location");
}

#[test]
fn a_renamed_file_shows_old_arrow_new_in_the_header() {
    let r = Repo::init();
    r.write("old_name.rs", "stable contents that survive the move\nplus a second line\n");
    r.commit_all("init");
    r.git(&["mv", "old_name.rs", "new_name.rs"]);
    r.write("new_name.rs", "stable contents that survive the move\nplus an edited line\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let out = render(&app);
    assert!(out.contains("old_name.rs → new_name.rs"), "header shows the rename: {out:?}");
}

#[test]
fn tabs_expand_to_spaces_in_the_diff() {
    let r = Repo::init();
    r.write("t.rs", "x\n");
    r.commit_all("init");
    r.write("t.rs", "x\n\tindented\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let out = render(&app);
    let line = out.lines().find(|l| l.contains("indented")).expect("the added line renders");
    assert!(!line.contains('\t'), "no literal tab in the rendered line");
    assert!(line.contains("    indented") || line.contains("   indented"), "tab became spaces");
}

#[test]
fn a_long_line_wraps_across_display_rows() {
    let long: String = std::iter::repeat_n("abcd", 60).collect();
    let r = Repo::init();
    r.write("w.rs", "x\n");
    r.commit_all("init");
    r.write("w.rs", &format!("x\n{long}\n"));
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let shown: String = render(&app).chars().filter(|c| *c == 'a').collect();
    assert!(shown.len() >= 60, "all of the wrapped line is shown, not truncated");
    let heights = ui::diff_row_heights(&app, AREA);
    let wrapped = app.visible.iter().position(|r| r.text().starts_with("abcd")).unwrap();
    assert!(heights[wrapped] > 1, "the long line spans multiple display rows");
}

#[test]
fn wrapping_breaks_at_word_boundaries() {
    let words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima \
                 mike november oscar papa quebec romeo sierra tango";
    let r = Repo::init();
    r.write("w.rs", "x\n");
    r.commit_all("init");
    r.write("w.rs", &format!("x\n{words}\n"));
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let heights = ui::diff_row_heights(&app, AREA);
    let wrapped = app.visible.iter().position(|r| r.text().starts_with("alpha")).unwrap();
    assert!(heights[wrapped] > 1, "the line wraps across rows");

    let out = render(&app);
    for word in words.split(' ') {
        assert!(out.lines().any(|l| l.contains(word)), "word {word:?} is not split across rows");
    }
}

#[test]
fn wide_glyphs_wrap_by_column_width_not_char_count() {
    let cjk: String = std::iter::repeat_n('あ', 50).collect();
    let ascii: String = std::iter::repeat_n('a', 50).collect();
    let r = Repo::init();
    r.write("w.rs", "x\n");
    r.commit_all("init");
    r.write("w.rs", &format!("x\n{ascii}\n{cjk}\n"));
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let heights = ui::diff_row_heights(&app, AREA);
    let ascii_h = heights[app.visible.iter().position(|r| r.text().starts_with('a')).unwrap()];
    let cjk_h = heights[app.visible.iter().position(|r| r.text().starts_with('あ')).unwrap()];
    assert!(cjk_h > ascii_h, "wide glyphs wrap by columns: cjk {cjk_h} > ascii {ascii_h}");
}

#[test]
fn horizontal_scroll_shifts_the_diff_left() {
    let r = Repo::init();
    r.write("w.rs", "x\n");
    r.commit_all("init");
    r.write("w.rs", "x\nAAAABBBBCCCCDDDD_marker\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.wrap = false;
    app.reload().unwrap();
    assert!(render(&app).contains("AAAABBBB"), "the line head shows before scrolling");

    app.scroll_h(8);
    let out = render(&app);
    assert!(!out.contains("AAAABBBB"), "the scrolled-off head is gone");
    assert!(out.contains("CCCCDDDD_marker"), "the later columns are now visible");
}

#[test]
fn a_changed_word_gets_the_emphasis_background() {
    const EMPH_INS_BG: ratatui::style::Color = ratatui::style::Color::Rgb(0x30, 0x55, 0x3f);
    let r = Repo::init();
    r.write("e.rs", "let x = foo(a);\n");
    r.commit_all("init");
    r.write("e.rs", "let x = bar(a, b);\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Files;
    let buf = render_buffer(&app);

    let mut found = false;
    for y in 0..40 {
        for x in 0..95 {
            if let Some(c) = buf.cell((x, y))
                && c.bg == EMPH_INS_BG
                && c.symbol() == "b"
            {
                found = true;
            }
        }
    }
    assert!(found, "a changed word carries the emphasis background");
}

#[test]
fn the_selected_file_row_fills_with_the_shared_selection_color() {
    let app = edited_app();
    let buf = render_buffer(&app);
    let files_x0 = 140 - 140 * 32 / 100 + 1;
    let selected =
        (files_x0..139).filter(|&x| buf.cell((x, 2)).is_some_and(|c| c.bg == SELECTION_BG)).count();
    assert!(selected > 10, "the selected file row fills wide with surface2: {selected} cells");
}

#[test]
fn shows_tab_bar_file_list_and_diff() {
    let app = edited_app();
    let out = render(&app);
    assert!(out.contains("Changes"), "tab bar names the view");
    assert!(out.contains("uncommitted"), "current scope shown");
    assert!(out.contains("hello.rs"), "file appears in the list");
    assert!(out.contains("BETA"), "diff content is rendered");
    assert!(out.contains("changed"), "the header shows the changed count");
}

fn footer_line(out: &str) -> String {
    out.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string()
}

fn on_changed_line(app: &mut App) {
    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|r| r.marker() == '+').unwrap();
}

#[test]
fn the_footer_shows_the_action_for_the_context() {
    let mut app = edited_app();
    on_changed_line(&mut app);
    let footer = footer_line(&render(&app));
    assert!(footer.contains("c comment"), "a diff line offers comment:\n{footer}");
    assert!(footer.contains("v select"), "and selecting a range:\n{footer}");
    assert!(!footer.contains("changed"), "the changed count is not in the footer:\n{footer}");
}

#[test]
fn the_footer_drops_to_fit_and_marks_the_clip() {
    let mut app = edited_app();
    on_changed_line(&mut app);
    let wide = footer_line(&render_at(&app, 120));
    assert!(
        wide.contains("c comment") && wide.contains("v select") && !wide.contains('…'),
        "wide footer shows all actions, no clip marker:\n{wide}"
    );
    let narrow = footer_line(&render_at(&app, 18));
    assert!(narrow.contains("c comment"), "the primary action is never dropped:\n{narrow}");
    assert!(narrow.contains('…'), "the clip is marked with …:\n{narrow}");
    assert!(!narrow.contains("v select"), "the least-relevant action is trimmed:\n{narrow}");
}

#[test]
fn the_pr_footer_keeps_the_open_action_when_the_state_line_is_long() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::forge::{Check, CheckStatus, Merge, PrSnapshot, PrState, PrView, Sync};
    let r = Repo::init();
    r.write("x.rs", "y\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::Pr).unwrap();
    app.pr = PrView::Pr(Box::new(PrSnapshot {
        number: 226,
        title: "t".into(),
        url: "u".into(),
        state: PrState::Open,
        is_draft: false,
        base_ref: "main".into(),
        merge: Merge::Conflicting,
        sync: Sync::Behind(3),
        checks: vec![Check { name: "ci".into(), status: CheckStatus::Failure }],
        comments: vec![],
        truncated: true,
    }));
    let footer = footer_line(&render_at(&app, 60));
    assert!(footer.contains("o open"), "the open action survives a long state line:\n{footer}");
}

#[test]
fn the_footer_keeps_its_actions_alongside_a_status() {
    let mut app = edited_app();
    on_changed_line(&mut app);
    app.status = "comment added".to_string();
    let footer = footer_line(&render(&app));
    assert!(footer.contains("comment added"), "the status shows:\n{footer}");
    assert!(
        footer.contains("c comment"),
        "the primary action persists alongside a status:\n{footer}"
    );
}

#[test]
fn empty_repo_shows_empty_states() {
    let r = Repo::init();
    r.write("seed.rs", "x\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let out = render(&app);
    assert!(out.contains("no changes"), "empty file list state");
}

#[test]
fn composing_renders_the_inline_multiline_box() {
    let mut app = edited_app();
    app.focus = Focus::Diff;
    app.diff_cursor = app.diff.rows.iter().position(|r| r.marker() == '+').unwrap();
    app.start_comment();
    for ch in "line one".chars() {
        app.input_push(ch);
    }
    app.input_push('\n');
    for ch in "line two".chars() {
        app.input_push(ch);
    }

    let out = render(&app);
    assert!(out.contains("comment ·"), "box titled with the location");
    assert!(out.contains("line one"), "first input line shown");
    assert!(out.contains("line two"), "second input line shown — the box is multi-line");
}

#[test]
fn the_box_grows_with_multiline_input_and_keeps_the_anchor_visible() {
    let r = Repo::init();
    r.write("mid.rs", "a\nb\nc\nd\ne\n");
    r.commit_all("init");
    r.write("mid.rs", "a\nB\nc\nd\ne\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor =
        app.diff.rows.iter().position(|r| r.marker() == '+' && r.text().contains('B')).unwrap();
    app.start_comment();
    for ch in "one\ntwo\nthree".chars() {
        app.input_push(ch);
    }

    let out = render(&app);
    assert!(out.contains("one") && out.contains("two") && out.contains("three"), "all box lines");
    let lines: Vec<&str> = out.lines().collect();
    let anchor = lines.iter().position(|l| l.contains('B')).expect("anchor line visible");
    let box_row = lines.iter().position(|l| l.contains("comment ·")).expect("box");
    assert!(anchor < box_row, "the commented line stays above the box as it grows");
}

#[test]
fn the_box_is_inserted_under_the_selected_line() {
    let r = Repo::init();
    r.write("mid.rs", "alpha\nbeta\ngamma\n");
    r.commit_all("init");
    r.write("mid.rs", "alpha\nBETA\ngamma\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = app.diff.rows.iter().position(|r| r.text().contains("BETA")).unwrap();
    app.start_comment();
    for ch in "note".chars() {
        app.input_push(ch);
    }

    let out = render(&app);
    let lines: Vec<&str> = out.lines().collect();
    let box_row = lines.iter().position(|l| l.contains("comment ·")).expect("box rendered");
    let below_row = lines.iter().position(|l| l.contains("gamma")).expect("context below shown");
    assert!(below_row > box_row, "the diff line below the selection is pushed under the box");
}

const AREA: Rect = Rect { x: 0, y: 0, width: 140, height: 40 };

#[test]
fn header_clicks_map_to_scope_and_send() {
    let mut app = edited_app();
    app.scope = Scope::LastTurn;
    let scope: Vec<u16> = (0..AREA.width)
        .filter(|&c| ui::hit_header(AREA, &app, c, 0) == Some(HeaderHit::Scope))
        .collect();
    let next: Vec<u16> = (0..AREA.width)
        .filter(|&c| ui::hit_header(AREA, &app, c, 0) == Some(HeaderHit::NextComment))
        .collect();
    let send: Vec<u16> = (0..AREA.width)
        .filter(|&c| ui::hit_header(AREA, &app, c, 0) == Some(HeaderHit::Send))
        .collect();

    assert!(!scope.is_empty(), "scope chip is clickable");
    assert!(!next.is_empty(), "next-comment button is clickable");
    assert!(!send.is_empty(), "send button is clickable");
    assert!(scope.iter().max() < next.iter().min(), "scope is left of the next-comment button");
    assert!(
        next.iter().max() < send.iter().min(),
        "next-comment button is left of send, no overlap"
    );
    assert!(*send.iter().max().unwrap() < AREA.width);

    // The suffix/pad between the left controls and the right-aligned button is inert.
    let gap = send.iter().min().unwrap() - 1;
    assert_eq!(ui::hit_header(AREA, &app, gap, 0), None, "the space between controls is inert");
    assert_eq!(ui::hit_header(AREA, &app, scope[0], 5), None, "only row 0 is the header");
}

#[test]
fn file_and_diff_clicks_map_to_row_indices() {
    let app = edited_app();
    assert_eq!(ui::hit_file(AREA, app.list_pct, 120, 2, app.file_rows.len(), 0), Some(0));
    assert_eq!(ui::hit_file(AREA, app.list_pct, 120, 9, app.file_rows.len(), 0), None);
    assert_eq!(ui::hit_file(AREA, app.list_pct, 120, 2, 50, 7), Some(7));
    assert_eq!(ui::hit_file(AREA, app.list_pct, 120, 3, 50, 7), Some(8));
    assert!(ui::in_files_pane(AREA, app.list_pct, 120, 3));
    assert!(!ui::in_files_pane(AREA, app.list_pct, 10, 3));
    assert!(app.visible.len() > 1);
    let heights = ui::diff_row_heights(&app, AREA);
    assert_eq!(ui::hit_diff(AREA, app.list_pct, 10, 2, &heights, 0), Some(0));
    assert_eq!(ui::hit_diff(AREA, app.list_pct, 10, 3, &heights, 0), Some(1));
    let tall = [2usize, 2, 2, 2];
    assert_eq!(ui::hit_diff(AREA, app.list_pct, 10, 2, &tall, 1), Some(1));
    assert_eq!(ui::hit_diff(AREA, app.list_pct, 10, 3, &tall, 1), Some(1));
    assert_eq!(ui::hit_diff(AREA, app.list_pct, 10, 4, &tall, 1), Some(2));
}

#[test]
fn a_binary_file_shows_the_no_line_comments_message() {
    let r = Repo::init();
    r.write("logo.bin", "\0\0\0\0seed\0\0");
    r.commit_all("init");
    r.write("logo.bin", "\0\0\0\0changed\0\0\0");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let idx = app.entries.iter().position(|f| f.path == "logo.bin").expect("binary file listed");
    app.select_file(idx).unwrap();

    let out = render(&app);
    assert!(out.contains("binary — no line comments"), "binary diff message shown:\n{out}");
}

#[test]
fn the_comments_list_flags_a_stale_comment() {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = app.diff.rows.iter().position(|r| r.marker() == '+').unwrap();
    app.start_comment();
    for ch in "look here".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    r.remove("a.rs");
    app.reload().unwrap();
    app.open_list();

    let out = render(&app);
    assert!(out.contains("(stale)"), "orphaned comment flagged in the list:\n{out}");
}

#[test]
fn open_list_renders_the_comments_overlay() {
    let mut app = edited_app();
    app.focus = Focus::Diff;
    app.diff_cursor = app.diff.rows.iter().position(|r| r.marker() == '+').unwrap();
    app.start_comment();
    for ch in "overlay note".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    app.open_list();

    let out = render(&app);
    assert!(out.contains("Comments ("), "overlay titled with a count");
    assert!(out.contains("overlay note"), "comment text listed");
}

#[test]
fn the_comments_list_groups_by_base_and_shows_checkboxes() {
    use herdr_reviewr::model::{Comment, Scope, Side};
    let mk = |text: &str, sent: bool, base: Option<&str>, diff: bool| Comment {
        file: "a.rs".into(),
        side: Side::New,
        start: 1,
        end: 1,
        lines: "x".into(),
        text: text.into(),
        diff_anchored: diff,
        scope: Scope::Commit,
        base: base.map(Into::into),
        sent,
    };
    let mut app = edited_app();
    app.store.add(mk("fresh note", false, Some("abc1234"), true));
    app.store.add(mk("sent note", true, Some("abc1234"), true));
    app.store.add(mk("file note", false, None, false));
    app.open_list();

    let out = render(&app);
    assert!(out.contains("Comments ("), "overlay titled");
    assert!(out.contains("── commit"), "a commit group header renders");
    assert!(out.contains("── All files ──"), "an all-files group header renders");
    assert!(out.contains("[ ]"), "checkboxes render");
    assert!(out.contains("fresh note") && out.contains("file note"), "comment text listed");
    assert!(out.contains("space check"), "the command status bar renders at the bottom");
}

#[test]
fn the_delete_confirmation_overlay_names_the_target() {
    let mut app = edited_app();
    app.request_delete();
    let out = render(&app);
    assert!(out.contains("Delete file"), "the overlay names the action");
    assert!(out.contains("hello.rs"), "and the target path");
    assert!(out.contains("y / enter"), "and the confirm key");
    assert!(out.contains("working tree"), "and warns it removes the file");
}

#[test]
fn the_help_panel_lists_the_key_sections() {
    let mut app = edited_app();
    app.open_help();
    let out = render(&app);
    assert!(out.contains("Keys"), "the help overlay is titled");
    assert!(out.contains("Navigate"), "a section header renders");
    assert!(out.contains("Comments"), "a section header renders");
    assert!(out.contains("resolve"), "the resolve key is documented");
}

#[test]
fn last_turn_without_a_baseline_renders_the_waiting_state() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.reload().unwrap();
    let out = render(&app);
    assert!(out.contains("[last turn]"), "the scope chip reads last turn");
    assert!(out.contains("waiting for the agent's next turn"), "the cold-start empty state shows");
}

#[test]
fn all_files_tab_bar_footer_and_count_read_for_the_tab() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::AllFiles).unwrap();

    let out = render(&app);
    assert!(out.contains("1 Changes"), "tab labels carry their switch digit:\n{out}");
    assert!(out.contains("2 All files"));
    assert!(
        out.contains("1 changed"),
        "the changed count stays in the header on All files:\n{out}"
    );
    let footer = footer_line(&out);
    assert!(footer.contains("scope"), "the footer shows context actions on All files:\n{footer}");
    assert!(
        !footer.contains("changed"),
        "the changed count is not repeated in the footer:\n{footer}"
    );
}

#[test]
fn a_narrow_overflowing_header_does_not_mis_map_a_click_to_send() {
    let r = Repo::init();
    r.write("a.rs", "x\n");
    r.commit_all("init");
    r.write("a.rs", "y\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let width: u16 = 34;
    let area = Rect::new(0, 0, width, 40);
    let phantom = (0..width).any(|c| ui::hit_header(area, &app, c, 0) == Some(HeaderHit::Send));
    assert!(!phantom, "no on-screen column mis-maps to Send when the narrow header overflows");

    let wide = Rect::new(0, 0, 140, 40);
    let send = (0..140).any(|c| ui::hit_header(wide, &app, c, 0) == Some(HeaderHit::Send));
    assert!(send, "Send is clickable when the header fits");
}

#[test]
fn all_files_empty_pane_reads_select_a_file() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("src/a.rs", "x\n");
    r.write("src/b.rs", "y\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::AllFiles).unwrap();

    let out = render(&app);
    assert!(out.contains("select a file to read"), "the empty All files left pane copy:\n{out}");
    assert!(!out.contains("no diff"), "no diff vocabulary in the content browser:\n{out}");
}

#[test]
fn renders_a_light_theme_without_panic() {
    let mut app = edited_app();
    app.set_cli_theme(Some("catppuccin-latte".to_string()));
    let buf = render_buffer(&app);
    let latte_lavender = herdr_reviewr::theme::resolve(Some("catppuccin-latte")).palette.lavender;
    let painted = (0..40)
        .flat_map(|y| (0..140).map(move |x| (x, y)))
        .any(|(x, y)| buf.cell((x, y)).is_some_and(|c| c.fg == latte_lavender));
    assert!(painted, "the Latte palette reaches the painted buffer");
}

fn commit_render_app() -> (Repo, App) {
    let r = Repo::init();
    r.write("base.rs", "0\n");
    r.commit_all("base");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("a.rs", "a\n");
    r.commit_all("add alpha feature");
    r.write("b.rs", "b\n");
    r.commit_all("add beta feature");
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();
    (r, app)
}

#[test]
fn the_commit_picker_lists_hashes_and_titles() {
    let (_r, mut app) = commit_render_app();
    app.open_commit_picker();
    let out = render(&app);
    assert!(out.contains("Compare with commit ("), "titled overlay with a count");
    assert!(out.contains("add beta feature"), "a commit title is listed");
    assert!(out.contains(&app.commit_choices[0].short), "its abbreviated hash is shown");
}

#[test]
fn the_commit_chip_shows_the_selected_commit() {
    let (_r, mut app) = commit_render_app();
    app.open_commit_picker();
    app.pick_commit(1).unwrap();
    let out = render(&app);
    assert!(
        out.contains(&format!("[>{}", app.commit_choices[1].short)),
        "the header chip shows the picked commit's hash"
    );
    assert!(out.contains("add alpha feature"), "and its title");
}

#[test]
fn entering_commit_scope_defaults_to_the_tip_without_the_picker() {
    let r = Repo::init();
    r.write("a.rs", "0\n");
    r.commit_all("only");
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();

    app.enter_commit_scope().unwrap();
    let out = render(&app);
    assert_eq!(app.mode, Mode::Normal, "cycling in does not open the picker");
    assert!(!out.contains("Compare with commit ("), "no picker popup on entry: {out}");
    assert!(out.contains("[uncommitted]"), "the tip is labelled uncommitted: {out}");
}

#[test]
fn icons_show_folder_and_file_glyphs_when_enabled() {
    let r = Repo::init();
    r.write("src/a.rs", "1\n");
    r.write("src/b.rs", "2\n");
    r.commit_all("init");
    r.write("src/a.rs", "11\n");
    r.write("src/b.rs", "22\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.icons = true;

    let out = render(&app);
    assert!(out.contains('\u{f07b}') || out.contains('\u{f07c}'), "a folder glyph shows for src/");
    assert!(out.contains('\u{e7a8}'), "the rust filetype glyph shows for the .rs files");
}

#[test]
fn icons_are_absent_by_default() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("init");
    r.write("a.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let out = render(&app);
    assert!(!out.contains('\u{e7a8}'), "no filetype glyph when icons are off (the default)");
}

#[test]
fn icons_replace_the_folder_arrows() {
    let r = Repo::init();
    r.write("src/a.rs", "1\n");
    r.write("src/b.rs", "2\n");
    r.commit_all("init");
    r.write("src/a.rs", "11\n");
    r.write("src/b.rs", "22\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.icons = true;

    let out = render(&app);
    assert!(
        !out.contains("\u{25be} src") && !out.contains("\u{25b8} src"),
        "no arrow before the folder"
    );
    assert!(
        out.contains('\u{f07c}') || out.contains('\u{f07b}'),
        "the folder glyph conveys expansion"
    );
}

#[test]
fn a_long_file_name_truncates_at_the_end() {
    let r = Repo::init();
    let long = "a_very_long_file_name_that_exceeds_the_narrow_list_pane_width.rs";
    r.write(long, "1\n");
    r.commit_all("init");
    r.write(long, "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let out = render(&app);
    assert!(out.contains("a_very_long"), "the start of the name is shown");
    assert!(out.contains('\u{2026}'), "a trailing ellipsis marks the truncation");
    assert!(!out.contains("\u{2026}rs"), "not the old leading-ellipsis (…name.rs) form");
}

#[test]
fn the_status_marker_sits_in_a_left_gutter_that_aligns_rows() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("aaa.rs", "1\n");
    r.write("bbb.rs", "1\n");
    r.commit_all("init");
    r.write("aaa.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.set_tab(Tab::AllFiles).unwrap();

    let out = render(&app);
    let column_of = |name: &str| -> (String, usize) {
        let line = out
            .lines()
            .filter(|l| !l.contains('┌'))
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name} not found in a list row"));
        let byte = line.rfind(name).unwrap();
        (line.to_string(), line[..byte].chars().count())
    };
    let (a_line, a_col) = column_of("aaa.rs");
    let (_b_line, b_col) = column_of("bbb.rs");
    assert_eq!(a_col, b_col, "the fixed gutter keeps the changed and unchanged names aligned");
    let chars: Vec<char> = a_line.chars().collect();
    assert_eq!(chars[a_col - 2], 'M', "the M marker leads the gutter");
    assert_eq!(chars[a_col - 1], '│', "a vertical rule closes the gutter");
}

#[test]
fn the_filter_query_shows_in_the_pane_title() {
    let r = Repo::init();
    r.write("evm.rs", "1\n");
    r.commit_all("init");
    r.write("evm.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.start_filter();
    for c in "evm".chars() {
        app.filter_push(c);
    }
    assert!(render(&app).contains("/evm"), "the pane title carries the active filter");
}

#[test]
fn the_markdown_preview_renders_the_document() {
    let r = Repo::init();
    r.write("doc.md", "# Heading\n\nsome text\n");
    r.commit_all("init");
    r.write("doc.md", "# Heading\n\nsome more text\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    app.open_preview();
    assert!(app.md_showing(), "the markdown view is on and the open file renders");
    let out = render(&app);
    assert!(out.contains("Heading"), "the heading text shows");
    assert!(!out.contains("# Heading"), "the '#' marker is rendered away, not shown raw");
    assert!(out.contains("preview \u{b7}"), "the pane titles itself a preview");
}

#[test]
fn the_branch_picker_title_counts_only_selectable_branches() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("base");
    r.git(&["update-ref", "refs/remotes/origin/main", "main"]);
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.rs", "2\n");
    r.commit_all("work");
    let mut app = App::new(r.path_buf(), Scope::Branch, None);
    app.reload().unwrap();
    app.open_branch_picker();

    assert_eq!(app.branch_choices.len(), 3, "main + divider + origin/main");
    let out = render(&app);
    assert!(out.contains("Compare with branch (2)"), "the count excludes the divider: {out}");
}
