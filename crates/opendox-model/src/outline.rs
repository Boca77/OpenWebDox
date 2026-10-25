//! A read-only snapshot of the document's structure: blocks (paragraphs and tables), the
//! paragraphs ("slots") that layout lays out, and a single position space covering body and cells.
//!
//! # Flat positions
//!
//! Editors address every caret position with one number. Body positions count body chars as
//! usual, except that each table placeholder is replaced by its cells, in row-major order: a cell
//! whose text has `n` chars takes `n + 1` positions (its caret positions `0..=n`). So a table
//! between "ab" and "cd" with cells "x" and "" reads `a b ¶ [x ·] [·] ¶ c d`, and positions grow
//! in reading order across the whole document.
//!
//! Positions just before and just after a placeholder belong to no paragraph; [`Outline::snap`]
//! moves such positions to the nearest real caret position.
//!
//! # Regions
//!
//! The page header and footer, when the document has them, come after the body in the same
//! position space: the header's first paragraph starts one past the body's last position
//! ([`Outline::len`]), the footer one past the header's end. Footnotes follow, in number
//! order, each one past the end of what comes before it. Moving and deleting never cross from
//! one region (body, header, footer, footnotes) into another, and deleting never crosses from
//! one footnote into another.

use std::ops::Range;

use crate::grapheme;
use crate::table::{CellRef, TABLE_PLACEHOLDER};

/// Which text container a position or paragraph lives in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextRef {
    Body,
    Cell(CellRef),
    /// The page header, repeated on every page.
    Header,
    /// The page footer, repeated on every page.
    Footer,
    /// A footnote's text, by note id.
    Footnote(String),
}

/// The part of the document a position is in. Table cells belong to the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Body,
    Header,
    Footer,
    /// Any footnote's text.
    Footnote,
}

impl TextRef {
    pub fn region(&self) -> Region {
        match self {
            TextRef::Body | TextRef::Cell(_) => Region::Body,
            TextRef::Header => Region::Header,
            TextRef::Footer => Region::Footer,
            TextRef::Footnote(_) => Region::Footnote,
        }
    }
}

/// A position inside one text container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub container: TextRef,
    /// Char offset within the container's text.
    pub offset: usize,
}

/// One paragraph of body or cell text: what layout lays out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub container: TextRef,
    /// Char offset of the paragraph's start within its container.
    pub offset: usize,
    pub text: String,
    /// Flat position of the paragraph's start.
    pub flat: usize,
}

impl Slot {
    /// Flat position just after the paragraph's last char.
    pub fn flat_end(&self) -> usize {
        self.flat + self.text.chars().count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A body paragraph: index into [`Outline::slots`].
    Paragraph(usize),
    Table(TableBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub id: String,
    /// Body char index of the placeholder.
    pub placeholder: usize,
    pub columns: usize,
    /// For each row, for each cell: its paragraphs as a range of slot indices.
    pub cells: Vec<Vec<Range<usize>>>,
    /// Flat positions covered by the cells: `first..=last`.
    pub flat_first: usize,
    pub flat_last: usize,
}

impl TableBlock {
    pub fn rows(&self) -> usize {
        self.cells.len()
    }
}

/// Input to [`Outline::build`]: a table found in the body.
pub(crate) struct TableInput {
    pub(crate) placeholder: usize,
    pub(crate) id: String,
    pub(crate) cells: Vec<Vec<String>>,
}

/// A footnote in the outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteSlots {
    pub id: String,
    /// Flat position of its reference (the placeholder) in body or cell text.
    pub reference: usize,
    /// Its paragraphs, as indices into [`Outline::slots`].
    pub slots: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The body's blocks (header and footer paragraphs aren't blocks).
    pub blocks: Vec<Block>,
    pub slots: Vec<Slot>,
    /// Largest flat position in the body.
    pub len: usize,
    /// Slots of the header's paragraphs (empty without a header).
    pub header: Range<usize>,
    /// Slots of the footer's paragraphs (empty without a footer).
    pub footer: Range<usize>,
    /// The footnotes in number order (note 1 first): in the order of their references.
    pub footnotes: Vec<FootnoteSlots>,
}

