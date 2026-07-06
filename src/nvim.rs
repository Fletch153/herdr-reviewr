//! Companion-nvim editor mode (`editor = "nvim"`). Instead of the built-in read-only diff pane,
//! the reviewer runs as a pure file-list navigator and drives a *separate* herdr pane running
//! nvim: selecting a file opens it there, prompting on unsaved changes. See `specs/herdr-host.md`.
//!
//! Mechanics (no herdr keystroke API exists for a non-agent pane): the editor pane launches
//! `nvim --listen <sock>` (see `herdr/editor.sh`); the reviewer opens files with
//! `nvim --server <sock> --remote-send ':confirm edit …'`. Both derive the same socket path from
//! the shared plugin state dir + workspace id, so they rendezvous without any handshake.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::app::App;
use crate::herdr::{herdr_bin, ok_or_api_error};

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

/// The plugin id herdr knows this plugin by (for `plugin pane open --plugin`), defaulting to the
/// published id when the host doesn't set it (standalone/tests).
fn plugin_id() -> String {
    std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "persiyanov.reviewr".to_string())
}

/// The rendezvous socket, byte-identical to `herdr/editor.sh`'s: `<state>/nvim-<workspace>.sock`,
/// with the same fallbacks so the reviewer and the editor pane always agree.
#[must_use]
pub fn editor_socket() -> PathBuf {
    let state = std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .or_else(|| std::env::var_os("TMPDIR"))
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let ws = std::env::var("HERDR_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    state.join(format!("nvim-{ws}.sock"))
}

/// The `herdr plugin pane open` argv that spawns the nvim editor beside the reviewer sidebar
/// (`target_pane`), to its left so the layout reads `[editor][file list]`. Pure so it is unit-
/// testable; the side effects (running it, parsing the pane id) live in [`open_editor_pane`].
#[must_use]
pub fn editor_open_command(target_pane: &str, repo: &Path) -> Command {
    let mut cmd = Command::new(herdr_bin());
    cmd.args(["plugin", "pane", "open", "--plugin", &plugin_id(), "--entrypoint", "editor"])
        .args(["--placement", "split", "--target-pane", target_pane, "--direction", "left"])
        .arg("--cwd")
        .arg(repo)
        .arg("--no-focus");
    cmd
}

/// The `nvim --server <sock> --remote-send` argv that opens `abs` in the running editor,
/// prompting on unsaved changes (`:confirm edit`). `<C-\><C-N>` first leaves any mode (insert,
/// visual, a pending operator) so the Ex command lands cleanly; `fnameescape` — computed inside
/// nvim — handles spaces and vim-special characters in the path, so we only single-quote-escape
/// it for the outer vimscript string.
#[must_use]
pub fn remote_edit_command(sock: &Path, abs: &Path) -> Command {
    let path = abs.to_string_lossy().replace('\'', "''");
    let keys = format!("<C-\\><C-N>:exe 'confirm edit ' . fnameescape('{path}')<CR>");
    let mut cmd = Command::new("nvim");
    cmd.arg("--server").arg(sock).arg("--remote-send").arg(keys);
    cmd
}

/// Pull the created pane id out of a `plugin pane open` response, tolerating either the nested
/// `plugin_pane` shape herdr returns today or a flatter `pane` one.
fn parse_pane_id(stdout: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let r = v.get("result")?;
    r.pointer("/plugin_pane/pane/pane_id")
        .or_else(|| r.pointer("/pane/pane_id"))
        .and_then(|p| p.as_str())
        .map(str::to_owned)
}

/// Open the nvim editor pane beside this reviewer pane, returning its herdr pane id.
fn open_editor_pane(repo: &Path) -> Result<String> {
    let target = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .context("no HERDR_PANE_ID — the nvim editor needs a herdr host pane to split from")?;
    let out =
        editor_open_command(&target, repo).output().context("spawning herdr plugin pane open")?;
    if !out.status.success() {
        bail!("herdr plugin pane open failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let stdout = ok_or_api_error(String::from_utf8_lossy(&out.stdout).into_owned())?;
    parse_pane_id(&stdout).context("herdr plugin pane open returned no pane id")
}

/// Block until the editor's socket exists (nvim creates it only once it is accepting), up to
/// `budget`. A one-time wait on the first open; later opens find it already present.
fn wait_for_socket(sock: &Path, budget: Duration) {
    let start = Instant::now();
    while !sock.exists() && start.elapsed() < budget {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Send the remote `:confirm edit`; surfaces a clear error if nvim isn't reachable yet.
fn remote_edit(sock: &Path, abs: &Path) -> Result<()> {
    let out = remote_edit_command(sock, abs).output().context("spawning nvim --remote-send")?;
    if !out.status.success() {
        bail!("nvim --remote-send failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// The reviewer's handle to the companion nvim pane over the life of the session: opens it lazily
/// on the first file, then drives it to follow the selection. Errors degrade to a status message
/// rather than crashing the reviewer — a closed editor pane just gets re-opened on the next file.
#[derive(Debug, Default)]
pub struct EditorPane {
    /// The herdr pane id once opened; `None` until the first file is shown.
    pane: Option<String>,
    /// The repo-relative path last sent, so an unchanged selection doesn't re-drive nvim.
    last: Option<String>,
}

impl EditorPane {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the editor show the app's currently open file (`diff_path`). Called every frame in
    /// nvim mode: a no-op unless the selection changed, so it's cheap on idle frames.
    pub fn sync(&mut self, app: &mut App) {
        if !app.editor_nvim {
            return;
        }
        let Some(rel) = app.diff_path.clone() else { return };
        if self.last.as_deref() == Some(rel.as_str()) {
            return;
        }
        let sock = editor_socket();
        if self.pane.is_none() {
            match open_editor_pane(&app.repo) {
                Ok(id) => {
                    self.pane = Some(id);
                    wait_for_socket(&sock, Duration::from_secs(3));
                }
                Err(e) => {
                    app.status = format!("editor: {e}");
                    return;
                }
            }
        }
        let abs = app.repo.join(&rel);
        match remote_edit(&sock, &abs) {
            Ok(()) => self.last = Some(rel),
            Err(e) => app.status = format!("editor: {e}"),
        }
    }

    /// Close the editor pane on shutdown (best-effort); a vanished pane is fine to "close" again.
    pub fn close(&mut self) {
        if let Some(id) = self.pane.take() {
            let _ = Command::new(herdr_bin()).args(["plugin", "pane", "close", &id]).output();
        }
    }
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
    fn open_command_targets_the_editor_entrypoint_to_the_left() {
        let cmd = editor_open_command("wZ:p2", Path::new("/repo"));
        let (program, args) = argv(&cmd);
        assert!(program.ends_with("herdr"));
        assert_eq!(
            args,
            vec![
                "plugin",
                "pane",
                "open",
                "--plugin",
                "persiyanov.reviewr",
                "--entrypoint",
                "editor",
                "--placement",
                "split",
                "--target-pane",
                "wZ:p2",
                "--direction",
                "left",
                "--cwd",
                "/repo",
                "--no-focus",
            ]
        );
    }

    #[test]
    fn remote_edit_confirm_edits_via_fnameescape() {
        let cmd = remote_edit_command(Path::new("/s.sock"), Path::new("/repo/src/a b.rs"));
        let (program, args) = argv(&cmd);
        assert_eq!(program, "nvim");
        assert_eq!(
            args,
            vec![
                "--server".to_string(),
                "/s.sock".to_string(),
                "--remote-send".to_string(),
                "<C-\\><C-N>:exe 'confirm edit ' . fnameescape('/repo/src/a b.rs')<CR>".to_string(),
            ]
        );
    }

    #[test]
    fn remote_edit_doubles_single_quotes_in_the_path() {
        let cmd = remote_edit_command(Path::new("/s.sock"), Path::new("/repo/o'brien.rs"));
        let payload = cmd.get_args().last().unwrap().to_string_lossy().into_owned();
        assert!(payload.contains("fnameescape('/repo/o''brien.rs')"), "{payload}");
    }

    #[test]
    fn socket_follows_state_dir_and_workspace() {
        // The env is process-global; this test documents the shape rather than mutating it.
        let sock = editor_socket();
        assert!(sock.file_name().unwrap().to_string_lossy().starts_with("nvim-"));
        assert_eq!(sock.extension().unwrap(), "sock");
    }

    #[test]
    fn parses_the_nested_pane_id() {
        let json = r#"{"result":{"plugin_pane":{"pane":{"pane_id":"wZ:p9"}}}}"#;
        assert_eq!(parse_pane_id(json).as_deref(), Some("wZ:p9"));
    }

    #[test]
    fn parses_the_flat_pane_id_fallback() {
        let json = r#"{"result":{"pane":{"pane_id":"wZ:p7"}}}"#;
        assert_eq!(parse_pane_id(json).as_deref(), Some("wZ:p7"));
    }
}
