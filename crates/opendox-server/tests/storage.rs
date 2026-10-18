//! Storage adapters: append, load, and compaction with a watermark. Every test runs against
//! both the in-memory and the SQLite adapter.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::Document;
use opendox_server::{MemoryStorage, SqliteStorage, Storage};

fn adapters() -> Vec<(&'static str, Box<dyn Storage>)> {
    vec![
        ("memory", Box::new(MemoryStorage::new())),
        ("sqlite", Box::new(SqliteStorage::in_memory().unwrap())),
    ]
}

/// Rebuilds a document from what storage returns.
fn restore(s: &dyn Storage, id: &str) -> Document {
    let stored = s.load(id).unwrap();
    let d = Document::new();
    if let Some(snap) = &stored.snapshot {
        d.import(snap).unwrap();
    }
    for (_, u) in &stored.updates {
        d.import(u).unwrap();
    }
    d
}

/// Applies an edit to `doc` and stores just that edit; returns its sequence number.
fn edit(s: &dyn Storage, id: &str, doc: &Document, f: impl FnOnce(&Document)) -> u64 {
    let before = doc.version();
    f(doc);
    s.append(id, &doc.export_updates_since(&before).unwrap())
        .unwrap()
}

#[test]
fn unknown_document_loads_empty() {
    for (name, s) in adapters() {
        let stored = s.load("nope").unwrap();
        assert!(
            stored.snapshot.is_none() && stored.updates.is_empty(),
            "{name}"
        );
    }
}

#[test]
fn updates_replay_in_order() {
    for (name, s) in adapters() {
        let d = Document::new();
        let a = edit(&*s, "x", &d, |d| d.insert(0, "hello").unwrap());
        let b = edit(&*s, "x", &d, |d| d.insert(5, " world").unwrap());
        assert!(b > a, "{name}: sequence numbers grow");
        assert_eq!(restore(&*s, "x").text(), "hello world", "{name}");
        // Other documents are separate.
        assert_eq!(restore(&*s, "y").text(), "", "{name}");
    }
}

#[test]
fn compaction_keeps_updates_above_the_watermark() {
    for (name, s) in adapters() {
        let d = Document::new();
        edit(&*s, "x", &d, |d| d.insert(0, "one").unwrap());
        let w = edit(&*s, "x", &d, |d| d.insert(3, " two").unwrap());
        // The snapshot is taken here, at the watermark...
        let snapshot = d.export_snapshot().unwrap();
        // ...and an update lands before the compaction is written.
        edit(&*s, "x", &d, |d| d.insert(7, " three").unwrap());
        s.compact("x", &snapshot, w).unwrap();

        let stored = s.load("x").unwrap();
        assert!(stored.snapshot.is_some(), "{name}");
        assert_eq!(
            stored.updates.len(),
            1,
            "{name}: only the late update is left"
        );
        assert_eq!(restore(&*s, "x").text(), "one two three", "{name}");
    }
}

#[test]
fn an_older_compaction_never_replaces_a_newer_one() {
    for (name, s) in adapters() {
        let d = Document::new();
        let w1 = edit(&*s, "x", &d, |d| d.insert(0, "a").unwrap());
        let old = d.export_snapshot().unwrap();
        let w2 = edit(&*s, "x", &d, |d| d.insert(1, "b").unwrap());
        s.compact("x", &d.export_snapshot().unwrap(), w2).unwrap();
        // A slow compaction from earlier finishes last.
        s.compact("x", &old, w1).unwrap();
        assert_eq!(restore(&*s, "x").text(), "ab", "{name}");
        assert!(s.load("x").unwrap().updates.is_empty(), "{name}");
    }
}

#[test]
fn sqlite_persists_across_reopen() {
    let dir = std::env::temp_dir().join(format!("opendox-storage-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("db.sqlite3");
    {
        let s = SqliteStorage::open(&path).unwrap();
        let d = Document::new();
        let w = edit(&s, "doc", &d, |d| d.insert(0, "kept").unwrap());
        s.compact("doc", &d.export_snapshot().unwrap(), w).unwrap();
        edit(&s, "doc", &d, |d| d.insert(4, " too").unwrap());
    }
    let s = SqliteStorage::open(&path).unwrap();
    assert_eq!(restore(&s, "doc").text(), "kept too");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
