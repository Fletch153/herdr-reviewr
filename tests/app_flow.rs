mod common;

use std::cell::RefCell;

use anyhow::{Result, bail};
use common::Repo;
use herdr_reviewr::app::{App, BranchRow, Focus, FooterAction, Mode, Tab};
use herdr_reviewr::export::ExportTarget;
use herdr_reviewr::model::{Scope, Side};

struct FakeTarget {
    ok: bool,
    marks_sent: bool,
    captured: RefCell<Vec<String>>,
}

impl FakeTarget {
    fn ok() -> Self {
        Self { ok: true, marks_sent: true, captured: RefCell::new(Vec::new()) }
    }
    fn failing() -> Self {
        Self { ok: false, marks_sent: true, captured: RefCell::new(Vec::new()) }
    }
    fn copy() -> Self {
        Self { ok: true, marks_sent: false, captured: RefCell::new(Vec::new()) }
    }
    fn last(&self) -> String {
        self.captured.borrow().last().cloned().unwrap_or_default()
    }
    fn count(&self) -> usize {
        self.captured.borrow().len()
    }
}

impl ExportTarget for FakeTarget {
    fn label(&self) -> &'static str {
        "fake"
    }
    fn marks_sent(&self) -> bool {
        self.marks_sent
    }
    fn export(&self, text: &str) -> Result<()> {
        self.captured.borrow_mut().push(text.to_string());
        if self.ok { Ok(()) } else { bail!("fake export failure") }
    }
}

fn edited_repo() -> Repo {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\ngamma\ndelta\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\ngamma\ndelta\nepsilon\n");
    r
}

fn app_on(r: &Repo) -> App {
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app
}

fn clamp(app: &mut App, viewport: usize) {
    let heights = vec![1usize; app.visible.len()];
    app.reveal_diff_cursor(&heights, viewport);
    app.bound_diff_scroll(&heights, viewport);
}

#[test]
fn the_file_list_decouples_viewport_scroll_from_selection() {
    let r = Repo::init();
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "one\n");
    }
    r.commit_all("init");
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "two\n");
    }
    let mut app = app_on(&r);
    assert_eq!(app.file_rows.len(), 20);
    let viewport = 6;

    assert_eq!(app.file_cursor, 0);
    let opened = app.diff_path.clone();
    assert!(opened.is_some());

    app.reveal_files = false;
    app.wheel_files(5);
    app.bound_file_scroll(viewport);
    assert_eq!(app.file_scroll, 5);
    assert_eq!(app.file_cursor, 0);
    assert_eq!(app.diff_path, opened);
    assert!(app.file_cursor < app.file_scroll);
    assert!(!app.reveal_files, "the wheel does not request a reveal");

    app.move_cursor(1).unwrap();
    app.reveal_file_cursor(viewport);
    assert_eq!(app.file_cursor, 1);
    assert!(app.file_cursor >= app.file_scroll && app.file_cursor < app.file_scroll + viewport);
    assert_ne!(app.diff_path, opened);

    for _ in 0..18 {
        app.move_cursor(1).unwrap();
    }
    app.reveal_file_cursor(viewport);
    assert_eq!(app.file_cursor, 19);
    assert!(app.file_cursor < app.file_scroll + viewport);
    assert_eq!(app.file_scroll, 20 - viewport);

    app.wheel_files(100);
    app.bound_file_scroll(viewport);
    assert_eq!(app.file_scroll, 20 - viewport);
}

fn long_diff_app(n: usize) -> App {
    use std::fmt::Write as _;
    let r = Repo::init();
    let (mut old, mut new) = (String::new(), String::new());
    for i in 0..n {
        let _ = writeln!(old, "line {i}");
        let _ = writeln!(new, "LINE {i}");
    }
    r.write("a.rs", &old);
    r.commit_all("init");
    r.write("a.rs", &new);
    let mut app = app_on(&r);
    app.reload().unwrap();
    app
}

#[test]
fn bound_diff_scroll_keeps_a_wrapped_bottom_reachable() {
    let mut app = long_diff_app(5);
    let heights = vec![3usize; 30];
    app.diff_scroll = 999;
    app.bound_diff_scroll(&heights, 20);
    assert!(
        app.diff_scroll > 10,
        "height-aware bound passes the row-count cap: {}",
        app.diff_scroll
    );
    assert!(app.diff_scroll <= 29);
}

#[test]
fn the_wheel_scrolls_the_diff_without_moving_its_cursor() {
    let mut app = long_diff_app(40);
    app.focus = Focus::Diff;
    app.diff_cursor = 3;
    app.reveal_diff = false;
    app.wheel_diff(10);
    let h = vec![1usize; app.visible.len()];
    app.bound_diff_scroll(&h, 8);
    assert_eq!(app.diff_cursor, 3, "the wheel leaves the comment cursor put");
    assert!(app.diff_scroll > 0, "the wheel moved the viewport");
    assert!(!app.reveal_diff, "the wheel does not request a reveal");
}

#[test]
fn a_boundary_move_reveals_the_cursor_after_wheeling() {
    let r = Repo::init();
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "one\n");
    }
    r.commit_all("init");
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "two\n");
    }
    let mut app = app_on(&r);
    let vp = 6;
    app.wheel_files(10);
    app.bound_file_scroll(vp);
    assert!(app.file_cursor < app.file_scroll, "cursor (row 0) is wheeled off-screen above");
    app.reveal_files = false;
    app.move_cursor(-1).unwrap();
    assert_eq!(app.file_cursor, 0);
    assert!(app.reveal_files, "a clamp-to-same-index move still requests a reveal");
    app.reveal_file_cursor(vp);
    assert_eq!(app.file_scroll, 0, "the cursor is pulled back into view");
}

#[test]
fn toggling_a_directory_requests_a_reveal() {
    let r = Repo::init();
    r.write("src/a.rs", "x\n");
    r.write("src/b.rs", "y\n");
    r.commit_all("init");
    r.write("src/a.rs", "x2\n");
    r.write("src/b.rs", "y2\n");
    let mut app = app_on(&r);
    app.focus = Focus::Files;
    let dir = app.file_rows.iter().position(|row| row.dir_path() == Some("src")).unwrap();
    app.file_cursor = dir;
    app.reveal_files = false;
    app.collapse_dir();
    assert!(app.reveal_files, "collapsing a directory requests a reveal (even at the same index)");
}

#[test]
fn page_keys_move_the_cursor_in_both_panes() {
    let mut app = long_diff_app(40);
    app.focus = Focus::Files;
    app.file_cursor = 0;
    app.reveal_files = false;
    app.move_cursor(5).unwrap();
    assert_eq!(app.file_cursor, 5usize.min(app.file_rows.len() - 1));
    assert!(app.reveal_files);
    app.focus = Focus::Diff;
    app.diff_cursor = 0;
    app.reveal_diff = false;
    app.move_cursor(5).unwrap();
    assert_eq!(app.diff_cursor, 5);
    assert!(app.reveal_diff);
}

#[test]
fn horizontal_scroll_is_inert_while_wrapping() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.wrap = true;
    app.scroll_h(8);
    assert_eq!(app.h_scroll, 0, "h-scroll does nothing while wrap is on, so it can't accumulate");
    app.wrap = false;
    app.scroll_h(8);
    assert_eq!(app.h_scroll, 8, "h-scroll moves once wrap is off");
}

#[test]
fn a_poll_preserves_the_wheel_scroll_in_both_panes() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let (mut old, mut new) = (String::new(), String::new());
    for i in 0..60 {
        let _ = writeln!(old, "line {i}");
        let _ = writeln!(new, "LINE {i}");
    }
    r.write("big.rs", &old);
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "one\n");
    }
    r.commit_all("init");
    r.write("big.rs", &new);
    for i in 0..20 {
        r.write(&format!("f{i:02}.txt"), "two\n");
    }
    let mut app = app_on(&r);

    app.select_file(file_row(&app, "big.rs")).unwrap();
    app.focus = Focus::Diff;
    app.wheel_diff(20);
    let h = vec![1usize; app.visible.len()];
    app.bound_diff_scroll(&h, 10);
    let diff_scroll = app.diff_scroll;
    assert!(diff_scroll > 0);
    app.wheel_files(8);
    app.bound_file_scroll(6);
    let file_scroll = app.file_scroll;
    assert!(file_scroll > 0);

    app.reveal_diff = false;
    app.reveal_files = false;
    app.reload().unwrap();
    assert!(!app.reveal_diff, "a poll does not reveal the diff cursor");
    assert!(!app.reveal_files, "a poll does not reveal the file cursor");
    let h = vec![1usize; app.visible.len()];
    app.bound_diff_scroll(&h, 10);
    app.bound_file_scroll(6);
    assert_eq!(app.diff_scroll, diff_scroll, "the diff wheel scroll survives the poll");
    assert_eq!(app.file_scroll, file_scroll, "the file-list wheel scroll survives the poll");
}

fn row_with(app: &App, marker: char) -> usize {
    app.diff.rows.iter().position(|r| r.marker() == marker).expect("a row with that marker")
}

fn file_row(app: &App, path: &str) -> usize {
    app.file_rows
        .iter()
        .position(|r| r.file_index().is_some_and(|i| app.entries[i].path == path))
        .expect("a file row for the path")
}

#[test]
fn editing_a_comment_surfaces_its_file_from_a_collapsed_directory() {
    let r = Repo::init();
    r.write("src/foo.rs", "a\nb\nc\n");
    r.write("src/bar.rs", "x\n");
    r.write("root.rs", "1\n");
    r.commit_all("init");
    r.write("src/foo.rs", "a\nB\nc\n");
    r.write("src/bar.rs", "y\n");
    r.write("root.rs", "2\n");
    let mut app = app_on(&r);

    app.select_file(file_row(&app, "src/foo.rs")).unwrap();
    comment_on(&mut app, '+', "note on foo");
    let commented_line = app.store.get(0).unwrap().start;

    app.select_file(file_row(&app, "root.rs")).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("root.rs"));
    app.file_cursor = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    app.collapse_dir();
    assert!(
        !app.file_rows
            .iter()
            .any(|r| r.file_index().is_some_and(|i| app.entries[i].path == "src/foo.rs")),
        "foo's row is hidden under the collapsed src/"
    );

    app.open_list();
    app.start_edit();
    assert_eq!(app.diff_path.as_deref(), Some("src/foo.rs"), "edit surfaced the comment's file");
    let row = app.visible.get(app.diff_cursor).expect("cursor on a row");
    assert_eq!(row.new_no(), Some(commented_line), "cursor landed on the commented line");
    assert!(matches!(app.mode, Mode::Composing { editing: Some(_) }));
}

fn expand_fold(app: &mut App) {
    let heights = vec![1usize; app.visible.len()];
    app.expand_fold(&heights, 80);
}

fn comment_on(app: &mut App, marker: char, text: &str) {
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(app, marker);
    app.start_comment();
    for ch in text.chars() {
        app.input_push(ch);
    }
    app.submit_comment();
}

fn composing_app() -> App {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    app
}

fn typed(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.input_push(ch);
    }
}

#[test]
fn the_editor_inserts_and_deletes_at_the_caret() {
    let mut app = composing_app();
    typed(&mut app, "ac");
    assert_eq!((app.input.as_str(), app.caret), ("ac", 2));
    app.caret_left();
    app.input_push('b');
    assert_eq!((app.input.as_str(), app.caret), ("abc", 2));
    app.input_backspace();
    assert_eq!((app.input.as_str(), app.caret), ("ac", 1));
    app.input_delete_forward();
    assert_eq!((app.input.as_str(), app.caret), ("a", 1));
}

#[test]
fn the_editor_moves_by_char_word_and_line() {
    let mut app = composing_app();
    typed(&mut app, "hello world");
    app.caret_home();
    assert_eq!(app.caret, 0);
    app.caret_end();
    assert_eq!(app.caret, 11);
    app.caret_word_left();
    assert_eq!(app.caret, 6, "to the start of 'world'");
    app.caret_word_left();
    assert_eq!(app.caret, 0, "to the start of 'hello'");
    app.caret_word_right();
    assert_eq!(app.caret, 5, "to the end of 'hello'");
}

