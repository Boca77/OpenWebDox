//! The real server over real WebSockets: two clients edit live, one drops off, edits offline,
//! reconnects and merges.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use opendox_model::Document;
use opendox_server::{Hub, HubConfig, MemoryStorage};
use opendox_sync::SyncClient;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Client {
    doc: Document,
    sync: SyncClient,
    ws: Option<Ws>,
}

impl Client {
    async fn connect(&mut self, url: &str) {
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let hello = self.sync.connect(&self.doc);
        ws.send(Message::Binary(hello.into())).await.unwrap();
        self.ws = Some(ws);
    }

    async fn disconnect(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            ws.close(None).await.unwrap();
        }
        self.sync.disconnected();
    }

    /// Flushes local edits, then reads until the socket is quiet for a moment.
    async fn pump(&mut self) {
        let Some(ws) = &mut self.ws else { return };
        if let Some(m) = self.sync.flush(&self.doc).unwrap() {
            ws.send(Message::Binary(m.into())).await.unwrap();
        }
        while let Ok(Some(Ok(msg))) =
            tokio::time::timeout(Duration::from_millis(150), ws.next()).await
        {
            if let Message::Binary(b) = msg {
                let r = self.sync.receive(&self.doc, &b).unwrap();
                if let Some(reply) = r.reply {
                    ws.send(Message::Binary(reply.into())).await.unwrap();
                }
            }
        }
    }
}

fn client() -> Client {
    Client {
        doc: Document::new(),
        sync: SyncClient::new(),
        ws: None,
    }
}

#[tokio::test]
async fn two_clients_edit_live_and_merge_offline_edits() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/doc/live-test", listener.local_addr().unwrap());
    let hub = Arc::new(Hub::new(
        Arc::new(MemoryStorage::new()),
        HubConfig::default(),
    ));
    tokio::spawn(opendox_server::serve(listener, hub.clone()));

    let (mut a, mut b) = (client(), client());
    a.connect(&url).await;
    b.connect(&url).await;
    a.pump().await;
    b.pump().await;

    a.doc.insert(0, "Hello").unwrap();
    a.pump().await;
    b.pump().await;
    assert_eq!(b.doc.text(), "Hello");

    b.doc.insert(5, ", world").unwrap();
    b.pump().await;
    a.pump().await;
    assert_eq!(a.doc.text(), "Hello, world");

    // A drops off and keeps typing; B types too.
    a.disconnect().await;
    a.doc.insert(0, "[offline] ").unwrap();
    b.doc.insert(12, "!").unwrap();
    b.pump().await;

    a.connect(&url).await;
    a.pump().await;
    b.pump().await;
    assert_eq!(a.doc.text(), "[offline] Hello, world!");
    assert_eq!(b.doc.text(), a.doc.text());

    // Both gone: the room closes.
    a.disconnect().await;
    b.disconnect().await;
    for _ in 0..50 {
        if hub.open_rooms() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(hub.open_rooms(), 0);
}

#[tokio::test]
async fn unknown_routes_and_bad_ids_are_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hub = Arc::new(Hub::new(
        Arc::new(MemoryStorage::new()),
        HubConfig::default(),
    ));
    tokio::spawn(opendox_server::serve(listener, hub.clone()));

    assert!(
        tokio_tungstenite::connect_async(format!("ws://{addr}/nope"))
            .await
            .is_err()
    );
    // A bad id upgrades, then the server closes it straight away without opening a room.
    if let Ok((mut ws, _)) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/doc/a%20b")).await
    {
        let next = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .unwrap();
        assert!(!matches!(next, Some(Ok(Message::Binary(_)))));
    }
    assert_eq!(hub.open_rooms(), 0);
}
