//! Comments on text: a range, the comment, replies, and whether it's resolved.
//!
//! A comment's range is two CRDT cursors, not marks: comments can overlap freely, and the
//! range follows concurrent edits. The start cursor holds on to the range's first character
//! and the end cursor to its last, so text typed just before or after the range stays outside
//! it (as in Google Docs) while text typed inside it is part of it. If all of the commented
//! text is deleted the comment stays, with an empty range.
//!
//! Everything about a comment is one Loro map in `comments`, one key per field, so replies,
//! resolving and edits from several people merge.

use loro::{LoroList, LoroMap, LoroValue, ValueOrContainer};
use serde::Serialize;

use crate::document::{Anchor, Document};
use crate::error::ModelError;
use crate::outline::Outline;

const COMMENTS: &str = "comments";

/// One reply to a comment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reply {
    pub author: String,
    pub text: String,
    /// Milliseconds since the Unix epoch, as the replying client said.
    pub created: f64,
}

/// A comment as it stands, with its range in flat positions per an outline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comment {
    pub id: String,
    /// `start..end` in flat positions; `start == end` once its text is all deleted.
    pub start: usize,
    pub end: usize,
    pub author: String,
    pub text: String,
    pub created: f64,
    pub resolved: bool,
    pub replies: Vec<Reply>,
}

impl Document {
    /// Comments the range between two flat positions (which must not be empty) and returns
    /// the new comment's id. `created` is milliseconds since the Unix epoch.
    pub fn add_comment(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
        author: &str,
        text: &str,
        created: f64,
    ) -> Result<String, ModelError> {
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi {
            return Err(ModelError::EmptyComment);
        }
        let start = self.anchor_at(outline, lo)?;
        // The last character of the range: see the module docs.
        let end = self.anchor_at(outline, outline.move_left(hi).max(lo))?;
        let id = self.new_id("c");
        let map = self
            .comments_map()
            .insert_container(&id, LoroMap::new())
            .map_err(crdt)?;
        map.insert("start", LoroValue::from(start.encode()))
            .map_err(crdt)?;
        map.insert("end", LoroValue::from(end.encode()))
            .map_err(crdt)?;
        map.insert("author", author).map_err(crdt)?;
        map.insert("text", text).map_err(crdt)?;
        map.insert("created", created).map_err(crdt)?;
        map.insert_container("replies", LoroList::new())
            .map_err(crdt)?;
        self.doc.commit();
        Ok(id)
    }

    /// Every comment, ordered by where its range starts (then by when it was made).
    pub fn comments(&self, outline: &Outline) -> Vec<Comment> {
        let map = self.comments_map();
        let mut out: Vec<Comment> = Vec::new();
        for id in map.keys() {
            let Some(ValueOrContainer::Container(c)) = map.get(&id) else {
                continue;
            };
            let Ok(m) = c.into_map() else {
                continue;
            };
            let anchor = |key: &str| Anchor::decode(&binary(&m, key)?).ok();
            let (Some(a), Some(z)) = (anchor("start"), anchor("end")) else {
                continue;
            };
            let (Some(start), Some(last)) =
                (self.resolve_at(outline, &a), self.resolve_at(outline, &z))
            else {
                continue;
            };
            // The end cursor holds the range's last character: the range ends just after it.
            // Once that character is deleted the cursor sits where it was, which is the end.
            let end = if self.anchor_alive(&z) {
                outline.move_right(last)
            } else {
                last
            }
            .max(start);
            out.push(Comment {
                id: id.to_string(),
                start,
                end,
                author: string(&m, "author"),
                text: string(&m, "text"),
                created: number(&m, "created"),
                resolved: matches!(
                    m.get("resolved"),
                    Some(ValueOrContainer::Value(LoroValue::Bool(true)))
                ),
                replies: replies(&m),
            });
        }
        out.sort_by(|a, b| {
            (a.start, a.created)
                .partial_cmp(&(b.start, b.created))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Adds a reply to comment `id`.
    pub fn reply_to_comment(
        &self,
        id: &str,
        author: &str,
        text: &str,
        created: f64,
    ) -> Result<(), ModelError> {
        let m = self.comment_map(id)?;
        let list = match m.get("replies") {
            Some(ValueOrContainer::Container(c)) => {
                c.into_list().map_err(|_| ModelError::NoSuchComment)?
            }
            _ => m
                .insert_container("replies", LoroList::new())
                .map_err(crdt)?,
        };
        let r = list
            .insert_container(list.len(), LoroMap::new())
            .map_err(crdt)?;
        r.insert("author", author).map_err(crdt)?;
        r.insert("text", text).map_err(crdt)?;
        r.insert("created", created).map_err(crdt)?;
        // Replying reopens a resolved comment, as in Google Docs.
        m.delete("resolved").map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Marks comment `id` resolved (hidden from the text) or reopens it.
    pub fn resolve_comment(&self, id: &str, resolved: bool) -> Result<(), ModelError> {
        let m = self.comment_map(id)?;
        if resolved {
            m.insert("resolved", true).map_err(crdt)?;
        } else {
            m.delete("resolved").map_err(crdt)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// Changes comment `id`'s own text.
    pub fn edit_comment(&self, id: &str, text: &str) -> Result<(), ModelError> {
        self.comment_map(id)?.insert("text", text).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Deletes comment `id` with its replies.
    pub fn delete_comment(&self, id: &str) -> Result<(), ModelError> {
        self.comment_map(id)?;
        self.comments_map().delete(id).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    fn comments_map(&self) -> LoroMap {
        self.doc.get_map(COMMENTS)
    }

    fn comment_map(&self, id: &str) -> Result<LoroMap, ModelError> {
        match self.comments_map().get(id) {
            Some(ValueOrContainer::Container(c)) => {
                c.into_map().map_err(|_| ModelError::NoSuchComment)
            }
            _ => Err(ModelError::NoSuchComment),
        }
    }
}

fn binary(m: &LoroMap, key: &str) -> Option<Vec<u8>> {
    match m.get(key)? {
        ValueOrContainer::Value(LoroValue::Binary(b)) => Some(b.to_vec()),
        _ => None,
    }
}

fn string(m: &LoroMap, key: &str) -> String {
    match m.get(key) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => s.to_string(),
        _ => String::new(),
    }
}

fn number(m: &LoroMap, key: &str) -> f64 {
    match m.get(key) {
        Some(ValueOrContainer::Value(LoroValue::Double(d))) => d,
        Some(ValueOrContainer::Value(LoroValue::I64(i))) => i as f64,
        _ => 0.0,
    }
}

fn replies(m: &LoroMap) -> Vec<Reply> {
    let Some(ValueOrContainer::Container(c)) = m.get("replies") else {
        return Vec::new();
    };
    let Ok(list) = c.into_list() else {
        return Vec::new();
    };
    (0..list.len())
        .filter_map(|i| match list.get(i)? {
            ValueOrContainer::Container(c) => c.into_map().ok(),
            ValueOrContainer::Value(_) => None,
        })
        .map(|r| Reply {
            author: string(&r, "author"),
            text: string(&r, "text"),
            created: number(&r, "created"),
        })
        .collect()
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
