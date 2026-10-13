//! Decides whether a pasteboard snapshot should be recorded as a clip.
//!
//! These rules mirror the behaviour of CopyClip 2's pasteboard poller:
//! concealed (password manager) data is skipped, oversized data is
//! rejected, rich text representations over 2 MB are dropped, and copies
//! that equal the previous clip, the last pasted clip, or that were made
//! inside Rayperclip itself are ignored.

use crate::model::NewClip;

/// Maximum plain-text length (in UTF-16-ish characters) that is recorded.
pub const MAX_TEXT_CHARS: usize = 0x20_0000;
/// Maximum size in bytes of an RTF or HTML representation.
pub const MAX_RICH_BYTES: usize = 0x20_0000;
/// Pasteboard type used by password managers to mark secret data.
pub const CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";
/// Pasteboard type used to mark transient data that should not be recorded.
pub const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

/// A point-in-time copy of the relevant pasteboard contents.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub change_count: i64,
    /// All type identifiers currently on the pasteboard.
    pub types: Vec<String>,
    pub text: Option<String>,
    pub rtf: Option<Vec<u8>>,
    pub html: Option<Vec<u8>>,
}

/// The application that is frontmost while the copy happens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrontApp {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub pid: i32,
}

/// Everything besides the snapshot that influences the decision.
#[derive(Debug, Clone, Copy)]
pub struct CaptureContext<'a> {
    pub recording: bool,
    pub ignore_concealed: bool,
    /// Process id of Rayperclip itself.
    pub own_pid: i32,
    pub front_app: Option<&'a FrontApp>,
    /// Contents of the most recently recorded clip.
    pub last_clip: Option<&'a str>,
    /// Contents of the clip most recently pasted from Rayperclip.
    pub last_pasted: Option<&'a str>,
}

/// Why a snapshot was not recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NotRecording,
    Concealed,
    NoText,
    TooLarge,
    Blank,
    SameAsLastClip,
    SameAsLastPasted,
    OwnApplication,
}

/// Outcome of [`evaluate`].
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Record(NewClip),
    Skip(SkipReason),
}

/// Apply the capture rules to a snapshot.
pub fn evaluate(snapshot: &Snapshot, ctx: &CaptureContext<'_>) -> Decision {
    use Decision::Skip;
    use SkipReason::*;

    if !ctx.recording {
        return Skip(NotRecording);
    }
    if ctx.ignore_concealed
        && snapshot
            .types
            .iter()
            .any(|t| t == CONCEALED_TYPE || t == TRANSIENT_TYPE)
    {
        return Skip(Concealed);
    }
    let Some(text) = snapshot.text.as_deref() else {
        return Skip(NoText);
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return Skip(TooLarge);
    }
    if text.trim().is_empty() {
        return Skip(Blank);
    }
    if ctx.last_clip == Some(text) {
        return Skip(SameAsLastClip);
    }
    if ctx.last_pasted == Some(text) {
        return Skip(SameAsLastPasted);
    }
    if let Some(app) = ctx.front_app
        && app.pid == ctx.own_pid
    {
        return Skip(OwnApplication);
    }

    Decision::Record(NewClip {
        contents: text.to_string(),
        rtf: limit_rich(snapshot.rtf.as_ref()),
        html: limit_rich(snapshot.html.as_ref()),
        source_app: ctx.front_app.and_then(|a| a.name.clone()),
        source_bundle: ctx.front_app.and_then(|a| a.bundle_id.clone()),
    })
}

