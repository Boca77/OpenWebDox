//! WebAssembly bindings: one [`Editor`] owns a document replica, the layout engine and the
//! paginator, and hands the renderer flat glyph data.
//!
//! Positions passed in from JS are **flat positions** (see `opendox_model::Outline`): one number
//! per caret position, counting body text and table cells together in reading order.

use opendox_layout::{
    BlockInput, FontId, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput,
    TableInput, TextPosition, TextStyle,
};
use opendox_model::{Block, Document, Outline};
use wasm_bindgen::prelude::*;

/// Floats per glyph in [`Editor::page_glyphs`].
pub const GLYPH_STRIDE: usize = 6;

/// Floats per rectangle in [`Editor::selection_rects`].
pub const RECT_STRIDE: usize = 5;

/// Floats per border line in [`Editor::page_rules`].
pub const RULE_STRIDE: usize = 4;

#[wasm_bindgen]
pub struct Editor {
    doc: Document,
    engine: LayoutEngine,
    paginator: Paginator,
    style: TextStyle,
    /// Document structure as of the last [`Editor::layout`]; maps flat positions to layout.
    outline: Outline,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Editor {
    /// An empty Letter-size document using `"Noto Sans"` at 12pt. Register fonts before layout.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Editor {
        let doc = Document::new();
        let outline = doc.outline();
        Editor {
            doc,
            engine: LayoutEngine::new(FontLibrary::new()),
            paginator: Paginator::new(PageGeometry::letter()),
            style: TextStyle {
                font_family: "Noto Sans".into(),
                font_size: 12.0,
                font_weight: 400.0,
            },
            outline,
        }
    }

    /// Registers a font file and returns its id.
    pub fn register_font(&mut self, data: Vec<u8>) -> u32 {
        self.engine.fonts_mut().register(data).0
    }

    /// Sets the default font as a CSS-style family list and a size in points.
    pub fn set_default_font(&mut self, family: String, size: f32) {
        self.style.font_family = family;
        self.style.font_size = size;
    }

    /// All text, paragraphs and cells separated by `\n`.
    pub fn text(&self) -> String {
        let o = self.doc.outline();
        o.slice(0, o.len)
    }

    /// Largest flat position.
    pub fn len(&self) -> usize {
        self.outline.len
    }

    pub fn is_empty(&self) -> bool {
        self.outline.len == 0
    }

