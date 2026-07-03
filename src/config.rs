//! Configuration: command-line flags.
//!
//! See `specs/tui.md` and `specs/herdr-host.md`. Flags override defaults; the positional
//! argument (if any) is the repo path, else the current directory.

use std::path::PathBuf;
use std::time::Duration;

/// Resolved runtime configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub repo: PathBuf,
    pub poll: Duration,
    pub base: Option<String>,
    pub theme: Option<String>,
    /// `Some(false)` when `--wrap off` is passed; `None` keeps the default (wrap on).
    pub wrap: Option<bool>,
    /// `--icons on|off`; `None` falls back to the config file, then off (Nerd Font required).
    pub icons: Option<bool>,
}

impl Config {
    /// Parse `args` (the process arguments *after* argv\[0\]).
    ///
    /// Recognises `--poll <ms>` (min 200, default 2000), `--base <ref>`,
    /// `--theme <name>`, and `--wrap on|off`; the first non-flag token is the repo path.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut repo: Option<PathBuf> = None;
        let mut poll_ms: u64 = 2000;
        let mut base: Option<String> = None;
        let mut theme: Option<String> = None;
        let mut wrap: Option<bool> = None;
        let mut icons: Option<bool> = None;
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--poll" => {
                    if let Some(v) = it.next() {
                        poll_ms = v.parse().unwrap_or(poll_ms);
                    }
                }
                "--base" => base = it.next(),
                "--theme" => theme = it.next(),
                "--wrap" => wrap = it.next().map(|v| v != "off"),
                "--icons" => icons = it.next().map(|v| v != "off"),
                other if !other.starts_with('-') => repo = Some(PathBuf::from(other)),
                _ => {}
            }
        }
        let repo =
            repo.or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."));
        Self { repo, poll: Duration::from_millis(poll_ms.max(200)), base, theme, wrap, icons }
    }

    /// Parse from the real process arguments.
    pub fn from_env() -> Self {
        Self::parse(std::env::args().skip(1))
    }
}

/// The `theme` value from reviewr's config file (`$HERDR_PLUGIN_CONFIG_DIR/config.toml`),
/// read on each refresh. `None` when the dir is unset (standalone), the file is absent or
/// unparseable, or it has no `theme` key — the caller then falls back to the default
/// (`specs/theme.md`).
pub fn config_file_theme() -> Option<String> {
    config_key_in(std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")?, "theme")
}

/// The `base` value from reviewr's config file, read once at startup. `None` when the dir is
/// unset, the file is absent or unparseable, or it has no `base` key.
pub fn config_file_base() -> Option<String> {
    config_key_in(std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")?, "base")
}

/// The `icons` boolean from reviewr's config file, read once at startup. `None` when the dir is
/// unset, the file is absent or unparseable, or it has no `icons` key.
pub fn config_file_icons() -> Option<bool> {
    config_icons_in(std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")?)
}

/// A string key from `<dir>/config.toml`, or `None` if the file is absent, unparseable,
/// or lacks the key. Split from the env lookup so it is testable.
fn config_key_in(dir: impl AsRef<std::path::Path>, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.as_ref().join("config.toml")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table.get(key).and_then(toml::Value::as_str).map(str::to_owned)
}

/// The `icons` boolean from `<dir>/config.toml`, or `None` if absent/unparseable/missing.
/// Split from the env lookup so it is testable.
fn config_icons_in(dir: impl AsRef<std::path::Path>) -> Option<bool> {
    let text = std::fs::read_to_string(dir.as_ref().join("config.toml")).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table.get("icons").and_then(toml::Value::as_bool)
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::time::Duration;

    fn parse(args: &[&str]) -> Config {
        Config::parse(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn defaults_when_no_args() {
        let c = parse(&[]);
        assert_eq!(c.poll, Duration::from_secs(2));
        assert_eq!(c.base, None);
    }

    #[test]
    fn flags_and_positional_repo() {
        let c = parse(&["--poll", "500", "--base", "origin/dev", "/tmp/work"]);
        assert_eq!(c.poll, Duration::from_millis(500));
        assert_eq!(c.base.as_deref(), Some("origin/dev"));
        assert_eq!(c.repo.to_str(), Some("/tmp/work"));
    }

    #[test]
    fn poll_has_a_floor() {
        assert_eq!(parse(&["--poll", "10"]).poll, Duration::from_millis(200));
        assert_eq!(parse(&["--poll", "garbage"]).poll, Duration::from_secs(2));
    }

    #[test]
    fn reads_theme_from_config_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "theme = \"gruvbox\"\n").unwrap();
        assert_eq!(super::config_key_in(dir.path(), "theme"), Some("gruvbox".to_string()));
    }

    #[test]
    fn reads_base_from_config_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "base = \"origin/develop\"\n").unwrap();
        assert_eq!(super::config_key_in(dir.path(), "base"), Some("origin/develop".to_string()));
    }

    #[test]
    fn reads_icons_bool_from_config_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "icons = true\n").unwrap();
        assert_eq!(super::config_icons_in(dir.path()), Some(true));
    }

    #[test]
    fn icons_flag_parses_on_and_off() {
        assert_eq!(parse(&["--icons", "on"]).icons, Some(true));
        assert_eq!(parse(&["--icons", "off"]).icons, Some(false));
        assert_eq!(parse(&[]).icons, None);
    }

    #[test]
    fn missing_file_or_absent_key_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(super::config_key_in(dir.path(), "theme"), None);
        std::fs::write(dir.path().join("config.toml"), "poll = 500\n").unwrap();
        assert_eq!(super::config_key_in(dir.path(), "theme"), None);
    }
}
