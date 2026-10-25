//! Rooms and message handling, independent of the transport: the WebSocket layer
//! ([`crate::serve`]) only moves bytes between sockets and a [`Hub`].
//!
//! One room per open document holds the server's replica, the connected clients and their
//! latest presence. A room is loaded from storage when its first client joins and dropped,
//! after a final compaction, when its last client leaves.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use opendox_model::{Document, Version};
use opendox_sync::{Message, SyncError};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::auth::Access;
use crate::storage::{Storage, StorageError};

#[derive(Debug, Error)]
pub enum HubError {
    #[error("document ids are 1-128 characters of A-Z, a-z, 0-9, '-' and '_'")]
    BadDocId,

    /// The client was dropped (it fell too far behind); it should reconnect.
    #[error("client is no longer connected")]
    Disconnected,

    /// A view-only client sent an edit; it was ignored.
    #[error("this connection may only view the document")]
    ReadOnly,

    #[error(transparent)]
    Sync(#[from] SyncError),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl From<opendox_model::ModelError> for HubError {
    fn from(e: opendox_model::ModelError) -> Self {
        HubError::Sync(e.into())
    }
}

#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Write a snapshot after this many stored updates.
    pub compact_every: usize,
    /// Messages queued per client before it counts as too slow and is disconnected. It loses
    /// nothing: on reconnect the handshake sends whatever it missed.
    pub outbox: usize,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            compact_every: 200,
            outbox: 1024,
        }
    }
}

/// All open documents.
pub struct Hub {
    storage: Arc<dyn Storage>,
    config: HubConfig,
    rooms: Mutex<HashMap<String, Arc<Mutex<Room>>>>,
    next_client: AtomicU32,
}

/// One client's membership in a room. Pass it back to [`Hub::receive`] and [`Hub::leave`].
pub struct Session {
    pub client: u32,
    pub access: Access,
    doc: String,
    room: Arc<Mutex<Room>>,
}

struct Room {
    doc: Document,
    clients: BTreeMap<u32, mpsc::Sender<Vec<u8>>>,
    presence: BTreeMap<u32, Vec<u8>>,
    /// Sequence number of the last stored update.
    last_seq: u64,
    /// Updates stored since the last snapshot.
    uncompacted: usize,
}

impl Hub {
    pub fn new(storage: Arc<dyn Storage>, config: HubConfig) -> Self {
        Self {
            storage,
            config,
            rooms: Mutex::new(HashMap::new()),
            next_client: AtomicU32::new(1),
        }
    }

    /// Joins document `doc` with edit access, loading it if no one has it open. Returns the
    /// session and the queue of messages to send to the client.
    pub fn join(&self, doc: &str) -> Result<(Session, mpsc::Receiver<Vec<u8>>), HubError> {
        self.join_with(doc, Access::Edit)
    }

    /// [`Hub::join`] with the access the client was given. A view-only client is told so first
    /// ([`Message::ReadOnly`]), and its edits are refused.
    pub fn join_with(
        &self,
        doc: &str,
        access: Access,
    ) -> Result<(Session, mpsc::Receiver<Vec<u8>>), HubError> {
        if !valid_doc_id(doc) {
            return Err(HubError::BadDocId);
        }
        let mut rooms = lock(&self.rooms);
        let room = match rooms.get(doc) {
            Some(r) => r.clone(),
            None => {
                let r = Arc::new(Mutex::new(self.load(doc)?));
                rooms.insert(doc.to_owned(), r.clone());
                r
            }
        };
        let client = self.next_client.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(self.config.outbox);
        {
            let mut r = lock(&room);
            r.clients.insert(client, tx);
            if access == Access::View {
                r.send(client, Message::ReadOnly.encode());
            }
        }
        Ok((
            Session {
                client,
                access,
                doc: doc.to_owned(),
                room,
            },
            rx,
        ))
    }