#[test]
fn the_editor_kills_to_line_bounds_and_pastes_multiline() {
    let mut app = composing_app();
    typed(&mut app, "alpha beta");
    app.caret_home();
    app.caret_word_right();
    app.input_kill_to_end();
    assert_eq!(app.input, "alpha");
    app.input_kill_to_start();
    assert_eq!((app.input.as_str(), app.caret), ("", 0));
    app.input_paste("x\r\ny");
    assert_eq!((app.input.as_str(), app.caret), ("x\ny", 3));
}

#[test]
fn a_paste_outside_the_editor_is_ignored() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.input_paste("ignored");
    assert!(app.input.is_empty(), "paste does nothing outside the comment editor");
}

fn primary(app: &App) -> FooterAction {
    app.footer_actions().first().expect("a footer action").0
}

#[test]
fn the_footer_offers_the_action_for_what_the_cursor_is_on() {
    let mut app = composing_app();
    app.cancel_comment();
    assert_eq!(primary(&app), FooterAction::Comment, "a diff line offers comment");

    app.toggle_select();
    assert_eq!(primary(&app), FooterAction::Comment, "a live selection still leads with comment");
    assert!(
        app.footer_actions().iter().any(|&(a, _)| a == FooterAction::ClearSelection),
        "and offers to clear the selection"
    );
    app.toggle_select();

    comment_on(&mut app, '+', "note");
    assert_eq!(primary(&app), FooterAction::EditComment, "a commented line offers edit");
    assert!(
        app.footer_actions().iter().any(|&(a, _)| a == FooterAction::Send),
        "a written comment surfaces send wherever the cursor is"
    );
}

#[test]
fn esc_clears_a_live_selection() {
    let mut app = composing_app();
    app.cancel_comment();
    app.toggle_select();
    assert!(app.select_anchor.is_some(), "v starts a selection");
    app.clear_selection();
    assert!(app.select_anchor.is_none(), "esc clears the selection");
}

#[test]
fn the_footer_offers_scope_everywhere_on_a_file_tab() {
    let mut app = composing_app();
    app.cancel_comment();
    let has_scope = |a: &App| a.footer_actions().iter().any(|&(x, _)| x == FooterAction::Scope);
    assert!(has_scope(&app), "scope shows while reviewing a diff line");
    app.focus = Focus::Files;
    assert!(has_scope(&app), "scope shows in the file list too");
}

#[test]
fn the_pr_footer_offers_open_for_any_resolved_pr() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::forge::{Merge, PrSnapshot, PrState, PrView, Sync};

    let r = edited_repo();
    let mut app = app_on(&r);
    app.set_tab(Tab::Pr).unwrap();
    assert!(
        !app.footer_actions().iter().any(|&(a, _)| a == FooterAction::OpenPr),
        "no resolved PR → no open action"
    );

    app.pr = PrView::Pr(Box::new(PrSnapshot {
        number: 7,
        title: "t".into(),
        url: "u".into(),
        state: PrState::Open,
        is_draft: false,
        base_ref: "main".into(),
        merge: Merge::Clean,
        sync: Sync::InSync,
        checks: vec![],
        comments: vec![],
        truncated: false,
    }));
    assert!(app.pr_selected_comment().is_none(), "zero comments → nothing selected");
    assert_eq!(
        app.footer_actions().first().map(|&(a, _)| a),
        Some(FooterAction::OpenPr),
        "a resolved PR offers open even with no comments"
    );
}

#[test]
fn the_footer_offers_send_only_once_a_comment_exists() {
    let mut app = composing_app();
    app.cancel_comment();
    assert!(
        !app.footer_actions().iter().any(|&(a, _)| a == FooterAction::Send),
        "no comments yet → no send action"
    );
    comment_on(&mut app, '+', "note");
    assert!(
        app.footer_actions().iter().any(|&(a, _)| a == FooterAction::Send),
        "a comment written → send appears"
    );
}

fn folded_repo() -> Repo {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut old = String::new();
    for i in 0..40 {
        writeln!(old, "line {i}").unwrap();
    }
    r.write("big.rs", &old);
    r.commit_all("init");
    r.write("big.rs", &old.replace("line 20", "LINE 20"));
    r
}

#[test]
fn a_fold_expands_permanently_and_keeps_the_cursor_in_range() {
    let r = folded_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    let folded = app.visible.len();
    assert!(app.visible.iter().any(|row| row.hidden() > 0), "opens folded");

    app.diff_cursor = app.visible.iter().position(|row| row.hidden() > 0).unwrap();
    assert!(app.on_fold(), "`→` expands here");
    expand_fold(&mut app);
    let expanded = app.visible.len();
    assert!(expanded > folded, "expanding reveals the hidden lines");
    assert!(app.diff_cursor < app.visible.len(), "cursor stays in range");
    assert!(!app.on_fold(), "the fold is gone, so `→` now scrolls instead");

    expand_fold(&mut app);
    assert_eq!(app.visible.len(), expanded, "no collapse-back");
}

#[test]
fn a_selection_cannot_cross_a_fold() {
    let r = folded_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;

    let tail = app.visible.iter().rposition(|row| row.hidden() > 0).unwrap();
    app.diff_cursor = tail - 1;
    app.toggle_select();
    app.move_cursor(10).unwrap();
    assert_eq!(app.diff_cursor, tail - 1, "the cursor stops shy of the trailing fold");
    let (lo, hi) = app.selection_range();
    assert!((lo..=hi).all(|i| app.visible[i].is_content()), "no fold row is in the selection");

    let head = app.visible.iter().position(|row| row.hidden() > 0).unwrap();
    app.select_anchor = None;
    app.diff_cursor = head + 1;
    app.toggle_select();
    app.move_cursor(-10).unwrap();
    assert_eq!(app.diff_cursor, head + 1, "the cursor stops just after the leading fold");
}

#[test]
fn paging_the_diff_cannot_cross_a_fold() {
    let r = folded_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    let tail = app.visible.iter().rposition(|row| row.hidden() > 0).unwrap();
    app.diff_cursor = tail - 1;
    app.toggle_select();
    app.move_cursor(50).unwrap();
    assert_eq!(app.diff_cursor, tail - 1, "page stops shy of the fold while selecting");
}

#[test]
fn expanding_a_fold_does_not_bleed_into_another_file() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut body = String::new();
    for i in 0..40 {
        let _ = writeln!(body, "line {i}");
    }
    r.write("a.rs", &body);
    r.write("b.rs", &body);
    r.commit_all("init");
    r.write("a.rs", &body.replace("line 20", "A20"));
    r.write("b.rs", &body.replace("line 20", "B20"));
    let mut app = app_on(&r);

    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|row| row.hidden() > 0).unwrap();
    expand_fold(&mut app);
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"));

    app.focus = Focus::Files;
    app.move_cursor(1).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"));
    assert!(app.visible[0].hidden() > 0, "b.rs's leading fold stays collapsed");
}

#[test]
fn expanding_a_fold_keeps_the_viewport_still() {
    let r = folded_repo();

    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    let head = app.visible.iter().position(|row| row.hidden() > 0).unwrap();
    let shift = app.visible[head].hidden() - 1;
    app.diff_cursor = head;
    app.diff_scroll = 0;
    let heights = vec![1usize; app.visible.len()];
    app.expand_fold(&heights, 20);
    assert_eq!(app.diff_scroll, shift, "top-half fold grows upward");

    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    let tail = app.visible.iter().rposition(|row| row.hidden() > 0).unwrap();
    app.diff_cursor = tail;
    app.diff_scroll = 0;
    let heights = vec![1usize; app.visible.len()];
    app.expand_fold(&heights, tail + 2);
    assert_eq!(app.diff_scroll, 0, "bottom-half fold grows downward");
}

#[test]
fn a_comment_through_a_fold_anchors_to_gits_line_and_survives_a_poll() {
    let r = folded_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;

    app.diff_cursor = app.visible.iter().position(|row| row.text().contains("LINE 20")).unwrap();
    app.start_comment();
    for ch in "here".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    let c = app.store.iter().next().unwrap();
    assert_eq!((c.side, c.start), (Side::New, 21));

    app.diff_cursor = app.visible.iter().position(|row| row.hidden() > 0).unwrap();
    expand_fold(&mut app);
    app.reload().unwrap();
    assert_eq!(app.store.len(), 1, "the comment survives a fold expand and a poll");
    assert!(app.commented_lines().iter().any(|&i| app.visible[i].text().contains("LINE 20")));
}

#[test]
fn comment_anchors_to_gits_real_line_numbers() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;

    app.diff_cursor = app.diff.rows.iter().position(|r| r.text().contains("epsilon")).unwrap();
    app.start_comment();
    for ch in "appended".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    app.diff_cursor =
        app.diff.rows.iter().position(|r| r.marker() == '-' && r.text().contains("beta")).unwrap();
    app.start_comment();
    for ch in "removed".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    let appended = app.store.iter().find(|c| c.text == "appended").unwrap();
    assert_eq!((appended.side, appended.start, appended.end), (Side::New, 5, 5));
    let removed = app.store.iter().find(|c| c.text == "removed").unwrap();
    assert_eq!((removed.side, removed.start, removed.end), (Side::Old, 2, 2));
}

#[test]
fn comments_on_added_and_removed_lines_keep_the_diff_marker() {
    let r = edited_repo();
    let mut app = app_on(&r);
    assert_eq!(app.entries.len(), 1);

    comment_on(&mut app, '+', "this addition needs a test");
    comment_on(&mut app, '-', "why was this dropped?");
    assert_eq!(app.store.len(), 2);

    for c in app.store.iter() {
        let has_marker = c.lines.lines().any(|l| l.starts_with('+') || l.starts_with('-'));
        assert!(has_marker, "the hunk snippet carries the +/- change: {:?}", c.lines);
    }
}

#[test]
fn a_saved_comment_survives_a_refresh() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "keep me");
    assert_eq!(app.store.len(), 1);

    r.write("b.rs", "another change\n");
    app.reload().unwrap();

    assert_eq!(app.store.len(), 1, "refresh must not drop a saved comment");
    assert_eq!(app.store.iter().next().unwrap().text, "keep me");
    assert!(app.entries.iter().any(|f| f.path == "b.rs"), "file list still refreshed");
}

#[test]
fn a_refresh_while_composing_freezes_input_and_diff() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "half-written thought".chars() {
        app.input_push(ch);
    }
    let frozen_diff = app.diff.clone();

    r.write("a.rs", "alpha\nBETA\ngamma\ndelta\nepsilon\nzeta\n");
    r.write("c.rs", "c\n");
    app.reload().unwrap();

    assert!(app.composing(), "still composing");
    assert_eq!(app.input, "half-written thought", "input untouched");
    assert_eq!(app.diff, frozen_diff, "the open diff is frozen while composing");
    assert!(app.entries.iter().any(|f| f.path == "c.rs"), "file list still refreshes");
}

#[test]
fn export_keeps_comments_so_the_round_trip_can_track_them() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "one");
    comment_on(&mut app, '-', "two");
    assert_eq!(app.store.len(), 2);

    app.export(&FakeTarget::failing());
    assert_eq!(app.store.len(), 2, "a failed export leaves every comment in place");

    let target = FakeTarget::ok();
    app.export(&target);
    assert_eq!(app.store.len(), 2, "a successful send keeps the comments to track as addressed");

    let sent = target.last();
    assert!(sent.contains("one") && sent.contains("two"), "both comment texts present: {sent:?}");
    assert!(sent.starts_with("<review>"), "leads with the review container: {sent:?}");
    assert!(sent.contains("<ref>a.rs:"), "each comment carries a tagged location: {sent:?}");
    assert!(sent.contains("<note>one</note>"), "the note is tagged: {sent:?}");
    assert!(sent.contains("<code>"), "each block carries its code snippet: {sent:?}");
    assert!(
        sent.lines().any(|l| l.starts_with('+') || l.starts_with('-')),
        "a Changes snippet reaches the agent with its `+/-` markers: {sent:?}"
    );
    assert!(sent.contains("<base>"), "and the git base ref it was diffed against: {sent:?}");
}

