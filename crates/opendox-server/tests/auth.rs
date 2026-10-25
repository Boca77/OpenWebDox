//! Access control: signed tokens, view-only sessions that can't edit, and the real server
//! refusing connections without a valid token.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use opendox_model::Document;
use opendox_server::{Access, Hub, HubConfig, HubError, MemoryStorage, SignedTokens};
use opendox_sync::{Message, SyncClient};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const SECRET: &str = "a test secret that is long enough";

#[test]
fn signed_tokens_open_the_document_they_name_until_they_expire() {
    let t = SignedTokens::new(SECRET);
    let edit = t.issue("report", Access::Edit, 2_000);
    assert_eq!(t.check("report", &edit, 1_000), Some(Access::Edit));
    assert_eq!(t.check("other", &edit, 1_000), None, "another document");
    assert_eq!(t.check("report", &edit, 2_000), None, "expired");
    let view = t.issue("report", Access::View, 2_000);
    assert_eq!(t.check("report", &view, 1_000), Some(Access::View));
    let all = t.issue("*", Access::Edit, 2_000);
    assert_eq!(t.check("anything", &all, 1_000), Some(Access::Edit));
}

#[test]
fn forged_or_mangled_tokens_are_refused() {
    let t = SignedTokens::new(SECRET);
    let view = t.issue("report", Access::View, 2_000);
    // Upgrading the access, moving the expiry or changing the document breaks the signature.
    for forged in [
        view.replace(".view.", ".edit."),
        view.replace(".2000.", ".9999."),
        view.replacen("report", "other", 1),
    ] {
        assert_eq!(t.check("report", &forged, 1_000), None, "{forged}");
        assert_eq!(t.check("other", &forged, 1_000), None, "{forged}");
    }
    // Another secret's token.
    let other =
        SignedTokens::new("some other secret, also long").issue("report", Access::Edit, 2_000);
    assert_eq!(t.check("report", &other, 1_000), None);
    for junk in [
        "",
        ".",
        "report.edit.2000",
        "report.edit.2000.zz",
        "a.b.c.d.e",
    ] {
        assert_eq!(t.check("report", junk, 1_000), None, "{junk:?}");
    }
}

#[test]
fn view_only_sessions_are_told_and_cannot_edit() {
    let hub = Hub::new(Arc::new(MemoryStorage::new()), HubConfig::default());
    let (viewer, mut inbox) = hub.join_with("doc", Access::View).unwrap();
    assert_eq!(
        Message::decode(&inbox.try_recv().unwrap()).unwrap(),
        Message::ReadOnly
    );
    let d = Document::new();
    d.insert(0, "sneaky").unwrap();
    let update = Message::Update {
        updates: d.export_updates_since(&Document::new().version()).unwrap(),
    }
    .encode();
    assert!(matches!(
        hub.receive(&viewer, &update),
        Err(HubError::ReadOnly)
    ));
    // An editor sees nothing of it.
    let (editor, mut out) = hub.join("doc").unwrap();
    let hello = SyncClient::new().connect(&Document::new());
    hub.receive(&editor, &hello).unwrap();
    let Message::SyncReply { updates, .. } = Message::decode(&out.try_recv().unwrap()).unwrap()
    else {
        panic!("expected a sync reply");
    };
    let fresh = Document::new();
    if !updates.is_empty() {
        fresh.import(&updates).unwrap();
    }
    assert_eq!(fresh.text(), "");
}

#[tokio::test]
async fn the_server_needs_a_valid_token() {
    let hub = Arc::new(Hub::new(
        Arc::new(MemoryStorage::new()),
        HubConfig::default(),
    ));
    let tokens = SignedTokens::new(SECRET);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(opendox_server::serve_with(
        listener,
        hub,
        Arc::new(tokens.clone()),
    ));
    let url = |q: &str| format!("ws://{addr}/doc/plan{q}");

    // No token, or one for another document: refused before any WebSocket opens.
    assert!(tokio_tungstenite::connect_async(url("")).await.is_err());
    let other = tokens.issue("other", Access::Edit, u64::MAX / 2);
    assert!(
        tokio_tungstenite::connect_async(url(&format!("?token={other}")))
            .await
            .is_err()
    );

    // A view token connects and is told it's read-only.
    let view = tokens.issue("plan", Access::View, u64::MAX / 2);
    let (mut ws, _) = tokio_tungstenite::connect_async(url(&format!("?token={view}")))
        .await
        .unwrap();
    let mut client = SyncClient::new();
    let doc = Document::new();
    ws.send(WsMessage::Binary(client.connect(&doc).into()))
        .await
        .unwrap();
    let mut told = false;
    while let Ok(Some(Ok(WsMessage::Binary(b)))) =
        tokio::time::timeout(Duration::from_millis(300), ws.next()).await
    {
        told |= client.receive(&doc, &b).unwrap().read_only;
    }
    assert!(told);
    assert!(client.is_read_only());
    // So the client never sends its edits.
    doc.insert(0, "x").unwrap();
    assert_eq!(client.flush(&doc).unwrap(), None);

    // An edit token connects normally.
    let edit = tokens.issue("plan", Access::Edit, u64::MAX / 2);
    assert!(
        tokio_tungstenite::connect_async(url(&format!("?token={edit}")))
            .await
            .is_ok()
    );
}
