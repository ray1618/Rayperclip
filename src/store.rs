//! Persistent clip storage backed by SQLite with an FTS5 search index.

use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use thiserror::Error;

use crate::model::{Clip, ClipId, NewClip};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not create database directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("clip {0} does not exist")]
    NotFound(ClipId),
    #[error("another clip already has these contents")]
    Duplicate,
    #[error("clip contents must not be empty")]
    EmptyContents,
}

pub type Result<T> = std::result::Result<T, StoreError>;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clips (
    id            INTEGER PRIMARY KEY,
    contents      TEXT    NOT NULL UNIQUE,
    rtf           BLOB,
    html          BLOB,
    source_app    TEXT,
    source_bundle TEXT,
    recorded_at   INTEGER NOT NULL,
    pinned        INTEGER NOT NULL DEFAULT 0,
    pin_order     INTEGER NOT NULL DEFAULT 0,
    title         TEXT,
    paste_count   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS clips_recorded ON clips(pinned, recorded_at);

CREATE VIRTUAL TABLE IF NOT EXISTS clips_fts USING fts5(
    contents, title, content='clips', content_rowid='id', tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS clips_ai AFTER INSERT ON clips BEGIN
    INSERT INTO clips_fts(rowid, contents, title) VALUES (new.id, new.contents, new.title);
END;
CREATE TRIGGER IF NOT EXISTS clips_ad AFTER DELETE ON clips BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, contents, title)
        VALUES ('delete', old.id, old.contents, old.title);
END;
CREATE TRIGGER IF NOT EXISTS clips_au AFTER UPDATE OF contents, title ON clips BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, contents, title)
        VALUES ('delete', old.id, old.contents, old.title);
    INSERT INTO clips_fts(rowid, contents, title) VALUES (new.id, new.contents, new.title);
END;
"#;

const COLUMNS: &str = "id, contents, rtf, html, source_app, source_bundle, recorded_at, \
                       pinned, pin_order, title, paste_count";

