//! Crossterm → nvim input-notation translation for the embedded editor (`specs/herdr-host.md`).
//! Pure functions, no state: the key handler calls [`key_notation`] for every key it forwards,
//! and the mouse handler [`mouse_modifier`] for `nvim_input_mouse`'s modifier string.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The nvim `nvim_input` notation for a crossterm key, or `None` for keys nvim can't take
/// (bare modifiers, media keys).
///
/// Rules: modifier prefixes in fixed `C-`, `S-`, `A-`, `D-` order. For `Char` keys `S-` is
/// included only when CTRL or ALT is also present — crossterm delivers the already-shifted
/// character (`Shift+a` arrives as `Char('A')`), so `<S-A>` would be wrong. A plain char is
/// sent literally except `<`, which must be `<lt>` (`nvim_input` parses `<>` notation).
#[must_use]
pub fn key_notation(key: &KeyEvent) -> Option<String> {
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    let shift = mods.contains(KeyModifiers::SHIFT);
    let sup = mods.contains(KeyModifiers::SUPER);

    // The <>-notation name, and whether S- applies to this key kind.
    let (name, shift_applies): (String, bool) = match key.code {
        KeyCode::Char(c) => {
            let name = if c == '<' { "lt".to_string() } else { c.to_string() };
            // Space in <>-notation must be spelled out; bare space is fine unprefixed.
            let name = if c == ' ' { "Space".to_string() } else { name };
            if !ctrl && !alt && !sup {
                // Plain (possibly shifted) character: literal, no wrapper.
                return Some(if c == '<' { "<lt>".to_string() } else { c.to_string() });
            }
            (name, false)
        }
        KeyCode::Enter => ("CR".to_string(), true),
        KeyCode::Esc => ("Esc".to_string(), true),
        KeyCode::Backspace => ("BS".to_string(), true),
        KeyCode::Tab => ("Tab".to_string(), true),
        KeyCode::BackTab => return Some(wrap("S-Tab", ctrl, false, alt, sup)),
        KeyCode::Delete => ("Del".to_string(), true),
        KeyCode::Insert => ("Insert".to_string(), true),
        KeyCode::Left => ("Left".to_string(), true),
        KeyCode::Right => ("Right".to_string(), true),
        KeyCode::Up => ("Up".to_string(), true),
        KeyCode::Down => ("Down".to_string(), true),
        KeyCode::Home => ("Home".to_string(), true),
        KeyCode::End => ("End".to_string(), true),
        KeyCode::PageUp => ("PageUp".to_string(), true),
        KeyCode::PageDown => ("PageDown".to_string(), true),
        KeyCode::F(n) => (format!("F{n}"), true),
        _ => return None,
    };
    Some(wrap(&name, ctrl, shift && shift_applies, alt, sup))
}

#[expect(clippy::fn_params_excessive_bools)] // one flag per modifier prefix, in emit order
fn wrap(name: &str, ctrl: bool, shift: bool, alt: bool, sup: bool) -> String {
    let mut out = String::from("<");
    if ctrl {
        out.push_str("C-");
    }
    if shift {
        out.push_str("S-");
    }
    if alt {
        out.push_str("A-");
    }
    if sup {
        out.push_str("D-");
    }
    out.push_str(name);
    out.push('>');
    out
}

