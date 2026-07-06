//! Embedded-nvim engine: spawns `nvim --embed` as a child process, attaches as a linegrid UI
//! host over msgpack-RPC (stdin/stdout — no second pane, no socket), and exposes the cell grid
//! plus input/command primitives to the reviewer's event loop. See `specs/herdr-host.md`.
//!
//! Threading: one reader thread decodes nvim's stream and applies redraw events to a private
//! working grid, copying it into the shared `front` grid on every `flush` (so a paint never
//! sees a half-applied batch). The event loop polls [`Nvim::needs_redraw`] — there is no waker,
//! so the loop caps its `event::poll` timeout to [`FRAME_POLL`] while the engine runs
//! (mirroring the PR-fetch in-flight cap in `lib.rs`).
//!
//! Invariants: this module never panics (no unwraps on protocol data or locks); anything that
//! can prompt inside nvim (`:confirm edit`, `ReviewrSend`) is sent fire-and-forget — a blocking
//! request would deadlock on a prompt that only our forwarded keys can answer; a request
//! `Timeout` means "nvim is busy", never death; only the reader marks the engine dead.

pub mod grid;
pub(crate) mod rpc;

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use rmpv::Value;

pub use grid::{Cell, CellText, CursorShape, Grid, HlAttr, Rgb};
pub use rpc::RpcFailure;

/// Cap for `event::poll` while the engine runs (~30fps repaint latency at negligible CPU).
pub const FRAME_POLL: Duration = Duration::from_millis(33);

const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// Grace between asking nvim to exit (or dropping stdin) and killing it.
const EXIT_WAIT: Duration = Duration::from_millis(500);
const EXIT_POLL_STEP: Duration = Duration::from_millis(10);

/// Whether `nvim` is on the `PATH` — the deciding factor for whether nvim-editor mode can run.
#[must_use]
pub fn nvim_present() -> bool {
    Command::new("nvim")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The bundled `reviewr.nvim` directory: `$HERDR_PLUGIN_ROOT/nvim` in a herdr install (managed
/// or `plugin link`), else `<exe_dir>/../nvim` for a plain `cargo` build. `None` when neither
/// exists — the editor then runs without the review plugin rather than failing.
#[must_use]
pub fn plugin_nvim_dir() -> Option<PathBuf> {
    let from_root = std::env::var_os("HERDR_PLUGIN_ROOT").map(|r| PathBuf::from(r).join("nvim"));
    let from_exe =
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("..").join("nvim")));
    [from_root, from_exe].into_iter().flatten().find(|p| p.is_dir())
}

/// Escape a path for use as a Vim `runtimepath` value: `runtimepath` is comma-separated and Vim
/// unescapes backslashes, so a literal `\`, `,`, or space in the path must be backslash-escaped
/// or nvim would split the entry or drop characters.
fn escape_rtp(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '\\' | ',' | ' ') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Spawn options. Tests use `clean: true` (`--clean`: no user config — deterministic and fast);
/// production uses the default so the user's own keymaps/colorscheme/plugins load.
#[derive(Clone, Debug, Default)]
pub struct StartOpts {
    pub clean: bool,
    /// Bundled reviewr.nvim dir to prepend to the runtimepath; production passes
    /// [`plugin_nvim_dir`]'s result.
    pub rtp: Option<PathBuf>,
}

/// The `nvim --embed` argv (pure, unit-testable): env is inherited (`HERDR_*` must reach
/// `reviewr.nvim` for agent send), cwd is the repo, stdio piped.
fn spawn_command(repo: &Path, opts: &StartOpts) -> Command {
    let mut cmd = Command::new("nvim");
    cmd.arg("--embed");
    if opts.clean {
        cmd.arg("--clean");
    }
    if let Some(rtp) = &opts.rtp {
        cmd.arg("--cmd")
            .arg(format!("set runtimepath^={}", escape_rtp(&rtp.display().to_string())));
    }
    cmd.current_dir(repo).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    cmd
}

/// Double single quotes for a vimscript `'…'` string literal.
fn sq(s: &str) -> String {
    s.replace('\'', "''")
}

