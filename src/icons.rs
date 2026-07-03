//! Nerd Font icons for the file tree (opt-in; see [`crate::app::App`]'s `icons` flag).
//!
//! Terminals can't draw an image icon pack, so these are Nerd Font glyphs — characters in the
//! font's Private Use Area, the same set eza/lsd/nvim-tree use. They render only on a Nerd Font
//! terminal, so the feature is off by default. This module is the one place that maps a name to a
//! glyph and a palette-tinted colour; the renderer (`ui.rs`) just paints what it returns.

use ratatui::style::Color;

use crate::theme::Palette;

/// A palette-relative colour, so the glyph tints follow the active theme.
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

/// Whole-filename matches, tried before the extension so `Cargo.toml` gets the rust icon rather
/// than the generic TOML one.
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

/// Extension matches (case-insensitive). Colours are grouped by category — systems, scripts,
/// config/web, docs, assets — because the palette has only six accents.
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

/// The fallback for an unrecognised file.
const GENERIC: (&str, Hue) = ("\u{f15b}", Hue::Neutral);

/// The folder glyph and colour, by expansion state (open vs closed).
#[must_use]
pub fn folder_icon(expanded: bool, p: &Palette) -> (&'static str, Color) {
    let glyph = if expanded { "\u{f07c}" } else { "\u{f07b}" };
    (glyph, tint(Hue::Yellow, p))
}

/// The filetype glyph and colour for a file. `name` may be a collapsed `dir/dir/base` chain —
/// only the basename matters. Special filenames win over the extension; an unknown type falls
/// back to a generic file glyph.
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
        // A special filename beats its extension.
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
