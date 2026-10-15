//! WebAssembly bindings: one [`Editor`] owns a document replica, the layout engine and the
//! paginator, and hands the renderer flat glyph data.
//!
//! Positions passed in from JS are char indices (see `opendox-model`).

use opendox_layout::{
    FontId, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, TextPosition,
    TextStyle,
};
use opendox_model::{Document, Paragraph};
use wasm_bindgen::prelude::*;

/// Floats per glyph in [`Editor::page_glyphs`].
pub const GLYPH_STRIDE: usize = 6;

/// Floats per rectangle in [`Editor::selection_rects`].
pub const RECT_STRIDE: usize = 5;

#[wasm_bindgen]
pub struct Editor {
    doc: Document,
    engine: LayoutEngine,
    paginator: Paginator,
    style: TextStyle,
    /// Paragraphs as of the last [`Editor::layout`]; maps char indices to layout positions.
    paragraphs: Vec<Paragraph>,
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
        Editor {
            doc: Document::new(),
            engine: LayoutEngine::new(FontLibrary::new()),
            paginator: Paginator::new(PageGeometry::letter()),
            style: TextStyle {
                font_family: "Noto Sans".into(),
                font_size: 12.0,
                font_weight: 400.0,
            },
            paragraphs: Vec::new(),
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

    pub fn text(&self) -> String {
        self.doc.text()
    }

    /// Document length in chars.
    pub fn len(&self) -> usize {
        self.doc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }

    /// The text between two char indices.
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.doc
            .text()
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    /// Inserts `text` at `pos` and returns the caret position just after it.
    pub fn insert(&mut self, pos: usize, text: &str) -> Result<usize, JsError> {
        self.doc.insert(pos, text)?;
        Ok(pos + text.chars().count())
    }

    pub fn delete(&mut self, start: usize, end: usize) -> Result<(), JsError> {
        Ok(self.doc.delete(start..end)?)
    }

    /// The grapheme boundary before `pos` (Backspace, Left arrow).
    pub fn prev_boundary(&self, pos: usize) -> Option<usize> {
        self.doc.prev_grapheme_boundary(pos)
    }

    /// The grapheme boundary after `pos` (Delete, Right arrow).
    pub fn next_boundary(&self, pos: usize) -> Option<usize> {
        self.doc.next_grapheme_boundary(pos)
    }

    /// Lays out the document and returns the page count. Call after every edit, before any
    /// hit testing or caret query.
    pub fn layout(&mut self) -> usize {
        self.paragraphs = self.doc.paragraphs();
        let inputs: Vec<ParagraphInput> = self
            .paragraphs
            .iter()
            .map(|p| ParagraphInput {
                text: &p.text,
                base: &self.style,
                spans: &[],
            })
            .collect();
        self.paginator.layout(&mut self.engine, &inputs).len()
    }

    /// The caret position nearest to `(x, y)` points on `page`, as `[pos, upstream]`.
    pub fn hit_test(&self, page: usize, x: f32, y: f32) -> Option<Vec<u32>> {
        let hit = self.paginator.hit_test(page, x, y)?;
        Some(self.caret_out(hit))
    }

    /// Where to draw the caret: `[page, x, top, bottom]` in points.
    pub fn caret_rect(&self, pos: usize, upstream: bool) -> Option<Vec<f32>> {
        let r = self.paginator.caret_rect(self.position(pos, upstream))?;
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
            .move_vertical(self.position(pos, upstream), goal_x, down)?;
        Some(self.caret_out(to))
    }

    /// Start of the visual line (Home): `[pos, upstream]`.
    pub fn line_start(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_start(self.position(pos, upstream))?;
        Some(self.caret_out(to))
    }

    /// End of the visual line (End): `[pos, upstream]`.
    pub fn line_end(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_end(self.position(pos, upstream))?;
        Some(self.caret_out(to))
    }

    /// Highlight boxes for the selection between two char indices, flattened in groups of
    /// [`RECT_STRIDE`]: `page, x, y, width, height` in points.
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<f32> {
        let rects = self
            .paginator
            .selection_rects(self.position(a, false), self.position(b, false));
        let mut out = Vec::with_capacity(rects.len() * RECT_STRIDE);
        for r in rects {
            out.extend_from_slice(&[r.page as f32, r.x, r.y, r.width, r.height]);
        }
        out
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
}

impl Editor {
    /// Document char index → paragraph + byte offset, per the last layout.
    fn position(&self, pos: usize, upstream: bool) -> TextPosition {
        let i = self
            .paragraphs
            .partition_point(|p| p.start <= pos)
            .saturating_sub(1);
        let byte = self.paragraphs.get(i).map_or(0, |p| {
            p.text
                .char_indices()
                .nth(pos.saturating_sub(p.start))
                .map_or(p.text.len(), |(b, _)| b)
        });
        TextPosition {
            paragraph: i,
            byte,
            upstream,
        }
    }

    /// Paragraph + byte offset → `[document char index, upstream]`.
    fn caret_out(&self, tp: TextPosition) -> Vec<u32> {
        let pos = self.paragraphs.get(tp.paragraph).map_or(0, |p| {
            p.start + p.text.get(..tp.byte).map_or(0, |s| s.chars().count())
        });
        vec![pos as u32, u32::from(tp.upstream)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

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

    fn editor_with(text: &str) -> Editor {
        let mut e = Editor::new();
        e.register_font(REGULAR.to_vec());
        e.insert(0, text).unwrap();
        e.layout();
        e
    }

    #[test]
    fn char_positions_round_trip_through_layout_positions() {
        // Multi-byte chars in several paragraphs: char index and byte offset differ.
        let text = "héllo wörld\nsecond ünïcode line\n\nlast";
        let e = editor_with(text);
        for pos in 0..=text.chars().count() {
            let tp = e.position(pos, false);
            assert_eq!(e.caret_out(tp), vec![pos as u32, 0], "pos {pos} -> {tp:?}");
        }
    }

    #[test]
    fn click_then_type_inserts_at_the_clicked_position() {
        let mut e = editor_with("first\nsecond");
        // Click the start of the second line (paragraph 2 starts at char 6).
        let line_h = 16.34;
        let hit = e.hit_test(0, 72.0, 72.0 + line_h * 1.5).unwrap();
        assert_eq!(hit, vec![6, 0]);
        let after = e.insert(hit[0] as usize, "the ").unwrap();
        assert_eq!(after, 10);
        assert_eq!(e.text(), "first\nthe second");
    }

    #[test]
    fn caret_and_selection_queries_use_char_indices() {
        let mut e = editor_with("ab\ncd");
        let r = e.caret_rect(3, false).unwrap(); // start of "cd"
        assert_eq!(r[0], 0.0);
        assert!((r[1] - 72.0).abs() < 0.01);
        let rects = e.selection_rects(1, 4);
        assert_eq!(rects.len(), 2 * RECT_STRIDE); // "b" + break, then "c"
        assert_eq!(e.slice(1, 4), "b\nc");
        assert_eq!(
            e.move_vertical(1, false, r[1] + 7.0, true),
            Some(vec![4, 0])
        );
        assert_eq!(e.line_end(3, false), Some(vec![5, 0]));
        e.delete(1, 4).unwrap();
        assert_eq!(e.text(), "ad");
    }
}