#[test]
fn send_dispatches_the_whole_set_at_once() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "first");
    comment_on(&mut app, '-', "second");

    let target = FakeTarget::ok();
    app.export(&target);
    assert_eq!(app.store.len(), 2, "the set stays after sending");
    assert!(target.last().contains("first") && target.last().contains("second"), "both sent");
}

#[test]
fn a_comment_of_only_blank_lines_is_cancelled() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    app.input_push(' ');
    app.input_push('\n');
    app.input_push('\n');
    app.submit_comment();

    assert!(app.store.is_empty(), "a whitespace-only comment is not saved");
    assert!(!app.composing(), "compose mode exits");
}

#[test]
fn the_composer_reserve_keeps_the_anchored_line_visible() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut original = String::new();
    for i in 0..60 {
        writeln!(original, "line {i}").unwrap();
    }
    r.write("big.rs", &original);
    r.commit_all("init");
    r.write("big.rs", &original.replace("line", "LINE"));

    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = 30;
    app.start_comment();
    for ch in "one\ntwo\nthree".chars() {
        app.input_push(ch);
    }

    let viewport = 12;
    let effective = viewport - herdr_reviewr::ui::composer_height(&app, 80);
    clamp(&mut app, effective);
    assert!(
        (app.diff_scroll..app.diff_scroll + effective).contains(&app.diff_cursor),
        "anchored line {} stays in the reserved viewport [{}, {})",
        app.diff_cursor,
        app.diff_scroll,
        app.diff_scroll + effective
    );
}

#[test]
fn a_comment_can_be_written_across_multiple_lines() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "first line".chars() {
        app.input_push(ch);
    }
    app.input_push('\n');
    for ch in "second line".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    let c = app.store.iter().next().unwrap();
    assert_eq!(c.text, "first line\nsecond line", "the body keeps its line break");

    let target = FakeTarget::ok();
    app.export(&target);
    let sent = target.last();
    assert!(sent.contains("first line\nsecond line"), "export preserves the break: {sent:?}");
    assert!(!sent.contains("\n\n\n"), "no blank-line run that could split a block");
}

#[test]
fn the_cursor_stays_on_a_folder_across_a_poll_and_toggle() {
    let r = Repo::init();
    r.write("src/a.rs", "x\n");
    r.write("src/b.rs", "y\n");
    r.write("root.rs", "z\n");
    r.commit_all("init");
    r.write("src/a.rs", "x2\n");
    r.write("src/b.rs", "y2\n");
    r.write("root.rs", "z2\n");
    let mut app = app_on(&r);
    app.focus = Focus::Files;

    let dir_row = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    app.file_cursor = dir_row;
    let open = app.diff_path.clone();
    assert!(open.is_some(), "a file diff is open");

    app.reload().unwrap();
    assert_eq!(app.file_cursor, dir_row, "cursor stays on the folder across a poll");
    assert_eq!(app.diff_path, open, "the open diff is unchanged");

    app.collapse_dir();
    app.reload().unwrap();
    let dir_row = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    assert_eq!(app.file_cursor, dir_row, "cursor stays on the folder after collapse + poll");
    assert_eq!(app.diff_path, open, "the open diff is still unchanged");
}

#[test]
fn arrows_collapse_and_expand_a_folder() {
    let r = Repo::init();
    r.write("src/a.rs", "x\n");
    r.write("src/b.rs", "y\n");
    r.commit_all("init");
    r.write("src/a.rs", "x2\n");
    r.write("src/b.rs", "y2\n");
    let mut app = app_on(&r);
    app.focus = Focus::Files;

    let dir_row = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    app.file_cursor = dir_row;
    assert!(app.on_folder(), "the cursor is on the folder");
    let expanded = app.file_rows.len();

    app.collapse_dir();
    assert!(app.file_rows.len() < expanded, "collapsing hides the children");
    assert!(app.on_folder(), "the cursor stays on the folder row");

    app.expand_dir();
    assert_eq!(app.file_rows.len(), expanded, "expanding shows them again");
}

#[test]
fn the_footer_offers_enter_to_expand_or_collapse_a_folder_tree() {
    let r = Repo::init();
    r.write("src/sub/a.rs", "x\n");
    r.write("src/b.rs", "y\n");
    r.commit_all("init");
    r.write("src/sub/a.rs", "x2\n");
    r.write("src/b.rs", "y2\n");
    let mut app = app_on(&r);
    app.focus = Focus::Files;

    let dir_row = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    app.file_cursor = dir_row;
    let has = |app: &App, a: FooterAction| app.footer_actions().iter().any(|&(x, _)| x == a);

    assert!(has(&app, FooterAction::CollapseTree), "⏎ collapses the open subtree");
    assert!(!has(&app, FooterAction::ExpandTree));

    app.collapse_dir();
    let dir_row = app.file_rows.iter().position(|r| r.dir_path() == Some("src")).unwrap();
    app.file_cursor = dir_row;
    assert!(has(&app, FooterAction::ExpandTree), "⏎ expands a shut folder's tree");
    assert!(!has(&app, FooterAction::CollapseTree));
}

#[test]
fn the_pane_divider_resizes_and_clamps() {
    let r = edited_repo();
    let mut app = app_on(&r);
    let start = app.list_pct;
    app.resize_list(4);
    assert_eq!(app.list_pct, start + 4, "[ / ] step the divider");
    for _ in 0..50 {
        app.resize_list(4);
    }
    assert!(app.list_pct <= 60, "the file list never swallows the diff");
    for _ in 0..50 {
        app.resize_list(-4);
    }
    assert!(app.list_pct >= 15, "the diff never swallows the file list");

    app.drag_divider(100, 70);
    assert_eq!(app.list_pct, 30);
}

#[test]
fn ctrl_w_deletes_the_previous_word_in_a_comment() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "needs a closer look".chars() {
        app.input_push(ch);
    }
    app.input_delete_word();
    assert_eq!(app.input, "needs a closer ");
    app.input_delete_word();
    assert_eq!(app.input, "needs a ");
}

#[test]
fn the_comment_box_grows_as_a_long_line_wraps() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    let width = 30;
    let one_word = herdr_reviewr::ui::composer_height(&app, width);
    for ch in "the quick brown fox jumps over the lazy dog again and again".chars() {
        app.input_push(ch);
    }
    let wrapped = herdr_reviewr::ui::composer_height(&app, width);
    assert!(wrapped > one_word, "box grew from {one_word} to {wrapped} rows as text wrapped");
}

#[test]
fn a_comment_can_be_edited_then_deleted() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "original");
    let snippet_before = app.store.get(0).unwrap().lines.clone();

    app.open_list();
    app.start_edit();
    app.input.clear();
    for ch in "rewritten".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    assert_eq!(app.store.get(0).unwrap().text, "rewritten");
    assert_eq!(app.store.get(0).unwrap().lines, snippet_before, "edit changes only the text");

    app.open_list();
    app.delete_comment();
    assert!(app.store.is_empty());
}

#[test]
fn deleting_the_last_comment_closes_the_list_overlay() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "only one");
    app.open_list();
    assert_eq!(app.mode, Mode::List);
    app.delete_comment();
    assert!(app.store.is_empty());
    assert_eq!(app.mode, Mode::Normal, "an emptied overlay closes instead of stranding the user");
}

#[test]
fn finishing_an_edit_returns_to_its_origin() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "first");
    comment_on(&mut app, ' ', "second");

    app.open_list();
    app.start_edit();
    app.input_push('!');
    app.submit_comment();
    assert_eq!(app.mode, Mode::List, "a list-initiated edit returns to the list");

    app.close_list();
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_edit();
    app.submit_comment();
    assert_eq!(app.mode, Mode::Normal, "a diff-initiated edit returns to Normal");
}

#[test]
fn editing_from_the_list_navigates_to_the_comments_file() {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\n");
    r.write("b.rs", "one\ntwo\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\n");
    r.write("b.rs", "one\nTWO\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let bi = app.entries.iter().position(|f| f.path == "b.rs").unwrap();
    app.select_file(bi).unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "fix this".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    let ai = app.entries.iter().position(|f| f.path == "a.rs").unwrap();
    app.select_file(ai).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"));

    app.open_list();
    app.start_edit();
    assert!(app.composing());
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"), "edit switched to the comment's file");
    let dl = &app.diff.rows[app.diff_cursor];
    assert!(dl.new_no().is_some() || dl.old_no().is_some(), "cursor sits on a real diff line");
}

#[test]
fn a_comment_on_a_reverted_file_stays_live_outside_the_diff() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "note");

    r.write("a.rs", "alpha\nbeta\ngamma\ndelta\n");
    app.reload().unwrap();

    assert!(app.entries.iter().all(|f| f.path != "a.rs"), "file left the changeset");
    assert_eq!(app.store.len(), 1, "the comment still exists");
    let c = app.store.get(0).unwrap();
    assert!(!app.is_stale(c), "a worktree-anchored comment survives its file leaving the diff");
    assert!(!app.in_changeset("a.rs"), "and the file is outside the changeset");
}

#[test]
fn switching_scope_swaps_the_changeset() {
    let r = Repo::init();
    r.write("base.rs", "b\n");
    r.commit_all("base");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("committed.rs", "c\n");
    r.commit_all("feature work");
    r.write("dirty.rs", "d\n");

    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();
    assert!(app.entries.iter().any(|f| f.path == "dirty.rs"));
    assert!(app.entries.iter().all(|f| f.path != "committed.rs"), "uncommitted omits commits");

    app.set_scope(Scope::Branch).unwrap();
    assert!(app.entries.iter().any(|f| f.path == "committed.rs"), "branch adds committed work");
    assert!(app.entries.iter().any(|f| f.path == "dirty.rs"), "branch keeps the working tree");
}

#[test]
fn a_multi_line_range_comment_spans_lines_and_keeps_the_whole_snippet() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;

    let first = row_with(&app, '-');
    app.diff_cursor = first;
    app.toggle_select();
    app.move_cursor(1).unwrap();
    app.move_cursor(1).unwrap();
    let (lo, hi) = app.selection_range();
    assert!(hi > lo, "selection spans more than one line");

    app.start_comment();
    for ch in "this whole hunk is suspicious".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    assert_eq!(app.store.len(), 1);
    let c = app.store.iter().next().unwrap();
    assert!(c.end > c.start, "comment covers a line range: {}..{}", c.start, c.end);
    let snippet: Vec<&str> = c.lines.lines().collect();
    let selected_rows = app.visible[lo..=hi].iter().filter(|r| r.is_content()).count();
    assert_eq!(snippet.len(), selected_rows, "captures exactly the selected lines: {:?}", c.lines);
    assert!(snippet.len() >= 2, "the selection spanned multiple lines: {:?}", c.lines);
    assert!(
        snippet.iter().all(|l| l.starts_with(['+', '-', ' '])),
        "each selected line keeps its diff marker: {:?}",
        c.lines
    );
}

#[test]
fn scope_cannot_change_while_composing() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    app.input_push('x');

    app.set_scope(Scope::Branch).unwrap();
    assert_eq!(app.scope, Scope::Commit, "scope is frozen mid-comment");
    assert!(app.composing(), "still composing");
    assert_eq!(app.input, "x", "input untouched");
}

#[test]
fn tab_cannot_change_while_composing() {
    use herdr_reviewr::app::Tab;
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    app.input_push('x');

    app.set_tab(Tab::AllFiles).unwrap();
    assert_eq!(app.tab, Tab::Changes, "the tab is frozen mid-comment");
    assert!(app.composing(), "still composing");
    assert_eq!(app.input, "x", "input untouched");
}