    /// The text between two positions (for copy).
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.outline.slice(start, end)
    }

    // --- editing -------------------------------------------------------------------------

    /// Inserts `text` at `pos` and returns the caret position just after it.
    pub fn insert(&mut self, pos: usize, text: &str) -> Result<usize, JsError> {
        Ok(self.doc.insert_at(&self.outline, pos, text)?)
    }

    /// Deletes between two positions; returns where the caret goes. Spanning body text and a
    /// table deletes the whole table; spanning cells of one table clears them.
    pub fn delete(&mut self, start: usize, end: usize) -> Result<usize, JsError> {
        Ok(self.doc.delete_range_at(&self.outline, start, end)?)
    }

    /// Backspace: returns the new caret position.
    pub fn delete_backward(&mut self, pos: usize) -> Result<usize, JsError> {
        Ok(self.doc.delete_backward_at(&self.outline, pos)?)
    }

    /// Delete key: returns the new caret position.
    pub fn delete_forward(&mut self, pos: usize) -> Result<usize, JsError> {
        Ok(self.doc.delete_forward_at(&self.outline, pos)?)
    }

    /// One grapheme left, crossing into the previous paragraph or cell.
    pub fn move_left(&self, pos: usize) -> usize {
        self.outline.move_left(pos)
    }

    /// One grapheme right, crossing into the next paragraph or cell.
    pub fn move_right(&self, pos: usize) -> usize {
        self.outline.move_right(pos)
    }

    // --- tables --------------------------------------------------------------------------

    /// Inserts a table at the caret (after the current table if the caret is in one) and
    /// returns the position of its first cell.
    pub fn insert_table(&mut self, pos: usize, rows: usize, cols: usize) -> Result<usize, JsError> {
        let body_pos = match self.outline.table_at(pos) {
            Some(t) => t.placeholder + 1,
            None => self.outline.locate(pos).offset,
        };
        let id = self.doc.insert_table(body_pos, rows, cols)?;
        let fresh = self.doc.outline();
        Ok(fresh.cell_start(&id, 0, 0).unwrap_or(pos))
    }

    /// `[row, col, rows, cols]` of the cell holding `pos`, or `undefined` outside tables.
    pub fn table_info(&self, pos: usize) -> Option<Vec<u32>> {
        let t = self.outline.table_at(pos)?;
        let c = self.outline.cell_at(pos)?;
        Some(vec![
            c.row as u32,
            c.col as u32,
            t.rows() as u32,
            t.columns as u32,
        ])
    }

    /// Start of the next cell (Tab), or `undefined` in the last cell or outside a table.
    pub fn next_cell(&self, pos: usize) -> Option<usize> {
        self.outline.next_cell(pos)
    }

    /// Start of the previous cell (Shift+Tab).
    pub fn prev_cell(&self, pos: usize) -> Option<usize> {
        self.outline.prev_cell(pos)
    }

    /// Inserts a row above or below the caret's row; returns the caret in the new row.
    pub fn table_insert_row(&mut self, pos: usize, below: bool) -> Result<usize, JsError> {
        let c = self.cell(pos)?;
        let row = c.row + usize::from(below);
        self.doc.insert_row(&c.table, row)?;
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, row, c.col)
            .unwrap_or(pos))
    }

    /// Inserts a column left or right of the caret's column; returns the caret in it.
    pub fn table_insert_column(&mut self, pos: usize, right: bool) -> Result<usize, JsError> {
        let c = self.cell(pos)?;
        let col = c.col + usize::from(right);
        self.doc.insert_column(&c.table, col)?;
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, c.row, col)
            .unwrap_or(pos))
    }

    /// Deletes the caret's row (the whole table if it's the only row).
    pub fn table_delete_row(&mut self, pos: usize) -> Result<usize, JsError> {
        let c = self.cell(pos)?;
        let rows = self.outline.table(&c.table).map_or(0, |t| t.rows());
        if rows <= 1 {
            return self.table_delete(pos);
        }
        self.doc.delete_row(&c.table, c.row)?;
        let row = c.row.min(rows - 2);
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, row, c.col)
            .unwrap_or(pos))
    }

    /// Deletes the caret's column (the whole table if it's the only column).
    pub fn table_delete_column(&mut self, pos: usize) -> Result<usize, JsError> {
        let c = self.cell(pos)?;
        let cols = self.outline.table(&c.table).map_or(0, |t| t.columns);
        if cols <= 1 {
            return self.table_delete(pos);
        }
        self.doc.delete_column(&c.table, c.col)?;
        let col = c.col.min(cols - 2);
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, c.row, col)
            .unwrap_or(pos))
    }

    /// Deletes the table holding the caret; returns the caret where the table was.
    pub fn table_delete(&mut self, pos: usize) -> Result<usize, JsError> {
        let t = self
            .outline
            .table_at(pos)
            .ok_or_else(|| JsError::new("not in a table"))?;
        let (id, placeholder) = (t.id.clone(), t.placeholder);
        self.doc.delete_table(&id)?;
        let fresh = self.doc.outline();
        Ok(fresh.snap(fresh.flat_of_body(placeholder)))
    }

    // --- layout and geometry -------------------------------------------------------------

    /// Lays out the document and returns the page count. Call after every edit, before any
    /// position or geometry query.
    pub fn layout(&mut self) -> usize {
        self.outline = self.doc.outline();
        let o = &self.outline;
        let style = &self.style;
        let para = |i: usize| ParagraphInput {
            text: &o.slots[i].text,
            base: style,
            spans: &[],
        };
        let blocks: Vec<BlockInput> = o
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(i) => BlockInput::Paragraph(para(*i)),
                Block::Table(t) => BlockInput::Table(TableInput {
                    rows: t
                        .cells
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|cell| cell.clone().map(para).collect())
                                .collect()
                        })
                        .collect(),
                }),
            })
            .collect();
        self.paginator
            .layout_blocks(&mut self.engine, &blocks)
            .len()
    }

    /// Page width in points.
    pub fn page_width(&self) -> f32 {
        self.paginator.geometry().width
    }

    /// Page height in points.
    pub fn page_height(&self) -> f32 {
        self.paginator.geometry().height
    }

    /// Glyphs on page `index` from the last layout, flattened into groups of [`GLYPH_STRIDE`]:
    /// `font id, face index, glyph id, x, y, font size` — positions in points from the page's
    /// top-left corner, `(x, y)` being the glyph's baseline origin.
    pub fn page_glyphs(&self, index: usize) -> Vec<f32> {
        let glyphs = self.paginator.placed_glyphs(index);
        let mut out = Vec::with_capacity(glyphs.len() * GLYPH_STRIDE);
        for g in glyphs {
            out.extend_from_slice(&[
                g.font.0 as f32,
                g.face_index as f32,
                g.id as f32,
                g.x,
                g.y,
                g.font_size,
            ]);
        }
        out
    }

    /// Table borders on page `index`, in groups of [`RULE_STRIDE`]: `x0, y0, x1, y1` in points.
    pub fn page_rules(&self, index: usize) -> Vec<f32> {
        let Some(page) = self.paginator.pages().get(index) else {
            return Vec::new();
        };
        page.rules
            .iter()
            .flat_map(|r| [r.x0, r.y0, r.x1, r.y1])
            .collect()
    }

    /// SVG path data for a glyph in font units with y pointing down, or `undefined` if the
    /// font or glyph is unknown. Scale by `font_size / units_per_em`.
    pub fn glyph_path(&self, font: u32, face_index: u32, glyph: u32) -> Option<String> {
        self.engine
            .fonts()
            .glyph_outline(FontId(font), face_index, glyph)
            .map(|o| o.svg_path)
    }

    /// Units per em of a font face, or 0 if unknown.
    pub fn units_per_em(&self, font: u32, face_index: u32) -> u16 {
        // Glyph 0 (.notdef) exists in every font; its outline carries the font's em size.
        self.engine
            .fonts()
            .glyph_outline(FontId(font), face_index, 0)
            .map_or(0, |o| o.units_per_em)
    }

    /// The caret position nearest to `(x, y)` points on `page`, as `[pos, upstream]`.
    pub fn hit_test(&self, page: usize, x: f32, y: f32) -> Option<Vec<u32>> {
        let hit = self.paginator.hit_test(page, x, y)?;
        Some(self.caret_out(hit))
    }

    /// Where to draw the caret: `[page, x, top, bottom]` in points.
    pub fn caret_rect(&self, pos: usize, upstream: bool) -> Option<Vec<f32>> {
        let r = self.paginator.caret_rect(self.position(pos, upstream)?)?;
        Some(vec![r.page as f32, r.x, r.top, r.bottom])
    }

    /// One line up or down, aiming for `goal_x` points: `[pos, upstream]`, or `undefined` at
    /// the first or last line.
    pub fn move_vertical(
        &self,
        pos: usize,
        upstream: bool,
        goal_x: f32,
        down: bool,
    ) -> Option<Vec<u32>> {
        let to = self
            .paginator
            .move_vertical(self.position(pos, upstream)?, goal_x, down)?;
        Some(self.caret_out(to))
    }

    /// Start of the visual line (Home): `[pos, upstream]`.
    pub fn line_start(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_start(self.position(pos, upstream)?)?;
        Some(self.caret_out(to))
    }

    /// End of the visual line (End): `[pos, upstream]`.
    pub fn line_end(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_end(self.position(pos, upstream)?)?;
        Some(self.caret_out(to))
    }

    /// Highlight boxes for the selection between two positions, flattened in groups of
    /// [`RECT_STRIDE`]: `page, x, y, width, height` in points. A selection reaching from body
    /// text into a table covers the whole table, matching what deleting it would remove.
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<f32> {
        let (a, b) = self.visible_range(a, b);
        let (Some(pa), Some(pb)) = (self.position(a, false), self.position(b, false)) else {
            return Vec::new();
        };
        let rects = self.paginator.selection_rects(pa, pb);
        let mut out = Vec::with_capacity(rects.len() * RECT_STRIDE);
        for r in rects {
            out.extend_from_slice(&[r.page as f32, r.x, r.y, r.width, r.height]);
        }
        out
    }
}

