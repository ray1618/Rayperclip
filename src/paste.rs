//! Pasting a clip into the frontmost application.
//!
//! The flow follows CopyClip 2: the clip is written to the pasteboard,
//! focus goes back to the previously active app, and – when "paste
//! directly" is enabled and the Accessibility permission is granted – a
//! synthetic Cmd+V is posted after a short delay.

use std::time::Duration;

use crate::model::Clip;

/// Delay between restoring focus and posting Cmd+V.
pub const PASTE_DELAY: Duration = Duration::from_millis(200);
/// macOS virtual key code of the `V` key.
pub const KEYCODE_V: u16 = 9;
/// `kCGEventFlagMaskCommand`.
pub const FLAG_COMMAND: u64 = 0x0010_0000;

/// One step of the paste flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteStep {
    WriteClipboard,
    RestoreFocus,
    SendKeystroke(Duration),
    RequestPermission,
}

/// Result reported to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteOutcome {
    /// Clip was pasted into the frontmost app.
    Pasted,
    /// Clip is only on the pasteboard.
    Copied,
    /// Clip is on the pasteboard; Accessibility permission is missing.
    CopiedNeedsPermission,
}

/// Compute the steps for a paste.
pub fn plan(paste_directly: bool, accessibility_trusted: bool) -> (Vec<PasteStep>, PasteOutcome) {
    use PasteStep::*;
    let mut steps = vec![WriteClipboard, RestoreFocus];
    let outcome = if !paste_directly {
        PasteOutcome::Copied
    } else if accessibility_trusted {
        steps.push(SendKeystroke(PASTE_DELAY));
        PasteOutcome::Pasted
    } else {
        steps.push(RequestPermission);
        PasteOutcome::CopiedNeedsPermission
    };
    (steps, outcome)
}

/// Platform operations needed by the paste flow.
pub trait PasteBackend {
    fn write_clipboard(&mut self, clip: &Clip);
    fn restore_focus(&mut self);
    fn send_paste_keystroke(&mut self, delay: Duration);
    fn request_permission(&mut self);
}

/// Run the planned steps against a backend.
pub fn execute<B: PasteBackend + ?Sized>(backend: &mut B, clip: &Clip, steps: &[PasteStep]) {
    for step in steps {
        match *step {
            PasteStep::WriteClipboard => backend.write_clipboard(clip),
            PasteStep::RestoreFocus => backend.restore_focus(),
            PasteStep::SendKeystroke(d) => backend.send_paste_keystroke(d),
            PasteStep::RequestPermission => backend.request_permission(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl PasteBackend for Recorder {
        fn write_clipboard(&mut self, clip: &Clip) {
            self.0.push(format!("write:{}", clip.contents));
        }
        fn restore_focus(&mut self) {
            self.0.push("focus".into());
        }
        fn send_paste_keystroke(&mut self, delay: Duration) {
            self.0.push(format!("cmd-v:{}", delay.as_millis()));
        }
        fn request_permission(&mut self) {
            self.0.push("permission".into());
        }
    }

    fn clip() -> Clip {
        Clip {
            id: 1,
            contents: "hi".into(),
            rtf: None,
            html: None,
            source_app: None,
            source_bundle: None,
            recorded_at: Utc::now(),
            pinned: false,
            pin_order: 0,
            title: None,
            paste_count: 0,
        }
    }

    #[test]
    fn pastes_when_enabled_and_trusted() {
        let (steps, outcome) = plan(true, true);
        assert_eq!(outcome, PasteOutcome::Pasted);
        let mut r = Recorder::default();
        execute(&mut r, &clip(), &steps);
        assert_eq!(r.0, vec!["write:hi", "focus", "cmd-v:200"]);
    }

    #[test]
    fn only_copies_when_paste_directly_is_off() {
        let (steps, outcome) = plan(false, true);
        assert_eq!(outcome, PasteOutcome::Copied);
        let mut r = Recorder::default();
        execute(&mut r, &clip(), &steps);
        assert_eq!(r.0, vec!["write:hi", "focus"]);
    }

    #[test]
    fn asks_for_permission_when_untrusted() {
        let (steps, outcome) = plan(true, false);
        assert_eq!(outcome, PasteOutcome::CopiedNeedsPermission);
        assert!(
            !steps
                .iter()
                .any(|s| matches!(s, PasteStep::SendKeystroke(_)))
        );
        assert_eq!(steps.last(), Some(&PasteStep::RequestPermission));
    }

    #[test]
    fn clipboard_is_written_before_focus_changes() {
        for (a, b) in [(true, true), (true, false), (false, false)] {
            let (steps, _) = plan(a, b);
            assert_eq!(steps[0], PasteStep::WriteClipboard);
            assert_eq!(steps[1], PasteStep::RestoreFocus);
        }
    }
}