impl Outline {
    /// Builds the outline from the body text and the tables whose placeholders it contains
    /// (sorted by placeholder index).
    pub(crate) fn build(body: &str, tables: Vec<TableInput>) -> Outline {
        let chars: Vec<char> = body.chars().collect();
        let n = chars.len();
        let placeholders: Vec<usize> = tables.iter().map(|t| t.placeholder).collect();
        let is_table = |i: usize| placeholders.contains(&i);
        let mut out = Outline {
            blocks: Vec::new(),
            slots: Vec::new(),
            len: 0,
            header: 0..0,
            footer: 0..0,
            footnotes: Vec::new(),
        };
        // Extra flat positions contributed by tables seen so far.
        let mut shift = 0usize;
        let mut seg_start = 0usize;
        let mut tables = tables.into_iter().peekable();

        let emit_paragraph = |out: &mut Outline, a: usize, b: usize, shift: usize| {
            // Empty stretches right next to a table aren't paragraphs: they're the line
            // breaks that keep the table on its own line. The very end of the document is the
            // exception, so there's always somewhere to type after a final table.
            let next_to_table = (a > 0 && is_table(a - 1)) || (b < n && is_table(b));
            if a == b && next_to_table && b != n {
                return;
            }
            out.slots.push(Slot {
                container: TextRef::Body,
                offset: a,
                text: chars[a..b].iter().collect(),
                flat: a + shift,
            });
            out.blocks.push(Block::Paragraph(out.slots.len() - 1));
        };

        for (i, &c) in chars.iter().enumerate() {
            if c == '\n' {
                emit_paragraph(&mut out, seg_start, i, shift);
                seg_start = i + 1;
            } else if c == TABLE_PLACEHOLDER && tables.peek().is_some_and(|t| t.placeholder == i) {
                emit_paragraph(&mut out, seg_start, i, shift);
                seg_start = i + 1;
                let Some(t) = tables.next() else { break };
                let flat_first = i + shift + 1;
                let mut flat = flat_first;
                let mut cells = Vec::with_capacity(t.cells.len());
                for (r, row) in t.cells.iter().enumerate() {
                    let mut row_slots = Vec::with_capacity(row.len());
                    for (col, text) in row.iter().enumerate() {
                        let first = out.slots.len();
                        let cell = CellRef {
                            table: t.id.clone(),
                            row: r,
                            col,
                        };
                        let mut offset = 0;
                        for para in text.split('\n') {
                            out.slots.push(Slot {
                                container: TextRef::Cell(cell.clone()),
                                offset,
                                text: para.to_owned(),
                                flat: flat + offset,
                            });
                            offset += para.chars().count() + 1;
                        }
                        row_slots.push(first..out.slots.len());
                        flat += text.chars().count() + 1;
                    }
                    cells.push(row_slots);
                }
                let span = flat - flat_first; // positions taken by all cells
                out.blocks.push(Block::Table(TableBlock {
                    id: t.id,
                    placeholder: i,
                    columns: t.cells.first().map_or(0, Vec::len),
                    cells,
                    flat_first,
                    flat_last: flat - 1,
                }));
                shift += span;
            }
        }
        emit_paragraph(&mut out, seg_start, n, shift);
        out.len = n + shift;
        let end = out.slots.len();
        out.header = end..end;
        out.footer = end..end;
        out
    }

    /// Adds a header or footer's text after everything so far, one slot per paragraph, and
    /// returns their slot range.
    pub(crate) fn add_region(&mut self, container: TextRef, text: &str) -> Range<usize> {
        let first = self.slots.len();
        let mut flat = self.slots.last().map_or(0, Slot::flat_end) + 1;
        let mut offset = 0;
        for para in text.split('\n') {
            let n = para.chars().count();
            self.slots.push(Slot {
                container: container.clone(),
                offset,
                text: para.to_owned(),
                flat,
            });
            offset += n + 1;
            flat += n + 1;
        }
        first..self.slots.len()
    }

    /// The region holding flat position `pos`.
    pub fn region(&self, pos: usize) -> Region {
        if pos <= self.len {
            return Region::Body;
        }
        self.slot_at(self.snap(pos))
            .map_or(Region::Body, |(i, _)| self.slots[i].container.region())
    }

    /// The first and last flat positions of `pos`'s region; in a footnote, of that note.
    pub fn region_bounds(&self, pos: usize) -> (usize, usize) {
        let range = match self.region(pos) {
            Region::Body => return (0, self.len),
            Region::Header => &self.header,
            Region::Footer => &self.footer,
            Region::Footnote => match self.footnote_at(pos) {
                Some(n) => &n.slots,
                None => return (pos, pos),
            },
        };
        match (self.slots.get(range.start), range.end.checked_sub(1)) {
            (Some(first), Some(last)) => (first.flat, self.slots[last].flat_end()),
            _ => (pos, pos),
        }
    }

