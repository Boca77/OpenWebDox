use std::ops::Range;

use loro::cursor::{Cursor, Side};
use loro::{ExportMode, LoroDoc, LoroText, VersionVector};

use crate::error::ModelError;
use crate::grapheme;

/// Name of the Loro text container holding the document body.
const BODY: &str = "body";

/// A collaborative plain-text document.
///
/// Paragraphs are separated by `\n`. Each `Document` is one replica; replicas exchange the
/// bytes from [`Document::export_updates_since`] / [`Document::export_snapshot`] and apply them
/// with [`Document::import`]. Replicas that have seen the same updates hold identical text,
/// whatever order the updates arrived in.
pub struct Document {
    doc: LoroDoc,
}

/// One paragraph: its text (without the trailing `\n`) and where it starts in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    /// Char index of the paragraph's first character.
    pub start: usize,
    pub text: String,
}

/// A position that follows the text it was created next to, so it stays correct when other
/// replicas insert or delete before it. Used for carets, selections and remote cursors.
#[derive(Debug, Clone)]
pub struct Anchor(Cursor);

/// The set of edits a replica has seen. Pass it to [`Document::export_updates_since`] to get
/// only what that replica is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version(VersionVector);

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates an empty document with a random replica id.
    pub fn new() -> Self {
        Self {
            doc: LoroDoc::new(),
        }
    }

    /// Creates an empty document with a fixed replica id. Every replica of a document must use
    /// a different id; fixed ids are mainly for deterministic tests.
    pub fn with_replica_id(id: u64) -> Result<Self, ModelError> {
        let doc = LoroDoc::new();
        doc.set_peer_id(id).map_err(crdt)?;
        Ok(Self { doc })
    }

    fn body(&self) -> LoroText {
        self.doc.get_text(BODY)
    }

    /// The full document text.
    pub fn text(&self) -> String {
        self.body().to_string()
    }

    /// Length in chars.
    pub fn len(&self) -> usize {
        self.body().len_unicode()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Inserts `s` at char index `pos`, which must be a grapheme boundary.
    pub fn insert(&self, pos: usize, s: &str) -> Result<(), ModelError> {
        self.check_boundary(pos)?;
        self.body().insert(pos, s).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Deletes the chars in `range`. Both ends must be grapheme boundaries.
    pub fn delete(&self, range: Range<usize>) -> Result<(), ModelError> {
        if range.start >= range.end {
            return Ok(());
        }
        self.check_boundary(range.start)?;
        self.check_boundary(range.end)?;
        self.body()
            .delete(range.start, range.end - range.start)
            .map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Splits the text into paragraphs on `\n`. An empty document has one empty paragraph, and
    /// a trailing `\n` produces a trailing empty paragraph, matching what the user sees.
    pub fn paragraphs(&self) -> Vec<Paragraph> {
        let text = self.text();
        let mut out = Vec::new();
        let mut start = 0;
        for line in text.split('\n') {
            let len = line.chars().count();
            out.push(Paragraph {
                start,
                text: line.to_owned(),
            });
            start += len + 1;
        }
        out
    }

    /// Whether `pos` is a grapheme boundary (0 and the end of the document always are).
    pub fn is_grapheme_boundary(&self, pos: usize) -> bool {
        grapheme::boundaries(&self.text())
            .binary_search(&pos)
            .is_ok()
    }

    /// The nearest grapheme boundary strictly before `pos`, for Backspace and Left arrow.
    pub fn prev_grapheme_boundary(&self, pos: usize) -> Option<usize> {
        let b = grapheme::boundaries(&self.text());
        let i = b.partition_point(|&x| x < pos);
        i.checked_sub(1).map(|i| b[i])
    }

    /// The nearest grapheme boundary strictly after `pos`, for Delete and Right arrow.
    pub fn next_grapheme_boundary(&self, pos: usize) -> Option<usize> {
        let b = grapheme::boundaries(&self.text());
        let i = b.partition_point(|&x| x <= pos);
        b.get(i).copied()
    }

    /// Creates an anchor at `pos`. It sticks to the character after `pos` (or to the end of the
    /// document), so text another replica inserts exactly at `pos` lands before the anchor.
    pub fn anchor(&self, pos: usize) -> Result<Anchor, ModelError> {
        self.check_in_bounds(pos)?;
        self.body()
            .get_cursor(pos, Side::Middle)
            .map(Anchor)
            .ok_or(ModelError::AnchorLost)
    }

    /// Current char index of `anchor`.
    pub fn resolve(&self, anchor: &Anchor) -> Result<usize, ModelError> {
        self.doc
            .get_cursor_pos(&anchor.0)
            .map(|r| r.current.pos)
            .map_err(|_| ModelError::AnchorLost)
    }

    /// The edits this replica has seen so far.
    pub fn version(&self) -> Version {
        Version(self.doc.oplog_vv())
    }

    /// Encodes every edit this replica has that `since` does not.
    pub fn export_updates_since(&self, since: &Version) -> Result<Vec<u8>, ModelError> {
        self.doc.export(ExportMode::updates(&since.0)).map_err(crdt)
    }

    /// Encodes the whole document, history included, for storage or for a new replica.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, ModelError> {
        self.doc.export(ExportMode::snapshot()).map_err(crdt)
    }

    /// Applies updates or a snapshot from another replica. Safe to call with bytes already
    /// applied, or out of order: edits whose dependencies are missing wait until they arrive.
    pub fn import(&self, bytes: &[u8]) -> Result<(), ModelError> {
        self.doc
            .import(bytes)
            .map(|_| ())
            .map_err(|e| ModelError::Import(e.to_string()))
    }

    fn check_in_bounds(&self, pos: usize) -> Result<(), ModelError> {
        let len = self.len();
        if pos > len {
            return Err(ModelError::OutOfBounds { pos, len });
        }
        Ok(())
    }

    fn check_boundary(&self, pos: usize) -> Result<(), ModelError> {
        self.check_in_bounds(pos)?;
        if !self.is_grapheme_boundary(pos) {
            return Err(ModelError::NotGraphemeBoundary { pos });
        }
        Ok(())
    }
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync(a: &Document, b: &Document) {
        b.import(&a.export_updates_since(&b.version()).unwrap())
            .unwrap();
        a.import(&b.export_updates_since(&a.version()).unwrap())
            .unwrap();
    }

    #[test]
    fn insert_and_delete() {
        let d = Document::new();
        d.insert(0, "hello world").unwrap();
        d.delete(5..11).unwrap();
        d.insert(5, "!").unwrap();
        assert_eq!(d.text(), "hello!");
        assert_eq!(d.len(), 6);
    }

    #[test]
    fn positions_count_chars_not_bytes() {
        let d = Document::new();
        d.insert(0, "äb").unwrap();
        d.insert(1, "X").unwrap();
        assert_eq!(d.text(), "äXb");
    }

    #[test]
    fn rejects_out_of_bounds() {
        let d = Document::new();
        d.insert(0, "ab").unwrap();
        assert_eq!(
            d.insert(3, "x"),
            Err(ModelError::OutOfBounds { pos: 3, len: 2 })
        );
    }

    #[test]
    fn rejects_edits_inside_a_grapheme() {
        let d = Document::new();
        d.insert(0, "e\u{301}").unwrap();
        assert_eq!(
            d.insert(1, "x"),
            Err(ModelError::NotGraphemeBoundary { pos: 1 })
        );
        assert_eq!(
            d.delete(0..1),
            Err(ModelError::NotGraphemeBoundary { pos: 1 })
        );
        d.delete(0..2).unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn grapheme_navigation_skips_whole_emoji() {
        let d = Document::new();
        d.insert(0, "a👨‍👩‍👧b").unwrap();
        assert_eq!(d.next_grapheme_boundary(1), Some(6));
        assert_eq!(d.prev_grapheme_boundary(6), Some(1));
        assert_eq!(d.prev_grapheme_boundary(0), None);
        assert_eq!(d.next_grapheme_boundary(7), None);
    }

    #[test]
    fn paragraphs_split_on_newline() {
        let d = Document::new();
        d.insert(0, "one\ntwo\n").unwrap();
        assert_eq!(
            d.paragraphs(),
            vec![
                Paragraph {
                    start: 0,
                    text: "one".into()
                },
                Paragraph {
                    start: 4,
                    text: "two".into()
                },
                Paragraph {
                    start: 8,
                    text: String::new()
                },
            ]
        );
        assert_eq!(Document::new().paragraphs().len(), 1);
    }

    #[test]
    fn anchor_moves_with_remote_insert_before_it() {
        let a = Document::with_replica_id(1).unwrap();
        let b = Document::with_replica_id(2).unwrap();
        a.insert(0, "hello world").unwrap();
        sync(&a, &b);

        // Caret on replica A between "hello " and "world".
        let caret = a.anchor(6).unwrap();
        b.insert(0, ">> ").unwrap();
        sync(&a, &b);

        assert_eq!(a.text(), ">> hello world");
        assert_eq!(a.resolve(&caret).unwrap(), 9);
    }

    #[test]
    fn anchor_survives_deletion_of_its_character() {
        let d = Document::new();
        d.insert(0, "abcdef").unwrap();
        let caret = d.anchor(3).unwrap();
        d.delete(2..5).unwrap();
        assert_eq!(d.text(), "abf");
        assert_eq!(d.resolve(&caret).unwrap(), 2);
    }

    #[test]
    fn anchor_at_end_stays_at_end() {
        let d = Document::new();
        d.insert(0, "ab").unwrap();
        let end = d.anchor(2).unwrap();
        d.insert(2, "cd").unwrap();
        assert_eq!(d.resolve(&end).unwrap(), 4);
    }

    #[test]
    fn anchor_counts_chars_after_emoji() {
        let d = Document::new();
        d.insert(0, "👨‍👩‍👧x").unwrap();
        let caret = d.anchor(5).unwrap();
        d.insert(0, "é").unwrap();
        assert_eq!(d.resolve(&caret).unwrap(), 6);
    }

    #[test]
    fn snapshot_restores_document() {
        let a = Document::new();
        a.insert(0, "saved text").unwrap();
        let b = Document::new();
        b.import(&a.export_snapshot().unwrap()).unwrap();
        assert_eq!(b.text(), "saved text");
    }

    #[test]
    fn import_rejects_garbage() {
        let d = Document::new();
        assert!(matches!(
            d.import(b"not a loro update"),
            Err(ModelError::Import(_))
        ));
    }
}
