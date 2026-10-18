use std::cell::Cell;
use std::ops::Range;

use loro::cursor::{Cursor, Side};
use loro::{
    Container, ExpandType, ExportMode, LoroDoc, LoroMap, LoroText, StyleConfig, StyleConfigMap,
    TextDelta, ValueOrContainer, VersionVector,
};

use crate::error::ModelError;
use crate::grapheme;
use crate::outline::{Outline, TableInput, TextRef};
use crate::table::{cell_container, CellRef, TableHandle, TABLES, TABLE_MARK, TABLE_PLACEHOLDER};

/// Name of the Loro text container holding the document body.
const BODY: &str = "body";

/// A collaborative document: body text with paragraphs separated by `\n`, and tables.
///
/// Each `Document` is one replica; replicas exchange the bytes from
/// [`Document::export_updates_since`] / [`Document::export_snapshot`] and apply them with
/// [`Document::import`]. Replicas that have seen the same updates hold identical content,
/// whatever order the updates arrived in.
///
/// Body-only methods ([`Document::insert`], [`Document::delete`], …) take body char indices.
/// Editors should work through an [`Outline`] and the `*_at` methods, which take flat positions
/// covering body and table cells alike.
pub struct Document {
    doc: LoroDoc,
    /// Makes ids created within one transaction unique.
    id_seq: Cell<u64>,
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
        Self::from_loro(LoroDoc::new())
    }

    /// Creates an empty document with a fixed replica id. Every replica of a document must use
    /// a different id; fixed ids are mainly for deterministic tests.
    pub fn with_replica_id(id: u64) -> Result<Self, ModelError> {
        let doc = LoroDoc::new();
        doc.set_peer_id(id).map_err(crdt)?;
        Ok(Self::from_loro(doc))
    }

    fn from_loro(doc: LoroDoc) -> Self {
        // Mark behaviour is local configuration, not synced: every replica must set it.
        // A table mark must never spread to text typed next to the placeholder.
        let mut styles = StyleConfigMap::new();
        styles.insert(
            TABLE_MARK.into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );
        doc.config_text_style(styles);
        Self {
            doc,
            id_seq: Cell::new(0),
        }
    }

    /// A fresh id, unique across replicas: replica id + a local sequence number.
    fn new_id(&self, prefix: &str) -> String {
        let seq = self.id_seq.get();
        self.id_seq.set(seq + 1);
        let peer = self.doc.peer_id();
        let ops = self.doc.oplog_vv().get(&peer).copied().unwrap_or(0);
        format!("{prefix}{peer:x}.{ops}.{seq}")
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

    /// Inserts `s` into the body at char index `pos`, which must be a grapheme boundary.
    pub fn insert(&self, pos: usize, s: &str) -> Result<(), ModelError> {
        self.insert_in(&TextRef::Body, pos, s)
    }

    /// Deletes body chars in `range`. Both ends must be grapheme boundaries.
    pub fn delete(&self, range: Range<usize>) -> Result<(), ModelError> {
        self.delete_in(&TextRef::Body, range)
    }

    /// The text of the body or of a cell.
    pub fn text_in(&self, container: &TextRef) -> Result<String, ModelError> {
        Ok(self.container(container)?.to_string())
    }

    /// Inserts `s` into a container at char index `pos` (a grapheme boundary).
    pub fn insert_in(&self, container: &TextRef, pos: usize, s: &str) -> Result<(), ModelError> {
        let text = self.container(container)?;
        check_boundary(&text.to_string(), pos)?;
        text.insert(pos, s).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Deletes chars in `range` of a container. Both ends must be grapheme boundaries.
    pub fn delete_in(&self, container: &TextRef, range: Range<usize>) -> Result<(), ModelError> {
        self.raw_delete(container, range)?;
        self.doc.commit();
        Ok(())
    }

    /// Like [`Document::delete_in`] without committing, so several deletes form one change.
    fn raw_delete(&self, container: &TextRef, range: Range<usize>) -> Result<(), ModelError> {
        if range.start >= range.end {
            return Ok(());
        }
        let text = self.container(container)?;
        let s = text.to_string();
        check_boundary(&s, range.start)?;
        check_boundary(&s, range.end)?;
        text.delete(range.start, range.end - range.start)
            .map_err(crdt)
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
}

/// Tables and the document outline.
impl Document {
    /// The document's structure: blocks, paragraphs to lay out, and flat positions.
    pub fn outline(&self) -> Outline {
        let tables = self
            .placeholders()
            .into_iter()
            .filter_map(|(placeholder, id)| {
                let cells = self.table(&id)?.cell_texts();
                Some(TableInput {
                    placeholder,
                    id,
                    cells,
                })
            })
            .collect();
        Outline::build(&self.text(), tables)
    }

    /// Inserts a `rows` × `cols` table at body char index `pos` and returns its id. The table
    /// goes on its own line: the paragraph is split at `pos` if `pos` is mid-paragraph.
    pub fn insert_table(&self, pos: usize, rows: usize, cols: usize) -> Result<String, ModelError> {
        if rows == 0 || cols == 0 {
            return Err(ModelError::InvalidTable);
        }
        let body = self.body();
        let text: Vec<char> = body.to_string().chars().collect();
        check_boundary(&body.to_string(), pos)?;
        let at_paragraph_start = pos == 0 || text.get(pos - 1) == Some(&'\n');
        let lead = if at_paragraph_start { "" } else { "\n" };
        body.insert(pos, &format!("{lead}{TABLE_PLACEHOLDER}\n"))
            .map_err(crdt)?;
        let placeholder = pos + lead.len();
        let id = self.new_id("t");
        body.mark(placeholder..placeholder + 1, TABLE_MARK, id.as_str())
            .map_err(crdt)?;
        let map = self
            .tables_map()
            .insert_container(&id, LoroMap::new())
            .map_err(crdt)?;
        let columns: Vec<String> = (0..cols).map(|_| self.new_id("c")).collect();
        TableHandle { map }.init(&columns, rows).map_err(crdt)?;
        self.doc.commit();
        Ok(id)
    }

    /// Inserts an empty row so it becomes row `at`.
    pub fn insert_row(&self, table: &str, at: usize) -> Result<(), ModelError> {
        let t = self.table(table).ok_or(ModelError::NoSuchTable)?;
        let rows = t.rows_list().ok_or(ModelError::NoSuchTable)?;
        let row = rows
            .insert_container(at.min(rows.len()), LoroMap::new())
            .map_err(crdt)?;
        for col in t.columns() {
            row.insert_container(&col, LoroText::new()).map_err(crdt)?;
        }
        self.doc.commit();
        Ok(())
    }

    pub fn delete_row(&self, table: &str, at: usize) -> Result<(), ModelError> {
        let t = self.table(table).ok_or(ModelError::NoSuchTable)?;
        let rows = t.rows_list().ok_or(ModelError::NoSuchTable)?;
        if at >= rows.len() {
            return Err(ModelError::NoSuchCell);
        }
        rows.delete(at, 1).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// Inserts an empty column so it becomes column `at`.
    pub fn insert_column(&self, table: &str, at: usize) -> Result<(), ModelError> {
        let t = self.table(table).ok_or(ModelError::NoSuchTable)?;
        let columns = t.columns_list().ok_or(ModelError::NoSuchTable)?;
        let id = self.new_id("c");
        columns
            .insert(at.min(columns.len()), id.as_str())
            .map_err(crdt)?;
        for row in t.rows() {
            row.insert_container(&id, LoroText::new()).map_err(crdt)?;
        }
        self.doc.commit();
        Ok(())
    }

    pub fn delete_column(&self, table: &str, at: usize) -> Result<(), ModelError> {
        let t = self.table(table).ok_or(ModelError::NoSuchTable)?;
        let columns = t.columns_list().ok_or(ModelError::NoSuchTable)?;
        let id = t.columns().get(at).cloned().ok_or(ModelError::NoSuchCell)?;
        columns.delete(at, 1).map_err(crdt)?;
        for row in t.rows() {
            row.delete(&id).map_err(crdt)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// Removes a table and the line break that kept it on its own line.
    pub fn delete_table(&self, table: &str) -> Result<(), ModelError> {
        let (k, _) = self
            .placeholders()
            .into_iter()
            .find(|(_, id)| id == table)
            .ok_or(ModelError::NoSuchTable)?;
        let body = self.body();
        let followed_by_break = body.to_string().chars().nth(k + 1) == Some('\n');
        body.delete(k, if followed_by_break { 2 } else { 1 })
            .map_err(crdt)?;
        self.tables_map().delete(table).map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }
}

/// Editing by flat position (see [`Outline`]). `outline` must describe the current state.
impl Document {
    /// Inserts `s` at flat position `pos`; returns the flat position just after it.
    pub fn insert_at(&self, outline: &Outline, pos: usize, s: &str) -> Result<usize, ModelError> {
        // A pasted placeholder would look like a table to nothing; drop it.
        let clean: String = s.chars().filter(|&c| c != TABLE_PLACEHOLDER).collect();
        if clean.is_empty() {
            return Ok(pos);
        }
        let loc = outline.locate(pos);
        self.insert_in(&loc.container, loc.offset, &clean)?;
        Ok(pos + clean.chars().count())
    }

    /// Deletes everything between two flat positions; returns where the caret goes.
    ///
    /// Within one container this is a plain delete. Across cells of one table, the covered
    /// part of each cell is cleared and the table stays. Anything spanning body text and a
    /// table removes the whole table.
    pub fn delete_range_at(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
    ) -> Result<usize, ModelError> {
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi {
            return Ok(lo);
        }
        let (la, lb) = (outline.locate(lo), outline.locate(hi));
        if la.container == lb.container {
            self.delete_in(&la.container, la.offset..lb.offset)?;
            return Ok(lo);
        }
        let (ta, tb) = (outline.table_at(lo), outline.table_at(hi));
        if let (Some(t), Some(u)) = (ta, tb) {
            if t.id == u.id {
                for cell in t.cells.iter().flatten() {
                    let first = &outline.slots[cell.start];
                    let start = first.flat;
                    let end = outline.slots[cell.end - 1].flat_end();
                    let (from, to) = (lo.max(start), hi.min(end));
                    if from < to {
                        self.raw_delete(&first.container, from - start..to - start)?;
                    }
                }
                self.doc.commit();
                return Ok(lo);
            }
        }
        let body_lo = ta.map_or(la.offset, |t| t.placeholder);
        let body_hi = tb.map_or(lb.offset, |t| t.placeholder + 1);
        self.delete_in(&TextRef::Body, body_lo..body_hi)?;
        Ok(outline.flat_of_body(body_lo))
    }

    /// Backspace at a caret: deletes the previous grapheme, or joins this paragraph to the
    /// previous one in the same container. Never reaches across a cell or table edge.
    pub fn delete_backward_at(&self, outline: &Outline, pos: usize) -> Result<usize, ModelError> {
        if let Some(prev) = outline.prev_in_slot(pos) {
            return self.delete_range_at(outline, prev, pos);
        }
        let Some((i, 0)) = outline.slot_at(pos) else {
            return Ok(pos);
        };
        let Some(prev) = i.checked_sub(1).map(|p| &outline.slots[p]) else {
            return Ok(pos);
        };
        let cur = &outline.slots[i];
        if joinable(prev, cur) {
            self.delete_in(&cur.container, cur.offset - 1..cur.offset)?;
            return Ok(prev.flat_end());
        }
        Ok(pos)
    }

    /// Delete key at a caret: the mirror of [`Document::delete_backward_at`].
    pub fn delete_forward_at(&self, outline: &Outline, pos: usize) -> Result<usize, ModelError> {
        if let Some(next) = outline.next_in_slot(pos) {
            return self.delete_range_at(outline, pos, next);
        }
        let Some((i, _)) = outline.slot_at(pos) else {
            return Ok(pos);
        };
        let (Some(cur), Some(next)) = (outline.slots.get(i), outline.slots.get(i + 1)) else {
            return Ok(pos);
        };
        if joinable(cur, next) {
            self.delete_in(&next.container, next.offset - 1..next.offset)?;
        }
        Ok(pos)
    }
}

/// Internals for tables and containers.
impl Document {
    fn tables_map(&self) -> LoroMap {
        self.doc.get_map(TABLES)
    }

    fn table(&self, id: &str) -> Option<TableHandle> {
        match self.tables_map().get(id)? {
            ValueOrContainer::Container(Container::Map(map)) => Some(TableHandle { map }),
            _ => None,
        }
    }

    /// Body placeholders that refer to an existing table: `(char index, table id)`.
    fn placeholders(&self) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> = Vec::new();
        let mut pos = 0;
        for d in self.body().to_delta() {
            let TextDelta::Insert { insert, attributes } = d else {
                continue;
            };
            let id = attributes
                .as_ref()
                .and_then(|a| a.get(TABLE_MARK))
                .and_then(|v| v.as_string().map(|s| s.to_string()));
            for c in insert.chars() {
                if let (TABLE_PLACEHOLDER, Some(id)) = (c, &id) {
                    // One placeholder per table; a duplicate would render the table twice.
                    if self.table(id).is_some() && !out.iter().any(|(_, x)| x == id) {
                        out.push((pos, id.clone()));
                    }
                }
                pos += 1;
            }
        }
        out
    }

    fn container(&self, r: &TextRef) -> Result<LoroText, ModelError> {
        match r {
            TextRef::Body => Ok(self.body()),
            TextRef::Cell(c) => self.cell(c),
        }
    }

    /// A cell's text container, created if a concurrent column/row change left it missing.
    fn cell(&self, c: &CellRef) -> Result<LoroText, ModelError> {
        let t = self.table(&c.table).ok_or(ModelError::NoSuchTable)?;
        let col = t
            .columns()
            .get(c.col)
            .cloned()
            .ok_or(ModelError::NoSuchCell)?;
        let row = t.rows().get(c.row).cloned().ok_or(ModelError::NoSuchCell)?;
        match cell_container(&row, &col) {
            Some(text) => Ok(text),
            None => row.insert_container(&col, LoroText::new()).map_err(crdt),
        }
    }
}

/// Whether two consecutive paragraphs are separated by exactly one `\n` in the same container,
/// so deleting that `\n` joins them.
fn joinable(prev: &crate::outline::Slot, next: &crate::outline::Slot) -> bool {
    prev.container == next.container && prev.offset + prev.text.chars().count() + 1 == next.offset
}

fn check_boundary(text: &str, pos: usize) -> Result<(), ModelError> {
    let b = grapheme::boundaries(text);
    let len = b.last().copied().unwrap_or(0);
    if pos > len {
        return Err(ModelError::OutOfBounds { pos, len });
    }
    if b.binary_search(&pos).is_err() {
        return Err(ModelError::NotGraphemeBoundary { pos });
    }
    Ok(())
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
