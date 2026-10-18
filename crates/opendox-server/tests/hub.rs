//! The server's room logic driven in-process by real `SyncClient`s: live edits, offline edits,
//! presence, persistence across restarts, compaction and slow clients.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use std::sync::Arc;

use opendox_model::{Anchor, Document};
use opendox_server::{Hub, HubConfig, HubError, MemoryStorage, Session, Storage};
use opendox_sync::{Presence, SyncClient};
use tokio::sync::mpsc::Receiver;

const DOC: &str = "doc-1";

struct Peer {
    doc: Document,
    sync: SyncClient,
    conn: Option<(Session, Receiver<Vec<u8>>)>,
    /// Messages received so far.
    received: usize,
}

impl Peer {
    fn new() -> Peer {
        Peer {
            doc: Document::new(),
            sync: SyncClient::new(),
            conn: None,
            received: 0,
        }
    }

    fn joined(hub: &Hub) -> Peer {
        let mut p = Peer::new();
        p.connect(hub);
        p
    }

    fn connect(&mut self, hub: &Hub) {
        let (s, rx) = hub.join(DOC).unwrap();
        hub.receive(&s, &self.sync.connect(&self.doc)).unwrap();
        self.conn = Some((s, rx));
    }

    fn disconnect(&mut self, hub: &Hub) {
        if let Some((s, _)) = self.conn.take() {
            hub.leave(s).unwrap();
        }
        self.sync.disconnected();
    }

    /// Sends batched local edits; handles everything queued for us. Returns whether anything
    /// moved.
    fn step(&mut self, hub: &Hub) -> bool {
        let Some((s, rx)) = &mut self.conn else {
            return false;
        };
        let mut moved = false;
        if let Some(m) = self.sync.flush(&self.doc).unwrap() {
            hub.receive(s, &m).unwrap();
            moved = true;
        }
        while let Ok(m) = rx.try_recv() {
            self.received += 1;
            moved = true;
            if let Some(reply) = self.sync.receive(&self.doc, &m).unwrap().reply {
                hub.receive(s, &reply).unwrap();
            }
        }
        moved
    }

    fn text(&self) -> String {
        let o = self.doc.outline();
        o.slice(0, o.len)
    }
}

/// Runs every peer until no messages are moving.
fn settle(hub: &Hub, peers: &mut [&mut Peer]) {
    for _ in 0..100 {
        let mut moved = false;
        for p in peers.iter_mut() {
            moved |= p.step(hub);
        }
        if !moved {
            return;
        }
    }
    panic!("peers never settled");
}

fn hub_with(storage: Arc<dyn Storage>, compact_every: usize) -> Hub {
    Hub::new(
        storage,
        HubConfig {
            compact_every,
            ..HubConfig::default()
        },
    )
}

fn hub() -> Hub {
    hub_with(Arc::new(MemoryStorage::new()), 1000)
}

#[test]
fn live_edits_reach_the_other_client() {
    let hub = hub();
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    a.doc.insert(0, "hello").unwrap();
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(b.text(), "hello");

    // Concurrent edits at the same spot, then a table, typed into from the other side.
    a.doc.insert(5, " from a").unwrap();
    b.doc.insert(5, " from b").unwrap();
    let t = a.doc.insert_table(0, 1, 2).unwrap();
    settle(&hub, &mut [&mut a, &mut b]);
    let o = b.doc.outline();
    b.doc
        .insert_at(&o, o.cell_start(&t, 0, 1).unwrap(), "cell")
        .unwrap();
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(a.text(), b.text());
    assert!(
        a.text().contains("cell") && a.text().contains("from a") && a.text().contains("from b")
    );
}

#[test]
fn senders_get_no_echo_of_their_own_edits() {
    let hub = hub();
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    settle(&hub, &mut [&mut a, &mut b]);
    let before = a.received;
    for i in 0..5 {
        a.doc.insert(i, "x").unwrap();
    }
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(a.received, before, "a got its own edits back");
    // And b doesn't bounce them back either.
    assert!(b.sync.flush(&b.doc).unwrap().is_none());
}

#[test]
fn several_keystrokes_go_out_as_one_message() {
    let hub = hub();
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    settle(&hub, &mut [&mut a, &mut b]);
    let before = b.received;
    for (i, c) in "typing".chars().enumerate() {
        a.doc.insert(i, &c.to_string()).unwrap();
    }
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(b.received - before, 1);
    assert_eq!(b.text(), "typing");
}

#[test]
fn offline_edits_merge_on_reconnect() {
    let hub = hub();
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    a.doc.insert(0, "shared text").unwrap();
    settle(&hub, &mut [&mut a, &mut b]);

    a.disconnect(&hub);
    a.doc.insert(0, "offline ").unwrap(); // a, while away
    b.doc.insert(11, " online").unwrap(); // b, meanwhile
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(a.text(), "offline shared text");

    a.connect(&hub);
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(a.text(), "offline shared text online");
    assert_eq!(b.text(), a.text());
}