impl Editor {
    fn cell(&self, pos: usize) -> Result<opendox_model::CellRef, JsError> {
        self.outline
            .cell_at(pos)
            .ok_or_else(|| JsError::new("not in a table"))
    }

    /// Flat position → paragraph + byte offset, per the last layout.
    fn position(&self, pos: usize, upstream: bool) -> Option<TextPosition> {
        let (slot, chars) = self.outline.slot_at(self.outline.snap(pos))?;
        let text = &self.outline.slots[slot].text;
        let byte = text
            .char_indices()
            .nth(chars)
            .map_or(text.len(), |(b, _)| b);
        Some(TextPosition {
            paragraph: slot,
            byte,
            upstream,
        })
    }

    /// Paragraph + byte offset → `[flat position, upstream]`.
    fn caret_out(&self, tp: TextPosition) -> Vec<u32> {
        let pos = self.outline.slots.get(tp.paragraph).map_or(0, |s| {
            s.flat + s.text.get(..tp.byte).map_or(0, |t| t.chars().count())
        });
        vec![pos as u32, u32::from(tp.upstream)]
    }

    /// Widens a selection that crosses a table edge to cover the whole table.
    fn visible_range(&self, a: usize, b: usize) -> (usize, usize) {
        let o = &self.outline;
        let (ta, tb) = (o.table_at(a).map(|t| &t.id), o.table_at(b).map(|t| &t.id));
        if ta == tb {
            return (a, b);
        }
        let widen = |p: usize, other: usize| match o.table_at(p) {
            Some(t) if other > p => t.flat_first,
            Some(t) => t.flat_last,
            None => p,
        };
        (widen(a, b), widen(b, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendox_model::TextRef;

    /// Which container a position is in, without going through JS.
    fn container_of(e: &Editor, pos: usize) -> TextRef {
        e.outline.locate(pos).container
    }

    const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

    fn editor_with(text: &str) -> Editor {
        let mut e = Editor::new();
        e.register_font(REGULAR.to_vec());
        e.insert(0, text).unwrap();
        e.layout();
        e
    }

    #[test]
    fn lays_out_inserted_text_into_flat_glyph_data() {
        let mut e = Editor::new();
        let font = e.register_font(REGULAR.to_vec());
        e.insert(0, "Hello\nworld").unwrap();
        assert_eq!(e.layout(), 1);

        let data = e.page_glyphs(0);
        assert_eq!(data.len(), 10 * GLYPH_STRIDE);
        let first = &data[..GLYPH_STRIDE];
        assert_eq!(first[0], font as f32);
        assert!((first[3] - 72.0).abs() < 1.0);

        let glyph = first[2] as u32;
        assert!(e.glyph_path(font, 0, glyph).unwrap().starts_with('M'));
        assert_eq!(e.units_per_em(font, 0), 1000);
        assert_eq!(e.page_width(), 612.0);
    }

    #[test]
    fn positions_round_trip_through_layout_positions() {
        // Multi-byte chars in several paragraphs: char index and byte offset differ.
        let text = "héllo wörld\nsecond ünïcode line\n\nlast";
        let e = editor_with(text);
        for pos in 0..=text.chars().count() {
            let tp = e.position(pos, false).unwrap();
            assert_eq!(e.caret_out(tp), vec![pos as u32, 0], "pos {pos} -> {tp:?}");
        }
    }

    #[test]
    fn click_then_type_inserts_at_the_clicked_position() {
        let mut e = editor_with("first\nsecond");
        let line_h = 16.34;
        let hit = e.hit_test(0, 72.0, 72.0 + line_h * 1.5).unwrap();
        assert_eq!(hit, vec![6, 0]);
        let after = e.insert(hit[0] as usize, "the ").unwrap();
        assert_eq!(after, 10);
        e.layout();
        assert_eq!(e.text(), "first\nthe second");
    }

    #[test]
    fn caret_and_selection_queries() {
        let mut e = editor_with("ab\ncd");
        let r = e.caret_rect(3, false).unwrap(); // start of "cd"
        assert_eq!(r[0], 0.0);
        assert!((r[1] - 72.0).abs() < 0.01);
        assert_eq!(e.selection_rects(1, 4).len(), 2 * RECT_STRIDE); // "b" + break, then "c"
        assert_eq!(e.slice(1, 4), "b\nc");
        assert_eq!(
            e.move_vertical(1, false, r[1] + 7.0, true),
            Some(vec![4, 0])
        );
        assert_eq!(e.line_end(3, false), Some(vec![5, 0]));
        e.delete(1, 4).unwrap();
        e.layout();
        assert_eq!(e.text(), "ad");
    }

    #[test]
    fn insert_table_type_in_cells_and_tab_between_them() {
        let mut e = editor_with("before");
        let c00 = e.insert_table(6, 2, 2).unwrap();
        e.layout();
        assert!(matches!(container_of(&e, c00), TextRef::Cell(_)));
        assert_eq!(e.table_info(c00), Some(vec![0, 0, 2, 2]));

        let after = e.insert(c00, "A").unwrap();
        e.layout();
        let c01 = e.next_cell(after).unwrap();
        e.insert(c01, "B").unwrap();
        e.layout();
        assert_eq!(e.text(), "before\nA\nB\n\n\n");
        assert_eq!(e.page_rules(0).len(), 2 * 5 * RULE_STRIDE);

        // The caret drawn in cell (0,1) sits in the right half of the page.
        let r = e.caret_rect(c01, false).unwrap();
        assert!(r[1] > 72.0 + 234.0);
        // Clicking there lands back in that cell.
        let hit = e.hit_test(0, r[1], (r[2] + r[3]) / 2.0).unwrap();
        assert_eq!(hit[0] as usize, c01);
    }

    #[test]
    fn table_rows_and_columns_from_the_caret() {
        let mut e = editor_with("");
        let c00 = e.insert_table(0, 1, 1).unwrap();
        e.layout();
        let below = e.table_insert_row(c00, true).unwrap();
        e.layout();
        assert_eq!(e.table_info(below), Some(vec![1, 0, 2, 1]));
        let right = e.table_insert_column(below, true).unwrap();
        e.layout();
        assert_eq!(e.table_info(right), Some(vec![1, 1, 2, 2]));
        let at = e.table_delete_row(right).unwrap();
        e.layout();
        assert_eq!(e.table_info(at), Some(vec![0, 1, 1, 2]));
        let at = e.table_delete_column(at).unwrap();
        e.layout();
        assert_eq!(e.table_info(at), Some(vec![0, 0, 1, 1]));
        let at = e.table_delete_column(at).unwrap(); // last column: removes the table
        e.layout();
        assert_eq!(e.table_info(at), None);
        assert_eq!(e.len(), 0);
    }

    #[test]
    fn selection_from_body_into_table_covers_the_table() {
        let mut e = editor_with("ab");
        let c00 = e.insert_table(2, 2, 2).unwrap();
        e.layout();
        let (a, b) = e.visible_range(1, c00);
        assert_eq!(a, 1);
        let t = e.outline.tables().next().unwrap();
        assert_eq!(b, t.flat_last);
        // Deleting that selection removes the table and the "b".
        let caret = e.delete(1, c00).unwrap();
        e.layout();
        assert_eq!(caret, 1);
        assert_eq!(e.text(), "a\n");
    }
}
