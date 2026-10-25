//! opendox sync server: clients connect a WebSocket to `/doc/{id}` and speak the
//! `opendox-sync` protocol. Documents are kept in memory while open and persisted through a
//! [`Storage`] (SQLite by default).
//!
//! Who may open and edit a document is an [`Authorizer`]'s decision, from a `token` query
//! parameter: [`serve`] lets everyone in (for development on your own machine), [`serve_with`]
//! takes the authorizer to use, e.g. [`SignedTokens`].

pub mod auth;
mod hub;
pub mod storage;

pub use auth::{Access, AllowAll, Authorizer, SignedTokens};
pub use hub::{Hub, HubConfig, HubError, Session};
pub use storage::{MemoryStorage, SqliteStorage, Storage, StorageError, Stored};

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;

/// Largest message accepted from a client. A paste of a long document is a few MB.
const MAX_MESSAGE: usize = 32 << 20;

#[derive(Clone)]
struct AppState {
    hub: Arc<Hub>,
    auth: Arc<dyn Authorizer>,
}

/// The HTTP routes, letting everyone in: `GET /doc/{id}` upgrades to a sync WebSocket. For
/// development only; see [`router_with`].
pub fn router(hub: Arc<Hub>) -> Router {
    router_with(hub, Arc::new(AllowAll))
}

/// The HTTP routes: `GET /doc/{id}?token=…` upgrades to a sync WebSocket if `auth` lets the
/// token open that document, and answers 403 otherwise.
pub fn router_with(hub: Arc<Hub>, auth: Arc<dyn Authorizer>) -> Router {
    Router::new()
        .route("/doc/{id}", get(upgrade))
        .with_state(AppState { hub, auth })
}

/// Serves sync connections on `listener`, letting everyone in (development only).
pub async fn serve(listener: TcpListener, hub: Arc<Hub>) -> std::io::Result<()> {
    axum::serve(listener, router(hub)).await
}

/// Serves sync connections on `listener`, checking each with `auth`.
pub async fn serve_with(
    listener: TcpListener,
    hub: Arc<Hub>,
    auth: Arc<dyn Authorizer>,
) -> std::io::Result<()> {
    axum::serve(listener, router_with(hub, auth)).await
}

async fn upgrade(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Response {
    let token = query.get("token").map(String::as_str);
    let Some(access) = state.auth.authorize(&id, token) else {
        return (StatusCode::FORBIDDEN, "not allowed to open this document").into_response();
    };
    let hub = state.hub;
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| connection(socket, hub, id, access))
        .into_response()
}

async fn connection(socket: WebSocket, hub: Arc<Hub>, doc: String, access: Access) {
    let (session, mut outbox) = match hub.join_with(&doc, access) {
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