/// The `nvim_input_mouse` modifier string: separator-free concatenation of `C`/`S`/`A`.
#[must_use]
pub fn mouse_modifier(mods: KeyModifiers) -> String {
    let mut out = String::new();
    if mods.contains(KeyModifiers::CONTROL) {
        out.push('C');
    }
    if mods.contains(KeyModifiers::SHIFT) {
        out.push('S');
    }
    if mods.contains(KeyModifiers::ALT) {
        out.push('A');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn plain_chars_are_literal_except_lt() {
        assert_eq!(key_notation(&key(KeyCode::Char('a'), KeyModifiers::NONE)).unwrap(), "a");
        assert_eq!(key_notation(&key(KeyCode::Char('"'), KeyModifiers::NONE)).unwrap(), "\"");
        assert_eq!(key_notation(&key(KeyCode::Char('<'), KeyModifiers::NONE)).unwrap(), "<lt>");
        // Shifted chars arrive pre-shifted: SHIFT is dropped.
        assert_eq!(key_notation(&key(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap(), "A");
        assert_eq!(key_notation(&key(KeyCode::Char(' '), KeyModifiers::NONE)).unwrap(), " ");
    }

    #[test]
    fn ctrl_and_alt_chars_wrap() {
        let m = KeyModifiers::CONTROL;
        assert_eq!(key_notation(&key(KeyCode::Char('w'), m)).unwrap(), "<C-w>");
        assert_eq!(key_notation(&key(KeyCode::Char('i'), m)).unwrap(), "<C-i>"); // jumplist fwd
        assert_eq!(key_notation(&key(KeyCode::Char('['), m)).unwrap(), "<C-[>");
        assert_eq!(key_notation(&key(KeyCode::Char('<'), m)).unwrap(), "<C-lt>");
        assert_eq!(key_notation(&key(KeyCode::Char(' '), m)).unwrap(), "<C-Space>");
        assert_eq!(key_notation(&key(KeyCode::Char('x'), KeyModifiers::ALT)).unwrap(), "<A-x>");
        assert_eq!(
            key_notation(&key(KeyCode::Char('x'), KeyModifiers::CONTROL | KeyModifiers::ALT))
                .unwrap(),
            "<C-A-x>"
        );
        // SHIFT is kept for chars only alongside CTRL/ALT.
        assert_eq!(
            key_notation(&key(KeyCode::Char('P'), KeyModifiers::CONTROL | KeyModifiers::SHIFT))
                .unwrap(),
            "<C-P>"
        );
    }

    #[test]
    fn specials_wrap_with_modifiers() {
        assert_eq!(key_notation(&key(KeyCode::Enter, KeyModifiers::NONE)).unwrap(), "<CR>");
        assert_eq!(key_notation(&key(KeyCode::Enter, KeyModifiers::ALT)).unwrap(), "<A-CR>");
        assert_eq!(key_notation(&key(KeyCode::Esc, KeyModifiers::NONE)).unwrap(), "<Esc>");
        assert_eq!(key_notation(&key(KeyCode::Backspace, KeyModifiers::NONE)).unwrap(), "<BS>");
        assert_eq!(
            key_notation(&key(KeyCode::Backspace, KeyModifiers::CONTROL)).unwrap(),
            "<C-BS>"
        );
        assert_eq!(key_notation(&key(KeyCode::Tab, KeyModifiers::CONTROL)).unwrap(), "<C-Tab>");
        assert_eq!(key_notation(&key(KeyCode::BackTab, KeyModifiers::NONE)).unwrap(), "<S-Tab>");
        assert_eq!(key_notation(&key(KeyCode::Delete, KeyModifiers::NONE)).unwrap(), "<Del>");
        assert_eq!(key_notation(&key(KeyCode::Up, KeyModifiers::CONTROL)).unwrap(), "<C-Up>");
        assert_eq!(key_notation(&key(KeyCode::Left, KeyModifiers::SHIFT)).unwrap(), "<S-Left>");
        assert_eq!(key_notation(&key(KeyCode::Home, KeyModifiers::NONE)).unwrap(), "<Home>");
        assert_eq!(
            key_notation(&key(KeyCode::PageDown, KeyModifiers::NONE)).unwrap(),
            "<PageDown>"
        );
        assert_eq!(key_notation(&key(KeyCode::F(5), KeyModifiers::NONE)).unwrap(), "<F5>");
        assert_eq!(
            key_notation(&key(KeyCode::F(5), KeyModifiers::CONTROL | KeyModifiers::SHIFT)).unwrap(),
            "<C-S-F5>"
        );
    }

    #[test]
    fn unmappable_keys_are_dropped() {
        assert_eq!(key_notation(&key(KeyCode::Null, KeyModifiers::NONE)), None);
        assert_eq!(key_notation(&key(KeyCode::CapsLock, KeyModifiers::NONE)), None);
    }

    #[test]
    fn mouse_modifier_concatenates() {
        assert_eq!(mouse_modifier(KeyModifiers::NONE), "");
        assert_eq!(mouse_modifier(KeyModifiers::CONTROL | KeyModifiers::SHIFT), "CS");
        assert_eq!(mouse_modifier(KeyModifiers::ALT), "A");
    }
}