#[test]
fn the_app_reads_branch_scoped_diffs_not_working_tree() {
    let r = Repo::init();
    r.write("shared.rs", "base\n");
    r.commit_all("base");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("on_branch.rs", "committed on the feature branch\n");
    r.commit_all("feature work");

    let mut app = App::new(r.path_buf(), Scope::Branch, Some("main".to_string()));
    app.reload().unwrap();

    let idx =
        app.entries.iter().position(|f| f.path == "on_branch.rs").expect("branch file listed");
    app.select_file(idx).unwrap();

    let on_branch = app
        .diff
        .rows
        .iter()
        .any(|r| r.marker() == '+' && r.text().contains("committed on the feature branch"));
    assert!(on_branch, "branch diff carries the committed line");
}

#[test]
fn the_diff_scroll_is_sticky_and_only_follows_the_cursor_off_screen() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut original = String::new();
    for i in 0..60 {
        writeln!(original, "line {i}").unwrap();
    }
    r.write("big.rs", &original);
    r.commit_all("init");
    let edited = original.replace("line", "LINE");
    r.write("big.rs", &edited);

    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    let height = 10;

    clamp(&mut app, height);
    assert_eq!(app.diff_scroll, 0);

    app.diff_cursor = 5;
    clamp(&mut app, height);
    assert_eq!(app.diff_scroll, 0, "no scroll while the cursor is visible");

    app.diff_cursor = 12;
    clamp(&mut app, height);
    assert_eq!(app.diff_scroll, 12 + 1 - height);

    app.diff_cursor = 1;
    clamp(&mut app, height);
    assert_eq!(app.diff_scroll, 1);

    app.diff_cursor = 0;
    let tall = app.visible.len() + 50;
    clamp(&mut app, tall);
    assert_eq!(app.diff_scroll, 0, "no scroll when the diff fits the viewport");
}

#[test]
fn a_refresh_keeps_the_diff_scroll_position() {
    use std::fmt::Write as _;
    let r = Repo::init();
    let mut original = String::new();
    for i in 0..60 {
        writeln!(original, "line {i}").unwrap();
    }
    r.write("big.rs", &original);
    r.commit_all("init");
    r.write("big.rs", &original.replace("line", "LINE"));

    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = 25;
    clamp(&mut app, 10);
    let (cursor, scroll) = (app.diff_cursor, app.diff_scroll);
    assert!(scroll > 0, "we scrolled down into the diff");

    app.reload().unwrap();
    assert_eq!(app.diff_cursor, cursor, "refresh keeps the cursor line");
    assert_eq!(app.diff_scroll, scroll, "refresh keeps the scroll position");
}

#[test]
fn the_diff_title_stays_on_the_composed_file_through_a_refresh() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"));

    app.start_comment();
    app.input_push('x');

    r.write("a.rs", "alpha\nbeta\ngamma\ndelta\n");
    r.write("z.rs", "new\n");
    app.reload().unwrap();

    assert!(app.composing());
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"), "diff title frozen on composed file");
    assert_ne!(app.current_entry().map(|f| f.path.as_str()), Some("a.rs"));
}

#[test]
fn a_comment_submitted_after_its_file_left_the_changeset_anchors_to_that_file() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "note for a.rs".chars() {
        app.input_push(ch);
    }

    r.write("a.rs", "alpha\nbeta\ngamma\ndelta\n");
    r.write("z.rs", "new\n");
    app.reload().unwrap();
    assert_ne!(app.current_entry().map(|f| f.path.as_str()), Some("a.rs"));

    app.submit_comment();
    let c = app.store.iter().next().unwrap();
    assert_eq!(c.file, "a.rs", "comment anchors to its diff's file, not the drifted cursor");
}

#[test]
fn deleting_the_last_listed_comment_clamps_the_list_cursor() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "one");
    comment_on(&mut app, '-', "two");

    app.open_list();
    app.list_move(1);
    assert_eq!(app.list_cursor, 1);

    app.delete_comment();
    assert_eq!(app.store.len(), 1);
    assert_eq!(app.list_cursor, 0, "list cursor clamps back into range");
}

#[test]
fn a_non_repo_path_yields_an_empty_state_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().to_path_buf(), Scope::Commit, None);
    assert!(app.reload().is_ok(), "a non-repo reload is graceful, not an error");
    assert!(app.entries.is_empty());
    assert!(app.diff.rows.is_empty());
}

#[test]
fn jump_moves_the_cursor_onto_a_commented_line() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "note");

    app.focus = Focus::Diff;
    app.diff_cursor = 0;
    app.jump_comment(1);
    assert!(app.commented_lines().contains(&app.diff_cursor), "cursor landed on a comment");
}

#[test]
fn last_turn_is_empty_until_a_turn_is_observed() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.reload().unwrap();
    assert!(app.awaiting_turn(), "no baseline captured yet");
    assert!(app.entries.is_empty(), "the scope is empty before a turn");
}

#[test]
fn last_turn_shows_a_change_producing_turn() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.apply_agent_status(Some("idle"));
    app.apply_agent_status(Some("working"));
    r.write("a.rs", "one\ntwo\n");
    app.apply_agent_status(Some("working"));
    app.reload().unwrap();
    assert!(!app.awaiting_turn(), "the baseline is now set");
    assert!(app.entries.iter().any(|f| f.path == "a.rs"), "the turn's edit shows");
}

#[test]
fn a_question_only_turn_keeps_the_previous_turns_diff() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.apply_agent_status(Some("idle"));
    app.apply_agent_status(Some("working"));
    r.write("a.rs", "one\ntwo\n");
    app.apply_agent_status(Some("working"));
    app.apply_agent_status(Some("idle"));
    app.apply_agent_status(Some("working"));
    app.apply_agent_status(Some("idle"));
    app.reload().unwrap();
    assert!(
        app.entries.iter().any(|f| f.path == "a.rs"),
        "A's diff persists across a question-only turn"
    );
}

#[test]
fn a_permission_pause_stays_one_turn() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.apply_agent_status(Some("idle"));
    app.apply_agent_status(Some("working"));
    r.write("a.rs", "one\nbefore\n");
    app.apply_agent_status(Some("blocked"));
    app.apply_agent_status(Some("working"));
    r.write("a.rs", "one\nbefore\nafter\n");
    app.apply_agent_status(Some("working"));
    app.reload().unwrap();
    let a = app.entries.iter().find(|f| f.path == "a.rs").expect("a.rs changed");
    let annotation = a.annotation.as_ref().expect("a changed file is annotated");
    assert_eq!(annotation.additions, 2, "both the pre- and post-prompt edits belong to one turn");
}

#[test]
fn the_baseline_survives_a_restart() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    {
        let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
        app.apply_agent_status(Some("idle"));
        app.apply_agent_status(Some("working"));
        r.write("a.rs", "one\ntwo\n");
        app.apply_agent_status(Some("working"));
    }
    let mut restarted = App::new(r.path_buf(), Scope::LastTurn, None);
    restarted.reload().unwrap();
    assert!(!restarted.awaiting_turn(), "baseline resumed from the private ref");
    assert!(restarted.entries.iter().any(|f| f.path == "a.rs"), "the turn's edit still shows");
}

#[test]
fn no_agent_status_pauses_tracking() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::LastTurn, None);
    app.apply_agent_status(None);
    r.write("a.rs", "one\ntwo\n");
    app.apply_agent_status(None);
    app.reload().unwrap();
    assert!(app.awaiting_turn(), "without a status signal the baseline never forms");
}

fn file_row_of(app: &App, path: &str) -> Option<usize> {
    app.file_rows
        .iter()
        .position(|row| row.file_index().is_some_and(|i| app.entries[i].path == path))
}

#[test]
fn all_files_tab_browses_the_whole_worktree_and_renders_content() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::diff::View;
    let r = Repo::init();
    r.write("src/app.rs", "fn main() {}\n");
    r.write("src/ui.rs", "fn render() {}\n");
    r.write("README.md", "# hi\n");
    r.commit_all("init");
    r.write("README.md", "# changed\n");
    let mut app = app_on(&r);

    assert_eq!(app.tab, Tab::Changes);
    assert_eq!(app.entries.len(), 1);
    assert_eq!(app.diff_path.as_deref(), Some("README.md"));

    app.set_tab(Tab::AllFiles).unwrap();
    assert_eq!(app.tab, Tab::AllFiles);
    assert!(app.entries.iter().any(|e| e.path == "src/ui.rs"), "an unchanged file is listed");
    assert_eq!(app.diff_path.as_deref(), Some("README.md"), "All files opens its first file");
    assert!(app.file_rows.iter().any(|row| row.dir_path() == Some("src")), "src/ is a dir row");
    assert!(file_row_of(&app, "src/ui.rs").is_none(), "a collapsed dir hides its children");

    let src_row = app.file_rows.iter().position(|row| row.dir_path() == Some("src")).unwrap();
    app.select_file(src_row).unwrap();
    let ui_row = file_row_of(&app, "src/ui.rs").expect("src/ui.rs visible once src/ is expanded");
    app.select_file(ui_row).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("src/ui.rs"));
    assert_eq!(app.diff.view, View::File);
    assert!(app.diff.rows.iter().any(|row| row.text().contains("fn render")));
}

#[test]
fn switching_tabs_restores_each_tab_selection() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::diff::View;
    let r = Repo::init();
    r.write("src/app.rs", "fn main() {}\n");
    r.write("README.md", "# hi\n");
    r.commit_all("init");
    r.write("src/app.rs", "fn main() { run() }\n");
    let mut app = app_on(&r);
    let changes_open = app.diff_path.clone();
    assert_eq!(changes_open.as_deref(), Some("src/app.rs"));

    app.set_tab(Tab::AllFiles).unwrap();
    let readme_row = file_row_of(&app, "README.md").expect("README.md at the top level");
    app.select_file(readme_row).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("README.md"));
    assert_eq!(app.diff.view, View::File);

    app.set_tab(Tab::Changes).unwrap();
    assert_eq!(app.tab, Tab::Changes);
    assert_eq!(app.entries.len(), 1, "Changes still lists only the changed file");
    assert_eq!(app.diff_path, changes_open);
    assert_eq!(app.diff.view, View::Diff);

    app.set_tab(Tab::AllFiles).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("README.md"));
    assert_eq!(app.diff.view, View::File);
}

#[test]
fn a_new_side_comment_survives_leaving_the_changeset() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::model::Comment;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.write("b.rs", "two\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = app_on(&r);
    assert_eq!(app.changed_count(), 1, "Changes counts the one changed file");

    let comment = Comment {
        file: "b.rs".into(),
        side: Side::New,
        start: 1,
        end: 1,
        lines: "two".into(),
        text: "?".into(),
        diff_anchored: true,
        scope: Scope::Commit,
        base: None,
        sent: false,
    };
    app.store.add(comment.clone());

    app.set_tab(Tab::AllFiles).unwrap();
    assert!(app.entries.len() >= 2, "All files lists the whole worktree");
    assert_eq!(app.changed_count(), 1, "the count is the changeset, not the worktree total");
    assert!(
        !app.is_stale(&comment),
        "a New-side comment is worktree-anchored, so it is not stale outside the changeset",
    );
    assert!(!app.in_changeset("b.rs"), "though b.rs is not part of the current diff");
}

#[allow(clippy::option_option)]
fn annotation_of(app: &App, path: &str) -> Option<Option<herdr_reviewr::file_list::Annotation>> {
    use herdr_reviewr::file_list::RowKind;
    app.file_rows.iter().find_map(|row| match &row.kind {
        RowKind::File { index, annotation } if app.entries[*index].path == path => {
            Some(annotation.clone())
        }
        _ => None,
    })
}

#[test]
fn all_files_annotates_changed_files_only() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::model::ChangeKind;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.write("b.rs", "two\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    assert!(
        matches!(annotation_of(&app, "a.rs"), Some(Some(a)) if a.change == ChangeKind::Modified),
        "a changed file carries its marker"
    );
    assert_eq!(
        annotation_of(&app, "b.rs"),
        Some(None),
        "an unchanged file is listed without a marker"
    );
}

