//! Where documents live between sessions.
//!
//! A document is stored as a snapshot plus the updates received after it. Each update gets a
//! sequence number. Compaction writes a new snapshot together with a **watermark**: the
//! sequence number of the last update the snapshot already contains. Updates at or below the
//! watermark are then deleted; updates above it (which may have arrived while the snapshot was
//! being made) are kept and replayed on load. A compaction with an older watermark than the
//! stored one is ignored, so a slow compaction can never overwrite a newer snapshot.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage backend error: {0}")]
    Backend(String),
}

/// What [`Storage::load`] returns: everything needed to rebuild a document.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stored {
    pub snapshot: Option<Vec<u8>>,
    /// Updates after the snapshot's watermark, oldest first, with their sequence numbers.
    pub updates: Vec<(u64, Vec<u8>)>,
}

impl Stored {
    /// Sequence number of the newest update, or 0.
    pub fn last_seq(&self) -> u64 {
        self.updates.last().map_or(0, |(s, _)| *s)
    }
}

/// Persistent document storage. Implementations must be safe to call from several threads.
///
/// Calls are synchronous and made while a document's room is locked, so they should be quick
/// (local disk). A networked store should batch behind its own queue.
pub trait Storage: Send + Sync + 'static {
    /// The snapshot and the updates after it. An unknown document loads as empty.
    fn load(&self, doc: &str) -> Result<Stored, StorageError>;

    /// Appends an update; returns its sequence number, larger than any before it for `doc`.
    fn append(&self, doc: &str, update: &[u8]) -> Result<u64, StorageError>;

    /// Replaces the snapshot with one containing every update up to and including `watermark`,
    /// and drops those updates. Ignored if the stored snapshot's watermark is already newer.
    fn compact(&self, doc: &str, snapshot: &[u8], watermark: u64) -> Result<(), StorageError>;
}

/// Keeps everything in memory. For tests and throwaway servers.
#[derive(Default)]
pub struct MemoryStorage {
    docs: Mutex<HashMap<String, MemoryDoc>>,
}

#[derive(Default)]
struct MemoryDoc {
    snapshot: Option<(Vec<u8>, u64)>,
    updates: Vec<(u64, Vec<u8>)>,
    next_seq: u64,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut HashMap<String, MemoryDoc>) -> T) -> T {
        // A panic while holding the lock leaves plain data behind; carry on with it.
        let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut docs)
    }
}

impl Storage for MemoryStorage {
    fn load(&self, doc: &str) -> Result<Stored, StorageError> {
        Ok(self.with(|docs| {
            docs.get(doc).map_or_else(Stored::default, |d| Stored {
                snapshot: d.snapshot.as_ref().map(|(s, _)| s.clone()),
                updates: d.updates.clone(),
            })
        }))
    }

    fn append(&self, doc: &str, update: &[u8]) -> Result<u64, StorageError> {
        Ok(self.with(|docs| {
            let d = docs.entry(doc.to_owned()).or_default();
            d.next_seq += 1;
            d.updates.push((d.next_seq, update.to_vec()));
            d.next_seq
        }))
    }

    fn compact(&self, doc: &str, snapshot: &[u8], watermark: u64) -> Result<(), StorageError> {
        self.with(|docs| {
            let d = docs.entry(doc.to_owned()).or_default();
            if d.snapshot.as_ref().is_some_and(|(_, w)| *w > watermark) {
                return;
            }
            d.snapshot = Some((snapshot.to_vec(), watermark));
            d.updates.retain(|(seq, _)| *seq > watermark);
        });
        Ok(())
    }
}

/// SQLite storage: one file holds every document.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
    PRAGMA journal_mode = WAL;
    CREATE TABLE IF NOT EXISTS snapshots (
        doc       TEXT PRIMARY KEY,
        data      BLOB NOT NULL,
        watermark INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS updates (
        seq  INTEGER PRIMARY KEY AUTOINCREMENT,
        doc  TEXT NOT NULL,
        data BLOB NOT NULL
    );
    CREATE INDEX IF NOT EXISTS updates_by_doc ON updates (doc, seq);
";

impl SqliteStorage {
    /// Opens (creating if needed) a database file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Self::init(Connection::open(path).map_err(backend)?)
    }

    /// A private in-memory database, for tests.
    pub fn in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory().map_err(backend)?)
    }

    fn init(conn: Connection) -> Result<Self, StorageError> {
        conn.execute_batch(SCHEMA).map_err(backend)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // SQLite rolls back an unfinished transaction itself, so a poisoned lock is safe to reuse.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Storage for SqliteStorage {
    fn load(&self, doc: &str) -> Result<Stored, StorageError> {
        let mut conn = self.conn();
        // One read transaction, so a compaction can't land between the two queries.
        let tx = conn.transaction().map_err(backend)?;
        let snap: Option<(Vec<u8>, i64)> = tx
            .query_row(
                "SELECT data, watermark FROM snapshots WHERE doc = ?1",
                params![doc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let watermark = snap.as_ref().map_or(0, |(_, w)| *w);
        let updates = {
            let mut stmt = tx
                .prepare("SELECT seq, data FROM updates WHERE doc = ?1 AND seq > ?2 ORDER BY seq")
                .map_err(backend)?;
            let rows = stmt
                .query_map(params![doc, watermark], |r| {
                    Ok((r.get::<_, i64>(0)? as u64, r.get::<_, Vec<u8>>(1)?))
                })
                .map_err(backend)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(backend)?
        };
        tx.commit().map_err(backend)?;
        Ok(Stored {
            snapshot: snap.map(|(s, _)| s),
            updates,
        })
    }

    fn append(&self, doc: &str, update: &[u8]) -> Result<u64, StorageError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO updates (doc, data) VALUES (?1, ?2)",
            params![doc, update],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid() as u64)
    }

    fn compact(&self, doc: &str, snapshot: &[u8], watermark: u64) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(backend)?;
        let w = watermark as i64;
        let stored: Option<i64> = tx
            .query_row(
                "SELECT watermark FROM snapshots WHERE doc = ?1",
                params![doc],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        if stored.is_some_and(|s| s > w) {
            return Ok(());
        }
        tx.execute(
            "INSERT INTO snapshots (doc, data, watermark) VALUES (?1, ?2, ?3)
             ON CONFLICT (doc) DO UPDATE SET data = excluded.data, watermark = excluded.watermark",
            params![doc, snapshot, w],
        )
        .map_err(backend)?;
        tx.execute(
            "DELETE FROM updates WHERE doc = ?1 AND seq <= ?2",
            params![doc, w],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)
    }
}

fn backend(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}