#[test]
fn a_new_client_with_offline_history_from_a_previous_session() {
    // A client that edited, lost the server entirely, then came back to a restarted server
    // (in-memory storage lost) re-uploads everything the server lacks.
    let hub1 = hub();
    let mut a = Peer::joined(&hub1);
    a.doc.insert(0, "survives").unwrap();
    settle(&hub1, &mut [&mut a]);
    a.disconnect(&hub1);

    let hub2 = hub();
    a.connect(&hub2);
    let mut b = Peer::joined(&hub2);
    settle(&hub2, &mut [&mut a, &mut b]);
    assert_eq!(b.text(), "survives");
}

#[test]
fn presence_is_relayed_and_cleared_on_leave() {
    let hub = hub();
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    a.doc.insert(0, "abcdef").unwrap();
    settle(&hub, &mut [&mut a, &mut b]);

    let o = a.doc.outline();
    let caret = a.doc.anchor_at(&o, 3).unwrap().encode();
    let me = Presence {
        name: "Ada".into(),
        color: "#0f766e".into(),
        selection: Some((caret.clone(), caret)),
    };
    let (s, _) = a.conn.as_ref().unwrap();
    hub.receive(s, &SyncClient::presence_message(&me)).unwrap();
    settle(&hub, &mut [&mut a, &mut b]);

    let (&id, seen) = b.sync.peers().iter().next().unwrap();
    assert_eq!(id, a.conn.as_ref().unwrap().0.client);
    assert_eq!(seen.name, "Ada");
    // B resolves A's caret in its own replica, after its own edit before it.
    b.doc.insert(0, ">>").unwrap();
    let (anchor, _) = seen.selection.clone().unwrap();
    let anchor = Anchor::decode(&anchor).unwrap();
    assert_eq!(b.doc.resolve_at(&b.doc.outline(), &anchor), Some(5));

    // A latecomer gets A's presence right after its handshake.
    let mut c = Peer::joined(&hub);
    settle(&hub, &mut [&mut a, &mut b, &mut c]);
    assert_eq!(c.sync.peers().len(), 1);

    a.disconnect(&hub);
    settle(&hub, &mut [&mut b, &mut c]);
    assert!(b.sync.peers().is_empty());
    assert!(c.sync.peers().is_empty());
}

#[test]
fn documents_survive_a_server_restart() {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
    {
        let hub = hub_with(storage.clone(), 1000);
        let mut a = Peer::joined(&hub);
        a.doc.insert(0, "persisted").unwrap();
        settle(&hub, &mut [&mut a]);
        // Server goes away with the client still connected: no final compaction.
    }
    let hub = hub_with(storage, 1000);
    let mut b = Peer::joined(&hub);
    settle(&hub, &mut [&mut b]);
    assert_eq!(b.text(), "persisted");
}

#[test]
fn compaction_folds_updates_into_a_snapshot() {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
    let hub = hub_with(storage.clone(), 5);
    let mut a = Peer::joined(&hub);
    for i in 0..12 {
        a.doc.insert(i, "x").unwrap();
        settle(&hub, &mut [&mut a]); // one stored update per keystroke
    }
    let stored = storage.load(DOC).unwrap();
    assert!(stored.snapshot.is_some());
    assert_eq!(stored.updates.len(), 2, "12 updates, compacted at 5 and 10");

    // The last client leaving compacts the rest and closes the room.
    a.disconnect(&hub);
    assert_eq!(hub.open_rooms(), 0);
    let stored = storage.load(DOC).unwrap();
    assert!(stored.updates.is_empty());

    let mut b = Peer::joined(&hub);
    settle(&hub, &mut [&mut b]);
    assert_eq!(b.text(), "x".repeat(12));
}

#[test]
fn a_client_that_falls_behind_is_dropped_and_catches_up_on_reconnect() {
    let hub = Hub::new(
        Arc::new(MemoryStorage::new()),
        HubConfig {
            outbox: 2,
            ..HubConfig::default()
        },
    );
    let (mut a, mut b) = (Peer::joined(&hub), Peer::joined(&hub));
    settle(&hub, &mut [&mut a, &mut b]);
    // B stops reading; A keeps sending.
    for i in 0..5 {
        a.doc.insert(i, "y").unwrap();
        a.step(&hub);
    }
    let (s, _) = b.conn.as_ref().unwrap();
    assert!(matches!(
        hub.receive(
            s,
            &SyncClient::presence_message(&Presence {
                name: String::new(),
                color: String::new(),
                selection: None,
            })
        ),
        Err(HubError::Disconnected)
    ));
    b.disconnect(&hub);
    b.connect(&hub);
    settle(&hub, &mut [&mut a, &mut b]);
    assert_eq!(b.text(), "yyyyy");
}

#[test]
fn rejects_bad_document_ids_and_garbage() {
    let hub = hub();
    for id in ["", "../etc", "a b", &"x".repeat(129)] {
        assert!(matches!(hub.join(id), Err(HubError::BadDocId)), "{id:?}");
    }
    let (s, _) = hub.join(DOC).unwrap();
    assert!(hub.receive(&s, b"\x09garbage").is_err());
    // The room still works afterwards.
    let mut a = Peer::joined(&hub);
    a.doc.insert(0, "fine").unwrap();
    settle(&hub, &mut [&mut a]);
}
