//! Who may open which document, and whether they may edit it.
//!
//! The server asks an [`Authorizer`] once per connection, with the document id and the
//! `token` query parameter of the WebSocket URL (browsers can't set headers on WebSockets).
//! Built in: [`AllowAll`] for local development, and [`SignedTokens`]: tokens an application
//! server issues with a secret it shares with the sync server, naming the document (or every
//! document), the access and an expiry. The sync server keeps no user list; deciding who gets
//! a token is the application's job.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// What a connection may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read the document and show a caret; edits are ignored.
    View,
    Edit,
}

impl Access {
    fn key(self) -> &'static str {
        match self {
            Access::View => "view",
            Access::Edit => "edit",
        }
    }
}

/// Decides whether a connection to `doc` with `token` may proceed. `None` refuses it.
pub trait Authorizer: Send + Sync + 'static {
    fn authorize(&self, doc: &str, token: Option<&str>) -> Option<Access>;
}

/// Lets everyone edit everything. Only for development on your own machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl Authorizer for AllowAll {
    fn authorize(&self, _doc: &str, _token: Option<&str>) -> Option<Access> {
        Some(Access::Edit)
    }
}

type HmacSha256 = Hmac<Sha256>;

/// Tokens signed with HMAC-SHA256: `{doc}.{view|edit}.{expiry}.{signature}`, where `doc` is a
/// document id or `*` for every document, `expiry` Unix seconds and `signature` the hex MAC
/// of everything before it. Document ids can't contain `.`, so the parts are unambiguous, and
/// the whole token is URL-safe.
#[derive(Clone)]
pub struct SignedTokens {
    secret: Vec<u8>,
}

impl std::fmt::Debug for SignedTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        f.debug_struct("SignedTokens").finish_non_exhaustive()
    }
}

impl SignedTokens {
    /// Uses `secret` to sign and check tokens. Use at least 32 random bytes.
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// A token giving `access` to `doc` (or `"*"` for all documents) until `expires` (Unix
    /// seconds).
    pub fn issue(&self, doc: &str, access: Access, expires: u64) -> String {
        let payload = format!("{doc}.{}.{expires}", access.key());
        format!("{payload}.{}", hex(&self.mac(&payload)))
    }

    fn mac(&self, payload: &str) -> Vec<u8> {
        // HMAC takes keys of any length, so this can't fail.
        let mut m = HmacSha256::new_from_slice(&self.secret).unwrap_or_else(|_| unreachable!());
        m.update(payload.as_bytes());
        m.finalize().into_bytes().to_vec()
    }

    /// Checks `token` for `doc` at time `now` (Unix seconds).
    pub fn check(&self, doc: &str, token: &str, now: u64) -> Option<Access> {
        let (payload, sig) = token.rsplit_once('.')?;
        let mut parts = payload.split('.');
        let (for_doc, access, expires) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let sig = unhex(sig)?;
        let mut m = HmacSha256::new_from_slice(&self.secret).ok()?;
        m.update(payload.as_bytes());
        // Constant-time comparison.
        m.verify_slice(&sig).ok()?;
        if for_doc != "*" && for_doc != doc {
            return None;
        }
        if expires.parse::<u64>().ok()? <= now {
            return None;
        }
        match access {
            "edit" => Some(Access::Edit),
            "view" => Some(Access::View),
            _ => None,
        }
    }
}

impl Authorizer for SignedTokens {
    fn authorize(&self, doc: &str, token: Option<&str>) -> Option<Access> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.check(doc, token?, now)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}
