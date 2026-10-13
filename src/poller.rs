//! Pasteboard polling: turns pasteboard changes into stored clips.

use crate::capture::{self, CaptureContext, Decision, FrontApp, SkipReason};
use crate::clipboard::Clipboard;
use crate::model::ClipId;
use crate::settings::Settings;
use crate::store::{Store, StoreError};

/// Polling interval used by the application (same as CopyClip 2).
pub const POLL_INTERVAL_SECS: f64 = 0.5;

/// What happened during one poll.
#[derive(Debug, Clone, PartialEq)]
pub enum PollResult {
    /// The pasteboard did not change since the last poll.
    Unchanged,
    /// The pasteboard changed but nothing was recorded.
    Skipped(SkipReason),
    /// A clip was recorded (new or moved to the top); `trimmed` old clips
    /// were removed to respect the maximum.
    Recorded { id: ClipId, trimmed: usize },
}

/// Pasteboard poller state.
#[derive(Debug, Default)]
pub struct Poller {
    last_change: Option<i64>,
    last_clip: Option<String>,
    last_pasted: Option<String>,
    own_pid: i32,
}

impl Poller {
    /// Create a poller. `last_clip` should be the newest stored clip so a
    /// restart does not record the current pasteboard again.
    pub fn new(own_pid: i32, last_clip: Option<String>) -> Self {
        Self {
            last_change: None,
            last_clip,
            last_pasted: None,
            own_pid,
        }
    }

    /// Start from the current change count, ignoring the existing contents.
    pub fn sync_change_count(&mut self, change_count: i64) {
        self.last_change = Some(change_count);
    }

    /// Remember a clip that Rayperclip itself put on the pasteboard so it
    /// is not recorded again.
    pub fn note_pasted(&mut self, contents: &str) {
        self.last_pasted = Some(contents.to_string());
    }

    pub fn last_clip(&self) -> Option<&str> {
        self.last_clip.as_deref()
    }

    /// Forget the last clip (e.g. after the history was cleared).
    pub fn reset_last_clip(&mut self, last_clip: Option<String>) {
        self.last_clip = last_clip;
    }

    /// Check the pasteboard once.
    pub fn tick<C: Clipboard + ?Sized>(
        &mut self,
        clipboard: &C,
        front_app: Option<&FrontApp>,
        settings: &Settings,
        store: &mut Store,
    ) -> Result<PollResult, StoreError> {
        let count = clipboard.change_count();
        if self.last_change == Some(count) {
            return Ok(PollResult::Unchanged);
        }
        self.last_change = Some(count);
        let snapshot = clipboard.snapshot();
        let ctx = CaptureContext {
            recording: settings.recording,
            ignore_concealed: settings.ignore_concealed,
            own_pid: self.own_pid,
            front_app,
            last_clip: self.last_clip.as_deref(),
            last_pasted: self.last_pasted.as_deref(),
        };
        match capture::evaluate(&snapshot, &ctx) {
            Decision::Skip(reason) => Ok(PollResult::Skipped(reason)),
            Decision::Record(clip) => {
                let id = store.insert(&clip)?;
                let trimmed = store.trim(settings.max_clips)?;
                self.last_clip = Some(clip.contents);
                Ok(PollResult::Recorded { id, trimmed })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::MemoryClipboard;

    fn setup() -> (Poller, MemoryClipboard, Settings, Store) {
        (
            Poller::new(1, None),
            MemoryClipboard::new(),
            Settings::default(),
            Store::open_in_memory().unwrap(),
        )
    }

    fn other() -> FrontApp {
        FrontApp {
            name: Some("Safari".into()),
            bundle_id: Some("com.apple.Safari".into()),
            pid: 2,
        }
    }

    #[test]
    fn records_once_per_change() {
        let (mut p, mut cb, s, mut st) = setup();
        cb.copy_text("one");
        let app = other();
        assert!(matches!(
            p.tick(&cb, Some(&app), &s, &mut st).unwrap(),
            PollResult::Recorded { .. }
        ));
        assert_eq!(
            p.tick(&cb, Some(&app), &s, &mut st).unwrap(),
            PollResult::Unchanged
        );
        assert_eq!(st.count().unwrap(), 1);
        assert_eq!(
            st.recent(1).unwrap()[0].source_app.as_deref(),
            Some("Safari")
        );
    }

    #[test]
    fn copying_same_text_again_is_skipped() {
        let (mut p, mut cb, s, mut st) = setup();
        cb.copy_text("x");
        p.tick(&cb, None, &s, &mut st).unwrap();
        cb.copy_text("x");
        assert_eq!(
            p.tick(&cb, None, &s, &mut st).unwrap(),
            PollResult::Skipped(SkipReason::SameAsLastClip)
        );
    }

    #[test]
    fn restart_does_not_rerecord_latest_clip() {
        let (_, mut cb, s, mut st) = setup();
        cb.copy_text("persisted");
        let mut p = Poller::new(1, Some("persisted".into()));
        assert_eq!(
            p.tick(&cb, None, &s, &mut st).unwrap(),
            PollResult::Skipped(SkipReason::SameAsLastClip)
        );
    }

    #[test]
    fn sync_change_count_ignores_existing_contents() {
        let (mut p, mut cb, s, mut st) = setup();
        cb.copy_text("before launch");
        p.sync_change_count(cb.change_count());
        assert_eq!(
            p.tick(&cb, None, &s, &mut st).unwrap(),
            PollResult::Unchanged
        );
    }

    #[test]
    fn pasted_clip_is_not_recorded_again() {
        let (mut p, mut cb, s, mut st) = setup();
        cb.copy_text("a");
        p.tick(&cb, None, &s, &mut st).unwrap();
        cb.copy_text("b");
        p.tick(&cb, None, &s, &mut st).unwrap();
        let a = st.find_by_contents("a").unwrap().unwrap();
        cb.write_clip(&a);
        p.note_pasted("a");
        assert_eq!(
            p.tick(&cb, None, &s, &mut st).unwrap(),
            PollResult::Skipped(SkipReason::SameAsLastPasted)
        );
    }

    #[test]
    fn trims_to_max_clips() {
        let (mut p, mut cb, mut s, mut st) = setup();
        s.max_clips = 2;
        for t in ["1", "2", "3"] {
            cb.copy_text(t);
            p.tick(&cb, None, &s, &mut st).unwrap();
        }
        assert_eq!(st.count().unwrap(), 2);
        assert!(st.find_by_contents("1").unwrap().is_none());
    }

    #[test]
    fn not_recording_skips() {
        let (mut p, mut cb, mut s, mut st) = setup();
        s.recording = false;
        cb.copy_text("x");
        assert_eq!(
            p.tick(&cb, None, &s, &mut st).unwrap(),
            PollResult::Skipped(SkipReason::NotRecording)
        );
        assert_eq!(st.count().unwrap(), 0);
    }

    #[test]
    fn own_app_copies_are_skipped() {
        let (mut p, mut cb, s, mut st) = setup();
        cb.copy_text("x");
        let me = FrontApp {
            pid: 1,
            ..FrontApp::default()
        };
        assert_eq!(
            p.tick(&cb, Some(&me), &s, &mut st).unwrap(),
            PollResult::Skipped(SkipReason::OwnApplication)
        );
    }
}
