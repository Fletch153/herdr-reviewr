//! herdr-reviewr — a herdr-native review sidebar.
//!
//! Browse an agent's changes (uncommitted / branch), leave line-range comments,
//! and send them back to the agent (or the clipboard) — entirely in a herdr pane.
//!
//! This crate is split into a thin binary (`src/main.rs`) and this library so the
//! interaction logic in [`app`] stays terminal-free and unit-testable. This module
//! owns the terminal lifecycle and the event loop; it maps input events onto
//! [`app::App`] methods and renders with [`ui`].

pub mod app;
pub mod browser;
pub mod config;
pub mod diff;
pub mod editor;
pub mod export;
pub mod file_list;
pub mod forge;
pub mod git;
pub mod herdr;
pub mod highlight;
pub mod icons;
#[macro_use]
pub mod log;
pub mod model;
pub mod nvim;
pub mod nvim_keys;
pub mod proc;
pub mod theme;
pub mod turn;
pub mod ui;

use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;
use ratatui::layout::Rect;

use crate::app::{App, Focus, Mode};
use crate::config::Config;
use crate::export::{Agent, Clipboard};
use crate::model::Scope;

/// Entry point: parse config, set up the terminal, run the loop, restore.
pub fn run() -> Result<()> {
    let cfg = Config::from_env();
    log::init();
    // A non-repo path is not an error — the sidebar opens to an empty state and
    // starts showing changes if the directory becomes a repo (specs/herdr-host.md).
    let repo = git::toplevel(&cfg.repo).unwrap_or_else(|| cfg.repo.clone());
    logln!("start repo={} poll={:?} base={:?}", repo.display(), cfg.poll, cfg.base);
    let base = cfg.base.clone().or_else(config::config_file_base);
    let mut app = App::new(repo, Scope::Commit, base);
    app.set_cli_theme(cfg.theme.clone());
    if let Some(wrap) = cfg.wrap {
        app.wrap = wrap;
    }
    if let Some(icons) = cfg.icons.or_else(config::config_file_icons) {
        app.icons = icons;
    }
    // Embedded-nvim editor mode is opt-in (`editor = "nvim"`) and needs nvim on PATH; on any
    // startup failure the mode degrades to the built-in diff view with a status note.
    let want_nvim = cfg.editor.or_else(config::config_file_editor).as_deref() == Some("nvim");
    app.editor_nvim = want_nvim && nvim::nvim_present();
    if want_nvim && !app.editor_nvim {
        app.status = "editor=nvim ignored: nvim not found on PATH".to_string();
    }
    let mut session = NvimSession::default();
    if app.editor_nvim {
        // Eager start: overlap nvim's spawn + init.lua load with the initial reload and first
        // paint, so the first file click renders instantly. The real size syncs on frame 1.
        match session.start(&app.repo, 80, 24) {
            Ok(()) => {}
            Err(e) => {
                app.editor_nvim = false;
                app.status = format!("editor=nvim disabled: {e}");
            }
        }
    }
    logln!("editor_nvim={}", app.editor_nvim);
    app.reload()?;

    let (mut terminal, kbd) = enter_tui();
    let result = event_loop(&mut terminal, &mut app, cfg.poll, kbd, &mut session);
    leave_tui(kbd);
    session.shutdown();
    result
}

/// The embedded editor's lifecycle state, owned by the event loop so [`App`] stays engine-free
/// and unit-testable. Handlers talk to it through [`NvimBridge`], letting routing tests use a
/// recorder instead of a live nvim.
#[derive(Default)]
struct NvimSession {
    engine: Option<nvim::Nvim>,
    /// Last (cols, rows) sent, so a divider drag doesn't spam resizes.
    last_size: Option<(u16, u16)>,
    /// Repo-relative path last opened — the per-frame watcher's change detector.
    last_sent: Option<String>,
    /// The diff base last published to the editor; a scope/base change re-diffs in place.
    last_base: Option<String>,
    /// Each death grants one automatic respawn on the next open; after that the dead panel's
    /// `r` restarts manually (a crash-looping nvim must not spin).
    auto_respawned: bool,
    /// A left-button press landed in the grid, so drags/release route to nvim.
    mouse_down: bool,
    /// The colorscheme leaves `Normal` without a background: the blit paints the terminal
    /// default instead of nvim's reported black, so transparent themes (e.g. catppuccin's
    /// `transparent_background`) look exactly as they do in a plain terminal nvim. Sampled
    /// once per engine start; a mid-session `:colorscheme` change refreshes on restart.
    transparent: bool,
}

impl NvimSession {
    fn start(&mut self, repo: &Path, cols: u16, rows: u16) -> anyhow::Result<()> {
        let opts = nvim::StartOpts { clean: false, rtp: nvim::plugin_nvim_dir() };
        let mut engine = nvim::Nvim::start(repo, cols, rows, &opts)?;
        self.transparent = engine.normal_bg_transparent().unwrap_or(false);
        self.engine = Some(engine);
        self.last_size = Some((cols, rows));
        self.last_sent = None;
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(mut engine) = self.engine.take() {
            let _ = engine.shutdown(true);
        }
    }

    fn engine_alive(&self) -> Option<&nvim::Nvim> {
        self.engine.as_ref().filter(|e| e.is_running())
    }
}