fn limit_rich(data: Option<&Vec<u8>>) -> Option<Vec<u8>> {
    data.filter(|d| !d.is_empty() && d.len() <= MAX_RICH_BYTES)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str) -> Snapshot {
        Snapshot {
            change_count: 1,
            types: vec!["public.utf8-plain-text".into()],
            text: Some(text.into()),
            rtf: None,
            html: None,
        }
    }

    fn ctx<'a>(front: Option<&'a FrontApp>) -> CaptureContext<'a> {
        CaptureContext {
            recording: true,
            ignore_concealed: true,
            own_pid: 100,
            front_app: front,
            last_clip: None,
            last_pasted: None,
        }
    }

    fn other_app() -> FrontApp {
        FrontApp {
            name: Some("TextEdit".into()),
            bundle_id: Some("com.apple.TextEdit".into()),
            pid: 200,
        }
    }

    #[test]
    fn records_plain_text_with_source_app() {
        let app = other_app();
        match evaluate(&snap("hello"), &ctx(Some(&app))) {
            Decision::Record(c) => {
                assert_eq!(c.contents, "hello");
                assert_eq!(c.source_app.as_deref(), Some("TextEdit"));
                assert_eq!(c.source_bundle.as_deref(), Some("com.apple.TextEdit"));
            }
            d => panic!("unexpected {d:?}"),
        }
    }

    #[test]
    fn skips_when_not_recording() {
        let mut c = ctx(None);
        c.recording = false;
        assert_eq!(
            evaluate(&snap("x"), &c),
            Decision::Skip(SkipReason::NotRecording)
        );
    }

    #[test]
    fn skips_concealed_data() {
        let mut s = snap("secret");
        s.types.push(CONCEALED_TYPE.into());
        assert_eq!(
            evaluate(&s, &ctx(None)),
            Decision::Skip(SkipReason::Concealed)
        );
    }

    #[test]
    fn skips_transient_data() {
        let mut s = snap("temp");
        s.types.push(TRANSIENT_TYPE.into());
        assert_eq!(
            evaluate(&s, &ctx(None)),
            Decision::Skip(SkipReason::Concealed)
        );
    }

    #[test]
    fn records_concealed_data_when_allowed() {
        let mut s = snap("secret");
        s.types.push(CONCEALED_TYPE.into());
        let mut c = ctx(None);
        c.ignore_concealed = false;
        assert!(matches!(evaluate(&s, &c), Decision::Record(_)));
    }

    #[test]
    fn skips_missing_text() {
        let mut s = snap("");
        s.text = None;
        assert_eq!(evaluate(&s, &ctx(None)), Decision::Skip(SkipReason::NoText));
    }

    #[test]
    fn skips_blank_text() {
        assert_eq!(
            evaluate(&snap(" \n\t "), &ctx(None)),
            Decision::Skip(SkipReason::Blank)
        );
    }

    #[test]
    fn skips_oversized_text() {
        let big = "a".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            evaluate(&snap(&big), &ctx(None)),
            Decision::Skip(SkipReason::TooLarge)
        );
    }

    #[test]
    fn accepts_text_at_the_limit() {
        let big = "a".repeat(MAX_TEXT_CHARS);
        assert!(matches!(
            evaluate(&snap(&big), &ctx(None)),
            Decision::Record(_)
        ));
    }

    #[test]
    fn skips_duplicate_of_last_clip() {
        let mut c = ctx(None);
        c.last_clip = Some("same");
        assert_eq!(
            evaluate(&snap("same"), &c),
            Decision::Skip(SkipReason::SameAsLastClip)
        );
    }

    #[test]
    fn skips_last_pasted_clip() {
        let mut c = ctx(None);
        c.last_pasted = Some("pasted");
        assert_eq!(
            evaluate(&snap("pasted"), &c),
            Decision::Skip(SkipReason::SameAsLastPasted)
        );
    }

    #[test]
    fn skips_copies_made_in_own_app() {
        let me = FrontApp {
            pid: 100,
            ..FrontApp::default()
        };
        assert_eq!(
            evaluate(&snap("x"), &ctx(Some(&me))),
            Decision::Skip(SkipReason::OwnApplication)
        );
    }

    #[test]
    fn keeps_rich_text_under_limit_and_drops_oversized() {
        let mut s = snap("styled");
        s.rtf = Some(vec![1; 10]);
        s.html = Some(vec![2; MAX_RICH_BYTES + 1]);
        match evaluate(&s, &ctx(None)) {
            Decision::Record(c) => {
                assert_eq!(c.rtf, Some(vec![1; 10]));
                assert_eq!(c.html, None);
                assert_eq!(c.contents, "styled");
            }
            d => panic!("unexpected {d:?}"),
        }
    }

    #[test]
    fn empty_rich_data_is_dropped() {
        let mut s = snap("x");
        s.rtf = Some(Vec::new());
        match evaluate(&s, &ctx(None)) {
            Decision::Record(c) => assert_eq!(c.rtf, None),
            d => panic!("unexpected {d:?}"),
        }
    }
}