#[test]
fn switching_scope_on_all_files_remarks_in_place() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.write("b.rs", "two\n");
    r.commit_all("init");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.rs", "TWO\n");
    r.commit_all("committed change to b");
    r.write("a.rs", "ONE\n");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    app.focus = Focus::Files;
    app.move_cursor(1).unwrap();
    let cursor = app.file_cursor;
    assert_eq!(app.changed_count(), 1, "uncommitted marks only the dirty file");
    assert!(
        matches!(annotation_of(&app, "a.rs"), Some(Some(_))),
        "a.rs is marked under uncommitted"
    );
    assert_eq!(annotation_of(&app, "b.rs"), Some(None), "b.rs is unmarked under uncommitted");

    app.set_scope(Scope::Branch).unwrap();
    assert_eq!(app.file_cursor, cursor, "the cursor holds across a scope re-mark");
    assert_eq!(app.changed_count(), 2, "branch marks both the committed and the dirty file");
    assert!(matches!(annotation_of(&app, "a.rs"), Some(Some(_))), "a.rs stays marked");
    assert!(matches!(annotation_of(&app, "b.rs"), Some(Some(_))), "b.rs is now marked");
}

#[test]
fn all_files_lazily_loads_an_expanded_ignored_directory() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("src/app.rs", "fn main() {}\n");
    r.commit_all("init");
    r.write(".gitignore", "target/\n");
    r.write("target/build.o", "x\n");
    r.write("target/sub/y.o", "y\n");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    app.focus = Focus::Files;

    assert!(app.entries.iter().any(|e| e.path == "target" && e.is_dir && e.ignored));
    assert!(!app.entries.iter().any(|e| e.path.starts_with("target/")), "children not loaded yet");

    let row = |a: &App| a.file_rows.iter().position(|r| r.dir_path() == Some("target")).unwrap();
    app.file_cursor = row(&app);
    app.expand_dir();
    assert!(
        app.entries.iter().any(|e| e.path == "target/build.o" && e.ignored),
        "file child loads"
    );
    assert!(
        app.entries.iter().any(|e| e.path == "target/sub" && e.is_dir),
        "subdir placeholder loads"
    );
    assert!(!app.entries.iter().any(|e| e.path == "target/sub/y.o"), "deeper level stays lazy");

    app.file_cursor = row(&app);
    app.collapse_dir();
    assert!(
        !app.entries.iter().any(|e| e.path.starts_with("target/")),
        "collapsing unloads children"
    );
}

#[test]
fn content_comment_is_stale_only_when_its_file_is_deleted() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    let row = file_row_of(&app, "a.rs").expect("a.rs at the top level");
    app.select_file(row).unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = 0;
    app.start_comment();
    for ch in "note".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    let c = app.store.get(0).expect("a comment was made").clone();
    assert!(!c.diff_anchored, "a File-view comment is content-anchored");

    app.reload().unwrap();
    assert!(!app.is_stale(&c), "a content comment on an existing, unchanged file is not stale");
    r.remove("a.rs");
    app.reload().unwrap();
    assert!(app.is_stale(&c), "it becomes stale only once its file is deleted");
}

#[test]
fn anchor_stamps_scope_base_and_keeps_the_diff_marker() {
    let r = Repo::init();
    r.write("a.rs", "fn main() {}\n");
    r.commit_all("init");
    r.write("a.rs", "fn main() { work(); }\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    goto_file(&mut app, "a.rs");
    comment_on(&mut app, '+', "note");

    let c = app.store.get(0).expect("a comment was made");
    assert_eq!(c.side, Side::New);
    assert_eq!(c.lines, "+fn main() { work(); }", "just the selected added line, marked");
    assert_eq!(c.scope, Scope::Commit, "stamped with the authoring scope");
    assert_eq!(c.base.as_deref(), app.selected_commit.as_deref(), "and the diff base");
    assert!(!c.sent, "a fresh comment starts un-sent");
}

#[test]
fn a_changes_comment_matches_only_its_authoring_base() {
    use herdr_reviewr::model::Comment;
    let r = Repo::init();
    r.write("a.rs", "l1\nl2\n");
    r.commit_all("c1");
    r.write("a.rs", "l1\nl2\nl3\n");
    r.commit_all("c2");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let head = app.selected_commit.clone().expect("commit scope has a base");
    assert!(app.commit_choices.len() >= 2, "two commits to switch between");
    let older = app.commit_choices[1].sha.clone();

    let c = Comment {
        file: "a.rs".into(),
        side: Side::New,
        start: 1,
        end: 1,
        lines: "l1".into(),
        text: "?".into(),
        diff_anchored: true,
        scope: Scope::Commit,
        base: Some(head.clone()),
        sent: false,
    };
    assert!(app.comment_matches_current(&c), "matches under its own commit base");

    app.set_commit(older).unwrap();
    assert!(!app.comment_matches_current(&c), "hidden once the base changes");

    app.set_commit(head).unwrap();
    assert!(app.comment_matches_current(&c), "returns when its base is restored");
}

#[test]
fn all_files_comments_ignore_the_base() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::model::Comment;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::AllFiles).unwrap();

    let c = Comment {
        file: "a.rs".into(),
        side: Side::New,
        start: 1,
        end: 1,
        lines: "ONE".into(),
        text: "note".into(),
        diff_anchored: false,
        scope: Scope::Commit,
        base: None,
        sent: false,
    };
    assert!(app.comment_matches_current(&c), "an All-files comment shows in the File view");
}

fn lit(
    file: &str,
    base: Option<&str>,
    diff_anchored: bool,
    text: &str,
) -> herdr_reviewr::model::Comment {
    herdr_reviewr::model::Comment {
        file: file.into(),
        side: Side::New,
        start: 1,
        end: 1,
        lines: "x".into(),
        text: text.into(),
        diff_anchored,
        scope: Scope::Commit,
        base: base.map(Into::into),
        sent: false,
    }
}

#[test]
fn send_dispatches_only_unsent_and_marks_them_sent() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "first");

    let t = FakeTarget::ok();
    app.export(&t);
    assert_eq!(t.count(), 1, "the fresh comment is dispatched");
    assert!(app.store.get(0).unwrap().sent, "and marked sent");

    let t2 = FakeTarget::ok();
    app.export(&t2);
    assert_eq!(t2.count(), 0, "nothing new to send");
    assert!(app.status.contains("nothing new"));
}

#[test]
fn a_second_send_only_sends_new_comments() {
    let r = edited_repo();
    let mut app = app_on(&r);
    let t = FakeTarget::ok();

    comment_on(&mut app, '+', "first");
    app.export(&t);
    comment_on(&mut app, '-', "second");
    app.export(&t);

    assert_eq!(t.count(), 2);
    let p2 = t.last();
    assert_eq!(p2.matches("<comment ").count(), 1, "the second send carries only the new comment");
    assert!(p2.contains("second") && !p2.contains("first"), "and it is the new one: {p2}");
}

#[test]
fn copy_does_not_mark_sent() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "x");

    app.export(&FakeTarget::copy());
    assert!(!app.store.get(0).unwrap().sent, "copy leaves the comment un-sent");

    let agent = FakeTarget::ok();
    app.export(&agent);
    assert_eq!(agent.count(), 1, "so a later agent send still carries it");
    assert!(app.store.get(0).unwrap().sent);
}

#[test]
fn a_sent_comment_cannot_be_edited_only_resolved() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "note");
    app.export(&FakeTarget::ok());

    app.open_list();
    app.start_edit();
    assert_eq!(app.mode, Mode::List, "edit is refused on a sent comment");
    assert!(app.status.contains("resolve only"), "status explains why: {}", app.status);

    app.resolve_selected();
    assert_eq!(app.store.len(), 0, "but it can still be resolved");
}

#[test]
fn list_groups_by_view_and_base() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.store.add(lit("a.rs", Some("aaaaaaa000"), true, "at A"));
    app.store.add(lit("a.rs", Some("bbbbbbb111"), true, "at B"));
    app.store.add(lit("a.rs", None, false, "in all files"));

    let groups = app.list_groups();
    assert_eq!(groups.len(), 3, "two commit groups and one All-files group");
    assert_eq!(groups.iter().filter(|(l, _)| l.starts_with("commit ")).count(), 2);
    assert!(groups.iter().any(|(l, _)| l == "All files"));
    assert!(groups.last().unwrap().0 == "All files");
    assert_eq!(app.list_order().len(), 3);
}

#[test]
fn space_toggles_selection_and_a_selects_all() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.store.add(lit("a.rs", Some("base"), true, "one"));
    app.store.add(lit("a.rs", Some("base"), true, "two"));
    app.open_list();
    assert!(app.list_selected.is_empty());

    app.toggle_list_select();
    assert_eq!(app.list_selected.len(), 1, "space checks the cursor row");
    app.select_all_or_none();
    assert_eq!(app.list_selected.len(), 2, "a checks all");
    app.select_all_or_none();
    assert!(app.list_selected.is_empty(), "a again clears when all were checked");
}

#[test]
fn resolve_selected_removes_the_checked_comments() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.store.add(lit("a.rs", Some("base"), true, "one"));
    app.store.add(lit("a.rs", Some("base"), true, "two"));
    app.store.add(lit("a.rs", Some("base"), true, "three"));
    app.open_list();

    app.toggle_list_select();
    app.list_move(1);
    app.toggle_list_select();
    app.resolve_selected();

    assert_eq!(app.store.len(), 1, "the two checked comments are removed");
    assert_eq!(app.store.get(0).unwrap().text, "three", "the unchecked one remains");
    assert!(app.list_selected.is_empty(), "the selection is cleared");
}

#[test]
fn open_comment_restores_scope_and_base_then_jumps() {
    let r = Repo::init();
    r.write("a.rs", "l1\n");
    r.commit_all("c1");
    r.write("a.rs", "l1\nl2\n");
    r.commit_all("c2");
    r.write("a.rs", "l1\nl2\nl3\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let head = app.selected_commit.clone().unwrap();
    let older = app.commit_choices[1].sha.clone();

    goto_file(&mut app, "a.rs");
    comment_on(&mut app, '+', "note");
    assert_eq!(app.store.get(0).unwrap().base.as_deref(), Some(head.as_str()));

    app.set_commit(older).unwrap();
    assert!(!app.comment_matches_current(app.store.get(0).unwrap()), "hidden off its base");

    app.open_list();
    app.open_comment(0);
    assert_eq!(app.selected_commit.as_deref(), Some(head.as_str()), "restored its commit base");
    assert_eq!(app.scope, Scope::Commit);
    assert_eq!(app.mode, Mode::Normal, "the list closed on the jump");
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"), "and its file is open");
}

#[test]
fn the_send_count_ignores_already_sent_comments() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "one");
    assert_eq!(app.unsent_count(), 1);

    app.export(&FakeTarget::ok());
    assert_eq!(app.unsent_count(), 0, "sent comments drop out of the send count");

    comment_on(&mut app, '-', "two");
    assert_eq!(app.unsent_count(), 1, "only the new comment counts toward the next send");
}

#[test]
fn a_changes_snippet_captures_only_the_selected_lines() {
    let r = Repo::init();
    r.write("a.rs", "keep1\nold\nkeep2\n");
    r.commit_all("init");
    r.write("a.rs", "keep1\nnew\nkeep2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    goto_file(&mut app, "a.rs");
    comment_on(&mut app, '+', "why the change?");

    let c = app.store.get(0).unwrap();
    assert_eq!(
        c.lines, "+new",
        "just the selected line, with its marker — not the surrounding hunk"
    );
}

#[test]
fn list_cursor_row_counts_group_headers() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.store.add(lit("a.rs", Some("aaaa"), true, "group one"));
    app.store.add(lit("a.rs", Some("bbbb"), true, "group two"));
    app.open_list();

    assert_eq!(app.list_cursor_row(), 1, "the first comment sits below its header");
    app.list_move(1);
    assert_eq!(app.list_cursor_row(), 3, "the second, in a new group, below a second header");
}

#[test]
fn the_tabs_keep_independent_selections() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    assert_eq!(app.changed_count(), 0);
    assert!(app.diff_path.is_none(), "Changes opens nothing with an empty changeset");

    app.set_tab(Tab::AllFiles).unwrap();
    let row = file_row_of(&app, "a.rs").unwrap();
    app.select_file(row).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"), "viewing a.rs in All files");

    app.set_tab(Tab::Changes).unwrap();
    assert!(app.diff_path.is_none(), "the All files selection does not carry into Changes");
}

