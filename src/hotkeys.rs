//! Parsing and validation of global hotkey strings such as `cmd+shift+v`.

use global_hotkey::hotkey::{HotKey, Modifiers};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HotkeyError {
    #[error("hotkey is empty")]
    Empty,
    #[error("hotkey \"{0}\" needs at least one modifier (cmd, ctrl, alt or shift)")]
    NoModifier(String),
    #[error("hotkey \"{0}\" is shift-only; add cmd, ctrl or alt")]
    ShiftOnly(String),
    #[error("invalid hotkey \"{0}\": {1}")]
    Invalid(String, String),
}

/// Parse a hotkey description. Returns `Ok(None)` for an empty/"none"
/// string, meaning the hotkey is disabled.
pub fn parse_optional(spec: &str) -> Result<Option<HotKey>, HotkeyError> {
    let s = spec.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("off") {
        return Ok(None);
    }
    parse(s).map(Some)
}

/// Parse a hotkey description. A global hotkey must include a modifier
/// other than shift so it does not swallow normal typing.
pub fn parse(spec: &str) -> Result<HotKey, HotkeyError> {
    let s = spec.trim();
    if s.is_empty() {
        return Err(HotkeyError::Empty);
    }
    let hk: HotKey = s
        .parse()
        .map_err(|e: global_hotkey::hotkey::HotKeyParseError| {
            HotkeyError::Invalid(s.to_string(), e.to_string())
        })?;
    if hk.mods.is_empty() {
        return Err(HotkeyError::NoModifier(s.to_string()));
    }
    if hk.mods == Modifiers::SHIFT {
        return Err(HotkeyError::ShiftOnly(s.to_string()));
    }
    Ok(hk)
}

/// Human readable rendering using macOS modifier symbols, e.g. `⇧⌘V`.
pub fn display(hk: &HotKey) -> String {
    let mut out = String::new();
    if hk.mods.contains(Modifiers::CONTROL) {
        out.push('⌃');
    }
    if hk.mods.contains(Modifiers::ALT) {
        out.push('⌥');
    }
    if hk.mods.contains(Modifiers::SHIFT) {
        out.push('⇧');
    }
    if hk.mods.contains(Modifiers::SUPER) {
        out.push('⌘');
    }
    let key = format!("{:?}", hk.key);
    let key = key
        .strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(&key);
    out.push_str(key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::Code;

    #[test]
    fn parses_cmd_shift_v() {
        let hk = parse("cmd+shift+v").unwrap();
        assert_eq!(
            hk,
            HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV)
        );
    }

    #[test]
    fn parse_is_case_insensitive_and_trims() {
        assert_eq!(parse("  CMD+Alt+V ").unwrap(), parse("cmd+alt+v").unwrap());
    }

    #[test]
    fn rejects_missing_modifier() {
        assert_eq!(parse("v"), Err(HotkeyError::NoModifier("v".into())));
    }

    #[test]
    fn rejects_shift_only() {
        assert!(matches!(parse("shift+v"), Err(HotkeyError::ShiftOnly(_))));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(parse("cmd+shift+"), Err(HotkeyError::Invalid(..))));
        assert!(matches!(
            parse("cmd+notakey"),
            Err(HotkeyError::Invalid(..))
        ));
        assert!(matches!(parse("cmd+v+x"), Err(HotkeyError::Invalid(..))));
        assert_eq!(parse(""), Err(HotkeyError::Empty));
    }

    #[test]
    fn optional_allows_disabling() {
        assert_eq!(parse_optional("").unwrap(), None);
        assert_eq!(parse_optional("none").unwrap(), None);
        assert!(parse_optional("ctrl+space").unwrap().is_some());
    }

    #[test]
    fn display_uses_mac_symbols() {
        assert_eq!(display(&parse("cmd+shift+v").unwrap()), "⇧⌘V");
        assert_eq!(display(&parse("ctrl+alt+1").unwrap()), "⌃⌥1");
    }
}