/// What the key/mouse handlers need from the editor — a seam so routing is unit-testable with
/// a recorder implementation.
trait NvimBridge {
    fn alive(&self) -> bool;
    /// The editor's current mode short-name ("normal", "insert", …); empty when unknown/dead.
    fn mode(&self) -> String;
    fn feed_keys(&mut self, notation: &str);
    fn feed_paste(&mut self, text: &str);
    fn feed_mouse(&mut self, button: &str, action: &str, modifier: &str, row: u16, col: u16);
    /// Run an Ex command fire-and-forget (`ReviewrSend`, `ReviewrList`): outcomes paint in the
    /// editor's own message area.
    fn run_command(&mut self, cmd: &str);
    /// How many buffers hold unsaved changes; `None` when the editor is gone or busy (a prompt
    /// is up) — callers must not block quitting on it.
    fn modified_count(&mut self) -> Option<i64>;
    fn restart(&mut self, repo: &Path, cols: u16, rows: u16) -> bool;
}

impl NvimBridge for NvimSession {
    fn alive(&self) -> bool {
        self.engine_alive().is_some()
    }

    fn mode(&self) -> String {
        self.engine_alive().map(|e| e.grid().mode.clone()).unwrap_or_default()
    }

    fn feed_keys(&mut self, notation: &str) {
        if let Some(e) = self.engine_alive() {
            let _ = e.input(notation);
        }
    }

    fn feed_paste(&mut self, text: &str) {
        if let Some(e) = self.engine_alive() {
            let _ = e.paste(text);
        }
    }

    fn feed_mouse(&mut self, button: &str, action: &str, modifier: &str, row: u16, col: u16) {
        if let Some(e) = self.engine_alive() {
            let _ = e.input_mouse(button, action, modifier, row, col);
        }
    }

    fn run_command(&mut self, cmd: &str) {
        if let Some(e) = self.engine_alive() {
            let _ = e.command_fire(cmd);
        }
    }

    fn modified_count(&mut self) -> Option<i64> {
        let e = self.engine.as_mut().filter(|e| e.is_running())?;
        e.eval("len(getbufinfo({'bufmodified':1}))").ok().and_then(|v| v.as_i64())
    }

    fn restart(&mut self, repo: &Path, cols: u16, rows: u16) -> bool {
        self.engine = None;
        self.auto_respawned = false;
        self.start(repo, cols, rows).is_ok()
    }
}

/// nvim mode's quit guard: ask before discarding unsaved editor buffers, but never block
/// quitting on a dead or wedged editor (an unanswered eval means "unknown" — quit).
fn request_quit(app: &mut App, session: &mut dyn NvimBridge) {
    match session.modified_count() {
        Some(n) if n > 0 => app.mode = Mode::ConfirmQuit,
        _ => app.should_quit = true,
    }
}

/// Make the editor follow the reviewer's selection and scope: open `diff_path` in nvim when it
/// changes (or a click re-requested it), re-diff in place when only the scope/base changed, and
/// respawn a dead editor once per death. `:confirm edit`'s unsaved prompt renders inside the
/// grid; `last_sent` is set optimistically so the watcher never re-sends against a showing
/// prompt — a cancel is the user's call, and re-clicking the file retries via `nvim_reopen`.
fn nvim_sync(app: &mut App, session: &mut NvimSession, grid: Rect) {
    if !app.editor_nvim || !app.tab.is_file_tab() {
        return;
    }
    let Some(rel) = app.diff_path.clone() else { return };
    let base = app.nvim_base_ref();
    let force = std::mem::take(&mut app.nvim_reopen);
    let same_path = session.last_sent.as_deref() == Some(rel.as_str());
    let same_base = session.last_base.as_deref() == Some(base.as_str());
    if !force && same_path && same_base {
        return;
    }
    if session.engine_alive().is_none() {
        if session.auto_respawned {
            return; // dead panel owns recovery now (its `r` key)
        }
        session.auto_respawned = true;
        let (cols, rows) = (grid.width.max(12), grid.height.max(3));
        match session.start(&app.repo, cols, rows) {
            Ok(()) => app.status = "editor restarted".to_string(),
            Err(e) => {
                app.status = format!("editor restart failed: {e}");
                return;
            }
        }
    } else {
        // A live open succeeded before this one: the next death earns a fresh respawn.
        session.auto_respawned = false;
    }
    if let Some(engine) = session.engine_alive() {
        if !force && same_path && app.repo.join(&rel).exists() {
            // Only the scope/base moved: re-diff the open buffer, no :edit (which would
            // prompt on a modified buffer for no reason).
            let _ = engine.rebase(&base);
        } else if app.repo.join(&rel).exists() {
            // The Changes tab opens into the focused view (unchanged regions folded, cursor
            // on the first change — the diff-pane experience); All files opens plain.
            let _ =
                engine.open_file(&app.repo.join(&rel), &base, app.tab == crate::app::Tab::Changes);
            session.last_sent = Some(rel);
        } else {
            // Deleted in the worktree: an all-red scratch view of the base content, never a
            // phantom :edit (an empty [New File] would set the user's LSP complaining).
            let _ = engine.show_deleted(&rel, &base);
            session.last_sent = Some(rel);
        }
        session.last_base = Some(base);
    }
}

fn enter_tui() -> (DefaultTerminal, bool) {
    let terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste);
    let kbd = supports_keyboard_enhancement().unwrap_or(false);
    logln!("keyboard enhancement supported={kbd}");
    if kbd {
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    (terminal, kbd)
}

