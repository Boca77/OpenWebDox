use std::cell::Cell;
use std::ops::Range;

use loro::cursor::{Cursor, Side};
use loro::{
    Container, ContainerTrait, ExpandType, ExportMode, LoroDoc, LoroMap, LoroText, StyleConfig,
    StyleConfigMap, TextDelta, ValueOrContainer, VersionVector,
};

use crate::error::ModelError;
use crate::format::{mark_paragraph, CharFormat, ParagraphChange, CHAR_KEYS, LINK, PARAGRAPH_KEYS};
use crate::grapheme;
use crate::image::IMAGE_MARK;
use crate::outline::{FootnoteSlots, Outline, TableInput, TextRef};
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
    pub(crate) doc: LoroDoc,
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
pub struct Anchor(pub(crate) Cursor);

/// The set of edits a replica has seen. Pass it to [`Document::export_updates_since`] to get
/// only what that replica is missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version(VersionVector);

impl Anchor {
    /// Bytes for sending the anchor to another replica (e.g. a remote caret).
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ModelError> {
        Cursor::decode(bytes)
            .map(Anchor)
            .map_err(|e| ModelError::Decode(e.to_string()))
    }
}

impl Version {
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ModelError> {
        VersionVector::decode(bytes)
            .map(Version)
            .map_err(|e| ModelError::Decode(e.to_string()))
    }

    /// Adds every edit `other` has seen.
    pub fn merge(&mut self, other: &Version) {
        self.0.merge(&other.0);
    }

    /// Adds every edit `other` has seen, except those made by replica `skip`.
    pub fn merge_except(&mut self, other: &Version, skip: u64) {
        for (&peer, &counter) in other.0.iter() {
            if peer != skip && self.0.get(&peer).is_none_or(|&c| c < counter) {
                self.0.insert(peer, counter);
            }
        }
    }

