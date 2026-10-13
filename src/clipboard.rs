//! Abstraction over the system pasteboard so the poller can be tested.

use crate::capture::Snapshot;
use crate::model::Clip;

/// Minimal pasteboard interface used by Rayperclip.
pub trait Clipboard {
    /// Monotonic counter that changes whenever the pasteboard contents change.
    fn change_count(&self) -> i64;
    /// Read the current contents.
    fn snapshot(&self) -> Snapshot;
    /// Replace the pasteboard contents with the given clip.
    fn write_clip(&mut self, clip: &Clip);
}

/// In-memory pasteboard, used in tests and as a fallback.
#[derive(Debug, Default, Clone)]
pub struct MemoryClipboard {
    pub snapshot: Snapshot,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate the user copying text in another application.
    pub fn copy_text(&mut self, text: &str) {
        self.snapshot.change_count += 1;
        self.snapshot.types = vec!["public.utf8-plain-text".into()];
        self.snapshot.text = Some(text.to_string());
        self.snapshot.rtf = None;
        self.snapshot.html = None;
    }

    /// Simulate a copy with extra pasteboard types (e.g. concealed data).
    pub fn copy_with_types(&mut self, text: &str, types: &[&str]) {
        self.copy_text(text);
        self.snapshot
            .types
            .extend(types.iter().map(|t| t.to_string()));
    }
}

impl Clipboard for MemoryClipboard {
    fn change_count(&self) -> i64 {
        self.snapshot.change_count
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    fn write_clip(&mut self, clip: &Clip) {
        self.snapshot.change_count += 1;
        self.snapshot.types = vec!["public.utf8-plain-text".into()];
        self.snapshot.text = Some(clip.contents.clone());
        self.snapshot.rtf = clip.rtf.clone();
        self.snapshot.html = clip.html.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_bumps_change_count() {
        let mut cb = MemoryClipboard::new();
        assert_eq!(cb.change_count(), 0);
        cb.copy_text("a");
        cb.copy_text("b");
        assert_eq!(cb.change_count(), 2);
        assert_eq!(cb.snapshot().text.as_deref(), Some("b"));
    }

    #[test]
    fn copy_with_types_keeps_extra_types() {
        let mut cb = MemoryClipboard::new();
        cb.copy_with_types("pw", &["org.nspasteboard.ConcealedType"]);
        assert!(
            cb.snapshot()
                .types
                .contains(&"org.nspasteboard.ConcealedType".to_string())
        );
    }
}