/// The `:confirm edit` command payload for `abs`: `fnameescape` (computed inside nvim) handles
/// vim-special characters, so only single quotes need doubling for the outer vimscript string;
/// `stopinsert` first normalizes insert mode so the Ex command lands cleanly. The payload also
/// publishes the reviewer's diff `base` ref for reviewr.nvim's inline diff (the reviewer's
/// scope decides it — branch merge-base, turn baseline, picked commit) and runs a silenced
/// `checktime` so files the agent rewrote reload instead of raising "file changed" prompts.
/// With `focus_changes` (the Changes tab) the open lands in the focused view — unchanged
/// regions folded, cursor on the first change; otherwise any focused-view folds are dropped.
fn open_file_command(abs: &Path, base: &str, focus_changes: bool) -> String {
    let path = sq(&abs.to_string_lossy());
    let tail = if focus_changes { "focus" } else { "unfocus" };
    format!(
        "let g:reviewr_base='{}' | silent! checktime | stopinsert \
         | exe 'confirm edit ' . fnameescape('{path}') | lua require('reviewr.diff').{tail}()",
        sq(base)
    )
}

/// The scope-changed payload (same file stays open): publish the new base and re-diff the
/// current buffer in place, refreshing any active review folds.
fn rebase_command(base: &str) -> String {
    format!(
        "let g:reviewr_base='{}' | silent! checktime | lua require('reviewr.diff').rebase()",
        sq(base)
    )
}

struct RpcResponse {
    msgid: u64,
    result: Result<Value, RpcFailure>,
}

/// Recover a poisoned lock: the reader thread is written to never panic, and even if it did,
/// the grid/dead state stays structurally valid — so the reviewer must not cascade the panic.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A running embedded nvim. Dropping it reaps the child unconditionally (stdin EOF → nvim
/// exits; then kill as a backstop) — an orphaned nvim is structurally impossible.
#[derive(Debug)]
pub struct Nvim {
    child: Child,
    writer: Arc<Mutex<Option<ChildStdin>>>,
    front: Arc<Mutex<Grid>>,
    dirty: Arc<AtomicBool>,
    dead: Arc<Mutex<Option<String>>>,
    resp_rx: Receiver<RpcResponse>,
    next_msgid: u64,
    reader: Option<JoinHandle<()>>,
    /// Last (cols, rows) sent, for resize dedup.
    size: (u16, u16),
}

impl Nvim {
    /// Spawn `nvim --embed` in `repo`, start the reader thread, and attach the UI at
    /// `cols`×`rows` with `{ext_linegrid: true, rgb: true}`. `--embed` pauses startup-file
    /// sourcing until a UI attaches, so the user config sees the real dimensions. On attach
    /// failure the child is killed and reaped before returning.
    pub fn start(repo: &Path, cols: u16, rows: u16, opts: &StartOpts) -> anyhow::Result<Self> {
        let mut child = spawn_command(repo, opts).spawn().context("spawning nvim --embed")?;
        let stdin = child.stdin.take().context("nvim stdin unavailable")?;
        let stdout = child.stdout.take().context("nvim stdout unavailable")?;

        let writer = Arc::new(Mutex::new(Some(stdin)));
        let front = Arc::new(Mutex::new(Grid::new(cols, rows)));
        let dirty = Arc::new(AtomicBool::new(false));
        let dead: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (resp_tx, resp_rx) = channel::<RpcResponse>();

        let reader = std::thread::spawn({
            let (writer, front, dirty, dead) =
                (writer.clone(), front.clone(), dirty.clone(), dead.clone());
            let mut work = Grid::new(cols, rows);
            move || reader_loop(stdout, &writer, &front, &dirty, &dead, &resp_tx, &mut work)
        });

        let mut nvim = Self {
            child,
            writer,
            front,
            dirty,
            dead,
            resp_rx,
            next_msgid: 1,
            reader: Some(reader),
            size: (cols, rows),
        };
        let attach = nvim.request_with_timeout(
            "nvim_ui_attach",
            vec![
                Value::from(cols),
                Value::from(rows),
                Value::Map(vec![
                    (Value::from("ext_linegrid"), Value::from(true)),
                    (Value::from("rgb"), Value::from(true)),
                ]),
            ],
            ATTACH_TIMEOUT,
        );
        if let Err(e) = attach {
            nvim.reap();
            anyhow::bail!("attaching the nvim UI: {e}");
        }
        logln!("nvim --embed attached {cols}x{rows} (clean={})", opts.clean);
        Ok(nvim)
    }