    /// Whether this version has seen every edit `other` has.
    pub fn includes(&self, other: &Version) -> bool {
        self.0.includes_vv(&other.0)
    }
}

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
        // A table mark must never spread to text typed next to the placeholder, nor a
        // paragraph format to the text before its line break. Character formats continue
        // into text typed at their end.
        let mut styles = StyleConfigMap::new();
        let never = StyleConfig {
            expand: ExpandType::None,
        };
        styles.insert(TABLE_MARK.into(), never);
        styles.insert(IMAGE_MARK.into(), never);
        styles.insert(crate::header::FIELD_MARK.into(), never);
        styles.insert(crate::footnote::FOOTNOTE_MARK.into(), never);
        // Typing right after a link isn't part of it.
        styles.insert(LINK.into(), never);
        for key in PARAGRAPH_KEYS {
            styles.insert(key.into(), never);
        }
        for key in CHAR_KEYS {
            styles.insert(
                key.into(),
                StyleConfig {
                    expand: ExpandType::After,
                },
            );
        }
        doc.config_text_style(styles);
        Self {
            doc,
            id_seq: Cell::new(0),
        }
    }

    /// A fresh id, unique across replicas: replica id + a local sequence number.
    pub(crate) fn new_id(&self, prefix: &str) -> String {
        let seq = self.id_seq.get();
        self.id_seq.set(seq + 1);
        let peer = self.doc.peer_id();
        let ops = self.doc.oplog_vv().get(&peer).copied().unwrap_or(0);
        format!("{prefix}{peer:x}.{ops}.{seq}")
    }

    pub(crate) fn body(&self) -> LoroText {
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
    ///
    /// Line breaks in `s` split the paragraph; every piece keeps its paragraph format. Text
    /// typed at the start of a paragraph takes the format of the text after it.
    pub fn insert_in(&self, container: &TextRef, pos: usize, s: &str) -> Result<(), ModelError> {
        self.raw_insert(container, pos, s, None)?;
        self.doc.commit();
        Ok(())
    }

    /// Inserts without committing. With `format`, the new text gets exactly that format
    /// instead of the one it would pick up from its neighbours.
    pub(crate) fn raw_insert(
        &self,
        container: &TextRef,
        pos: usize,
        s: &str,
        format: Option<&CharFormat>,
    ) -> Result<(), ModelError> {
        let text = self.container(container)?;
        check_boundary(&text, pos)?;
        let split = s
            .contains('\n')
            .then(|| self.paragraph_format_in(container, pos));
        let prev = pos.checked_sub(1).and_then(|p| text.char_at(p).ok());
        let next = text.char_at(pos).ok();
        let lead = (prev.is_none_or(|c| c == '\n')
            && next.is_some_and(|c| c != '\n' && c != TABLE_PLACEHOLDER))
        .then(|| CharFormat {
            // A link never spreads to text typed before it.
            link: None,
            ..self.char_format_in(&text, pos)
        });

        text.insert(pos, s).map_err(crdt)?;
        let n = s.chars().count();
        if let Some(f) = format.cloned().or(lead) {
            self.set_char_format(&text, pos..pos + n, &f)?;
        }
        if let Some(f) = split {
            // A page break before the paragraph stays with its first piece only.
            let mut rest = f;
            rest.apply(ParagraphChange::PageBreakBefore(false));
            let mut first = true;
            for (i, c) in s.chars().enumerate() {
                if c == '\n' {
                    mark_paragraph(&text, pos + i, if first { f } else { rest })?;
                    first = false;
                }
            }
            // Enter at the end of a ticked checklist item starts an unticked one, and at the
            // end of a heading or title starts normal text, as in Google Docs.
            let mut last = rest;
            if next.is_none_or(|c| c == '\n' || c == TABLE_PLACEHOLDER) {
                last.apply(ParagraphChange::Checked(false));
                last.apply(ParagraphChange::Style(crate::style::StyleName::Normal));
            }
            if last != f {
                self.write_paragraph_format(container, pos + n, last)?;
            }
        }
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
        check_boundary(&text, range.start)?;
        check_boundary(&text, range.end)?;
        // Joining paragraphs keeps the first one's format, as in Google Docs. Its line break,
        // which held the format, is among the deleted characters.
        let joins = text
            .slice(range.start, range.end)
            .map_err(crdt)?
            .contains('\n');
        let first = joins.then(|| self.paragraph_format_in(container, range.start));
        text.delete(range.start, range.end - range.start)
            .map_err(crdt)?;
        if let Some(f) = first {
            self.write_paragraph_format(container, range.start, f)?;
        }
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

    /// This replica's id: the one edits made here are tagged with.
    pub fn replica_id(&self) -> u64 {
        self.doc.peer_id()
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
        let (tables, notes) = self.scan_body();
        let tables = tables
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
        let mut o = Outline::build(&self.text(), tables);
        if self.has_header() {
            let text = self.container(&TextRef::Header).map(|t| t.to_string());
            o.header = o.add_region(TextRef::Header, &text.unwrap_or_default());
        }
        let end = o.slots.len();
        o.footer = end..end;
        if self.has_footer() {
            let text = self.container(&TextRef::Footer).map(|t| t.to_string());
            o.footer = o.add_region(TextRef::Footer, &text.unwrap_or_default());
        }
        for (reference, id) in self.note_refs(&o, notes) {
            let text = self.note_text(&id).map(|t| t.to_string());
            let slots = o.add_region(TextRef::Footnote(id.clone()), &text.unwrap_or_default());
            o.footnotes.push(FootnoteSlots {
                id,
                reference,
                slots,
            });
        }
        o
    }

    /// Inserts a `rows` × `cols` table at body char index `pos` and returns its id. The table
    /// goes on its own line: the paragraph is split at `pos` if `pos` is mid-paragraph.
    pub fn insert_table(&self, pos: usize, rows: usize, cols: usize) -> Result<String, ModelError> {
        if rows == 0 || cols == 0 {
            return Err(ModelError::InvalidTable);
        }
        let body = self.body();
        let text: Vec<char> = body.to_string().chars().collect();
        check_boundary(&body, pos)?;
        let at_paragraph_start = pos == 0 || text.get(pos - 1) == Some(&'\n');
        let lead = if at_paragraph_start { "" } else { "\n" };
        let split = self.paragraph_format_in(&TextRef::Body, pos);
        body.insert(pos, &format!("{lead}{TABLE_PLACEHOLDER}\n"))
            .map_err(crdt)?;
        if !lead.is_empty() {
            mark_paragraph(&body, pos, split)?;
        }
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
            Document::delete_cell_tail(&row, &id)?;
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
        // A pasted placeholder would look like a table or image to nothing; drop it.
        let clean: String = s.chars().filter(|&c| c != TABLE_PLACEHOLDER).collect();
        if clean.is_empty() {
            return Ok(pos);
        }
        let loc = outline.locate(pos);
        self.insert_in(&loc.container, loc.offset, &clean)?;
        Ok(pos + clean.chars().count())
    }

    /// [`Document::insert_at`] giving the new text exactly `format`, as one change. For typing
    /// after toggling a format with nothing selected.
    pub fn insert_formatted_at(
        &self,
        outline: &Outline,
        pos: usize,
        s: &str,
        format: &CharFormat,
    ) -> Result<usize, ModelError> {
        let clean: String = s.chars().filter(|&c| c != TABLE_PLACEHOLDER).collect();
        if clean.is_empty() {
            return Ok(pos);
        }
        let loc = outline.locate(pos);
        self.raw_insert(&loc.container, loc.offset, &clean, Some(format))?;
        self.doc.commit();
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
        // Never across regions: a selection from the body into the footer deletes only the
        // body part.
        let hi = hi.min(outline.region_bounds(lo).1);
        if lo >= hi {
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

    /// Creates an anchor at flat position `pos`, in body text or in a cell. Like
    /// [`Document::anchor`], it follows the text it sits next to through other replicas' edits.
    pub fn anchor_at(&self, outline: &Outline, pos: usize) -> Result<Anchor, ModelError> {
        let loc = outline.locate(outline.snap(pos));
        let (text, offset) = match &loc.container {
            TextRef::Body => (self.body(), loc.offset),
            TextRef::Header | TextRef::Footer | TextRef::Footnote(_) => {
                (self.container(&loc.container)?, loc.offset)
            }
            TextRef::Cell(c) => match self.existing_cell(c) {
                Some(text) => (text, loc.offset),
                // A cell that was never written has no container to anchor to (creating one
                // here would be an edit); the table's placeholder is the nearest stable spot.
                None => {
                    let t = outline.table(&c.table).ok_or(ModelError::NoSuchTable)?;
                    (self.body(), t.placeholder)
                }
            },
        };
        text.get_cursor(offset, Side::Middle)
            .map(Anchor)
            .ok_or(ModelError::AnchorLost)
    }

    /// Current flat position of an anchor from [`Document::anchor_at`], or `None` if the cell or
    /// table it was in has been deleted. `outline` must describe the current state.
    /// Whether the character an anchor holds on to still exists (it hasn't been deleted).
    pub(crate) fn anchor_alive(&self, anchor: &Anchor) -> bool {
        self.doc
            .get_cursor_pos(&anchor.0)
            .is_ok_and(|r| r.update.is_none())
    }

    pub fn resolve_at(&self, outline: &Outline, anchor: &Anchor) -> Option<usize> {
        let pos = self.doc.get_cursor_pos(&anchor.0).ok()?.current.pos;
        let id = &anchor.0.container;
        if *id == self.body().id() {
            return Some(outline.snap(outline.flat_of_body(pos)));
        }
        for region in [TextRef::Header, TextRef::Footer] {
            if self.container(&region).is_ok_and(|t| t.id() == *id) {
                return outline.flat_in_region(&region, pos);
            }
        }
        for n in &outline.footnotes {
            if self.note_text(&n.id).is_some_and(|t| t.id() == *id) {
                return outline.flat_in_region(&TextRef::Footnote(n.id.clone()), pos);
            }
        }
        for t in outline.tables() {
            let Some(handle) = self.table(&t.id) else {
                continue;
            };
            let columns = handle.columns();
            for (r, row) in handle.rows().iter().enumerate() {
                for (c, col) in columns.iter().enumerate() {
                    if cell_container(row, col).is_none_or(|text| text.id() != *id) {
                        continue;
                    }
                    let slots = &outline.slots[t.cells.get(r)?.get(c)?.clone()];
                    let slot = slots.iter().rev().find(|s| s.offset <= pos)?;
                    let within = (pos - slot.offset).min(slot.text.chars().count());
                    return Some(slot.flat + within);
                }
            }
        }
        None
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

    pub(crate) fn table(&self, id: &str) -> Option<TableHandle> {
        match self.tables_map().get(id)? {
            ValueOrContainer::Container(Container::Map(map)) => Some(TableHandle { map }),
            _ => None,
        }
    }

    /// Body placeholders that refer to an existing table: `(char index, table id)`.
    pub(crate) fn placeholders(&self) -> Vec<(usize, String)> {
        self.scan_body().0
    }

    /// One pass over the body's placeholders: the tables (as [`Document::placeholders`]) and
    /// the footnote references, `(char index, note id)`.
    #[allow(clippy::type_complexity)]
    fn scan_body(&self) -> (Vec<(usize, String)>, Vec<(usize, String)>) {
        let mut tables: Vec<(usize, String)> = Vec::new();
        let mut notes: Vec<(usize, String)> = Vec::new();
        let mut pos = 0;
        for d in self.body().to_delta() {
            let TextDelta::Insert { insert, attributes } = d else {
                continue;
            };
            let mark = |key: &str| {
                attributes
                    .as_ref()
                    .and_then(|a| a.get(key))
                    .and_then(|v| v.as_string().map(|s| s.to_string()))
            };
            let table = mark(TABLE_MARK);
            let note = mark(crate::footnote::FOOTNOTE_MARK);
            if table.is_none() && note.is_none() {
                pos += insert.chars().count();
                continue;
            }
            for c in insert.chars() {
                if c == TABLE_PLACEHOLDER {
                    if let Some(id) = &table {
                        // One placeholder per table; a duplicate would render the table twice.
                        if self.table(id).is_some() && !tables.iter().any(|(_, x)| x == id) {
                            tables.push((pos, id.clone()));
                        }
                    }
                    if let Some(id) = &note {
                        notes.push((pos, id.clone()));
                    }
                }
                pos += 1;
            }
        }
        (tables, notes)
    }

    pub(crate) fn container(&self, r: &TextRef) -> Result<LoroText, ModelError> {
        match r {
            TextRef::Body => Ok(self.body()),
            TextRef::Cell(c) => self.cell(c),
            TextRef::Header => Ok(self.doc.get_text(crate::header::HEADER)),
            TextRef::Footer => Ok(self.doc.get_text(crate::header::FOOTER)),
            TextRef::Footnote(id) => self.note_text(id).ok_or(ModelError::NoSuchFootnote),
        }
    }

    /// A cell's text container if it exists, without creating it.
    pub(crate) fn existing_cell(&self, c: &CellRef) -> Option<LoroText> {
        let t = self.table(&c.table)?;
        let col = t.columns().get(c.col).cloned()?;
        let row = t.rows().get(c.row).cloned()?;
        cell_container(&row, &col)
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

/// Checks that `pos` is in `text` and on a grapheme boundary, looking only at the text around
/// it: grapheme clusters are far shorter than the window (the longest real ones, emoji ZWJ
/// sequences, are a dozen chars), so the cost doesn't grow with the document.
fn check_boundary(text: &LoroText, pos: usize) -> Result<(), ModelError> {
    const WINDOW: usize = 64;
    let len = text.len_unicode();
    if pos > len {
        return Err(ModelError::OutOfBounds { pos, len });
    }
    let start = pos.saturating_sub(WINDOW);
    let around = text.slice(start, (pos + WINDOW).min(len)).map_err(crdt)?;
    if grapheme::boundaries(&around)
        .binary_search(&(pos - start))
        .is_err()
    {
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
    fn cell_anchor_travels_to_another_replica_and_follows_edits() {
        let a = Document::with_replica_id(1).unwrap();
        let b = Document::with_replica_id(2).unwrap();
        a.insert(0, "intro").unwrap();
        let t = a.insert_table(5, 2, 2).unwrap();
        let o = a.outline();
        let c11 = o.cell_start(&t, 1, 1).unwrap();
        a.insert_at(&o, c11, "world").unwrap();
        sync(&a, &b);

        // A's caret between "wor" and "ld", sent to B as bytes.
        let o = a.outline();
        let bytes = a.anchor_at(&o, c11 + 3).unwrap().encode();
        let on_b = Anchor::decode(&bytes).unwrap();

        // B types before it in the same cell, and in the body before the table.
        let ob = b.outline();
        b.insert_at(&ob, ob.cell_start(&t, 1, 1).unwrap(), "hi ")
            .unwrap();
        b.insert(0, ">").unwrap();
        let ob = b.outline();
        let at = b.resolve_at(&ob, &on_b).unwrap();
        assert_eq!(ob.slice(at - 6, at), "hi wor");
        assert_eq!(ob.cell_at(at).map(|c| (c.row, c.col)), Some((1, 1)));

        // Body anchors resolve through the same call.
        let body = b.anchor_at(&ob, 3).unwrap();
        b.insert(0, "x").unwrap();
        assert_eq!(b.resolve_at(&b.outline(), &body), Some(4));

        // Deleting the table loses the cell anchor.
        b.delete_table(&t).unwrap();
        assert_eq!(b.resolve_at(&b.outline(), &on_b), None);
    }

    #[test]
    fn anchor_in_a_never_written_cell_falls_back_to_the_table() {
        let a = Document::with_replica_id(1).unwrap();
        let b = Document::with_replica_id(2).unwrap();
        a.insert(0, "ab").unwrap();
        let t = a.insert_table(2, 1, 1).unwrap();
        sync(&a, &b);
        // A concurrent row + column insert leaves cell (1, 1) without a container.
        a.insert_row(&t, 1).unwrap();
        b.insert_column(&t, 1).unwrap();
        sync(&a, &b);
        let o = a.outline();
        let before = a.version();
        let anchor = a.anchor_at(&o, o.cell_start(&t, 1, 1).unwrap()).unwrap();
        assert_eq!(a.version(), before, "anchoring must not edit the document");
        let at = a.resolve_at(&o, &anchor).unwrap();
        assert_eq!(o.snap(at), at);
        assert!(matches!(
            Anchor::decode(b"\xff\xff"),
            Err(ModelError::Decode(_))
        ));
    }

    #[test]
    fn versions_encode_and_merge_without_a_replica() {
        let a = Document::with_replica_id(1).unwrap();
        let b = Document::with_replica_id(2).unwrap();
        a.insert(0, "aaa").unwrap();
        b.insert(0, "bb").unwrap();
        sync(&a, &b);
        let v = Version::decode(&a.version().encode()).unwrap();
        assert_eq!(v, a.version());

        let mut known = Version::default();
        known.merge_except(&a.version(), 1);
        assert!(!known.includes(&a.version()));
        // Everything except replica 1's edits: exporting since `known` sends only "aaa".
        let fresh = Document::new();
        fresh
            .import(&b.export_updates_since(&Version::default()).unwrap())
            .unwrap();
        let only_a = Document::new();
        only_a
            .import(&a.export_updates_since(&known).unwrap())
            .unwrap();
        assert_eq!(only_a.text(), "aaa");
        assert_eq!(a.replica_id(), 1);
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