    /// Flat position of char `offset` of a header, footer or footnote's text.
    pub fn flat_in_region(&self, container: &TextRef, offset: usize) -> Option<usize> {
        let range = match container {
            TextRef::Header => self.header.clone(),
            TextRef::Footer => self.footer.clone(),
            TextRef::Footnote(id) => self.footnote(id)?.slots.clone(),
            _ => return None,
        };
        let slot = self.slots[range]
            .iter()
            .rev()
            .find(|s| s.offset <= offset)?;
        Some(slot.flat + (offset - slot.offset).min(slot.text.chars().count()))
    }

    /// The footnote with this id, if its reference is in the document.
    pub fn footnote(&self, id: &str) -> Option<&FootnoteSlots> {
        self.footnotes.iter().find(|n| n.id == id)
    }

    /// The footnote whose text holds flat position `pos`.
    pub fn footnote_at(&self, pos: usize) -> Option<&FootnoteSlots> {
        if pos <= self.len {
            return None;
        }
        let (i, _) = self.slot_at(self.snap(pos))?;
        self.footnotes.iter().find(|n| n.slots.contains(&i))
    }

    pub fn tables(&self) -> impl Iterator<Item = &TableBlock> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Table(t) => Some(t),
            Block::Paragraph(_) => None,
        })
    }

    pub fn table(&self, id: &str) -> Option<&TableBlock> {
        self.tables().find(|t| t.id == id)
    }

    /// The table whose cells contain flat position `pos`.
    pub fn table_at(&self, pos: usize) -> Option<&TableBlock> {
        self.tables()
            .find(|t| t.flat_first <= pos && pos <= t.flat_last)
    }

    /// Where flat position `pos` is in the document's containers.
    pub fn locate(&self, pos: usize) -> Location {
        if pos > self.len {
            if let Some((i, off)) = self.slot_at(self.snap(pos)) {
                let s = &self.slots[i];
                return Location {
                    container: s.container.clone(),
                    offset: s.offset + off,
                };
            }
        }
        let mut shift = 0;
        for t in self.tables() {
            if pos < t.flat_first {
                break;
            }
            if pos <= t.flat_last {
                // Inside the table: find the cell by walking its slots.
                for row in &t.cells {
                    for cell in row {
                        let Some(first) = self.slots.get(cell.start) else {
                            continue;
                        };
                        let last = &self.slots[cell.end - 1];
                        if pos >= first.flat && pos <= last.flat_end() {
                            return Location {
                                container: first.container.clone(),
                                offset: pos - first.flat,
                            };
                        }
                    }
                }
            }
            shift += t.flat_last + 1 - t.flat_first;
        }
        Location {
            container: TextRef::Body,
            offset: pos - shift,
        }
    }

    /// Flat position of body char index `body`.
    pub fn flat_of_body(&self, body: usize) -> usize {
        body + self
            .tables()
            .filter(|t| t.placeholder < body)
            .map(|t| t.flat_last + 1 - t.flat_first)
            .sum::<usize>()
    }

    /// Flat position of the start of a cell.
    pub fn cell_start(&self, table: &str, row: usize, col: usize) -> Option<usize> {
        let t = self.table(table)?;
        let slots = t.cells.get(row)?.get(col)?;
        self.slots.get(slots.start).map(|s| s.flat)
    }

    /// The slot holding `pos`, and the char offset within it.
    pub fn slot_at(&self, pos: usize) -> Option<(usize, usize)> {
        let i = self
            .slots
            .partition_point(|s| s.flat <= pos)
            .checked_sub(1)?;
        let s = &self.slots[i];
        (pos <= s.flat_end()).then(|| (i, pos - s.flat))
    }

    /// The nearest position that is inside some paragraph.
    pub fn snap(&self, pos: usize) -> usize {
        if self.slot_at(pos).is_some() {
            return pos;
        }
        let after = self.slots.partition_point(|s| s.flat <= pos);
        let before = after.checked_sub(1).map(|i| self.slots[i].flat_end());
        let next = self.slots.get(after).map(|s| s.flat);
        match (before, next) {
            (Some(b), Some(n)) if pos - b <= n - pos => b,
            (_, Some(n)) => n,
            (Some(b), None) => b,
            (None, None) => 0,
        }
    }

    /// One grapheme right, moving into the next paragraph or cell at an end.
    pub fn move_right(&self, pos: usize) -> usize {
        let Some((i, off)) = self.slot_at(pos) else {
            return self.snap(pos);
        };
        let s = &self.slots[i];
        let b = grapheme::boundaries(&s.text);
        match b.iter().find(|&&x| x > off) {
            Some(&next) => s.flat + next,
            None => self
                .slots
                .get(i + 1)
                .filter(|n| n.container.region() == s.container.region())
                .map_or(pos, |n| n.flat),
        }
    }

    /// One grapheme left, moving into the previous paragraph or cell at a start.
    pub fn move_left(&self, pos: usize) -> usize {
        let Some((i, off)) = self.slot_at(pos) else {
            return self.snap(pos);
        };
        let s = &self.slots[i];
        let b = grapheme::boundaries(&s.text);
        match b.iter().rev().find(|&&x| x < off) {
            Some(&prev) => s.flat + prev,
            None => i
                .checked_sub(1)
                .map(|p| &self.slots[p])
                .filter(|p| p.container.region() == s.container.region())
                .map_or(pos, Slot::flat_end),
        }
    }

    /// Ctrl+Right: the start of the next word in the paragraph, else the paragraph's end; from
    /// a paragraph's end, the start of the next one (in the same region).
    pub fn word_right(&self, pos: usize) -> usize {
        let Some((i, off)) = self.slot_at(pos) else {
            return self.snap(pos);
        };
        let s = &self.slots[i];
        let n = s.text.chars().count();
        if off >= n {
            return self.move_right(pos);
        }
        let next_word = grapheme::word_segments(&s.text)
            .into_iter()
            .find(|(r, word)| *word && r.start > off)
            .map_or(n, |(r, _)| r.start);
        s.flat + next_word
    }

    /// Ctrl+Left: the start of the word the caret is in or after; from a paragraph's start, the
    /// end of the previous one (in the same region).
    pub fn word_left(&self, pos: usize) -> usize {
        let Some((i, off)) = self.slot_at(pos) else {
            return self.snap(pos);
        };
        if off == 0 {
            return self.move_left(pos);
        }
        let s = &self.slots[i];
        let start = grapheme::word_segments(&s.text)
            .into_iter()
            .filter(|(r, word)| *word && r.start < off)
            .map(|(r, _)| r.start)
            .next_back()
            .unwrap_or(0);
        s.flat + start
    }

    /// The word (or run of spaces or punctuation) at the caret, as flat `start..end`: what a
    /// double-click selects. At a paragraph's end, the segment before the caret.
    pub fn word_at(&self, pos: usize) -> Range<usize> {
        let Some((i, off)) = self.slot_at(pos) else {
            return pos..pos;
        };
        let s = &self.slots[i];
        let segs = grapheme::word_segments(&s.text);
        let hit = segs
            .iter()
            .find(|(r, _)| r.contains(&off))
            .or_else(|| segs.iter().rev().find(|(r, _)| r.end == off));
        hit.map_or(pos..pos, |(r, _)| s.flat + r.start..s.flat + r.end)
    }

    /// The previous grapheme boundary within the same paragraph, if any.
    pub fn prev_in_slot(&self, pos: usize) -> Option<usize> {
        let (i, off) = self.slot_at(pos)?;
        let s = &self.slots[i];
        let b = grapheme::boundaries(&s.text);
        b.iter().rev().find(|&&x| x < off).map(|&x| s.flat + x)
    }

    /// The next grapheme boundary within the same paragraph, if any.
    pub fn next_in_slot(&self, pos: usize) -> Option<usize> {
        let (i, off) = self.slot_at(pos)?;
        let s = &self.slots[i];
        let b = grapheme::boundaries(&s.text);
        b.iter().find(|&&x| x > off).map(|&x| s.flat + x)
    }

    /// Start of the next cell in reading order (Tab), or `None` from the last cell or outside
    /// a table.
    pub fn next_cell(&self, pos: usize) -> Option<usize> {
        let t = self.table_at(pos)?;
        let cells: Vec<&Range<usize>> = t.cells.iter().flatten().collect();
        let i = cells
            .iter()
            .position(|c| self.slots[c.end - 1].flat_end() >= pos)?;
        cells.get(i + 1).map(|c| self.slots[c.start].flat)
    }

    /// Start of the previous cell in reading order (Shift+Tab).
    pub fn prev_cell(&self, pos: usize) -> Option<usize> {
        let t = self.table_at(pos)?;
        let cells: Vec<&Range<usize>> = t.cells.iter().flatten().collect();
        let i = cells
            .iter()
            .position(|c| self.slots[c.end - 1].flat_end() >= pos)?;
        i.checked_sub(1).map(|p| self.slots[cells[p].start].flat)
    }

    /// Row and column of the cell holding `pos`.
    pub fn cell_at(&self, pos: usize) -> Option<CellRef> {
        match self.locate(pos).container {
            TextRef::Cell(c) => Some(c),
            TextRef::Body | TextRef::Header | TextRef::Footer | TextRef::Footnote(_) => None,
        }
    }

    /// The text between two flat positions, paragraphs and cells separated by `\n`.
    pub fn slice(&self, a: usize, b: usize) -> String {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut parts: Vec<String> = Vec::new();
        for s in &self.slots {
            let (start, end) = (s.flat, s.flat_end());
            if end < lo || start > hi {
                continue;
            }
            let from = lo.saturating_sub(start);
            let to = hi.min(end) - start;
            parts.push(
                s.text
                    .chars()
                    .skip(from)
                    .take(to.saturating_sub(from))
                    .collect(),
            );
        }
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(placeholder: usize, cells: &[&[&str]]) -> TableInput {
        TableInput {
            placeholder,
            id: "t".into(),
            cells: cells
                .iter()
                .map(|r| r.iter().map(|s| s.to_string()).collect())
                .collect(),
        }
    }

    fn texts(o: &Outline) -> Vec<(String, usize)> {
        o.slots.iter().map(|s| (s.text.clone(), s.flat)).collect()
    }

    #[test]
    fn plain_body_is_one_slot_per_paragraph() {
        let o = Outline::build("ab\ncd", vec![]);
        assert_eq!(texts(&o), [("ab".into(), 0), ("cd".into(), 3)]);
        assert_eq!(o.len, 5);
    }

    #[test]
    fn table_cells_take_flat_positions_in_reading_order() {
        // "ab\n¤\ncd" with a 1x2 table: cells "x" and "".
        let body = format!("ab\n{TABLE_PLACEHOLDER}\ncd");
        let o = Outline::build(&body, vec![table(3, &[&["x", ""]])]);
        // ab: 0..=2; table cells start at 3+1=4: "x" 4..=5, "" 6..=6; body resumes:
        // placeholder+1 (4) maps to 4+3=7, "\n" at 7, "cd" starts at body 5 -> flat 8.
        assert_eq!(
            texts(&o),
            [
                ("ab".into(), 0),
                ("x".into(), 4),
                ("".into(), 6),
                ("cd".into(), 8)
            ]
        );
        assert_eq!(o.len, body.chars().count() + 3);
        let t = o.tables().next().unwrap();
        assert_eq!((t.flat_first, t.flat_last), (4, 6));
        assert_eq!(o.locate(5).offset, 1);
        assert!(matches!(
            o.locate(6).container,
            TextRef::Cell(CellRef { col: 1, .. })
        ));
        assert_eq!(
            o.locate(8),
            Location {
                container: TextRef::Body,
                offset: 5
            }
        );
        assert_eq!(o.flat_of_body(5), 8);
    }

    #[test]
    fn table_at_document_end_keeps_a_paragraph_after_it() {
        let body = format!("{TABLE_PLACEHOLDER}\n");
        let o = Outline::build(&body, vec![table(0, &[&["a"]])]);
        assert_eq!(o.blocks.len(), 2, "{:?}", o.blocks);
        assert!(matches!(o.blocks[1], Block::Paragraph(_)));
    }

    #[test]
    fn movement_crosses_cells_and_skips_table_gaps() {
        let body = format!("ab\n{TABLE_PLACEHOLDER}\ncd");
        let o = Outline::build(&body, vec![table(3, &[&["x", ""]])]);
        assert_eq!(o.move_right(2), 4, "end of ab -> first cell");
        assert_eq!(o.move_right(5), 6, "end of x -> next cell");
        assert_eq!(o.move_right(6), 8, "last cell -> after table");
        assert_eq!(o.move_left(8), 6);
        assert_eq!(o.move_left(4), 2);
        assert_eq!(o.snap(3), 2);
        assert_eq!(o.snap(7), 6);
        assert_eq!(o.next_cell(4), Some(6));
        assert_eq!(o.next_cell(6), None);
        assert_eq!(o.prev_cell(6), Some(4));
        assert_eq!(o.prev_in_slot(4), None, "backspace never leaves a cell");
    }

    #[test]
    fn multi_paragraph_cells_and_slices() {
        let body = TABLE_PLACEHOLDER.to_string();
        let o = Outline::build(&body, vec![table(0, &[&["one\ntwo"]])]);
        assert_eq!(texts(&o)[..2], [("one".into(), 1), ("two".into(), 5)]);
        assert_eq!(o.slice(1, 8), "one\ntwo");
    }

    #[test]
    fn stray_placeholder_without_table_is_plain_text() {
        let body = format!("a{TABLE_PLACEHOLDER}b");
        let o = Outline::build(&body, vec![]);
        assert_eq!(o.slots.len(), 1);
        assert_eq!(o.len, 3);
    }
}