#[test]
fn a_file_view_comment_exports_as_path_line_with_a_context_snippet() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\ngamma\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    let row = file_row_of(&app, "a.rs").expect("a.rs listed");
    app.select_file(row).unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = 1;
    app.start_comment();
    for ch in "why".chars() {
        app.input_push(ch);
    }
    app.submit_comment();

    let target = FakeTarget::ok();
    app.export(&target);
    let out = target.last();
    assert!(out.contains("a.rs:2"), "ref is path:line:\n{out}");
    assert!(!out.contains("(removed)"), "a content comment never carries (removed):\n{out}");
    assert!(out.contains("<code>\nbeta\n</code>"), "the snippet is the marker-free line:\n{out}");
}

#[test]
fn an_oversize_file_in_all_files_degrades_to_a_notice() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::diff::{FileState, View};
    let r = Repo::init();
    r.write("small.rs", "fn main() {}\n");
    r.write("big.bin", &"x\n".repeat(1_100_000));
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    let row = file_row_of(&app, "big.bin").expect("big.bin listed");
    app.select_file(row).unwrap();
    assert_eq!(app.diff.state, FileState::TooLarge, "an over-budget file is not read whole");
    assert_eq!(app.diff.view, View::File);
    assert!(app.visible.is_empty());
}

#[test]
fn switching_to_an_empty_file_view_focuses_the_tree() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "alpha\n");
    r.commit_all("init");
    r.remove("a.rs");
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.set_tab(Tab::AllFiles).unwrap();
    assert!(app.visible.is_empty(), "the deleted file's content view is empty");
    assert_eq!(app.focus, Focus::Files, "an empty left pane focuses the tree, not traps the keys");
}

#[test]
fn a_diff_comment_does_not_render_in_the_file_view() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::diff::View;
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\ngamma\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\ngamma\n");
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    app.input_push('x');
    app.submit_comment();
    assert!(app.store.get(0).unwrap().diff_anchored, "made in the Changes diff");
    assert!(!app.commented_lines().is_empty(), "renders in its own diff view");

    app.set_tab(Tab::AllFiles).unwrap();
    let row = file_row_of(&app, "a.rs").expect("a.rs listed");
    app.select_file(row).unwrap();
    assert_eq!(app.diff.view, View::File);
    assert!(
        app.commented_lines().is_empty(),
        "a diff-anchored comment does not render in the File view"
    );
}

#[test]
fn editing_a_comment_on_all_files_opens_the_file_view() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::diff::View;
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\n");
    r.write("b.rs", "one\ntwo\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::AllFiles).unwrap();
    let arow = file_row_of(&app, "a.rs").expect("a.rs listed");
    app.select_file(arow).unwrap();
    app.focus = Focus::Diff;
    app.diff_cursor = 0;
    app.start_comment();
    app.input_push('x');
    app.submit_comment();
    let brow = file_row_of(&app, "b.rs").expect("b.rs listed");
    app.select_file(brow).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"));

    app.open_list();
    app.start_edit();
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"));
    assert_eq!(app.diff.view, View::File, "editing on All files opens the File view, not a diff");
    assert!(app.composing());
}

#[test]
fn changing_scope_on_all_files_snaps_the_changes_diff_to_the_top() {
    use std::fmt::Write as _;

    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    let mut body = String::new();
    for i in 0..40 {
        writeln!(body, "line {i}").unwrap();
    }
    r.write("a.rs", &body);
    r.commit_all("base");
    r.git(&["checkout", "-b", "feature"]);
    r.write("a.rs", &body.replace("line 5", "LINE 5"));
    r.commit_all("feature edit");
    r.write("a.rs", &body.replace("line 5", "LINE 5").replace("line 30", "LINE 30"));

    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = 2;
    app.diff_scroll = 1;

    app.set_tab(Tab::AllFiles).unwrap();
    app.set_scope(Scope::Branch).unwrap();
    app.set_tab(Tab::Changes).unwrap();

    assert!(app.entries.iter().any(|e| e.path == "a.rs"), "a.rs is in the branch changeset");
    assert_eq!(app.diff_scroll, 0, "an explicit scope switch snaps the Changes diff to the top");
    assert_eq!(app.diff_cursor, 0);
}

#[test]
fn the_pr_tab_detour_preserves_each_file_tab_state() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.write("b.rs", "two\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = app_on(&r);

    assert_eq!(app.tab, Tab::Changes);
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"));

    app.set_tab(Tab::AllFiles).unwrap();
    app.select_file(file_row(&app, "b.rs")).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"));

    app.set_tab(Tab::Pr).unwrap();
    assert_eq!(app.tab, Tab::Pr);

    app.set_tab(Tab::AllFiles).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"), "All files restored after the PR detour");

    app.set_tab(Tab::Changes).unwrap();
    assert_eq!(app.tab, Tab::Changes);
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"), "Changes restored without bleeding b.rs");
}

#[test]
fn pr_navigator_walks_comments_only_and_clamps() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::forge::{
        Check, CheckStatus, Comment, CommentKind, Merge, PrSnapshot, PrState, PrView, Sync,
    };

    let finding = |author: &str| Comment {
        kind: CommentKind::Finding,
        author: author.into(),
        author_is_bot: true,
        anchor: "a.rs:1".into(),
        body: "b".into(),
        snippet: None,
        created_at: "2026-06-27T10:00:00Z".into(),
        is_resolved: false,
        is_outdated: false,
        reply_count: 0,
    };
    let snap = PrSnapshot {
        number: 1,
        title: "t".into(),
        url: "u".into(),
        state: PrState::Open,
        is_draft: false,
        base_ref: "main".into(),
        merge: Merge::Clean,
        sync: Sync::InSync,
        checks: vec![
            Check { name: "build".into(), status: CheckStatus::Success },
            Check { name: "test".into(), status: CheckStatus::Failure },
        ],
        comments: vec![finding("first"), finding("second")],
        truncated: false,
    };

    let r = Repo::init();
    r.write("x.rs", "y\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::Pr).unwrap();
    app.pr = PrView::Pr(Box::new(snap));

    assert_eq!(app.pr_row_count(), 2, "two comments; the two checks are not cursor stops");
    assert_eq!(app.pr_selected_comment().map(|c| c.author.as_str()), Some("first"));
    app.pr_move(1);
    assert_eq!(app.pr_selected_comment().map(|c| c.author.as_str()), Some("second"));
    app.pr_move(5);
    assert_eq!(
        app.pr_selected_comment().map(|c| c.author.as_str()),
        Some("second"),
        "clamps at the last comment"
    );
    app.pr_move(-10);
    assert_eq!(
        app.pr_selected_comment().map(|c| c.author.as_str()),
        Some("first"),
        "clamps at the first comment"
    );
}

#[test]
fn apply_pr_follows_the_selected_comment_across_a_refresh() {
    use herdr_reviewr::app::Tab;
    use herdr_reviewr::forge::{Comment, CommentKind, Merge, PrSnapshot, PrState, PrView, Sync};

    let comment = |author: &str, created: &str| Comment {
        kind: CommentKind::Comment,
        author: author.into(),
        author_is_bot: false,
        anchor: "comment".into(),
        body: "b".into(),
        snippet: None,
        created_at: created.into(),
        is_resolved: false,
        is_outdated: false,
        reply_count: 0,
    };
    let snap = |comments: Vec<Comment>| {
        PrView::Pr(Box::new(PrSnapshot {
            number: 1,
            title: "t".into(),
            url: "u".into(),
            state: PrState::Open,
            is_draft: false,
            base_ref: "main".into(),
            merge: Merge::Clean,
            sync: Sync::InSync,
            checks: Vec::new(),
            comments,
            truncated: false,
        }))
    };

    let r = Repo::init();
    r.write("x.rs", "y\n");
    r.commit_all("init");
    let mut app = app_on(&r);
    app.set_tab(Tab::Pr).unwrap();

    app.apply_pr(snap(vec![
        comment("ann", "2026-06-27T10:00:00Z"),
        comment("bob", "2026-06-27T09:00:00Z"),
    ]));
    assert_eq!(app.pr_selected_comment().map(|c| c.author.as_str()), Some("ann"));
    app.pr_move(1);
    assert_eq!(app.pr_selected_comment().map(|c| c.author.as_str()), Some("bob"));

    app.apply_pr(snap(vec![
        comment("cara", "2026-06-27T11:00:00Z"),
        comment("ann", "2026-06-27T10:00:00Z"),
        comment("bob", "2026-06-27T09:00:00Z"),
    ]));
    assert_eq!(
        app.pr_selected_comment().map(|c| c.author.as_str()),
        Some("bob"),
        "the cursor follows the same comment by identity, not its old index"
    );

    app.apply_pr(snap(vec![
        comment("cara", "2026-06-27T11:00:00Z"),
        comment("ann", "2026-06-27T10:00:00Z"),
    ]));
    assert_eq!(
        app.pr_selected_comment().map(|c| c.author.as_str()),
        Some("ann"),
        "a vanished selection clamps to the last row"
    );
}

#[test]
fn theme_selection_swaps_the_palette_and_falls_back() {
    use herdr_reviewr::theme;
    let repo = Repo::init();
    let mut app = App::new(repo.path_buf(), Scope::Commit, None);

    assert_eq!(*app.palette(), theme::resolve(Some("catppuccin")).palette);

    app.set_cli_theme(Some("catppuccin-latte".to_string()));
    assert_eq!(*app.palette(), theme::resolve(Some("catppuccin-latte")).palette);

    app.set_cli_theme(Some("nope".to_string()));
    assert_eq!(*app.palette(), theme::resolve(Some("catppuccin")).palette);
}

#[test]
fn e_requests_the_editor_for_the_file_under_the_cursor() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    let expected_line = app.visible[app.diff_cursor].new_no();
    assert!(expected_line.is_some(), "an inserted line carries a new-side number");

    app.request_editor();
    let req = app.take_pending_editor().expect("`e` queued an editor request");
    assert_eq!(req.line, expected_line, "opens at the line under the cursor");
    assert!(req.path.is_absolute(), "the editor is handed an absolute path");
    assert!(req.path.ends_with("a.rs"), "the path is the file under review, got {:?}", req.path);
    assert!(app.take_pending_editor().is_none(), "the request drains exactly once");
}

#[test]
fn the_editor_request_needs_the_diff_pane_and_no_open_composer() {
    let r = edited_repo();

    let mut app = app_on(&r);
    app.focus = Focus::Files;
    app.request_editor();
    assert!(app.take_pending_editor().is_none(), "no editor from the file list");

    let mut app = composing_app();
    app.request_editor();
    assert!(app.take_pending_editor().is_none(), "no editor while composing a comment");
}

#[test]
fn the_editor_hint_shows_in_the_diff_and_yields_to_edit_on_a_comment() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    assert!(
        app.footer_actions().iter().any(|&(a, _)| a == FooterAction::OpenEditor),
        "the diff viewer offers `e editor`"
    );

    comment_on(&mut app, '+', "note");
    let acts = app.footer_actions();
    assert!(
        acts.iter().any(|&(a, _)| a == FooterAction::EditComment),
        "commented line offers edit"
    );
    assert!(
        !acts.iter().any(|&(a, _)| a == FooterAction::OpenEditor),
        "and does not also show the editor hint on the same key"
    );
}

#[test]
fn the_base_picker_is_chip_click_only_no_footer_key() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.rs", "two\n");
    r.commit_all("add b");

    let mut app = App::new(r.path_buf(), Scope::Branch, None);
    app.reload().unwrap();

    assert!(!app.file_rows.is_empty(), "branch scope sees the feature commit's file");
    assert!(
        !app.footer_actions().iter().any(|&(a, _)| a == FooterAction::Base),
        "no base key hint — the picker is chip-click only"
    );
}

