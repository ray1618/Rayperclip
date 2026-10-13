//! The clip data model.

use chrono::{DateTime, Utc};

/// Database identifier of a clip.
pub type ClipId = i64;

/// Default maximum number of characters shown for a clip title.
pub const DEFAULT_TITLE_LENGTH: usize = 50;

/// A clip as stored in the history.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub id: ClipId,
    /// Plain-text contents (always present).
    pub contents: String,
    /// Optional RTF representation captured alongside the text.
    pub rtf: Option<Vec<u8>>,
    /// Optional HTML representation captured alongside the text.
    pub html: Option<Vec<u8>>,
    /// Localized name of the application the clip was copied from.
    pub source_app: Option<String>,
    /// Bundle identifier of the application the clip was copied from.
    pub source_bundle: Option<String>,
    /// Moment the clip was (last) recorded.
    pub recorded_at: DateTime<Utc>,
    /// Pinned clips are never trimmed and are listed first.
    pub pinned: bool,
    /// Position among the pinned clips (ascending).
    pub pin_order: i64,
    /// Optional user supplied title.
    pub title: Option<String>,
    /// How often the clip was pasted from Rayperclip.
    pub paste_count: i64,
}

impl Clip {
    /// Title to show in menus and lists: the custom title when set,
    /// otherwise a title derived from the contents.
    pub fn display_title(&self, max_chars: usize) -> String {
        match self.title.as_deref().map(str::trim) {
            Some(t) if !t.is_empty() => truncate_with_ellipsis(t, max_chars),
            _ => default_title(&self.contents, max_chars),
        }
    }
}

/// A clip that has not been stored yet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewClip {
    pub contents: String,
    pub rtf: Option<Vec<u8>>,
    pub html: Option<Vec<u8>>,
    pub source_app: Option<String>,
    pub source_bundle: Option<String>,
}

impl NewClip {
    /// Convenience constructor for a plain-text clip.
    pub fn text(contents: impl Into<String>) -> Self {
        Self {
            contents: contents.into(),
            ..Self::default()
        }
    }
}

/// Derive a single-line title from clip contents: the first non-blank line,
/// with runs of whitespace collapsed, truncated with an ellipsis.
pub fn default_title(contents: &str, max_chars: usize) -> String {
    let line = contents
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let more_lines = contents.trim().lines().count() > 1;
    let title = truncate_with_ellipsis(&collapsed, max_chars);
    if more_lines && !title.ends_with('…') && max_chars > 0 {
        let mut t = truncate_with_ellipsis(&collapsed, max_chars.saturating_sub(1));
        if !t.ends_with('…') {
            t.push('…');
        }
        return t;
    }
    title
}

/// Truncate `s` to at most `max_chars` characters (Unicode scalar values),
/// replacing the last character with `…` when truncation happens.
pub fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

/// Split text into lowercase search tokens (alphanumeric runs).
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(contents: &str, title: Option<&str>) -> Clip {
        Clip {
            id: 1,
            contents: contents.into(),
            rtf: None,
            html: None,
            source_app: None,
            source_bundle: None,
            recorded_at: Utc::now(),
            pinned: false,
            pin_order: 0,
            title: title.map(Into::into),
            paste_count: 0,
        }
    }

    #[test]
    fn short_text_is_unchanged() {
        assert_eq!(default_title("hello world", 50), "hello world");
    }

    #[test]
    fn long_text_is_truncated_with_ellipsis() {
        let t = default_title("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(t, "abcdefghi…");
        assert_eq!(t.chars().count(), 10);
    }

    #[test]
    fn uses_first_non_blank_line_and_marks_more_lines() {
        assert_eq!(
            default_title("\n\n  first line \nsecond", 50),
            "first line…"
        );
    }

    #[test]
    fn collapses_internal_whitespace() {
        assert_eq!(default_title("a \t  b    c", 50), "a b c");
    }

    #[test]
    fn unicode_is_truncated_on_char_boundaries() {
        let t = default_title("ééééééééééé", 5);
        assert_eq!(t, "éééé…");
    }

    #[test]
    fn zero_length_gives_empty_title() {
        assert_eq!(truncate_with_ellipsis("abc", 0), "");
    }

    #[test]
    fn custom_title_takes_precedence() {
        assert_eq!(clip("body", Some("My title")).display_title(50), "My title");
    }

    #[test]
    fn blank_custom_title_falls_back_to_contents() {
        assert_eq!(clip("body", Some("   ")).display_title(50), "body");
    }

    #[test]
    fn custom_title_is_truncated_too() {
        assert_eq!(clip("x", Some("0123456789")).display_title(4), "012…");
    }

    #[test]
    fn tokenize_splits_and_lowercases() {
        assert_eq!(
            tokenize("Hello, World! foo_bar 42"),
            vec!["hello", "world", "foo", "bar", "42"]
        );
    }
}
