use std::collections::BTreeMap;

use opendox_model::{Document, Version};

use crate::protocol::{Message, Presence};
use crate::SyncError;

/// The client side of a sync session, without any networking: feed it the server's messages,
/// send what it returns. Works the same in the browser (through `opendox-wasm`) and in tests.
///
/// Local edits are not sent one by one. Call [`SyncClient::flush`] on a timer (the editor uses
/// ~30 ms): it sends every edit made since the last flush as one [`Message::Update`].
#[derive(Debug, Default)]
pub struct SyncClient {
    /// Edits the server is known to have. `None` while disconnected or mid-handshake, when
    /// nothing may be sent: we don't yet know what the server is missing.
    server_has: Option<Version>,
    peers: BTreeMap<u32, Presence>,
    /// The server allows this connection to view only: never send edits.
    read_only: bool,
}

/// What a received message changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Received {
    /// The document has new content; relayout and remap the local selection.
    pub doc_changed: bool,
    /// A remote caret appeared, moved or left.
    pub peers_changed: bool,
    /// A message to send back right away.
    pub reply: Option<Vec<u8>>,
    /// The server says this connection may only view the document.
    pub read_only: bool,
}

impl SyncClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts the handshake on a new connection (also after a reconnect). Send the returned
    /// message first. Edits made while offline go out once the server replies.
    pub fn connect(&mut self, doc: &Document) -> Vec<u8> {
        self.disconnected();
        Message::SyncRequest {
            version: doc.version().encode(),
        }
        .encode()
    }

    /// The connection dropped: stop sending, forget remote carets. Local editing carries on.
    pub fn disconnected(&mut self) {
        self.server_has = None;
        self.peers.clear();
        self.read_only = false;
    }

    /// Whether the server allows this connection to view only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Whether the handshake has finished and edits are flowing.
    pub fn is_synced(&self) -> bool {
        self.server_has.is_some()
    }

    /// Every local edit the server doesn't have yet, as one message, or `None` if there are
    /// none or the handshake hasn't finished.
    pub fn flush(&mut self, doc: &Document) -> Result<Option<Vec<u8>>, SyncError> {
        let Some(has) = &mut self.server_has else {
            return Ok(None);
        };
        if self.read_only {
            return Ok(None);
        }
        let now = doc.version();
        if has.includes(&now) {
            return Ok(None);
        }
        let updates = doc.export_updates_since(has)?;
        has.merge(&now);
        Ok(Some(Message::Update { updates }.encode()))
    }

    /// Handles one message from the server.
    pub fn receive(&mut self, doc: &Document, bytes: &[u8]) -> Result<Received, SyncError> {
        let mut out = Received::default();
        match Message::decode(bytes)? {
            Message::SyncReply { updates, version } => {
                out.doc_changed = import(doc, &updates)?;
                self.server_has = Some(Version::decode(&version)?);
                out.reply = self.flush(doc)?;
            }
            Message::Update { updates } => {
                out.doc_changed = import(doc, &updates)?;
                // Whatever the server sent, it has. Our own unsent edits stay unsent.
                if let Some(has) = &mut self.server_has {
                    has.merge_except(&doc.version(), doc.replica_id());
                }
            }
            Message::Presence { client, payload } => {
                self.peers.insert(client, Presence::decode(&payload)?);
                out.peers_changed = true;
            }
            Message::Leave { client } => {
                out.peers_changed = self.peers.remove(&client).is_some();
            }
            Message::ReadOnly => {
                self.read_only = true;
                out.read_only = true;
            }
            Message::SyncRequest { .. } => {
                return Err(SyncError::Malformed("server sent a sync request".into()))
            }
        }
        Ok(out)
    }

    /// Other clients on the same document, by connection id.
    pub fn peers(&self) -> &BTreeMap<u32, Presence> {
        &self.peers
    }

    /// The message announcing this client's presence.
    pub fn presence_message(presence: &Presence) -> Vec<u8> {
        Message::Presence {
            client: 0,
            payload: presence.encode(),
        }
        .encode()
    }
}

/// Imports `updates`; returns whether the document gained anything.
fn import(doc: &Document, updates: &[u8]) -> Result<bool, SyncError> {
    if updates.is_empty() {
        return Ok(false);
    }
    let before = doc.version();
    doc.import(updates)?;
    Ok(doc.version() != before)
}
