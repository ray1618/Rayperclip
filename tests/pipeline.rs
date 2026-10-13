//! End-to-end tests of the platform independent pipeline:
//! pasteboard → poller → store → search → paste.

use std::time::Duration;

use rayperclip::capture::{CONCEALED_TYPE, FrontApp, SkipReason};
use rayperclip::clipboard::{Clipboard, MemoryClipboard};
use rayperclip::model::Clip;
use rayperclip::paste::{self, PasteBackend, PasteOutcome};
use rayperclip::poller::{PollResult, Poller};
use rayperclip::settings::{Paths, Settings};
use rayperclip::store::Store;

const OWN_PID: i32 = 4242;

fn safari() -> FrontApp {
    FrontApp {
        name: Some("Safari".into()),
        bundle_id: Some("com.apple.Safari".into()),
        pid: 99,
    }
}

/// Paste backend that writes to an in-memory pasteboard and logs steps.
struct FakeBackend<'a> {
    clipboard: &'a mut MemoryClipboard,
    log: Vec<&'static str>,
}

impl PasteBackend for FakeBackend<'_> {
    fn write_clipboard(&mut self, clip: &Clip) {
        self.clipboard.write_clip(clip);
        self.log.push("write");
    }
    fn restore_focus(&mut self) {
        self.log.push("focus");
    }
    fn send_paste_keystroke(&mut self, _delay: Duration) {
        self.log.push("cmd-v");
    }
    fn request_permission(&mut self) {
        self.log.push("permission");
    }
}

#[test]
fn copy_search_pin_paste_roundtrip() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cb = MemoryClipboard::new();
    let settings = Settings::default();
    let mut poller = Poller::new(OWN_PID, None);
    let app = safari();

    for text in ["alpha one", "beta two", "gamma three"] {
        cb.copy_text(text);
        assert!(matches!(
            poller.tick(&cb, Some(&app), &settings, &mut store).unwrap(),
            PollResult::Recorded { .. }
        ));
    }
    // Password manager copy is ignored.
    cb.copy_with_types("hunter2", &[CONCEALED_TYPE]);
    assert_eq!(
        poller.tick(&cb, Some(&app), &settings, &mut store).unwrap(),
        PollResult::Skipped(SkipReason::Concealed)
    );
    assert_eq!(store.count().unwrap(), 3);

    let hits = store.search("bet", 10).unwrap();
    assert_eq!(hits.len(), 1);
    let beta = hits[0].clone();
    assert_eq!(beta.source_app.as_deref(), Some("Safari"));

    store.set_pinned(beta.id, true).unwrap();
    assert_eq!(store.all().unwrap()[0].id, beta.id);

    // Paste it.
    let (steps, outcome) = paste::plan(settings.paste_directly, true);
    assert_eq!(outcome, PasteOutcome::Pasted);
    store.increment_paste_count(beta.id).unwrap();
    poller.note_pasted(&beta.contents);
    let mut backend = FakeBackend {
        clipboard: &mut cb,
        log: vec![],
    };
    paste::execute(&mut backend, &beta, &steps);
    assert_eq!(backend.log, vec!["write", "focus", "cmd-v"]);
    assert_eq!(cb.snapshot().text.as_deref(), Some("beta two"));

    // The pasted clip is not recorded again by the poller.
    assert_eq!(
        poller.tick(&cb, Some(&app), &settings, &mut store).unwrap(),
        PollResult::Skipped(SkipReason::SameAsLastPasted)
    );
    assert_eq!(store.get(beta.id).unwrap().unwrap().paste_count, 1);
}

#[test]
fn history_limit_respects_pins_across_many_copies() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cb = MemoryClipboard::new();
    let settings = Settings {
        max_clips: 5,
        ..Settings::default()
    };
    let mut poller = Poller::new(OWN_PID, None);

    cb.copy_text("keep me");
    poller.tick(&cb, None, &settings, &mut store).unwrap();
    let keep = store.find_by_contents("keep me").unwrap().unwrap();
    store.set_pinned(keep.id, true).unwrap();

    for i in 0..50 {
        cb.copy_text(&format!("clip number {i}"));
        poller.tick(&cb, None, &settings, &mut store).unwrap();
    }
    assert_eq!(store.recent(100).unwrap().len(), 5);
    assert_eq!(store.pinned().unwrap().len(), 1);
    assert_eq!(store.recent(1).unwrap()[0].contents, "clip number 49");
}

#[test]
fn recopying_old_clip_moves_it_to_top() {
    let mut store = Store::open_in_memory().unwrap();
    let mut cb = MemoryClipboard::new();
    let settings = Settings::default();
    let mut poller = Poller::new(OWN_PID, None);
    for t in ["a", "b", "c", "a"] {
        cb.copy_text(t);
        poller.tick(&cb, None, &settings, &mut store).unwrap();
    }
    let order: Vec<_> = store
        .recent(10)
        .unwrap()
        .into_iter()
        .map(|c| c.contents)
        .collect();
    assert_eq!(order, vec!["a", "c", "b"]);
}

#[test]
fn state_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(&dir.path().join("cfg"), &dir.path().join("data"));
    let settings = Settings {
        theme: "Forest".into(),
        ..Settings::default()
    };
    settings.save(&paths.settings).unwrap();

    let mut cb = MemoryClipboard::new();
    {
        let mut store = Store::open(&paths.database).unwrap();
        let mut poller = Poller::new(OWN_PID, store.latest_contents().unwrap());
        cb.copy_text("before restart");
        poller.tick(&cb, None, &settings, &mut store).unwrap();
    }

    // "Restart": reload everything from disk.
    let settings = Settings::load(&paths.settings).unwrap();
    assert_eq!(settings.theme, "Forest");
    let mut store = Store::open(&paths.database).unwrap();
    let mut poller = Poller::new(OWN_PID, store.latest_contents().unwrap());
    // The pasteboard still holds the same text: it must not be duplicated.
    assert_eq!(
        poller.tick(&cb, None, &settings, &mut store).unwrap(),
        PollResult::Skipped(SkipReason::SameAsLastClip)
    );
    assert_eq!(store.count().unwrap(), 1);
}
