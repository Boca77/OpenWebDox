//! The wire format between editors and the sync server. One [`Message`] per WebSocket binary
//! frame: a tag byte, then the fields in order. Byte strings and text are a `u32` little-endian
//! length followed by the data; integers are `u32` little-endian.
//!
//! A session goes:
//!
//! 1. client → server [`Message::SyncRequest`] with the client's version;
//! 2. server → client [`Message::SyncReply`]: the edits the client is missing, and the server's
//!    version, so the client can send back what the server is missing (offline edits);
//! 3. then both sides send [`Message::Update`]s as edits happen, and clients send
//!    [`Message::Presence`] when their caret moves. The server relays presence and sends
//!    [`Message::Leave`] when a client disconnects.

use crate::SyncError;

const SYNC_REQUEST: u8 = 1;
const SYNC_REPLY: u8 = 2;
const UPDATE: u8 = 3;
const PRESENCE: u8 = 4;
const LEAVE: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Client → server, first message after connecting: an encoded `Version`.
    SyncRequest { version: Vec<u8> },
    /// Server → client: edits the client lacks, and the server's encoded `Version`.
    SyncReply { updates: Vec<u8>, version: Vec<u8> },
    /// Either direction: new edits, encoded by `Document::export_updates_since`.
    Update { updates: Vec<u8> },
    /// A client's caret, name and colour (an encoded [`Presence`]). `client` is the sender's
    /// connection id, filled in by the server when relaying; clients send 0.
    Presence { client: u32, payload: Vec<u8> },
    /// Server → client: connection `client` has gone; drop its caret.
    Leave { client: u32 },
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        match self {
            Message::SyncRequest { version } => {
                w.u8(SYNC_REQUEST);
                w.bytes(version);
            }
            Message::SyncReply { updates, version } => {
                w.u8(SYNC_REPLY);
                w.bytes(updates);
                w.bytes(version);
            }
            Message::Update { updates } => {
                w.u8(UPDATE);
                w.bytes(updates);
            }
            Message::Presence { client, payload } => {
                w.u8(PRESENCE);
                w.u32(*client);
                w.bytes(payload);
            }
            Message::Leave { client } => {
                w.u8(LEAVE);
                w.u32(*client);
            }
        }
        w.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Message, SyncError> {
        let mut r = Reader(bytes);
        let m = match r.u8()? {
            SYNC_REQUEST => Message::SyncRequest {
                version: r.bytes()?,
            },
            SYNC_REPLY => Message::SyncReply {
                updates: r.bytes()?,
                version: r.bytes()?,
            },
            UPDATE => Message::Update {
                updates: r.bytes()?,
            },
            PRESENCE => Message::Presence {
                client: r.u32()?,
                payload: r.bytes()?,
            },
            LEAVE => Message::Leave { client: r.u32()? },
            tag => return Err(SyncError::Malformed(format!("unknown message tag {tag}"))),
        };
        r.finish()?;
        Ok(m)
    }
}

/// What a client shows other people: who it is and where its selection is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub name: String,
    /// A CSS colour.
    pub color: String,
    /// Selection as encoded `Anchor`s: `(anchor, focus)`. `None` hides the caret.
    pub selection: Option<(Vec<u8>, Vec<u8>)>,
}

impl Presence {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.bytes(self.name.as_bytes());
        w.bytes(self.color.as_bytes());
        match &self.selection {
            Some((anchor, focus)) => {
                w.u8(1);
                w.bytes(anchor);
                w.bytes(focus);
            }
            None => w.u8(0),
        }
        w.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Presence, SyncError> {
        let mut r = Reader(bytes);
        let name = r.string()?;
        let color = r.string()?;
        let selection = match r.u8()? {
            0 => None,
            1 => Some((r.bytes()?, r.bytes()?)),
            x => return Err(SyncError::Malformed(format!("bad selection flag {x}"))),
        };
        r.finish()?;
        Ok(Presence {
            name,
            color,
            selection,
        })
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        // Frames are far below 4 GiB (the server caps message size), so the length fits.
        self.u32(u32::try_from(b.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(b);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SyncError> {
        if self.0.len() < n {
            return Err(SyncError::Malformed("message ends early".into()));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, SyncError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, SyncError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, SyncError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }

    fn string(&mut self) -> Result<String, SyncError> {
        String::from_utf8(self.bytes()?)
            .map_err(|_| SyncError::Malformed("text is not UTF-8".into()))
    }

    fn finish(&self) -> Result<(), SyncError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(SyncError::Malformed("trailing bytes".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let all = [
            Message::SyncRequest {
                version: vec![1, 2],
            },
            Message::SyncReply {
                updates: vec![],
                version: vec![9],
            },
            Message::Update {
                updates: vec![0; 300],
            },
            Message::Presence {
                client: 7,
                payload: vec![4],
            },
            Message::Leave { client: u32::MAX },
        ];
        for m in all {
            assert_eq!(Message::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn presence_round_trips() {
        let p = Presence {
            name: "Zoë 李".into(),
            color: "#c2410c".into(),
            selection: Some((vec![1, 2, 3], vec![])),
        };
        assert_eq!(Presence::decode(&p.encode()).unwrap(), p);
        let hidden = Presence {
            selection: None,
            ..p
        };
        assert_eq!(Presence::decode(&hidden.encode()).unwrap(), hidden);
    }

    #[test]
    fn rejects_malformed_messages() {
        assert!(Message::decode(&[]).is_err());
        assert!(Message::decode(&[99]).is_err());
        assert!(Message::decode(&[UPDATE, 5, 0, 0, 0, 1]).is_err());
        let mut extra = Message::Leave { client: 1 }.encode();
        extra.push(0);
        assert!(Message::decode(&extra).is_err());
    }
}