fn leave_tui(kbd: bool) {
    if kbd {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
}

/// A transient status message (e.g. "sent 3 comments") fades after this long idle.
const STATUS_TTL: Duration = Duration::from_secs(4);

/// While the `PR` tab is active, refetch GitHub at least this often — a fallback for forge-side
/// changes with no local signal (a reviewer's comment). Local pushes and `gh` PR actions refresh
/// sooner, on the agent's turn-end, so this cadence is the slow safety net (specs/forge-host.md).
const PR_POLL: Duration = Duration::from_secs(60);

/// Draw, then wait up to the poll deadline for input; refresh on each tick.
fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    poll: Duration,
    kbd: bool,
    session: &mut NvimSession,
) -> Result<()> {
    let mut last_poll = Instant::now();
    let mut last_pr_poll = Instant::now();
    // The PR snapshot is fetched on a worker thread and delivered over this channel, so the slow
    // `gh` calls never block the draw loop (specs/forge-host.md).
    let (pr_tx, pr_rx) = mpsc::channel::<crate::forge::PrView>();
    let mut pr_inflight = false;
    let mut status_at = Instant::now();
    let mut last_status = String::new();
    // Fetch the PR snapshot as soon as the panel opens, not on first switching to the tab, so the
    // tab is already populated when the user gets there (specs/forge-host.md).
    app.pr_pending = true;
    while !app.should_quit {
        // Expire a stale status line: restart the timer when the message changes, and clear
        // it once it has lingered past the TTL, so a notification doesn't stay up forever.
        if app.status != last_status {
            last_status.clone_from(&app.status);
            status_at = Instant::now();
        }
        if !app.status.is_empty() && status_at.elapsed() >= STATUS_TTL {
            app.status.clear();
            last_status.clear();
        }
        // Settle both panes' scroll for this frame's viewport before painting, so the
        // diff window matches what mouse hit-testing will map against. Each pane reveals its
        // cursor only when a navigation requested it (so the wheel can scroll freely), then
        // bounds the offset every frame. While composing, reserve the inline box's rows and
        // keep revealing so the anchored line stays above the growing box.
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        let viewport = ui::diff_viewport_height(area, app.list_pct);
        let effective = if app.composing() {
            let box_h = ui::composer_height(app, ui::diff_inner_width(area, app.list_pct));
            viewport.saturating_sub(box_h).max(1)
        } else {
            viewport
        };
        let heights = ui::diff_row_heights(app, area);
        if std::mem::take(&mut app.reveal_diff) || app.composing() {
            app.reveal_diff_cursor(&heights, effective);
        }
        app.bound_diff_scroll(&heights, effective);
        let file_vp = ui::file_viewport_height(area, app.list_pct);
        if std::mem::take(&mut app.reveal_files) {
            app.reveal_file_cursor(file_vp);
        }
        app.bound_file_scroll(file_vp);
        if app.mode == Mode::Preview {
            let (lines, vp) = ui::preview_metrics(app, area, app.list_pct);
            app.bound_preview_scroll(lines, vp);
        }
        if app.mode == Mode::Help {
            let (lines, vp) = ui::help_metrics(area, app.editor_nvim);
            app.bound_help_scroll(lines, vp);
        }
        // Embedded editor: keep its grid sized to the pane interior (dedup'd — nvim acks with
        // a grid_resize event), surface death to the footer, then paint and drive it.
        let grid_rect = ui::nvim_grid_rect(area, app.list_pct);
        if app.editor_nvim
            && app.tab.is_file_tab()
            && grid_rect.width > 0
            && grid_rect.height > 0
            && let Some(engine) = session.engine.as_mut().filter(|e| e.is_running())
        {
            let want = (grid_rect.width.max(12), grid_rect.height.max(3));
            if session.last_size != Some(want) {
                let _ = engine.resize(want.0, want.1);
                session.last_size = Some(want);
            }
        }
        app.nvim_dead = app.editor_nvim && session.engine_alive().is_none();
        let view = if app.editor_nvim {
            Some(match &session.engine {
                Some(e) if e.is_running() => {
                    ui::NvimView::Grid { engine: e, transparent: session.transparent }
                }
                Some(e) => ui::NvimView::Dead(e.died()),
                None => ui::NvimView::Dead(None),
            })
        } else {
            None
        };
        terminal.draw(|f| ui::render_with_nvim(f, app, view.as_ref()))?;
        drop(view);
        // Make the editor follow the selection (open-on-change; one auto-respawn per death).
        nvim_sync(app, session, grid_rect);
        // Deliver a completed background fetch, then trigger a new one when `pr_pending` is set
        // (panel open, tab entry, `r`, or the agent's turn-end) or the slow fallback poll elapses
        // — never more than one in flight, and never on the draw thread.
        if let Ok(view) = pr_rx.try_recv() {
            app.apply_pr(view);
            pr_inflight = false;
        }
        if !pr_inflight
            && (app.pr_pending
                || (app.tab == crate::app::Tab::Pr && last_pr_poll.elapsed() >= PR_POLL))
        {
            app.pr_pending = false;
            last_pr_poll = Instant::now();
            pr_inflight = true;
            let (tx, repo) = (pr_tx.clone(), app.repo.clone());
            thread::spawn(move || {
                let _ = tx.send(crate::forge::fetch(&repo));
            });
        }
        // Wake at the status-expiry boundary too, so it clears on time when idle.
        let poll_left = poll.saturating_sub(last_poll.elapsed());
        let mut timeout = if app.status.is_empty() {
            poll_left
        } else {
            poll_left.min(STATUS_TTL.saturating_sub(status_at.elapsed()))
        };
        // While a fetch is in flight, wake often so its result paints promptly when it lands.
        if pr_inflight {
            timeout = timeout.min(Duration::from_millis(100));
        }
        // The embedded editor has no waker: while it runs on a file tab, poll at ~30fps so its
        // async repaints (LSP, timers, the :confirm prompt) land promptly. ratatui diffs
        // unchanged frames to zero terminal writes, so the idle cost is negligible.
        if app.editor_nvim && app.tab.is_file_tab() && session.engine_alive().is_some() {
            timeout = timeout.min(nvim::FRAME_POLL);
        }
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if let Err(e) = handle_key(app, session, k, area) {
                        app.status = format!("error: {e}");
                    }
                    logln!(
                        "key {:?}{} -> mode={:?} focus={:?} scope={:?} file={}/{} diff_cursor={} scroll={} comments={}",
                        k.code,
                        if k.modifiers.is_empty() {
                            String::new()
                        } else {
                            format!(" {:?}", k.modifiers)
                        },
                        app.mode,
                        app.focus,
                        app.scope,
                        app.file_cursor,
                        app.entries.len(),
                        app.diff_cursor,
                        app.diff_scroll,
                        app.store.len()
                    );
                    if let Some(req) = app.take_pending_editor() {
                        leave_tui(kbd);
                        let status = editor::open(&app.repo, &req.path, req.line);
                        let (t, _) = enter_tui();
                        *terminal = t;
                        let _ = terminal.clear();
                        match status {
                            Ok(s) if !s.success() => app.status = format!("editor exited: {s}"),
                            Err(e) => app.status = format!("editor failed: {e}"),
                            _ => {}
                        }
                        if let Err(e) = app.reload() {
                            app.status = format!("error: {e}");
                        }
                    }
                }
                Event::Mouse(m) => {
                    // Reuse this frame's `area` and `heights` (computed above for the scroll
                    // settle) so a drag-select doesn't re-measure the whole diff per motion.
                    if let Err(e) = handle_mouse(app, session, m, area, &heights) {
                        app.status = format!("error: {e}");
                    }
                    logln!(
                        "mouse {:?} col={} row={} -> focus={:?} file={} diff_cursor={} scroll={} anchor={:?}",
                        m.kind,
                        m.column,
                        m.row,
                        app.focus,
                        app.file_cursor,
                        app.diff_cursor,
                        app.diff_scroll,
                        app.select_anchor
                    );
                }
                // Bracketed paste: into the editor when it has focus (nvim_paste inserts
                // literally, mode-appropriately, in one undo step); else the composer caret.
                Event::Paste(text) => {
                    if app.editor_nvim
                        && app.tab.is_file_tab()
                        && app.focus == Focus::Diff
                        && app.mode == Mode::Normal
                        && session.alive()
                    {
                        session.feed_paste(&text);
                    } else {
                        app.input_paste(&text);
                    }
                    logln!("paste {} chars -> composing={}", text.len(), app.composing());
                }
                _ => {}
            }
        }
        if last_poll.elapsed() >= poll {
            // Advance the last-turn baseline before reloading, so a turn promoted this poll
            // is visible to this poll's changed-files build. When the agent just went idle, its
            // turn may have pushed or run `gh pr merge`; refetch the PR if the tab is showing it
            // (entering the tab refetches on its own otherwise) (specs/forge-host.md).
            if app.track_turn() && app.tab == crate::app::Tab::Pr {
                app.pr_pending = true;
            }
            // A failed refresh must never crash the UI or drop a comment.
            if let Err(e) = app.reload() {
                app.status = format!("refresh failed: {e}");
            }
            logln!(
                "poll files={} composing={} diff_cursor={} scroll={}",
                app.entries.len(),
                app.composing(),
                app.diff_cursor,
                app.diff_scroll
            );
            last_poll = Instant::now();
        }
    }
    Ok(())
}