fn commit_repo() -> Repo {
    let r = Repo::init();
    r.write("base.rs", "0\n");
    r.commit_all("base");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("a.rs", "a\n");
    r.commit_all("add a");
    r.write("b.rs", "b\n");
    r.commit_all("add b");
    r.write("c.rs", "c\n");
    r.commit_all("add c");
    r.write("work.rs", "uncommitted\n");
    r
}

#[test]
fn picking_a_commit_diffs_the_worktree_against_it() {
    let r = commit_repo();
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();

    app.open_commit_picker();
    assert_eq!(app.mode, Mode::CommitPick, "the dropdown opens");
    let titles: Vec<&str> = app.commit_choices.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["add c", "add b", "add a", "base"], "full history, newest first");

    app.pick_commit(0).unwrap();
    assert_eq!(app.scope, Scope::Commit);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.changed_count(), 1, "vs HEAD: just the uncommitted work.rs");

    app.open_commit_picker();
    app.pick_commit(2).unwrap();
    assert_eq!(app.changed_count(), 3, "vs add a: b.rs, c.rs, and work.rs");
}

#[test]
fn entering_commit_scope_defaults_to_the_latest_commit() {
    let r = commit_repo();
    let head = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();

    app.enter_commit_scope().unwrap();
    assert_eq!(app.scope, Scope::Commit);
    assert_eq!(app.mode, Mode::Normal, "cycling in shows the diff, not the picker");
    assert_eq!(
        app.selected_commit.as_deref(),
        Some(head.as_str()),
        "the base defaults to the newest commit (HEAD)"
    );
}

#[test]
fn the_commit_picker_cursor_moves_and_clamps() {
    let r = commit_repo();
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();
    app.open_commit_picker();

    assert_eq!(app.commit_cursor, 0);
    app.commit_move(1);
    assert_eq!(app.commit_cursor, 1);
    app.commit_move(-5);
    assert_eq!(app.commit_cursor, 0, "clamps at the top");
    app.commit_move(100);
    assert_eq!(app.commit_cursor, app.commit_choices.len() - 1, "clamps at the bottom");
}

#[test]
fn the_commit_picker_lists_history_even_with_nothing_ahead_of_the_base() {
    let r = Repo::init();
    r.write("a.rs", "0\n");
    r.commit_all("only");
    let mut app = App::new(r.path_buf(), Scope::Commit, Some("main".to_string()));
    app.reload().unwrap();

    app.open_commit_picker();
    assert_eq!(app.mode, Mode::CommitPick, "the picker shows the full history, not just fork→HEAD");
    let titles: Vec<&str> = app.commit_choices.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["only"], "the base branch's own commit is listed");
}

#[test]
fn dir_has_changes_detects_a_nested_change_respecting_boundaries() {
    let r = Repo::init();
    r.write("src/deep/a.rs", "1\n");
    r.write("other/b.rs", "1\n");
    r.commit_all("init");
    r.write("src/deep/a.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    assert!(app.dir_has_changes("src"), "an ancestor of the changed file");
    assert!(app.dir_has_changes("src/deep"), "the direct parent");
    assert!(!app.dir_has_changes("other"), "no change under a sibling dir");
    assert!(!app.dir_has_changes("s"), "prefix match respects the / boundary");
}

#[test]
fn filtering_narrows_the_tree_and_clearing_restores() {
    let r = Repo::init();
    r.write("contracts/evm_pool.rs", "1\n");
    r.write("contracts/sol_pool.rs", "1\n");
    r.write("docs/readme.md", "1\n");
    r.commit_all("init");
    for f in ["contracts/evm_pool.rs", "contracts/sol_pool.rs", "docs/readme.md"] {
        r.write(f, "2\n");
    }
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let full = app.file_rows.len();

    app.start_filter();
    for c in "evm".chars() {
        app.filter_push(c);
    }
    let names: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(names.iter().any(|n| n == "evm_pool.rs"), "the match is shown: {names:?}");
    assert!(
        !names.iter().any(|n| n == "sol_pool.rs" || n == "readme.md"),
        "non-matches are pruned: {names:?}"
    );
    assert!(app.file_rows.len() < full, "the tree is narrower while filtering");

    app.clear_filter();
    assert_eq!(app.file_rows.len(), full, "clearing restores the full tree");
    assert_eq!(app.mode, Mode::Normal, "and leaves filter mode");
}

#[test]
fn a_leading_slash_is_ignored_when_filtering() {
    let r = Repo::init();
    r.write("src/evm.rs", "1\n");
    r.commit_all("init");
    r.write("src/evm.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    app.start_filter();
    app.filter_push('/');
    assert_eq!(app.filter, "", "a leading slash is dropped");
    for c in "evm".chars() {
        app.filter_push(c);
    }
    assert_eq!(app.filter, "evm", "no stray leading slash");
    app.filter_push('/');
    assert_eq!(app.filter, "evm/", "a slash inside the query is kept");
}

#[test]
fn filtering_focuses_files_and_arrows_navigate_the_results() {
    let r = Repo::init();
    for f in ["evm_a.rs", "evm_b.rs", "evm_c.rs"] {
        r.write(f, "1\n");
    }
    r.commit_all("init");
    for f in ["evm_a.rs", "evm_b.rs", "evm_c.rs"] {
        r.write(f, "2\n");
    }
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.focus = Focus::Diff;

    app.start_filter();
    assert_eq!(app.focus, Focus::Files, "starting a search focuses the file list");
    for c in "evm".chars() {
        app.filter_push(c);
    }
    let start = app.file_cursor;
    app.move_cursor(1).unwrap();
    assert_eq!(app.file_cursor, start + 1, "down moves through the filtered results");
    app.move_cursor(-1).unwrap();
    assert_eq!(app.file_cursor, start, "up moves back, without leaving the search");
}

#[test]
fn clearing_the_filter_keeps_the_same_file_selected() {
    let r = Repo::init();
    for f in ["aaa.rs", "readme.md", "zzz.rs"] {
        r.write(f, "1\n");
    }
    r.commit_all("init");
    for f in ["aaa.rs", "readme.md", "zzz.rs"] {
        r.write(f, "2\n");
    }
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    app.start_filter();
    for c in "readme".chars() {
        app.filter_push(c);
    }
    assert_eq!(app.diff_path.as_deref(), Some("readme.md"), "the match is loaded while filtering");

    app.clear_filter();
    assert_eq!(
        app.diff_path.as_deref(),
        Some("readme.md"),
        "clearing the filter keeps the same file selected, not a different row"
    );
}

#[test]
fn enter_expands_a_folder_and_its_child_folders_one_level() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    for f in ["src/app/mod.rs", "src/ui/view.rs", "src/app/deep/x.rs"] {
        r.write(f, "1\n");
    }
    r.commit_all("init");
    for f in ["src/app/mod.rs", "src/ui/view.rs", "src/app/deep/x.rs"] {
        r.write(f, "2\n");
    }
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.set_tab(Tab::AllFiles).unwrap();
    app.file_cursor = 0;

    app.toggle_dir_children();
    let names: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(names.iter().any(|n| n == "mod.rs"), "child folder app/ expanded: {names:?}");
    assert!(names.iter().any(|n| n == "view.rs"), "child folder ui/ expanded: {names:?}");
    assert!(
        !names.iter().any(|n| n == "x.rs"),
        "the deeper deep/ folder stays collapsed: {names:?}"
    );

    app.toggle_dir_children();
    let after: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(!after.iter().any(|n| n == "mod.rs"), "pressing enter again collapses: {after:?}");
}

#[test]
fn enter_completes_a_partial_expand_before_collapsing() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    for f in ["src/app/mod.rs", "src/ui/view.rs"] {
        r.write(f, "1\n");
    }
    r.commit_all("init");
    for f in ["src/app/mod.rs", "src/ui/view.rs"] {
        r.write(f, "2\n");
    }
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.set_tab(Tab::AllFiles).unwrap();
    app.file_cursor = 0;

    app.expand_dir();
    let opened: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(!opened.iter().any(|n| n == "mod.rs"), "-> left child folders shut: {opened:?}");

    app.toggle_dir_children();
    let done: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(done.iter().any(|n| n == "mod.rs"), "enter fills in the shut child folders: {done:?}");

    app.toggle_dir_children();
    let shut: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert!(!shut.iter().any(|n| n == "mod.rs"), "enter again collapses: {shut:?}");
}

#[test]
fn the_branch_picker_sections_lineage_skips_the_divider_and_picks_a_base() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("base");
    r.git(&["update-ref", "refs/remotes/origin/main", "main"]);
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.rs", "2\n");
    r.commit_all("on feature");
    let mut app = App::new(r.path_buf(), Scope::Branch, None);
    app.reload().unwrap();

    app.open_branch_picker();
    assert_eq!(app.mode, Mode::BranchPick, "the dropdown opens in branch scope");
    assert_eq!(
        app.branch_choices,
        vec![
            BranchRow::Item("main".to_string()),
            BranchRow::Divider,
            BranchRow::Item("origin/main".to_string()),
        ],
        "local section, divider, origin section",
    );

    assert_eq!(app.branch_cursor, 0);
    app.branch_move(1);
    assert_eq!(app.branch_cursor, 2, "moving down skips the divider onto origin/main");
    app.branch_move(-1);
    assert_eq!(app.branch_cursor, 0, "moving up skips the divider back onto main");

    app.pick_branch(1).unwrap();
    assert_eq!(app.mode, Mode::BranchPick, "a divider pick keeps the picker open");
    assert_eq!(app.base, None, "a divider pick sets no base");

    app.pick_branch(2).unwrap();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.base.as_deref(), Some("origin/main"), "picking sets the chosen ref as the base");
}

fn goto_file(app: &mut App, path: &str) {
    for _ in 0..app.file_rows.len() {
        if app.current_entry().map(|e| e.path.as_str()) == Some(path) {
            return;
        }
        app.move_cursor(1).unwrap();
    }
    panic!("file {path} not found in the tree");
}

