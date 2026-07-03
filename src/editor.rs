//! Launching the user's `$EDITOR` on the file under review.
//!
//! The diff pane is read-only; pressing `e` there hands the current file to a real editor so a
//! change can be made without leaving the review. The terminal hand-off (suspend the TUI, run the
//! editor on the inherited stdio, re-init) lives in [`crate::run`] because it owns the terminal;
//! this module only resolves which editor to run and builds its command line — kept terminal-free
//! and pure (no env read in [`command`]) so it is unit-testable.

use std::path::Path;
use std::process::{Command, ExitStatus};

/// The editor to launch: `$VISUAL`, then `$EDITOR`, then `vi` — the conventional unix precedence
/// (`VISUAL` is the full-screen editor, `EDITOR` the line-editor fallback).
fn editor_spec() -> String {
    pick(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok())
}

/// Choose between `$VISUAL` and `$EDITOR`, skipping blank values so an exported-but-empty var
/// doesn't win, and falling back to `vi`. Split from the env read so it's testable without the
/// (crate-forbidden) `unsafe` env mutation.
fn pick(visual: Option<String>, editor: Option<String>) -> String {
    let nonblank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    nonblank(visual).or_else(|| nonblank(editor)).unwrap_or_else(|| "vi".to_string())
}

/// Build the editor invocation `<program> [spec args…] [+<line>] <path>`, run from `repo`.
///
/// `spec` is split on whitespace so a configured `EDITOR="emacsclient -t"` or `"code -w"` keeps its
/// flags; the first token is the program. `+<line>` is the cursor-position convention understood by
/// the vi family, emacs, and nano — an editor that doesn't parse it simply opens at the top.
/// Pure (no env access) so tests can assert the argv without touching process env.
#[must_use]
pub fn command(spec: &str, repo: &Path, path: &Path, line: Option<u32>) -> Command {
    let mut parts = spec.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut cmd = Command::new(program);
    cmd.args(parts);
    cmd.current_dir(repo);
    if let Some(n) = line {
        cmd.arg(format!("+{n}"));
    }
    cmd.arg(path);
    cmd
}

/// Resolve `$EDITOR` and run it on `path` (at `line` when known), inheriting the terminal. Blocks
/// until the editor exits; the caller suspends/restores the TUI around this call.
pub fn open(repo: &Path, path: &Path, line: Option<u32>) -> std::io::Result<ExitStatus> {
    command(&editor_spec(), repo, path, line).status()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &Command) -> (String, Vec<String>) {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let args = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        (program, args)
    }

    #[test]
    fn builds_program_line_flag_and_path() {
        let cmd = command("nvim", Path::new("/repo"), Path::new("/repo/src/a.rs"), Some(42));
        let (program, args) = argv(&cmd);
        assert_eq!(program, "nvim");
        assert_eq!(args, vec!["+42".to_string(), "/repo/src/a.rs".to_string()]);
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/repo")));
    }

    #[test]
    fn keeps_configured_editor_flags_and_omits_line_when_none() {
        let cmd = command("code -w", Path::new("/repo"), Path::new("/repo/a.rs"), None);
        let (program, args) = argv(&cmd);
        assert_eq!(program, "code");
        // Flags preserved, no `+line`, path last.
        assert_eq!(args, vec!["-w".to_string(), "/repo/a.rs".to_string()]);
    }

    #[test]
    fn spec_precedence_visual_over_editor_over_vi() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(pick(None, None), "vi", "nothing set falls back to vi");
        assert_eq!(pick(None, s("nano")), "nano", "EDITOR used when VISUAL is unset");
        assert_eq!(pick(s("nvim"), s("nano")), "nvim", "VISUAL wins over EDITOR");
        assert_eq!(pick(s("  "), s("nano")), "nano", "a blank VISUAL is skipped");
        assert_eq!(pick(s(""), None), "vi", "a blank EDITOR falls back too");
    }
}
