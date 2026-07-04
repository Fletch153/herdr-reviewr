use std::path::Path;
use std::process::{Command, ExitStatus};

fn editor_spec() -> String {
    pick(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok())
}

fn pick(visual: Option<String>, editor: Option<String>) -> String {
    let nonblank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    nonblank(visual).or_else(|| nonblank(editor)).unwrap_or_else(|| "vi".to_string())
}

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