#[test]
fn preview_mode_toggles_only_for_markdown_files() {
    let r = Repo::init();
    r.write("README.md", "# Title\n\nbody\n");
    r.write("code.rs", "fn main() {}\n");
    r.commit_all("init");
    r.write("README.md", "# Title\n\nmore body\n");
    r.write("code.rs", "fn main() { let x = 1; }\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    let has_preview =
        |app: &App| app.footer_actions().iter().any(|&(a, _)| a == FooterAction::Preview);

    goto_file(&mut app, "README.md");
    assert!(app.cursor_is_markdown());
    assert!(has_preview(&app), "the preview hint shows while highlighting a markdown file");
    app.open_preview();
    assert_eq!(app.mode, Mode::Preview, "a markdown file opens the preview");
    app.preview_scroll_by(3);
    assert_eq!(app.preview_scroll, 3);
    app.close_preview();
    assert_eq!(app.mode, Mode::Normal);

    goto_file(&mut app, "code.rs");
    assert!(!app.cursor_is_markdown());
    assert!(!has_preview(&app), "no preview hint while highlighting a non-markdown file");
    app.open_preview();
    assert_eq!(app.mode, Mode::Normal, "preview is refused for non-markdown files");
    assert!(app.status.contains("markdown"), "and it says why: {:?}", app.status);
}

#[test]
fn sending_a_path_requires_a_highlighted_file() {
    let r = Repo::init();
    r.write("dir/a.rs", "1\n");
    r.commit_all("init");
    r.write("dir/a.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    goto_file(&mut app, "dir/a.rs");
    let has_send =
        |app: &App| app.footer_actions().iter().any(|&(a, _)| a == FooterAction::SendPath);
    assert!(has_send(&app), "the send hint shows while a file is highlighted");

    let dir = app.file_rows.iter().position(|row| row.dir_path() == Some("dir")).unwrap();
    app.file_cursor = dir;
    assert!(!has_send(&app), "no send hint on a directory row");
    app.send_path_to_agent();
    assert!(app.status.contains("highlight a file"), "refuses without a file: {:?}", app.status);
}

#[test]
fn space_marks_a_file_reviewed_and_advances_to_the_next() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.write("b.rs", "1\n");
    r.commit_all("init");
    r.write("a.rs", "2\n");
    r.write("b.rs", "2\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    goto_file(&mut app, "a.rs");
    assert!(!app.is_reviewed("a.rs"));
    app.toggle_reviewed();
    assert!(app.is_reviewed("a.rs"), "marked reviewed");
    assert_eq!(app.reviewed_count(), 1);
    assert_eq!(app.current_entry().map(|e| e.path.as_str()), Some("b.rs"), "advanced to next");

    r.write("a.rs", "3\n");
    app.reload().unwrap();
    assert!(!app.is_reviewed("a.rs"), "mark clears when the file changes again");
}

#[test]
fn a_reviewed_mark_survives_a_poll_for_an_unchanged_file() {
    let r = Repo::init();
    r.write("keep.rs", "1\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::AllFiles).unwrap();

    goto_file(&mut app, "keep.rs");
    app.toggle_reviewed();
    assert!(app.is_reviewed("keep.rs"), "marked reviewed");
    app.reload().unwrap();
    assert!(app.is_reviewed("keep.rs"), "the mark survives a poll for an unchanged file");
}

#[test]
fn stage_toggle_stages_then_unstages_via_git_status() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("scratch.rs", "new\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    assert_eq!(app.file_status("scratch.rs").map(|s| s.staged), Some(false), "untracked, unstaged");

    let row = file_row(&app, "scratch.rs");
    assert!(app.stage_toggle(row), "the marker click acts");
    assert_eq!(app.file_status("scratch.rs").map(|s| s.staged), Some(true), "git add staged it");

    let row = file_row(&app, "scratch.rs");
    assert!(app.stage_toggle(row));
    assert_eq!(
        app.file_status("scratch.rs").map(|s| s.staged),
        Some(false),
        "git reset unstaged it"
    );
}

#[test]
fn request_delete_opens_a_confirmation_then_confirm_removes_the_file() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.write("b.rs", "two\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    r.write("b.rs", "TWO\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    goto_file(&mut app, "a.rs");

    app.request_delete();
    assert_eq!(app.mode, Mode::ConfirmDelete, "delete asks first");
    let pd = app.pending_delete().expect("a pending delete");
    assert_eq!(pd.path, "a.rs");
    assert!(!pd.is_dir);

    app.confirm_delete();
    assert_eq!(app.mode, Mode::Normal);
    assert!(!r.path_buf().join("a.rs").exists(), "the file is gone from the working tree");
    assert!(r.path_buf().join("b.rs").exists(), "other files are untouched");
}

#[test]
fn cancel_delete_keeps_the_file() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    goto_file(&mut app, "a.rs");

    app.request_delete();
    app.cancel_delete();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.pending_delete().is_none(), "the pending delete is cleared");
    assert!(r.path_buf().join("a.rs").exists(), "the file is left in place");
}

#[test]
fn request_delete_on_a_folder_removes_it_recursively() {
    let r = Repo::init();
    r.write("src/a.rs", "x\n");
    r.commit_all("init");
    r.write("src/a.rs", "X\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let dir_row =
        app.file_rows.iter().position(|row| row.dir_path() == Some("src")).expect("a src dir row");
    app.file_cursor = dir_row;

    app.request_delete();
    let pd = app.pending_delete().expect("a pending delete");
    assert_eq!(pd.path, "src");
    assert!(pd.is_dir, "a directory row targets the folder");

    app.confirm_delete();
    assert!(!r.path_buf().join("src").exists(), "the folder and its contents are removed");
}

#[test]
fn delete_is_blocked_on_the_read_only_pr_tab() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    app.set_tab(Tab::Pr).unwrap();

    app.request_delete();
    assert_ne!(app.mode, Mode::ConfirmDelete, "no delete on the read-only PR tab");
    assert!(app.pending_delete().is_none());
}

#[test]
fn a_reviewed_tick_survives_a_base_switch_but_not_a_content_change() {
    let r = Repo::init();
    r.write("a.rs", "l1\n");
    r.commit_all("c1");
    r.write("a.rs", "l1\nl2\n");
    r.commit_all("c2");
    r.write("a.rs", "l1\nl2\nl3\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    let older = app.commit_choices[1].sha.clone();

    goto_file(&mut app, "a.rs");
    app.toggle_reviewed();
    assert!(app.is_reviewed("a.rs"), "marked reviewed under the current base");

    app.set_commit(older).unwrap();
    assert!(app.is_reviewed("a.rs"), "the tick survives switching the base");

    r.write("a.rs", "l1\nl2\nCHANGED\n");
    app.reload().unwrap();
    assert!(!app.is_reviewed("a.rs"), "a worktree content change drops the tick");
}

#[test]
fn a_reviewed_tick_follows_content_across_a_branch_switch() {
    use herdr_reviewr::app::Tab;
    let r = Repo::init();
    r.write("same.rs", "identical\n");
    r.write("diff.rs", "on-main\n");
    r.commit_all("main");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("diff.rs", "on-feature\n");
    r.commit_all("feature change");

    let mut app = App::new(r.path_buf(), Scope::Branch, None);
    app.reload().unwrap();
    app.set_tab(Tab::AllFiles).unwrap();

    let d = file_row(&app, "diff.rs");
    app.select_file(d).unwrap();
    app.toggle_reviewed();
    let s = file_row(&app, "same.rs");
    app.select_file(s).unwrap();
    app.toggle_reviewed();
    assert!(app.is_reviewed("same.rs") && app.is_reviewed("diff.rs"), "both ticked on feature");

    r.git(&["checkout", "-q", "main"]);
    app.reload().unwrap();
    assert!(app.is_reviewed("same.rs"), "an identical file keeps its tick across branches");
    assert!(!app.is_reviewed("diff.rs"), "a file whose content changed loses its tick");
}

fn two_block_app() -> (Repo, App) {
    let r = Repo::init();
    r.write("a.rs", "top\na\nb\nc\nd\ne\nf\ng\nbot\n");
    r.write("b.rs", "one\n");
    r.commit_all("init");
    r.write("a.rs", "top\nA\nb\nc\nd\ne\nf\nG\nbot\n");
    r.write("b.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();
    (r, app)
}

#[test]
fn change_block_starts_finds_each_contiguous_run() {
    let (_r, mut app) = two_block_app();
    goto_file(&mut app, "a.rs");
    app.focus = Focus::Diff;
    assert_eq!(app.change_block_starts().len(), 2, "the two edits are two blocks");
}

#[test]
fn space_steps_blocks_then_marks_file_and_lands_on_next_files_first_block() {
    let (_r, mut app) = two_block_app();
    goto_file(&mut app, "a.rs");
    app.focus = Focus::Diff;
    let starts = app.change_block_starts();
    assert_eq!(starts.len(), 2);

    app.review_advance();
    assert_eq!(app.diff_cursor, starts[0], "steps to the first block");
    app.review_advance();
    assert_eq!(app.diff_cursor, starts[1], "steps to the second block");

    app.review_advance();
    assert!(app.is_reviewed("a.rs"), "the file is marked reviewed after its last block");
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"), "advances to the next file");
    assert_eq!(app.focus, Focus::Diff, "stays in the diff to keep stepping");
    assert_eq!(
        app.diff_cursor,
        app.change_block_starts().first().copied().unwrap(),
        "lands on the next file's first change block",
    );
}

#[test]
fn resolve_removes_the_comment_and_shrinks_the_list() {
    let r = edited_repo();
    let mut app = app_on(&r);
    comment_on(&mut app, '+', "fix this");
    assert_eq!(app.store.len(), 1);
    app.open_list();
    assert_eq!(app.mode, Mode::List);

    app.resolve_comment();
    assert_eq!(app.store.len(), 0, "resolving removes the comment");
    assert_eq!(app.mode, Mode::Normal, "the emptied list overlay closes");
}

#[test]
fn jump_to_comment_opens_file_and_sets_diff_cursor() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.write("b.rs", "one\ntwo\n");
    r.commit_all("init");
    r.write("a.rs", "1x\n");
    r.write("b.rs", "one\nTWO\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap();

    goto_file(&mut app, "b.rs");
    app.focus = Focus::Diff;
    app.diff_cursor = row_with(&app, '+');
    app.start_comment();
    for ch in "look here".chars() {
        app.input_push(ch);
    }
    app.submit_comment();
    let arow = file_row(&app, "a.rs");
    app.select_file(arow).unwrap();
    assert_eq!(app.diff_path.as_deref(), Some("a.rs"), "browsed to another file");

    app.open_list();
    app.jump_to_comment(0);
    assert_eq!(app.diff_path.as_deref(), Some("b.rs"), "jumped to the comment's file");
    assert_eq!(app.mode, Mode::Normal, "the list overlay closed on the jump");
    assert_eq!(app.focus, Focus::Diff);
    assert_eq!(
        app.visible[app.diff_cursor].new_no(),
        Some(2),
        "the cursor sits on the commented line",
    );
}

// --- `x` expand-all-changes toggle ---------------------------------------------------

fn row_visible(app: &App, needle: &str) -> bool {
    app.file_rows.iter().any(|r| r.name.contains(needle))
}

#[test]
fn x_expands_change_folders_then_collapses_back_to_the_prior_state() {
    let r = Repo::init();
    r.write("src/deep/a.rs", "one\n");
    r.write("top.rs", "t\n");
    r.commit_all("init");
    r.write("src/deep/a.rs", "ONE\n"); // a change nested under collapsed folders
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.set_tab(Tab::AllFiles).unwrap(); // default-collapsed tab, so expansion is observable
    app.reload().unwrap();

    assert!(!row_visible(&app, "a.rs"), "nested change is hidden under a collapsed folder");

    app.expand_changes();
    assert!(row_visible(&app, "a.rs"), "x expands the folders leading to the change");

    app.expand_changes();
    assert!(!row_visible(&app, "a.rs"), "x again collapses back to the prior state");
}

#[test]
fn x_is_a_noop_when_the_change_folders_are_already_expanded() {
    let r = Repo::init();
    r.write("src/deep/a.rs", "one\n");
    r.commit_all("init");
    r.write("src/deep/a.rs", "ONE\n");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.reload().unwrap(); // Changes tab: folders open by default

    assert!(row_visible(&app, "a.rs"), "changes tab shows the nested change already");
    let before: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();

    app.expand_changes(); // already fully expanded -> nothing to do
    let after: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(before, after, "x does nothing when change folders are already expanded");

    app.expand_changes(); // and must not spuriously collapse on a second press
    let after2: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(before, after2, "a second x still does nothing");
}

#[test]
fn x_does_nothing_with_no_changes() {
    let r = Repo::init();
    r.write("src/deep/a.rs", "one\n");
    r.commit_all("init");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    app.set_tab(Tab::AllFiles).unwrap();
    app.reload().unwrap();
    let before: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    app.expand_changes();
    let after: Vec<String> = app.file_rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(before, after, "no changes -> x is inert");
}

// --- `/` context-sensitive search / filter ------------------------------------------

#[test]
fn slash_searches_the_diff_and_navigates_matches() {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\ngamma\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA needle\ngamma needle\ndelta\n");
    let mut app = app_on(&r);
    app.focus = Focus::Diff;
    app.diff_cursor = 0;

    app.slash();
    assert_eq!(app.mode, Mode::Search, "/ on the diff opens search");
    for c in "needle".chars() {
        app.search_push(c);
    }
    let first = app.diff_cursor;
    assert!(app.visible[first].text().to_lowercase().contains("needle"), "cursor sits on a match");
    assert_eq!(app.search_status().map(|(_, n)| n), Some(2), "two lines contain the needle");

    app.search_next();
    let second = app.diff_cursor;
    assert_ne!(second, first, "next jumps to the other match");
    app.search_next();
    assert_eq!(app.diff_cursor, first, "next wraps back to the first");

    app.clear_search();
    assert_eq!(app.mode, Mode::Normal, "esc closes search");
}

#[test]
fn slash_filters_the_list_when_the_file_pane_is_focused() {
    let r = edited_repo();
    let mut app = app_on(&r);
    app.focus = Focus::Files;
    app.slash();
    assert_eq!(app.mode, Mode::Filter, "/ on the file list still filters");
}