/// Diff scroll steps: a full page for `PageUp`/`PageDown`, half for `ctrl+u`/`ctrl+d`.
const PAGE: isize = 15;
const HALF_PAGE: isize = 8;

fn handle_key(app: &mut App, session: &mut NvimSession, key: KeyEvent, area: Rect) -> Result<()> {
    use KeyCode::{
        Backspace, Char, Delete, Down, End, Enter, Esc, Home, Left, PageDown, PageUp, Right, Tab,
        Up,
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // A keypress ends any in-progress divider drag, so opening a modal mid-drag (which makes
    // the mouse handler ignore the releasing Up) can't strand `resizing` true.
    app.resizing = false;

    if app.composing() {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let alt_or_shift = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT);
        let word = alt || ctrl; // word-jump on Alt/Ctrl + arrow (terminal-dependent)
        // The wrapped width of the box, for vertical (wrapped-row) caret movement.
        let cw = ui::composer_content_width(ui::diff_inner_width(area, app.list_pct));
        match key.code {
            Esc => app.cancel_comment(),
            // Alt/Shift+Enter (and Ctrl+J) insert a newline; plain Enter submits.
            Enter if alt_or_shift => app.input_push('\n'),
            Enter => app.submit_comment(),
            Char('j') if ctrl => app.input_push('\n'),
            Char('w') if ctrl => app.input_delete_word(),
            Char('a') if ctrl => app.caret_home(),
            Char('e') if ctrl => app.caret_end(),
            Char('u') if ctrl => app.input_kill_to_start(),
            Char('k') if ctrl => app.input_kill_to_end(),
            // Word-jump: `Alt+b`/`Alt+f` (readline; survives as ESC-prefixed, unlike modified
            // arrows, which many terminals/multiplexers strip) and modified arrows where they
            // are delivered. These precede the plain-character insert below.
            Char('b') if alt => app.caret_word_left(),
            Char('f') if alt => app.caret_word_right(),
            Left if word => app.caret_word_left(),
            Right if word => app.caret_word_right(),
            Left => app.caret_left(),
            Right => app.caret_right(),
            Up => app.caret = ui::caret_vertical(&app.input, app.caret, cw, false),
            Down => app.caret = ui::caret_vertical(&app.input, app.caret, cw, true),
            Home => app.caret_home(),
            End => app.caret_end(),
            Delete => app.input_delete_forward(),
            Backspace => app.input_backspace(),
            Char(c) if !ctrl => app.input_push(c),
            _ => {}
        }
        return Ok(());
    }

    // The read-only PR tab: navigate the snapshot and open links; authoring keys are inert.
    if app.tab == crate::app::Tab::Pr {
        match key.code {
            Char('q') => app.should_quit = true,
            Char('r') => app.pr_pending = true,
            Char('1') => app.set_tab(crate::app::Tab::Changes)?,
            Char('2') => app.set_tab(crate::app::Tab::AllFiles)?,
            Char('o') => app.pr_open(),
            Char('j') | Down => app.pr_move(1),
            Char('k') | Up => app.pr_move(-1),
            // The navigator is short; the read pane is what overflows, so the page keys scroll it.
            PageDown => app.pr_scroll_read(PAGE),
            PageUp => app.pr_scroll_read(-PAGE),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::List {
        match key.code {
            Esc | Char('l' | 'q') => app.close_list(),
            Char('j') | Down => app.list_move(1),
            Char('k') | Up => app.list_move(-1),
            Char(' ') => app.toggle_list_select(),
            Char('a') => app.select_all_or_none(),
            Enter => {
                if let Some(i) = app.list_current() {
                    app.open_comment(i);
                }
            }
            Char('s') => app.export(&Agent),
            Char('y') => app.export(&Clipboard),
            Char('e') => app.start_edit(),
            Char('r') => app.resolve_selected(),
            Char('d') => app.delete_comment(),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::CommitPick {
        match key.code {
            Esc | Char('q') => app.close_commit_picker(),
            Char('j') | Down => app.commit_move(1),
            Char('k') | Up => app.commit_move(-1),
            Enter | Char('C' | 'l') => app.pick_commit(app.commit_cursor)?,
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::BranchPick {
        match key.code {
            Esc | Char('q') => app.close_branch_picker(),
            Char('j') | Down => app.branch_move(1),
            Char('k') | Up => app.branch_move(-1),
            Enter | Char('B' | 'l') => app.pick_branch(app.branch_cursor)?,
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::Filter {
        match key.code {
            Esc => app.clear_filter(),
            Enter => app.confirm_filter(),
            Backspace => app.filter_backspace(),
            Up => app.move_cursor(-1)?,
            Down => app.move_cursor(1)?,
            PageUp => app.move_cursor(-PAGE)?,
            PageDown => app.move_cursor(PAGE)?,
            Char(c) if !ctrl => app.filter_push(c),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::Search {
        match key.code {
            Esc => app.clear_search(),
            Enter | Down => app.search_next(),
            Up => app.search_prev(),
            Backspace => app.search_backspace(),
            Char(c) if !ctrl => app.search_push(c),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::Preview {
        match (key.code, ctrl) {
            (Esc | Char('q' | 'p'), _) => app.close_preview(),
            (Char('j') | Down, _) => app.preview_scroll_by(1),
            (Char('k') | Up, _) => app.preview_scroll_by(-1),
            (PageDown, _) => app.preview_scroll_by(PAGE),
            (PageUp, _) => app.preview_scroll_by(-PAGE),
            (Char('d'), true) => app.preview_scroll_by(HALF_PAGE),
            (Char('u'), true) => app.preview_scroll_by(-HALF_PAGE),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::Help {
        match (key.code, ctrl) {
            (Esc | Char('q' | '?'), _) => app.close_help(),
            (Char('j') | Down, _) => app.help_scroll_by(1),
            (Char('k') | Up, _) => app.help_scroll_by(-1),
            (PageDown, _) => app.help_scroll_by(PAGE),
            (PageUp, _) => app.help_scroll_by(-PAGE),
            (Char('d'), true) => app.help_scroll_by(HALF_PAGE),
            (Char('u'), true) => app.help_scroll_by(-HALF_PAGE),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::ConfirmDelete {
        match key.code {
            Char('y') | Enter => app.confirm_delete(),
            Esc | Char('n' | 'q') => app.cancel_delete(),
            _ => {}
        }
        return Ok(());
    }

    if app.mode == Mode::ConfirmQuit {
        match key.code {
            Char('y') | Enter => app.should_quit = true,
            Esc | Char('n' | 'q') => app.mode = Mode::Normal,
            _ => {}
        }
        return Ok(());
    }

    // Embedded-nvim editor mode (the `PR` tab keeps its own full keymap, handled earlier).
    // Files focus is the navigator: the file-list keymap, with send/list retargeted through
    // reviewr.nvim. Editor focus forwards everything to nvim except a mode-aware plain Tab.
    if app.editor_nvim {
        if app.focus == Focus::Diff {
            if session.alive() {
                // Plain Tab returns to the file list only in nvim's normal-ish modes; while
                // inserting or on the cmdline it must type (indent, completion). <C-i> stays
                // distinct under the kitty protocol, so the jumplist survives.
                let normal_ish = {
                    let mode = session.mode();
                    mode.starts_with("normal")
                        || mode.starts_with("visual")
                        || mode.starts_with("operator")
                };
                if key.code == Tab && key.modifiers.is_empty() && normal_ish {
                    app.toggle_focus();
                } else if let Some(notation) = nvim_keys::key_notation(&key) {
                    session.feed_keys(&notation);
                }
            } else {
                // The dead-editor panel: restart, leave, or quit.
                match key.code {
                    Char('r') => {
                        let grid = ui::nvim_grid_rect(area, app.list_pct);
                        if session.restart(&app.repo, grid.width.max(12), grid.height.max(3)) {
                            app.status = "editor restarted".to_string();
                            app.nvim_reopen = true; // re-open the shown file next frame
                        } else {
                            app.status = "editor restart failed".to_string();
                        }
                    }
                    Char('q') => request_quit(app, session),
                    Tab => app.toggle_focus(),
                    _ => {}
                }
            }
            return Ok(());
        }
        match (key.code, ctrl) {
            (Char('u'), true) => app.move_cursor(-HALF_PAGE)?,
            (Char('d'), true) => app.move_cursor(HALF_PAGE)?,
            (Char('q'), _) => request_quit(app, session),
            (Char('r'), _) => app.reload()?,
            (Char('1'), _) => app.set_tab(crate::app::Tab::Changes)?,
            (Char('2'), _) => app.set_tab(crate::app::Tab::AllFiles)?,
            (Char('3'), _) => app.set_tab(crate::app::Tab::Pr)?,
            (Tab, _) => app.toggle_focus(),
            (Char('j') | Down, _) => app.move_cursor(1)?,
            (Char('k') | Up, _) => app.move_cursor(-1)?,
            (PageDown, _) => app.move_cursor(PAGE)?,
            (PageUp, _) => app.move_cursor(-PAGE)?,
            (Enter, _) if app.on_folder() => app.toggle_dir_children(),
            (Right, _) if app.on_folder() => app.expand_dir(),
            (Left, _) if app.on_folder() => app.collapse_dir(),
            (Char('x'), _) => app.expand_changes(),
            (Char(']'), _) => app.resize_list(4),
            (Char('['), _) => app.resize_list(-4),
            (Char('b'), false) => app.set_scope(Scope::Branch)?,
            (Char('t'), false) => app.set_scope(Scope::LastTurn)?,
            (Char('C'), false) => app.enter_commit_scope()?,
            (Char(' '), _) => app.review_advance(),
            (Char('+'), _) => app.send_path_to_agent(),
            // One send path, one store: comments live in reviewr.nvim, so `s` and `l` drive
            // the editor's commands; outcomes paint in its message area.
            (Char('s' | 'S'), _) => {
                if session.alive() {
                    session.run_command("ReviewrSend");
                } else {
                    app.status = "editor not running".to_string();
                }
            }
            (Char('l'), _) => {
                if session.alive() {
                    session.run_command("ReviewrList");
                    app.focus = Focus::Diff; // the quickfix lives in nvim; put the keys there
                }
            }
            (Backspace, _) => app.request_delete(),
            (Char('/'), false) => app.slash(),
            (Char('?'), _) => app.open_help(),
            (Esc, _) => {
                if app.filter.is_empty() {
                    app.clear_selection();
                } else {
                    app.clear_filter();
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match (key.code, ctrl) {
        // ctrl combos first, so they win over the plain `u`/`d` bindings below. Half-page
        // keys move the focused pane's cursor (the view follows), like `j`/`k`.
        (Char('u'), true) => app.move_cursor(-HALF_PAGE)?,
        (Char('d'), true) => app.move_cursor(HALF_PAGE)?,
        (Char('q'), _) => app.should_quit = true,
        // `r` resolves the comment under the diff cursor; with none there it reloads.
        (Char('r'), false) if app.focus == Focus::Diff && app.comment_under_cursor().is_some() => {
            app.resolve_comment();
        }
        (Char('r'), _) => app.reload()?,
        // `1` / `2` / `3` switch tabs (provisional; the keymap is an Open Decision in tui.md).
        (Char('1'), _) => app.set_tab(crate::app::Tab::Changes)?,
        (Char('2'), _) => app.set_tab(crate::app::Tab::AllFiles)?,
        (Char('3'), _) => app.set_tab(crate::app::Tab::Pr)?,
        (Tab, _) => app.toggle_focus(),
        (Char('j') | Down, _) => app.move_cursor(1)?,
        (Char('k') | Up, _) => app.move_cursor(-1)?,
        // Page keys move the focused pane's cursor.
        (PageDown, _) => app.move_cursor(PAGE)?,
        (PageUp, _) => app.move_cursor(-PAGE)?,
        (Char('w'), _) => app.toggle_wrap(),
        // `]` widens the file list, `[` narrows it (widening the diff).
        (Char(']'), _) => app.resize_list(4),
        (Char('['), _) => app.resize_list(-4),
        // `←`/`→` expand/collapse the collapsible under the cursor — a directory in the file
        // list, a fold in the diff (expand-only); otherwise they scroll the diff sideways
        // (`scroll_h` is a no-op while wrapping, so it only acts when h-scroll is meaningful).
        (Enter, _) if app.on_folder() => app.toggle_dir_children(),
        (Right, _) if app.on_folder() => app.expand_dir(),
        (Left, _) if app.on_folder() => app.collapse_dir(),
        (Right, _) if app.on_fold() => {
            let heights = ui::diff_row_heights(app, area);
            app.expand_fold(&heights, ui::diff_viewport_height(area, app.list_pct));
        }
        (Right, _) => app.scroll_h(8),
        (Left, _) => app.scroll_h(-8),
        (Char('b'), false) => app.set_scope(Scope::Branch)?,
        (Char('t'), false) => app.set_scope(Scope::LastTurn)?,
        (Char('C'), false) => app.enter_commit_scope()?,
        (Char('v'), _) => app.toggle_select(),
        (Char('c'), _) => app.start_comment(),
        (Char('e'), _) if app.focus == Focus::Diff => {
            if app.comment_under_cursor().is_some() {
                app.start_edit();
            } else {
                app.request_editor();
            }
        }
        (Char('d'), false) if app.focus == Focus::Diff => app.delete_comment(),
        (Char('s' | 'S'), _) => app.export(&Agent),
        (Char('y' | 'Y'), _) => app.export(&Clipboard),
        (Char('n'), _) => app.jump_comment(1),
        (Char('N'), _) => app.jump_comment(-1),
        (Char('l'), _) => app.open_list(),
        (Char('p'), false) => app.open_preview(),
        (Char('+'), _) => app.send_path_to_agent(),
        (Char(' '), _) => app.review_advance(),
        // `x` expands every folder containing a change; press again to collapse back.
        (Char('x'), _) => app.expand_changes(),
        (Char('?'), _) => app.open_help(),
        (Backspace, _) => app.request_delete(),
        (Char('/'), false) => app.slash(),
        (Esc, _) => {
            if app.filter.is_empty() {
                app.clear_selection();
            } else {
                app.clear_filter();
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_mouse(
    app: &mut App,
    session: &mut NvimSession,
    m: MouseEvent,
    area: Rect,
    heights: &[usize],
) -> Result<()> {
    // The comment composer captures the screen and is keyboard-driven, so the mouse is inert
    // while it is open — otherwise clicks and the wheel would drive the panes drawn underneath.
    if app.composing() {
        return Ok(());
    }
    // The comments-list overlay: click a row to jump to its code, click outside to close, wheel
    // to move the selection.
    if app.mode == Mode::List {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match ui::hit_comments_list(area, app, m.column, m.row) {
                    Some(i) => app.open_comment(i),
                    None if !ui::in_picker_popup(area, m.column, m.row) => app.close_list(),
                    None => {}
                }
            }
            MouseEventKind::ScrollDown => app.list_move(3),
            MouseEventKind::ScrollUp => app.list_move(-3),
            _ => {}
        }
        return Ok(());
    }
    if app.mode == Mode::Help {
        match m.kind {
            MouseEventKind::ScrollDown => app.help_scroll_by(3),
            MouseEventKind::ScrollUp => app.help_scroll_by(-3),
            _ => {}
        }
        return Ok(());
    }
    if app.mode == Mode::ConfirmDelete || app.mode == Mode::ConfirmQuit {
        return Ok(()); // keyboard-only confirmations
    }
    if app.mode == Mode::CommitPick {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match ui::hit_commit_pick(area, app, m.column, m.row) {
                    Some(i) => app.pick_commit(i)?,
                    None => app.close_commit_picker(),
                }
            }
            MouseEventKind::ScrollDown => app.commit_move(3),
            MouseEventKind::ScrollUp => app.commit_move(-3),
            _ => {}
        }
        return Ok(());
    }
    if app.mode == Mode::BranchPick {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                match ui::hit_branch_pick(area, app, m.column, m.row) {
                    Some(i) => app.pick_branch(i)?,
                    None if !ui::in_picker_popup(area, m.column, m.row) => {
                        app.close_branch_picker();
                    }
                    None => {}
                }
            }
            MouseEventKind::ScrollDown => app.branch_move(3),
            MouseEventKind::ScrollUp => app.branch_move(-3),
            _ => {}
        }
        return Ok(());
    }
    if app.mode == Mode::Preview {
        match m.kind {
            MouseEventKind::ScrollDown => app.preview_scroll_by(3),
            MouseEventKind::ScrollUp => app.preview_scroll_by(-3),
            _ => {}
        }
        return Ok(());
    }
    // The read-only PR tab: click a tab or the open button, click a row to read it, wheel the
    // navigator (right) to move, wheel the read pane (left) to scroll.
    if app.tab == crate::app::Tab::Pr {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(ui::HeaderHit::Tab(tab)) = ui::hit_header(area, app, m.column, m.row) {
                    app.set_tab(tab)?;
                } else if ui::hit_pr_open(area, app, m.column, m.row) {
                    app.pr_open();
                } else if let Some(i) = ui::pr_nav_hit(area, app, m.column, m.row) {
                    app.pr_select(i);
                }
            }
            MouseEventKind::ScrollDown
                if ui::in_files_pane(area, app.list_pct, m.column, m.row) =>
            {
                app.pr_move(3);
            }
            MouseEventKind::ScrollUp if ui::in_files_pane(area, app.list_pct, m.column, m.row) => {
                app.pr_move(-3);
            }
            MouseEventKind::ScrollDown => app.pr_scroll_read(3),
            MouseEventKind::ScrollUp => app.pr_scroll_read(-3),
            _ => {}
        }
        return Ok(());
    }
    // Embedded-nvim editor mode: the grid rect swallows clicks/drags/wheel as nvim mouse input
    // (drag-select, wheel scroll and prompts all behave nvim-natively); the file list, header
    // and divider keep their exact default behavior.
    if app.editor_nvim && app.tab.is_file_tab() {
        let grid = ui::nvim_grid_rect(area, app.list_pct);
        let rel = |col: u16, row: u16| {
            let right = grid.x + grid.width.saturating_sub(1);
            let bottom = grid.y + grid.height.saturating_sub(1);
            (row.clamp(grid.y, bottom) - grid.y, col.clamp(grid.x, right) - grid.x)
        };
        let mods = nvim_keys::mouse_modifier(m.modifiers);
        let in_grid = ui::in_nvim_grid(area, app.list_pct, m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if ui::hit_divider(area, app.list_pct, m.column, m.row) {
                    app.resizing = true;
                } else if let Some(hit) = ui::hit_header(area, app, m.column, m.row) {
                    match hit {
                        ui::HeaderHit::Tab(tab) => app.set_tab(tab)?,
                        ui::HeaderHit::Scope => app.set_scope(app.scope.cycle())?,
                        ui::HeaderHit::Base => app.open_branch_picker(),
                        ui::HeaderHit::Commit => app.open_commit_picker(),
                        // One send path: comments live in reviewr.nvim.
                        ui::HeaderHit::Send => {
                            if session.alive() {
                                session.run_command("ReviewrSend");
                            } else {
                                app.status = "editor not running".to_string();
                            }
                        }
                    }
                } else if let Some(i) = ui::hit_file(
                    area,
                    app.list_pct,
                    m.column,
                    m.row,
                    app.file_rows.len(),
                    app.file_scroll,
                ) {
                    if !(ui::on_file_marker(area, app.list_pct, m.column, m.row)
                        && app.stage_toggle(i))
                    {
                        app.select_file(i)?;
                    }
                } else if in_grid {
                    app.focus = Focus::Diff; // a click claims focus, like the old diff pane
                    session.mouse_down = true;
                    let (row, col) = rel(m.column, m.row);
                    session.feed_mouse("left", "press", &mods, row, col);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if app.resizing {
                    let body = ui::body_rect(area);
                    app.drag_divider(body.width, m.column.saturating_sub(body.x));
                } else if session.mouse_down {
                    // Clamp so a drag-select that leaves the pane keeps extending.
                    let (row, col) = rel(m.column, m.row);
                    session.feed_mouse("left", "drag", &mods, row, col);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                app.resizing = false;
                if session.mouse_down {
                    session.mouse_down = false;
                    let (row, col) = rel(m.column, m.row);
                    session.feed_mouse("left", "release", &mods, row, col);
                }
            }
            MouseEventKind::Down(MouseButton::Right) if in_grid => {
                let (row, col) = rel(m.column, m.row);
                session.feed_mouse("right", "press", &mods, row, col);
            }
            MouseEventKind::Up(MouseButton::Right) if in_grid => {
                let (row, col) = rel(m.column, m.row);
                session.feed_mouse("right", "release", &mods, row, col);
            }
            MouseEventKind::ScrollDown
                if ui::in_files_pane(area, app.list_pct, m.column, m.row) =>
            {
                app.wheel_files(3);
            }
            MouseEventKind::ScrollUp if ui::in_files_pane(area, app.list_pct, m.column, m.row) => {
                app.wheel_files(-3);
            }
            MouseEventKind::ScrollDown if in_grid => {
                let (row, col) = rel(m.column, m.row);
                session.feed_mouse("wheel", "down", &mods, row, col);
            }
            MouseEventKind::ScrollUp if in_grid => {
                let (row, col) = rel(m.column, m.row);
                session.feed_mouse("wheel", "up", &mods, row, col);
            }
            _ => {}
        }
        return Ok(());
    }

    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The divider is checked first: a grab there starts a resize, not a selection.
            if ui::hit_divider(area, app.list_pct, m.column, m.row) {
                app.resizing = true;
            } else if let Some(hit) = ui::hit_header(area, app, m.column, m.row) {
                match hit {
                    ui::HeaderHit::Tab(tab) => app.set_tab(tab)?,
                    ui::HeaderHit::Scope => app.set_scope(app.scope.cycle())?,
                    ui::HeaderHit::Base => app.open_branch_picker(),
                    ui::HeaderHit::Commit => app.open_commit_picker(),
                    ui::HeaderHit::Send => app.export(&Agent),
                }
            } else if let Some(i) = ui::hit_file(
                area,
                app.list_pct,
                m.column,
                m.row,
                app.file_rows.len(),
                app.file_scroll,
            ) {
                // A click on the change marker toggles staging; anywhere else opens the file.
                if !(ui::on_file_marker(area, app.list_pct, m.column, m.row) && app.stage_toggle(i))
                {
                    app.select_file(i)?;
                }
            } else if let Some(i) =
                ui::hit_diff(area, app.list_pct, m.column, m.row, heights, app.diff_scroll)
            {
                app.focus = Focus::Diff;
                app.diff_cursor = i;
                app.select_anchor = None;
                // A click on a fold marker expands it, keeping the viewport still.
                app.expand_fold(heights, ui::diff_viewport_height(area, app.list_pct));
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.resizing {
                let body = ui::body_rect(area);
                app.drag_divider(body.width, m.column.saturating_sub(body.x));
            } else if let Some(i) =
                ui::hit_diff(area, app.list_pct, m.column, m.row, heights, app.diff_scroll)
            {
                app.drag_select_to(i);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => app.resizing = false,
        // The wheel scrolls the viewport of whichever pane it is over — never the cursor, so
        // a comment is never anchored to a wheeled-past line. Horizontal scroll is
        // keyboard-only (`←`/`→`), since multiplexers don't reliably deliver h-wheel events.
        MouseEventKind::ScrollDown if ui::in_files_pane(area, app.list_pct, m.column, m.row) => {
            app.wheel_files(3);
        }
        MouseEventKind::ScrollUp if ui::in_files_pane(area, app.list_pct, m.column, m.row) => {
            app.wheel_files(-3);
        }
        MouseEventKind::ScrollDown => app.wheel_diff(3),
        MouseEventKind::ScrollUp => app.wheel_diff(-3),
        _ => {}
    }
    Ok(())
}
