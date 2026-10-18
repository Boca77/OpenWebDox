//! opendox sync server: clients connect a WebSocket to `/doc/{id}` and speak the
//! `opendox-sync` protocol. Documents are kept in memory while open and persisted through a
//! [`Storage`] (SQLite by default).
//!
//! There is no authentication yet: anyone who can reach the server can open any document.

mod hub;
pub mod storage;

pub use hub::{Hub, HubConfig, HubError, Session};
pub use storage::{MemoryStorage, SqliteStorage, Storage, StorageError, Stored};

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;

/// Largest message accepted from a client. A paste of a long document is a few MB.
const MAX_MESSAGE: usize = 32 << 20;

/// The HTTP routes: `GET /doc/{id}` upgrades to a sync WebSocket.
pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/doc/{id}", get(upgrade))
        .with_state(hub)
}

/// Serves sync connections on `listener` until the process ends.
pub async fn serve(listener: TcpListener, hub: Arc<Hub>) -> std::io::Result<()> {
    axum::serve(listener, router(hub)).await
}

async fn upgrade(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(hub): State<Arc<Hub>>,
) -> Response {
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| connection(socket, hub, id))
        .into_response()
}

async fn connection(socket: WebSocket, hub: Arc<Hub>, doc: String) {
    let (session, mut outbox) = match hub.join(&doc) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("opendox-server: {doc}: join failed: {e}");
            return;
        }
    };
    let (mut sink, mut stream) = socket.split();
    let writer = async {
        while let Some(bytes) = outbox.recv().await {
            if sink.send(WsMessage::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
        // The hub dropped us (too slow) or the socket failed: close so the client reconnects.
        let _ = sink.close().await;
    };
    let reader = async {
        while let Some(Ok(msg)) = stream.next().await {
            let bytes = match msg {
                WsMessage::Binary(b) => b,
                WsMessage::Close(_) => break,
                _ => continue,
            };
            match hub.receive(&session, &bytes) {
                Ok(()) => {}
                Err(HubError::Disconnected) => break,
                // A bad message from one client shouldn't take the room down; log and go on.
                Err(e) => eprintln!("opendox-server: {doc}: client {}: {e}", session.client),
            }
        }
    };
    // Whichever side finishes first ends the connection.
    tokio::select! {
        () = writer => {}
        () = reader => {}
    }
    if let Err(e) = hub.leave(session) {
        eprintln!("opendox-server: {doc}: {e}");
    }
}