    /// Handles one message from a client.
    pub fn receive(&self, s: &Session, bytes: &[u8]) -> Result<(), HubError> {
        let mut room = lock(&s.room);
        if !room.clients.contains_key(&s.client) {
            return Err(HubError::Disconnected);
        }
        match Message::decode(bytes)? {
            Message::SyncRequest { version } => {
                let theirs = Version::decode(&version)?;
                let reply = Message::SyncReply {
                    updates: room.doc.export_updates_since(&theirs)?,
                    version: room.doc.version().encode(),
                };
                room.send(s.client, reply.encode());
                // Carets of everyone already here, now that the client has the text they're in.
                let others: Vec<_> = room
                    .presence
                    .iter()
                    .filter(|(c, _)| **c != s.client)
                    .map(|(&client, p)| Message::Presence {
                        client,
                        payload: p.clone(),
                    })
                    .collect();
                for m in others {
                    room.send(s.client, m.encode());
                }
            }
            Message::Update { .. } if s.access == Access::View => {
                return Err(HubError::ReadOnly);
            }
            Message::Update { updates } => {
                let before = room.doc.version();
                room.doc.import(&updates)?;
                if room.doc.version() == before {
                    return Ok(()); // nothing new (a resend, or edits still missing dependencies)
                }
                // Store and forward exactly what was new, not whatever the client resent.
                let fresh = room.doc.export_updates_since(&before)?;
                room.broadcast(
                    Some(s.client),
                    Message::Update {
                        updates: fresh.clone(),
                    },
                );
                room.last_seq = self.storage.append(&s.doc, &fresh)?;
                room.uncompacted += 1;
                if room.uncompacted >= self.config.compact_every {
                    self.compact(&s.doc, &mut room)?;
                }
            }
            Message::Presence { payload, .. } => {
                room.presence.insert(s.client, payload.clone());
                room.broadcast(
                    Some(s.client),
                    Message::Presence {
                        client: s.client,
                        payload,
                    },
                );
            }
            Message::SyncReply { .. } | Message::Leave { .. } | Message::ReadOnly => {
                return Err(SyncError::Malformed("clients don't send this message".into()).into())
            }
        }
        Ok(())
    }

    /// Removes a client. The last one out closes the room, compacting it first.
    pub fn leave(&self, s: Session) -> Result<(), HubError> {
        let mut rooms = lock(&self.rooms);
        let mut room = lock(&s.room);
        room.remove(s.client);
        if !room.clients.is_empty() {
            return Ok(());
        }
        rooms.remove(&s.doc);
        if room.uncompacted > 0 {
            self.compact(&s.doc, &mut room)?;
        }
        Ok(())
    }

    /// Number of documents currently open.
    pub fn open_rooms(&self) -> usize {
        lock(&self.rooms).len()
    }

    fn load(&self, id: &str) -> Result<Room, HubError> {
        let stored = self.storage.load(id)?;
        let doc = Document::new();
        if let Some(s) = &stored.snapshot {
            doc.import(s)?;
        }
        for (_, u) in &stored.updates {
            doc.import(u)?;
        }
        Ok(Room {
            doc,
            clients: BTreeMap::new(),
            presence: BTreeMap::new(),
            last_seq: stored.last_seq(),
            uncompacted: stored.updates.len(),
        })
    }

    fn compact(&self, id: &str, room: &mut Room) -> Result<(), HubError> {
        let snapshot = room.doc.export_snapshot()?;
        self.storage.compact(id, &snapshot, room.last_seq)?;
        room.uncompacted = 0;
        Ok(())
    }
}

impl Room {
    /// Queues a message for one client; drops the client if its queue is full or gone.
    fn send(&mut self, client: u32, msg: Vec<u8>) {
        let ok = self
            .clients
            .get(&client)
            .is_some_and(|tx| tx.try_send(msg).is_ok());
        if !ok {
            self.remove(client);
        }
    }

    fn broadcast(&mut self, except: Option<u32>, msg: Message) {
        let bytes = msg.encode();
        let targets: Vec<u32> = self
            .clients
            .keys()
            .copied()
            .filter(|&c| Some(c) != except)
            .collect();
        for c in targets {
            self.send(c, bytes.clone());
        }
    }

    fn remove(&mut self, client: u32) {
        if self.clients.remove(&client).is_some() && self.presence.remove(&client).is_some() {
            self.broadcast(None, Message::Leave { client });
        }
    }
}

fn valid_doc_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Locks a mutex, carrying on if a panic poisoned it: room state stays consistent because every
/// mutation is a single map insert/remove or a CRDT import.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
