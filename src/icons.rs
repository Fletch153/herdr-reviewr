use ratatui::style::Color;

use crate::theme::Palette;

#[derive(Clone, Copy)]
enum Hue {
    Peach,
    Yellow,
    Mauve,
    Lavender,
    Green,
    Red,
    Neutral,
}

fn tint(hue: Hue, p: &Palette) -> Color {
    match hue {
        Hue::Peach => p.peach,
        Hue::Yellow => p.yellow,
        Hue::Mauve => p.mauve,
        Hue::Lavender => p.lavender,
        Hue::Green => p.green,
        Hue::Red => p.red,
        Hue::Neutral => p.subtext0,
    }
}

const SPECIAL: &[(&str, &str, Hue)] = &[
    ("Cargo.toml", "\u{e7a8}", Hue::Peach),
    ("Cargo.lock", "\u{f023}", Hue::Red),
    ("package.json", "\u{e71e}", Hue::Red),
    ("package-lock.json", "\u{f023}", Hue::Red),
    ("Dockerfile", "\u{f308}", Hue::Mauve),
    ("Makefile", "\u{f085}", Hue::Neutral),
    ("README.md", "\u{f48a}", Hue::Lavender),
    ("LICENSE", "\u{f0e3}", Hue::Yellow),
    (".gitignore", "\u{e702}", Hue::Peach),
];

const EXT: &[(&str, &str, Hue)] = &[
    ("rs", "\u{e7a8}", Hue::Peach),
    ("go", "\u{e627}", Hue::Peach),
    ("c", "\u{e61e}", Hue::Peach),
    ("h", "\u{e61e}", Hue::Peach),
    ("cpp", "\u{e61d}", Hue::Peach),
    ("cc", "\u{e61d}", Hue::Peach),
    ("hpp", "\u{e61d}", Hue::Peach),
    ("java", "\u{e738}", Hue::Peach),
    ("py", "\u{e606}", Hue::Yellow),
    ("js", "\u{e781}", Hue::Yellow),
    ("mjs", "\u{e781}", Hue::Yellow),
    ("cjs", "\u{e781}", Hue::Yellow),
    ("jsx", "\u{e781}", Hue::Yellow),
    ("ts", "\u{e628}", Hue::Mauve),
    ("tsx", "\u{e628}", Hue::Mauve),
    ("rb", "\u{e739}", Hue::Red),
    ("lua", "\u{e620}", Hue::Mauve),
    ("sh", "\u{f489}", Hue::Green),
    ("bash", "\u{f489}", Hue::Green),
    ("zsh", "\u{f489}", Hue::Green),
    ("json", "\u{e60b}", Hue::Yellow),
    ("toml", "\u{f013}", Hue::Mauve),
    ("yaml", "\u{f013}", Hue::Mauve),
    ("yml", "\u{f013}", Hue::Mauve),
    ("ini", "\u{f013}", Hue::Mauve),
    ("cfg", "\u{f013}", Hue::Mauve),
    ("conf", "\u{f013}", Hue::Mauve),
    ("html", "\u{f13b}", Hue::Peach),
    ("css", "\u{f13c}", Hue::Mauve),
    ("scss", "\u{f13c}", Hue::Mauve),
    ("md", "\u{f48a}", Hue::Lavender),
    ("markdown", "\u{f48a}", Hue::Lavender),
    ("txt", "\u{f15c}", Hue::Neutral),
    ("rst", "\u{f15c}", Hue::Neutral),
    ("lock", "\u{f023}", Hue::Red),
    ("png", "\u{f1c5}", Hue::Green),
    ("jpg", "\u{f1c5}", Hue::Green),
    ("jpeg", "\u{f1c5}", Hue::Green),
    ("gif", "\u{f1c5}", Hue::Green),
    ("svg", "\u{f1c5}", Hue::Green),
    ("ico", "\u{f1c5}", Hue::Green),
];

const GENERIC: (&str, Hue) = ("\u{f15b}", Hue::Neutral);

#[must_use]
pub fn folder_icon(expanded: bool, p: &Palette) -> (&'static str, Color) {
    let glyph = if expanded { "\u{f07c}" } else { "\u{f07b}" };
    (glyph, tint(Hue::Yellow, p))
}

#[must_use]
pub fn file_icon(name: &str, p: &Palette) -> (&'static str, Color) {
    let base = name.rsplit('/').next().unwrap_or(name);
    if let Some(&(_, glyph, hue)) = SPECIAL.iter().find(|(n, _, _)| *n == base) {
        return (glyph, tint(hue, p));
    }
    let ext = base.rsplit_once('.').map_or("", |(_, e)| e);
    if let Some(&(_, glyph, hue)) = EXT.iter().find(|(e, _, _)| e.eq_ignore_ascii_case(ext)) {
        return (glyph, tint(hue, p));
    }
    (GENERIC.0, tint(GENERIC.1, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn palette() -> Palette {
        theme::resolve(None).palette
    }

    #[test]
    fn known_extensions_and_special_names() {
        let p = palette();
        assert_eq!(file_icon("src/main.rs", &p).0, "\u{e7a8}", "extension from a path chain");
        assert_eq!(file_icon("data.json", &p).0, "\u{e60b}");
        assert_eq!(file_icon("Cargo.toml", &p).0, "\u{e7a8}");
        assert_ne!(file_icon("Cargo.toml", &p).0, file_icon("other.toml", &p).0);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let p = palette();
        assert_eq!(file_icon("MAIN.RS", &p).0, file_icon("main.rs", &p).0);
    }

    #[test]
    fn unknown_or_extensionless_falls_back_to_generic() {
        let p = palette();
        assert_eq!(file_icon("mystery.zzz", &p).0, GENERIC.0);
        assert_eq!(file_icon("noext", &p).0, GENERIC.0);
    }

    #[test]
    fn folder_icon_differs_by_state() {
        let p = palette();
        assert_ne!(folder_icon(true, &p).0, folder_icon(false, &p).0);
    }
}