    /// False once the reader saw EOF/corruption or shutdown ran.
    #[must_use]
    pub fn is_running(&self) -> bool {
        lock(&self.dead).is_none()
    }

    /// Why nvim is gone; `None` while healthy. Respawning is the caller's decision: drop this
    /// handle and `start()` again.
    #[must_use]
    pub fn died(&self) -> Option<String> {
        lock(&self.dead).clone()
    }

    /// The last flushed frame, for the blit. The lock is held only while painting the pane;
    /// the reader contends only for the microseconds of a flush copy.
    pub fn grid(&self) -> MutexGuard<'_, Grid> {
        lock(&self.front)
    }

    /// Check-and-clear the redraw flag (set on every flush, and once on death so the UI
    /// repaints the dead state).
    #[must_use]
    pub fn needs_redraw(&self) -> bool {
        self.dirty.swap(false, Ordering::SeqCst)
    }

    /// `nvim_ui_try_resize` (notification — the resulting `grid_resize` event is the ack).
    /// A no-op when the size is unchanged.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), RpcFailure> {
        if self.size == (cols, rows) {
            return Ok(());
        }
        self.size = (cols, rows);
        self.notify("nvim_ui_try_resize", vec![Value::from(cols), Value::from(rows)])
    }

    /// `nvim_input` (notification — never blocks; keys land even while nvim shows a modal
    /// prompt, which is exactly how the user answers it). `keys` is nvim notation.
    pub fn input(&self, keys: &str) -> Result<(), RpcFailure> {
        self.notify("nvim_input", vec![Value::from(keys)])
    }

    /// `nvim_paste` (notification): literal insert — no `<>` interpretation, mode-appropriate,
    /// one undo entry. Single chunk (`phase = -1`).
    pub fn paste(&self, text: &str) -> Result<(), RpcFailure> {
        self.notify("nvim_paste", vec![Value::from(text), Value::from(false), Value::from(-1i64)])
    }

    /// `nvim_input_mouse` (notification). `grid` is 0 (no multigrid); `row`/`col` are
    /// grid-relative. button: "left"/"right"/"middle"/"wheel"; action: "press"/"drag"/
    /// "release" or "up"/"down"/"left"/"right" for wheel; modifier: "C"/"S"/"A" concat.
    pub fn input_mouse(
        &self,
        button: &str,
        action: &str,
        modifier: &str,
        row: u16,
        col: u16,
    ) -> Result<(), RpcFailure> {
        self.notify(
            "nvim_input_mouse",
            vec![
                Value::from(button),
                Value::from(action),
                Value::from(modifier),
                Value::from(0),
                Value::from(row),
                Value::from(col),
            ],
        )
    }

    /// `nvim_command` as a request, surfacing nvim's error message. **Never use this for a
    /// command that can prompt** (`:confirm …`): the prompt blocks the response while this
    /// thread is the one that must forward the user's answer — use [`Self::command_fire`].
    pub fn command(&mut self, cmd: &str) -> Result<(), RpcFailure> {
        self.request("nvim_command", vec![Value::from(cmd)]).map(|_| ())
    }

    /// `nvim_command` as a notification: fire-and-forget; errors surface inside nvim's own
    /// message area, which the user sees in the grid.
    pub fn command_fire(&self, cmd: &str) -> Result<(), RpcFailure> {
        self.notify("nvim_command", vec![Value::from(cmd)])
    }

    /// `nvim_eval` as a request. A `Timeout` means nvim is busy (e.g. a prompt is up) — treat
    /// it as "unknown", never as death.
    pub fn eval(&mut self, expr: &str) -> Result<Value, RpcFailure> {
        self.request("nvim_eval", vec![Value::from(expr)])
    }

    /// Show `abs` in the editor, prompting on unsaved changes (`:confirm edit`, fire-and-forget
    /// by design — the prompt renders in the grid and forwarded keys answer it).
    pub fn open_file(&self, abs: &Path, base: &str, focus_changes: bool) -> Result<(), RpcFailure> {
        self.command_fire(&open_file_command(abs, base, focus_changes))
    }

    /// The reviewer's scope changed while the same file stays open: publish the new diff base
    /// and re-diff in place.
    pub fn rebase(&self, base: &str) -> Result<(), RpcFailure> {
        self.command_fire(&rebase_command(base))
    }

    /// Whether the colorscheme leaves `Normal` without a background (a "transparent" theme,
    /// e.g. catppuccin's `transparent_background`). In a terminal that means "show the
    /// terminal's background"; an embedded UI would render it black, so the blit substitutes
    /// the terminal default instead. Sampled after startup (requests queue behind config
    /// sourcing, so the user's colorscheme has applied by the time this answers).
    pub fn normal_bg_transparent(&mut self) -> Result<bool, RpcFailure> {
        self.eval("empty(synIDattr(hlID('Normal'), 'bg#'))").map(|v| v.as_i64().unwrap_or(0) != 0)
    }

    /// Stop the editor. `force`: `qa!` then reap — always succeeds. Non-force: `confirm qall`;
    /// if nvim is still alive after the grace (a save-prompt is showing), returns `Timeout` and
    /// leaves the pane interactive so the user can answer.
    pub fn shutdown(&mut self, force: bool) -> Result<(), RpcFailure> {
        let cmd = if force { "qa!" } else { "confirm qall" };
        let _ = self.command_fire(cmd);
        let deadline = Instant::now() + EXIT_WAIT;
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                self.mark_dead("shut down");
                self.reap();
                return Ok(());
            }
            std::thread::sleep(EXIT_POLL_STEP);
        }
        if force {
            self.mark_dead("shut down");
            self.reap();
            Ok(())
        } else {
            Err(RpcFailure::Timeout)
        }
    }

    fn mark_dead(&self, reason: &str) {
        let mut dead = lock(&self.dead);
        if dead.is_none() {
            *dead = Some(reason.to_string());
        }
    }

    fn notify(&self, method: &str, params: Vec<Value>) -> Result<(), RpcFailure> {
        if !self.is_running() {
            return Err(RpcFailure::Dead);
        }
        let mut writer = lock(&self.writer);
        let Some(w) = writer.as_mut() else { return Err(RpcFailure::Dead) };
        rpc::write_notification(w, method, params).map_err(|e| RpcFailure::Io(e.to_string()))
    }

    fn request(&mut self, method: &str, params: Vec<Value>) -> Result<Value, RpcFailure> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
    }

    /// Single-outstanding request: `&mut self` serializes callers; the reader routes every
    /// response into `resp_rx`. A response with an older msgid is a stale reply from a
    /// previously timed-out request and is dropped.
    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Vec<Value>,
        timeout: Duration,
    ) -> Result<Value, RpcFailure> {
        if !self.is_running() {
            return Err(RpcFailure::Dead);
        }
        let msgid = self.next_msgid;
        self.next_msgid += 1;
        {
            let mut writer = lock(&self.writer);
            let Some(w) = writer.as_mut() else { return Err(RpcFailure::Dead) };
            rpc::write_request(w, msgid, method, params)
                .map_err(|e| RpcFailure::Io(e.to_string()))?;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(RpcFailure::Timeout);
            }
            match self.resp_rx.recv_timeout(remaining) {
                Ok(resp) if resp.msgid == msgid => return resp.result,
                Ok(_) => {} // stale reply to a timed-out earlier request
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Err(RpcFailure::Timeout),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(RpcFailure::Dead);
                }
            }
        }
    }

    /// Reap unconditionally: drop stdin (EOF → nvim exits — verified), grace-poll, then kill
    /// as a backstop, and join the reader (prompt: the child's death closes its stdout).
    fn reap(&mut self) {
        lock(&self.writer).take(); // stdin EOF
        let deadline = Instant::now() + EXIT_WAIT;
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(EXIT_POLL_STEP);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Nvim {
    fn drop(&mut self) {
        self.reap();
    }
}

/// The reader thread: decode messages until the channel ends; apply redraw batches to the
/// private working grid, publishing to `front` (+ dirty) on each flush; route responses;
/// answer unexpected nvim→host requests with an error so nvim never blocks on us.
fn reader_loop(
    stdout: std::process::ChildStdout,
    writer: &Mutex<Option<ChildStdin>>,
    front: &Mutex<Grid>,
    dirty: &AtomicBool,
    dead: &Mutex<Option<String>>,
    resp_tx: &Sender<RpcResponse>,
    work: &mut Grid,
) {
    let mut reader = BufReader::new(stdout);
    let mut mode_shapes: Vec<CursorShape> = Vec::new();
    let reason = loop {
        match rpc::read_msg(&mut reader) {
            Ok(rpc::RpcIn::Notification { method, params }) => {
                if method == "redraw" && grid::apply_redraw(work, &mut mode_shapes, &params).flush {
                    lock(front).copy_from(work);
                    dirty.store(true, Ordering::SeqCst);
                }
            }
            Ok(rpc::RpcIn::Response { msgid, error, result }) => {
                let result = if error == Value::Nil {
                    Ok(result)
                } else {
                    Err(RpcFailure::Nvim(rpc::error_message(&error)))
                };
                let _ = resp_tx.send(RpcResponse { msgid, result });
            }
            Ok(rpc::RpcIn::Request { msgid }) => {
                let mut w = lock(writer);
                if let Some(w) = w.as_mut() {
                    let _ = rpc::write_error_response(w, msgid, "reviewr hosts no nvim requests");
                }
            }
            Err(rpc::ReadError::Eof) => break "nvim exited".to_string(),
            Err(rpc::ReadError::Corrupt(e)) => break format!("nvim protocol corrupt: {e}"),
        }
    };
    let mut dead = lock(dead);
    if dead.is_none() {
        *dead = Some(reason);
    }
    drop(dead);
    dirty.store(true, Ordering::SeqCst); // one repaint to show the dead state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &Command) -> (String, Vec<String>) {
        (
            cmd.get_program().to_string_lossy().into_owned(),
            cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect(),
        )
    }

    #[test]
    fn spawn_command_embeds_with_rtp_in_the_repo() {
        let opts = StartOpts { clean: true, rtp: Some(PathBuf::from("/plug in/nvim")) };
        let cmd = spawn_command(Path::new("/repo"), &opts);
        let (program, args) = argv(&cmd);
        assert_eq!(program, "nvim");
        assert_eq!(
            args,
            vec![
                "--embed".to_string(),
                "--clean".to_string(),
                "--cmd".to_string(),
                "set runtimepath^=/plug\\ in/nvim".to_string(),
            ]
        );
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/repo")));
    }

    #[test]
    fn spawn_command_defaults_load_the_user_config() {
        let cmd = spawn_command(Path::new("/repo"), &StartOpts::default());
        let (_, args) = argv(&cmd);
        assert_eq!(args, vec!["--embed".to_string()]); // no --clean, no rtp
    }

    #[test]
    fn rtp_escapes_spaces_commas_and_backslashes() {
        assert_eq!(escape_rtp("/a b,c"), "/a\\ b\\,c");
        assert_eq!(escape_rtp("/plain/path"), "/plain/path");
    }

    #[test]
    fn open_file_command_confirm_edits_via_fnameescape() {
        assert_eq!(
            open_file_command(Path::new("/repo/src/a b.rs"), "abc123", true),
            "let g:reviewr_base='abc123' | silent! checktime | stopinsert \
             | exe 'confirm edit ' . fnameescape('/repo/src/a b.rs') \
             | lua require('reviewr.diff').focus()"
        );
        // Single quotes double for the vimscript string literal; a non-Changes open drops the
        // focused view instead of entering it.
        assert_eq!(
            open_file_command(Path::new("/repo/o'brien.rs"), "HEAD", false),
            "let g:reviewr_base='HEAD' | silent! checktime | stopinsert \
             | exe 'confirm edit ' . fnameescape('/repo/o''brien.rs') \
             | lua require('reviewr.diff').unfocus()"
        );
    }

    #[test]
    fn rebase_command_publishes_the_base_and_rediffs() {
        assert_eq!(
            rebase_command("deadbeef"),
            "let g:reviewr_base='deadbeef' | silent! checktime \
             | lua require('reviewr.diff').rebase()"
        );
    }
}