/// The clip database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// Open a private in-memory database (used by tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Next recording timestamp; strictly greater than any stored one so the
    /// ordering is stable even when several clips arrive within a millisecond.
    fn next_timestamp(&self) -> Result<i64> {
        let max: Option<i64> =
            self.conn
                .query_row("SELECT MAX(recorded_at) FROM clips", [], |r| r.get(0))?;
        let now = Utc::now().timestamp_millis();
        Ok(match max {
            Some(m) if m >= now => m + 1,
            _ => now,
        })
    }

    /// Store a new clip. When a clip with identical contents already exists it
    /// is moved to the top of the history (and its rich data/source refreshed)
    /// instead of creating a duplicate.
    pub fn insert(&mut self, clip: &NewClip) -> Result<ClipId> {
        if clip.contents.is_empty() {
            return Err(StoreError::EmptyContents);
        }
        let ts = self.next_timestamp()?;
        if let Some(existing) = self.find_by_contents(&clip.contents)? {
            self.conn.execute(
                "UPDATE clips SET recorded_at = ?1,
                    rtf = COALESCE(?2, rtf), html = COALESCE(?3, html),
                    source_app = COALESCE(?4, source_app),
                    source_bundle = COALESCE(?5, source_bundle)
                 WHERE id = ?6",
                params![
                    ts,
                    clip.rtf,
                    clip.html,
                    clip.source_app,
                    clip.source_bundle,
                    existing.id
                ],
            )?;
            return Ok(existing.id);
        }
        self.conn.execute(
            "INSERT INTO clips (contents, rtf, html, source_app, source_bundle, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                clip.contents,
                clip.rtf,
                clip.html,
                clip.source_app,
                clip.source_bundle,
                ts
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get(&self, id: ClipId) -> Result<Option<Clip>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM clips WHERE id = ?1"),
                [id],
                row_to_clip,
            )
            .optional()?)
    }

    fn require(&self, id: ClipId) -> Result<Clip> {
        self.get(id)?.ok_or(StoreError::NotFound(id))
    }

    pub fn find_by_contents(&self, contents: &str) -> Result<Option<Clip>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM clips WHERE contents = ?1"),
                [contents],
                row_to_clip,
            )
            .optional()?)
    }

    /// Contents of the most recently recorded clip.
    pub fn latest_contents(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT contents FROM clips ORDER BY recorded_at DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM clips", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Clip>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params, row_to_clip)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Pinned clips in their user defined order.
    pub fn pinned(&self) -> Result<Vec<Clip>> {
        self.query(
            &format!("SELECT {COLUMNS} FROM clips WHERE pinned = 1 ORDER BY pin_order, id"),
            [],
        )
    }

    /// Most recent unpinned clips, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<Clip>> {
        self.query(
            &format!(
                "SELECT {COLUMNS} FROM clips WHERE pinned = 0 ORDER BY recorded_at DESC LIMIT ?1"
            ),
            [limit as i64],
        )
    }

    /// Pinned clips followed by all unpinned clips.
    pub fn all(&self) -> Result<Vec<Clip>> {
        let mut v = self.pinned()?;
        v.extend(self.recent(usize::MAX >> 1)?);
        Ok(v)
    }

    /// Full-text search over contents and titles. Every word in `query` must
    /// match (as a prefix). An empty query returns [`Store::all`].
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Clip>> {
        let Some(fts) = fts_query(query) else {
            let mut all = self.all()?;
            all.truncate(limit);
            return Ok(all);
        };
        self.query(
            &format!(
                "SELECT {} FROM clips c JOIN clips_fts f ON f.rowid = c.id
                 WHERE clips_fts MATCH ?1
                 ORDER BY c.pinned DESC, c.pin_order, c.recorded_at DESC LIMIT ?2",
                COLUMNS
                    .split(", ")
                    .map(|c| format!("c.{c}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            params![fts, limit as i64],
        )
    }

    /// Pin or unpin a clip. Newly pinned clips are appended to the pinned list.
    pub fn set_pinned(&mut self, id: ClipId, pinned: bool) -> Result<()> {
        let clip = self.require(id)?;
        if clip.pinned == pinned {
            return Ok(());
        }
        if pinned {
            let next: i64 = self.conn.query_row(
                "SELECT COALESCE(MAX(pin_order), -1) + 1 FROM clips WHERE pinned = 1",
                [],
                |r| r.get(0),
            )?;
            self.conn.execute(
                "UPDATE clips SET pinned = 1, pin_order = ?1 WHERE id = ?2",
                params![next, id],
            )?;
        } else {
            let ts = self.next_timestamp()?;
            self.conn.execute(
                "UPDATE clips SET pinned = 0, pin_order = 0, recorded_at = ?1 WHERE id = ?2",
                params![ts, id],
            )?;
            self.renumber_pins()?;
        }
        Ok(())
    }

    /// Move a pinned clip to `new_index` within the pinned list.
    pub fn move_pinned(&mut self, id: ClipId, new_index: usize) -> Result<()> {
        let mut pins = self.pinned()?;
        let Some(pos) = pins.iter().position(|c| c.id == id) else {
            return Err(StoreError::NotFound(id));
        };
        let clip = pins.remove(pos);
        let idx = new_index.min(pins.len());
        pins.insert(idx, clip);
        let tx = self.conn.transaction()?;
        for (i, c) in pins.iter().enumerate() {
            tx.execute(
                "UPDATE clips SET pin_order = ?1 WHERE id = ?2",
                params![i as i64, c.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn renumber_pins(&mut self) -> Result<()> {
        let ids: Vec<ClipId> = self.pinned()?.into_iter().map(|c| c.id).collect();
        let tx = self.conn.transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE clips SET pin_order = ?1 WHERE id = ?2",
                params![i as i64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Replace the text of a clip. Rich representations are discarded because
    /// they no longer match the edited text.
    pub fn update_contents(&mut self, id: ClipId, contents: &str) -> Result<()> {
        if contents.is_empty() {
            return Err(StoreError::EmptyContents);
        }
        let clip = self.require(id)?;
        if clip.contents == contents {
            return Ok(());
        }
        if let Some(other) = self.find_by_contents(contents)?
            && other.id != id
        {
            return Err(StoreError::Duplicate);
        }
        self.conn.execute(
            "UPDATE clips SET contents = ?1, rtf = NULL, html = NULL WHERE id = ?2",
            params![contents, id],
        )?;
        Ok(())
    }

    /// Set or clear (empty/blank string or `None`) the custom title.
    pub fn set_title(&mut self, id: ClipId, title: Option<&str>) -> Result<()> {
        self.require(id)?;
        let title = title.map(str::trim).filter(|t| !t.is_empty());
        self.conn.execute(
            "UPDATE clips SET title = ?1 WHERE id = ?2",
            params![title, id],
        )?;
        Ok(())
    }

    pub fn increment_paste_count(&mut self, id: ClipId) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE clips SET paste_count = paste_count + 1 WHERE id = ?1",
            [id],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(id));
        }
        Ok(())
    }

    pub fn delete(&mut self, id: ClipId) -> Result<()> {
        let was_pinned = self.require(id)?.pinned;
        self.conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
        if was_pinned {
            self.renumber_pins()?;
        }
        Ok(())
    }

    /// Delete every clip that is not pinned. Returns the number removed.
    pub fn delete_all_unpinned(&mut self) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM clips WHERE pinned = 0", [])?)
    }

    /// Remove the oldest unpinned clips so at most `max` unpinned clips remain.
    /// Pinned clips are never trimmed. Returns the number removed.
    pub fn trim(&mut self, max: usize) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM clips WHERE id IN (
                SELECT id FROM clips WHERE pinned = 0
                ORDER BY recorded_at DESC LIMIT -1 OFFSET ?1)",
            [max as i64],
        )?)
    }
}

/// Convert user input into an FTS5 query where every word must match as a
/// prefix. Returns `None` when the input contains no searchable words.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|w| w.replace('"', "\"\""))
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn row_to_clip(r: &Row<'_>) -> rusqlite::Result<Clip> {
    let ms: i64 = r.get(6)?;
    Ok(Clip {
        id: r.get(0)?,
        contents: r.get(1)?,
        rtf: r.get(2)?,
        html: r.get(3)?,
        source_app: r.get(4)?,
        source_bundle: r.get(5)?,
        recorded_at: millis_to_datetime(ms),
        pinned: r.get::<_, i64>(7)? != 0,
        pin_order: r.get(8)?,
        title: r.get(9)?,
        paste_count: r.get(10)?,
    })
}

fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn add(s: &mut Store, text: &str) -> ClipId {
        s.insert(&NewClip::text(text)).unwrap()
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let mut s = store();
        let clip = NewClip {
            contents: "hello".into(),
            rtf: Some(b"{\\rtf1}".to_vec()),
            html: Some(b"<b>hello</b>".to_vec()),
            source_app: Some("Notes".into()),
            source_bundle: Some("com.apple.Notes".into()),
        };
        let id = s.insert(&clip).unwrap();
        let got = s.get(id).unwrap().unwrap();
        assert_eq!(got.contents, "hello");
        assert_eq!(got.rtf.as_deref(), Some(&b"{\\rtf1}"[..]));
        assert_eq!(got.html.as_deref(), Some(&b"<b>hello</b>"[..]));
        assert_eq!(got.source_app.as_deref(), Some("Notes"));
        assert!(!got.pinned);
        assert_eq!(got.paste_count, 0);
    }

    #[test]
    fn empty_contents_are_rejected() {
        assert!(matches!(
            store().insert(&NewClip::text("")),
            Err(StoreError::EmptyContents)
        ));
    }

    #[test]
    fn duplicate_contents_move_existing_clip_to_top() {
        let mut s = store();
        let a = add(&mut s, "a");
        add(&mut s, "b");
        let again = add(&mut s, "a");
        assert_eq!(a, again);
        assert_eq!(s.count().unwrap(), 2);
        let recent: Vec<_> = s.recent(10).unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(recent[0], a);
        assert_eq!(s.latest_contents().unwrap().as_deref(), Some("a"));
    }

    #[test]
    fn recent_is_newest_first_and_limited() {
        let mut s = store();
        for t in ["1", "2", "3", "4"] {
            add(&mut s, t);
        }
        let r: Vec<_> = s
            .recent(3)
            .unwrap()
            .into_iter()
            .map(|c| c.contents)
            .collect();
        assert_eq!(r, vec!["4", "3", "2"]);
    }

    #[test]
    fn pinning_orders_and_separates_clips() {
        let mut s = store();
        let a = add(&mut s, "a");
        let b = add(&mut s, "b");
        add(&mut s, "c");
        s.set_pinned(b, true).unwrap();
        s.set_pinned(a, true).unwrap();
        let pins: Vec<_> = s.pinned().unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(pins, vec![b, a]);
        assert_eq!(s.recent(10).unwrap().len(), 1);
        let all: Vec<_> = s.all().unwrap().into_iter().map(|c| c.contents).collect();
        assert_eq!(all, vec!["b", "a", "c"]);
    }

    #[test]
    fn unpinning_renumbers_and_moves_clip_to_recent_top() {
        let mut s = store();
        let a = add(&mut s, "a");
        let b = add(&mut s, "b");
        let c = add(&mut s, "c");
        for id in [a, b, c] {
            s.set_pinned(id, true).unwrap();
        }
        s.set_pinned(a, false).unwrap();
        let pins = s.pinned().unwrap();
        assert_eq!(
            pins.iter().map(|c| (c.id, c.pin_order)).collect::<Vec<_>>(),
            vec![(b, 0), (c, 1)]
        );
        assert_eq!(s.recent(1).unwrap()[0].id, a);
    }

    #[test]
    fn move_pinned_reorders() {
        let mut s = store();
        let ids: Vec<_> = ["a", "b", "c"].iter().map(|t| add(&mut s, t)).collect();
        for id in &ids {
            s.set_pinned(*id, true).unwrap();
        }
        s.move_pinned(ids[2], 0).unwrap();
        let order: Vec<_> = s.pinned().unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(order, vec![ids[2], ids[0], ids[1]]);
        s.move_pinned(ids[2], 99).unwrap();
        let order: Vec<_> = s.pinned().unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(order, vec![ids[0], ids[1], ids[2]]);
    }

    #[test]
    fn move_unpinned_clip_is_an_error() {
        let mut s = store();
        let a = add(&mut s, "a");
        assert!(matches!(s.move_pinned(a, 0), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn trim_keeps_newest_and_never_removes_pinned() {
        let mut s = store();
        let first = add(&mut s, "first");
        s.set_pinned(first, true).unwrap();
        for i in 0..10 {
            add(&mut s, &format!("clip {i}"));
        }
        let removed = s.trim(3).unwrap();
        assert_eq!(removed, 7);
        let recent: Vec<_> = s
            .recent(10)
            .unwrap()
            .into_iter()
            .map(|c| c.contents)
            .collect();
        assert_eq!(recent, vec!["clip 9", "clip 8", "clip 7"]);
        assert!(s.get(first).unwrap().is_some());
    }

    #[test]
    fn trim_below_limit_removes_nothing() {
        let mut s = store();
        add(&mut s, "x");
        assert_eq!(s.trim(5).unwrap(), 0);
    }

    #[test]
    fn search_matches_prefixes_and_all_words() {
        let mut s = store();
        add(&mut s, "The quick brown fox");
        add(&mut s, "quick silver");
        add(&mut s, "unrelated");
        let r = s.search("qui", 10).unwrap();
        assert_eq!(r.len(), 2);
        let r = s.search("quick fox", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].contents, "The quick brown fox");
        assert!(s.search("zebra", 10).unwrap().is_empty());
    }

    #[test]
    fn search_finds_titles_and_follows_edits() {
        let mut s = store();
        let id = add(&mut s, "some body text");
        s.set_title(id, Some("Invoice template")).unwrap();
        assert_eq!(s.search("invoice", 10).unwrap().len(), 1);
        s.update_contents(id, "replaced words").unwrap();
        assert!(s.search("body", 10).unwrap().is_empty());
        assert_eq!(s.search("replaced", 10).unwrap().len(), 1);
        s.delete(id).unwrap();
        assert!(s.search("invoice", 10).unwrap().is_empty());
    }

    #[test]
    fn search_handles_quotes_and_symbols_safely() {
        let mut s = store();
        add(&mut s, r#"say "hi" now"#);
        assert_eq!(s.search(r#""hi""#, 10).unwrap().len(), 1);
        assert_eq!(s.search("*** --", 10).unwrap().len(), 1); // treated as empty
    }

    #[test]
    fn search_lists_pinned_first() {
        let mut s = store();
        let a = add(&mut s, "apple one");
        add(&mut s, "apple two");
        s.set_pinned(a, true).unwrap();
        let r = s.search("apple", 10).unwrap();
        assert_eq!(r[0].id, a);
    }

    #[test]
    fn empty_search_returns_everything_limited() {
        let mut s = store();
        for t in ["a", "b", "c"] {
            add(&mut s, t);
        }
        assert_eq!(s.search("  ", 2).unwrap().len(), 2);
    }

    #[test]
    fn update_contents_rejects_duplicates_and_clears_rich_data() {
        let mut s = store();
        let a = s
            .insert(&NewClip {
                contents: "a".into(),
                rtf: Some(vec![1]),
                ..NewClip::default()
            })
            .unwrap();
        add(&mut s, "b");
        assert!(matches!(
            s.update_contents(a, "b"),
            Err(StoreError::Duplicate)
        ));
        assert!(matches!(
            s.update_contents(a, ""),
            Err(StoreError::EmptyContents)
        ));
        s.update_contents(a, "a2").unwrap();
        let c = s.get(a).unwrap().unwrap();
        assert_eq!(c.contents, "a2");
        assert_eq!(c.rtf, None);
    }

    #[test]
    fn titles_can_be_set_and_cleared() {
        let mut s = store();
        let a = add(&mut s, "a");
        s.set_title(a, Some("  T  ")).unwrap();
        assert_eq!(s.get(a).unwrap().unwrap().title.as_deref(), Some("T"));
        s.set_title(a, Some("   ")).unwrap();
        assert_eq!(s.get(a).unwrap().unwrap().title, None);
        assert!(matches!(
            s.set_title(999, Some("x")),
            Err(StoreError::NotFound(999))
        ));
    }

    #[test]
    fn paste_count_increments() {
        let mut s = store();
        let a = add(&mut s, "a");
        s.increment_paste_count(a).unwrap();
        s.increment_paste_count(a).unwrap();
        assert_eq!(s.get(a).unwrap().unwrap().paste_count, 2);
        assert!(s.increment_paste_count(42).is_err());
    }

    #[test]
    fn delete_all_unpinned_keeps_pins() {
        let mut s = store();
        let a = add(&mut s, "a");
        add(&mut s, "b");
        add(&mut s, "c");
        s.set_pinned(a, true).unwrap();
        assert_eq!(s.delete_all_unpinned().unwrap(), 2);
        assert_eq!(s.count().unwrap(), 1);
    }

    #[test]
    fn delete_pinned_renumbers() {
        let mut s = store();
        let a = add(&mut s, "a");
        let b = add(&mut s, "b");
        s.set_pinned(a, true).unwrap();
        s.set_pinned(b, true).unwrap();
        s.delete(a).unwrap();
        assert_eq!(s.pinned().unwrap()[0].pin_order, 0);
        assert!(matches!(s.delete(a), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn fts_query_builds_prefix_terms() {
        assert_eq!(fts_query("foo bar").as_deref(), Some("\"foo\"* \"bar\"*"));
        assert_eq!(fts_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        assert_eq!(fts_query("  ... "), None);
    }

    #[test]
    fn data_persists_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/clips.sqlite");
        {
            let mut s = Store::open(&path).unwrap();
            add(&mut s, "persisted");
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.latest_contents().unwrap().as_deref(), Some("persisted"));
        assert_eq!(s.search("persist", 5).unwrap().len(), 1);
    }
}
