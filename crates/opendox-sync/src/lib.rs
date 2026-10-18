//! Real-time sync for opendox documents: the wire protocol ([`protocol`]) and the client-side
//! session state ([`SyncClient`]). No networking here: the browser drives it through
//! `opendox-wasm` over a WebSocket, the server (`opendox-server`) speaks the same messages.

mod client;
pub mod protocol;

pub use client::{Received, SyncClient};
pub use protocol::{Message, Presence};

use opendox_model::ModelError;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SyncError {
    #[error("malformed sync message: {0}")]
    Malformed(String),

    #[error(transparent)]
    Model(#[from] ModelError),
}
