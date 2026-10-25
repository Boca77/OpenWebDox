use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::fonts::FontId;
use crate::navigation::{PlacedLine, Rect, Zone};
use crate::paragraph::{
    Align, DecorationKind, Exclusion, FieldKind, GlyphRun, LayoutEngine, Line, Marker, ObjectInput,
    ObjectMode, ParagraphLayout, ParagraphStyle, StyleSpan, TextStyle,
};

/// Space between a cell's border and its text, in points.
pub const CELL_PADDING: f32 = 5.0;

/// Space kept between a wrapped object and the text beside it, in points (1/8 in, as in
/// Google Docs).
pub const WRAP_MARGIN: f32 = 9.0;

/// Space above and below an object that breaks text, in points.
pub const BREAK_MARGIN: f32 = 9.0;

/// Space above a page's footnotes, holding the short rule that sets them apart from the body,
/// in points.
pub const NOTE_SEPARATOR: f32 = 12.0;

/// Where the separator rule sits within [`NOTE_SEPARATOR`], from its top.
const NOTE_RULE_Y: f32 = 4.0;

/// How many times a layout is redone so text settles around wrapped objects. Each pass lays
/// text around the objects where the previous pass put them; documents without wrapped
/// objects take one pass.
const MAX_WRAP_PASSES: usize = 5;

/// Page size and margins, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    /// From the page's top edge to the header's first line.
    pub header_distance: f32,
    /// From the page's bottom edge to the footer's last line.
    pub footer_distance: f32,
}

impl PageGeometry {
    /// US Letter (8.5 × 11 in) with 1 in margins.
    pub fn letter() -> Self {
        Self::with_inch_margins(612.0, 792.0)
    }

    /// A4 (210 × 297 mm) with 1 in margins.
    pub fn a4() -> Self {
        Self::with_inch_margins(595.28, 841.89)
    }

    fn with_inch_margins(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            margin_top: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
            margin_right: 72.0,
            header_distance: 36.0,
            footer_distance: 36.0,
        }
    }

    pub fn content_width(&self) -> f32 {
        self.width - self.margin_left - self.margin_right
    }

    pub fn content_height(&self) -> f32 {
        self.height - self.margin_top - self.margin_bottom
    }
}

/// One paragraph as input to pagination.
#[derive(Debug, Clone, Copy)]
pub struct ParagraphInput<'a> {
    pub text: &'a str,
    pub base: &'a TextStyle,
    pub spans: &'a [StyleSpan],
    /// Alignment, line spacing and the space around the paragraph.
    pub style: ParagraphStyle,
    /// Objects (images) anchored in the paragraph. In table cells every object is laid out in
    /// line, whatever its mode.
    pub objects: &'a [ObjectInput],
    /// A list marker before the first line.
    pub marker: Option<Marker<'a>>,
}

/// A table as input to pagination: rows of cells, each cell a list of paragraphs. Columns
/// share the content width equally.
#[derive(Debug, Clone)]
pub struct TableInput<'a> {
    pub rows: Vec<Vec<Vec<ParagraphInput<'a>>>>,
}

/// A footnote as input to pagination: where its reference is, and the note's paragraphs. The
/// note goes at the foot of the page holding its reference.
#[derive(Debug, Clone)]
pub struct NoteInput<'a> {
    /// The paragraph holding the reference, numbered as in [`Fragment::paragraph`].
    pub paragraph: usize,
    /// UTF-8 byte offset of the reference's placeholder in that paragraph.
    pub byte: usize,
    pub paragraphs: Vec<ParagraphInput<'a>>,
}

/// A footnote after numbering its paragraphs.
struct NotePlan {
    paragraph: usize,
    byte: usize,
    /// Indices into `Paginator::note_keys`.
    paragraphs: Range<usize>,
}

/// One block of the document.
#[derive(Debug, Clone)]
pub enum BlockInput<'a> {
    Paragraph(ParagraphInput<'a>),
    Table(TableInput<'a>),
}

/// A page: the pieces of paragraphs placed on it, the table borders and the objects to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub fragments: Vec<Fragment>,
    pub rules: Vec<Rule>,
    pub objects: Vec<PlacedObject>,
    /// What the header and footer fields and the footnote references show on this page.
    pub labels: Vec<PlacedLabel>,
}

/// A field's text as drawn on one page (its page number, say).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabel {
    /// The paragraph and byte offset of the field it fills in.
    pub paragraph: usize,
    pub byte: usize,
    pub text: String,
    /// Glyph positions in points from the page's top-left corner; byte ranges and
    /// `glyph_text` index into `text`.
    pub runs: Vec<GlyphRun>,
}

/// Where an object is drawn relative to the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Behind,
    /// Among the text: in-line, wrapped and text-breaking objects, which never overlap it.
    Text,
    Front,
}

/// An object placed on a page. Points from the page's top-left corner.
///
/// To draw it: rotate a `width` x `height` box by `rotation` about the centre of `bounds`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedObject {
    /// The caller's [`ObjectInput::key`].
    pub key: usize,
    pub paragraph: usize,
    /// Index into the paragraph's objects.
    pub object: usize,
    pub layer: Layer,
    /// The space the object takes: its rotated bounding box.
    pub bounds: Rect,
    /// Drawn size before rotation (smaller than asked for if an in-line object had to shrink
    /// to fit its line).
    pub width: f32,
    pub height: f32,
    /// Degrees clockwise.
    pub rotation: f32,
}

/// The lines of one paragraph that landed on one page.
///
/// To place a line on the page: `x = fragment.left + glyph.x`,
/// `y = fragment.top + (line.top - first_line.top)` where `first_line` is
/// `paragraph.lines[fragment.lines.start]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// Paragraph index: body and cell paragraphs numbered together in reading order.
    pub paragraph: usize,
    /// Which of the paragraph's lines are on this page.
    pub lines: Range<usize>,
    /// Y of the fragment's first line top, measured from the top edge of the page.
    pub top: f32,
    /// X of the paragraph's left edge, measured from the left edge of the page.
    pub left: f32,
}

/// A straight line to draw (a table border), in points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rule {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// A glyph positioned on a page: `(x, y)` is its baseline origin in points from the page's
/// top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    pub font: FontId,
    pub face_index: u32,
    pub font_size: f32,
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// `0xRRGGBB`; `None` is the renderer's ink colour.
    pub color: Option<u32>,
}

/// A highlight, underline or strikethrough placed on a page: a filled rectangle in points from
/// the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedDecoration {
    pub kind: DecorationKind,
    pub rect: Rect,
    /// `0xRRGGBB`; `None` is the renderer's ink colour.
    pub color: Option<u32>,
}

/// What layout needs per block after numbering its paragraphs.
enum Plan {
    Paragraph(usize),
    /// Rows of cells; each cell is a range of paragraph indices.
    Table(Vec<Vec<Range<usize>>>),
}

/// Lays out a whole document onto pages, caching paragraph layouts between calls.
///
/// Paragraphs are cached by content (text, styles, width), not by position, so inserting or
/// deleting a paragraph doesn't invalidate the others. Restacking lines into pages is plain
/// arithmetic and is redone in full on every call.
///
/// Paragraph indices count body paragraphs and table-cell paragraphs together, in reading
/// order (a table's cells row by row).
pub struct Paginator {
    geometry: PageGeometry,
    cache: HashMap<u64, ParagraphLayout>,
    keys: Vec<u64>,
    /// Each paragraph's style from the last call, for spacing while stacking.
    styles: Vec<ParagraphStyle>,
    /// Each paragraph's objects from the last call (forced in line in table cells).
    objects: Vec<Vec<ObjectInput>>,
    /// Whether each paragraph is in a table cell.
    in_cell: Vec<bool>,
    plan: Vec<Plan>,
    pages: Vec<Page>,
    /// Every line in document order with its page position; navigation walks this.
    pub(crate) lines: Vec<PlacedLine>,
    /// Index into `lines` of each paragraph's first line.
    pub(crate) first_line: Vec<usize>,
    /// Paragraph indices of the header's and the footer's paragraphs (after the body's).
    header: Range<usize>,
    footer: Range<usize>,
    /// For each header and footer paragraph, the index into `lines` of its first line on each
    /// page.
    margin_first: Vec<Vec<usize>>,
    /// Which page's copy of the header and footer positions refer to (see
    /// [`Paginator::set_margin_page`]).
    margin_page: Cell<usize>,
    /// Footnotes from the last call, and their paragraphs' layouts (at the content width),
    /// styles and objects.
    notes: Vec<NotePlan>,
    note_keys: Vec<u64>,
    note_styles: Vec<ParagraphStyle>,
    note_objects: Vec<Vec<ObjectInput>>,
    /// Paragraph indices of the footnotes' paragraphs (after the footer's).
    note_range: Range<usize>,
    last_shaped: usize,
}

impl Paginator {
    pub fn new(geometry: PageGeometry) -> Self {
        Self {
            geometry,
            cache: HashMap::new(),
            keys: Vec::new(),
            styles: Vec::new(),
            objects: Vec::new(),
            in_cell: Vec::new(),
            plan: Vec::new(),
            pages: Vec::new(),
            lines: Vec::new(),
            first_line: Vec::new(),
            header: 0..0,
            footer: 0..0,
            margin_first: Vec::new(),
            margin_page: Cell::new(0),
            notes: Vec::new(),
            note_keys: Vec::new(),
            note_styles: Vec::new(),
            note_objects: Vec::new(),
            note_range: 0..0,
            last_shaped: 0,
        }
    }

    /// Paragraph indices of the footnotes' paragraphs from the last call, in note order.
    pub fn note_paragraphs(&self) -> Range<usize> {
        self.note_range.clone()
    }

    /// Paragraph indices of the header's paragraphs from the last call.
    pub fn header_paragraphs(&self) -> Range<usize> {
        self.header.clone()
    }

    /// Paragraph indices of the footer's paragraphs from the last call.
    pub fn footer_paragraphs(&self) -> Range<usize> {
        self.footer.clone()
    }

    /// The header and footer are on every page, so a position in them is on every page too:
    /// this picks the page whose copy carets, selections and Up/Down use (the page clicked).
    pub fn set_margin_page(&self, page: usize) {
        self.margin_page.set(page);
    }

    pub(crate) fn is_margin(&self, paragraph: usize) -> bool {
        self.header.contains(&paragraph) || self.footer.contains(&paragraph)
    }

    /// Index into `lines` of `paragraph`'s first line, on the margin page for the header and
    /// footer.
    pub(crate) fn first_line_of(&self, paragraph: usize) -> Option<usize> {
        if self.is_margin(paragraph) {
            let copies = self.margin_first.get(paragraph - self.header.start)?;
            let page = self.margin_page.get().min(copies.len().saturating_sub(1));
            return copies.get(page).copied();
        }
        self.first_line.get(paragraph).copied()
    }

    pub fn geometry(&self) -> &PageGeometry {
        &self.geometry
    }

    /// Changes the page size. Every paragraph whose width changes is laid out again next call.
    pub fn set_geometry(&mut self, geometry: PageGeometry) {
        self.geometry = geometry;
    }

    /// Forgets every cached paragraph layout, so the next call shapes everything again.
    ///
    /// Call after registering a font: the cache is keyed by content, so paragraphs that asked
    /// for the new face earlier would otherwise keep their fallback shaping.
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// Lays out a document made only of paragraphs.
    pub fn layout(&mut self, engine: &mut LayoutEngine, paragraphs: &[ParagraphInput]) -> &[Page] {
        let blocks: Vec<BlockInput> = paragraphs
            .iter()
            .copied()
            .map(BlockInput::Paragraph)
            .collect();
        self.layout_blocks(engine, &blocks)
    }

    /// Lays out paragraphs and tables and splits them into pages.
    pub fn layout_blocks(&mut self, engine: &mut LayoutEngine, blocks: &[BlockInput]) -> &[Page] {
        self.layout_document(engine, blocks, &[], &[])
    }

    /// [`Paginator::layout_blocks`] with a header and footer drawn on every page, in the top
    /// and bottom margins ([`PageGeometry::header_distance`] from the edges). Their paragraphs
    /// are numbered after the body's, header first. Objects with [`ObjectMode::Field`] show
    /// each page's number or the page count; everything else in them is laid out in line.
    pub fn layout_document(
        &mut self,
        engine: &mut LayoutEngine,
        blocks: &[BlockInput],
        header: &[ParagraphInput],
        footer: &[ParagraphInput],
    ) -> &[Page] {
        self.layout_with_notes(engine, blocks, header, footer, &[])
    }

    /// [`Paginator::layout_document`] with footnotes. Each note goes at the foot of the page
    /// holding its reference, which moves to the next page if the note doesn't fit beside it.
    /// Notes are stacked in the order given, under a short rule; their paragraphs are
    /// numbered after the footer's. A reference is an object with
    /// [`ObjectMode::Field`]`(`[`FieldKind::Note`]`)`: it's drawn as its number.
    ///
    /// A note is never split across pages: one taller than the space left under its
    /// reference's line moves that line to a new page, and one taller than a page overlaps
    /// the body.
    pub fn layout_with_notes(
        &mut self,
        engine: &mut LayoutEngine,
        blocks: &[BlockInput],
        header: &[ParagraphInput],
        footer: &[ParagraphInput],
        notes: &[NoteInput],
    ) -> &[Page] {
        let content = self.geometry.content_width();
        // Number every paragraph in reading order and note the width it's laid out at.
        let mut inputs: Vec<(ParagraphInput, f32)> = Vec::new();
        self.plan.clear();
        self.objects.clear();
        self.in_cell.clear();
        for block in blocks {
            match block {
                BlockInput::Paragraph(p) => {
                    inputs.push((*p, content));
                    self.objects.push(size_references(engine, p, false));
                    self.in_cell.push(false);
                    self.plan.push(Plan::Paragraph(inputs.len() - 1));
                }
                BlockInput::Table(t) => {
                    let cols = column_count(t);
                    let width = cell_text_width(content, cols);
                    let mut rows = Vec::with_capacity(t.rows.len());
                    for row in &t.rows {
                        let mut cells = Vec::with_capacity(cols);
                        for cell in row.iter().take(cols) {
                            let start = inputs.len();
                            for p in cell {
                                inputs.push((*p, width));
                                self.objects.push(size_references(engine, p, true));
                                self.in_cell.push(true);
                            }
                            cells.push(start..inputs.len());
                        }
                        rows.push(cells);
                    }
                    self.plan.push(Plan::Table(rows));
                }
            }
        }
        self.styles = inputs.iter().map(|(p, _)| p.style).collect();
        self.last_shaped = 0;

        let mut old = std::mem::take(&mut self.cache);
        let mut fresh: HashMap<u64, ParagraphLayout> = HashMap::with_capacity(inputs.len());
        // Notes don't wrap around anything, so they're laid out once, before the passes.
        self.notes.clear();
        self.note_keys.clear();
        self.note_styles.clear();
        self.note_objects.clear();
        for n in notes {
            let start = self.note_keys.len();
            for p in &n.paragraphs {
                let objects = size_references(engine, p, true);
                let key = cache_key(p, content, &objects, &[]);
                if let std::collections::hash_map::Entry::Vacant(slot) = fresh.entry(key) {
                    let layout = match old.remove(&key) {
                        Some(l) => l,
                        None => {
                            self.last_shaped += 1;
                            engine.layout_list_item(
                                p.text,
                                p.base,
                                p.spans,
                                &p.style,
                                content,
                                &objects,
                                &[],
                                p.marker,
                            )
                        }
                    };
                    slot.insert(layout);
                }
                self.note_keys.push(key);
                self.note_styles.push(p.style);
                self.note_objects.push(objects);
            }
            self.notes.push(NotePlan {
                paragraph: n.paragraph,
                byte: n.byte,
                paragraphs: start..self.note_keys.len(),
            });
        }
        let mut exclusions: Vec<Vec<Exclusion>> = vec![Vec::new(); inputs.len()];
        let mut pass = 0;
        let stacked = loop {
            self.keys.clear();
            for (i, (p, width)) in inputs.iter().enumerate() {
                let objects = &self.objects[i];
                let key = cache_key(p, *width, objects, &exclusions[i]);
                self.keys.push(key);
                if fresh.contains_key(&key) {
                    continue;
                }
                let layout = match old.remove(&key) {
                    Some(l) => l,
                    None => {
                        self.last_shaped += 1;
                        engine.layout_list_item(
                            p.text,
                            p.base,
                            p.spans,
                            &p.style,
                            *width,
                            objects,
                            &exclusions[i],
                            p.marker,
                        )
                    }
                };
                fresh.insert(key, layout);
            }
            self.cache = fresh;
            let stacked = self.stack();
            pass += 1;
            if stacked.wraps.is_empty() || pass >= MAX_WRAP_PASSES {
                break stacked;
            }
            let next = self.exclusions(&stacked);
            if next == exclusions {
                break stacked;
            }
            exclusions = next;
            fresh = std::mem::take(&mut self.cache);
        };
        let mut stacked = stacked;
        self.place_references(engine, &inputs, &mut stacked.pages);
        self.place_margins(engine, &mut stacked, &mut old, header, footer);
        self.place_notes(&mut stacked);
        let notes = self.note_range.clone();
        self.place_inline_objects(&mut stacked.pages, notes);
        // Dropping entries not used this call keeps memory proportional to the document.
        let used: HashSet<u64> = self.keys.iter().copied().collect();
        self.cache.retain(|k, _| used.contains(k));
        self.pages = stacked.pages;
        self.lines = stacked.lines;
        self.first_line = stacked.first_line;
        &self.pages
    }

    /// Lays out the header and footer (now that the page count is known) and places them
    /// on every page, with each page's field labels.
    fn place_margins(
        &mut self,
        engine: &mut LayoutEngine,
        stacked: &mut Stacked,
        old: &mut HashMap<u64, ParagraphLayout>,
        header: &[ParagraphInput],
        footer: &[ParagraphInput],
    ) {
        let start = self.keys.len();
        self.header = start..start + header.len();
        self.footer = self.header.end..self.header.end + footer.len();
        let mut margin_first: Vec<Vec<usize>> = vec![Vec::new(); header.len() + footer.len()];
        let count = stacked.pages.len();
        let width = self.geometry.content_width();
        // Every number takes the room of the widest (Noto's digits all have one width).
        let widest = "0".repeat(count.to_string().len());
        for p in header.iter().chain(footer) {
            let objects: Vec<ObjectInput> = p
                .objects
                .iter()
                .map(|o| match o.mode {
                    ObjectMode::Field(_) => {
                        let style = style_at(p, o.byte);
                        let size = engine.shape_label(&widest, style);
                        ObjectInput {
                            width: size.as_ref().map_or(0.0, |s| s.width),
                            height: size.as_ref().map_or(0.0, |s| s.ascent),
                            rotation: 0.0,
                            ..*o
                        }
                    }
                    _ => ObjectInput {
                        mode: ObjectMode::Inline,
                        ..*o
                    },
                })
                .collect();
            let key = cache_key(p, width, &objects, &[]);
            if !self.cache.contains_key(&key) {
                let layout = match old.remove(&key) {
                    Some(l) => l,
                    None => {
                        self.last_shaped += 1;
                        engine.layout_list_item(
                            p.text,
                            p.base,
                            p.spans,
                            &p.style,
                            width,
                            &objects,
                            &[],
                            p.marker,
                        )
                    }
                };
                self.cache.insert(key, layout);
            }
            self.keys.push(key);
            self.styles.push(p.style);
            self.objects.push(objects);
            self.in_cell.push(true);
            stacked.first_line.push(0);
        }
        let g = self.geometry;
        let left = g.margin_left;
        let mut labels: HashMap<String, Option<(Vec<GlyphRun>, f32)>> = HashMap::new();
        for page in 0..count {
            for (zone, range) in [
                (Zone::Header, self.header.clone()),
                (Zone::Footer, self.footer.clone()),
            ] {
                // Heights with spacing, so the footer can end at its distance from the bottom.
                let mut total = 0.0;
                for i in range.clone() {
                    let st = self.style(i);
                    let h = self.paragraph(i).map_or(0.0, |p| p.height);
                    total += st.space_before + h + st.space_after;
                }
                let mut y = match zone {
                    Zone::Header => g.header_distance,
                    _ => g.height - g.footer_distance - total,
                };
                for i in range {
                    let st = self.style(i);
                    y += st.space_before;
                    let Some(para) = self.paragraph(i) else {
                        continue;
                    };
                    let Some(first) = para.lines.first() else {
                        continue;
                    };
                    let top = y;
                    let dy = top - first.top;
                    margin_first[i - start].push(stacked.lines.len());
                    for (n, line) in para.lines.iter().enumerate() {
                        let (lt, lb) = (dy + line.top, dy + line.bottom);
                        stacked.lines.push(PlacedLine {
                            page,
                            paragraph: i,
                            line: n,
                            top: lt,
                            bottom: lb,
                            left,
                            width,
                            region: Rect {
                                x: left,
                                y: lt,
                                width,
                                height: lb - lt,
                            },
                            zone,
                        });
                        for b in &line.objects {
                            let Some(ObjectMode::Field(kind)) =
                                self.objects[i].get(b.object).map(|o| o.mode)
                            else {
                                continue;
                            };
                            let text = match kind {
                                FieldKind::PageNumber => (page + 1).to_string(),
                                FieldKind::PageCount => count.to_string(),
                                FieldKind::Note(n) => n.to_string(),
                            };
                            let input = &header.iter().chain(footer).nth(i - start);
                            let Some(p) = input else {
                                continue;
                            };
                            let byte = self.objects[i][b.object].byte;
                            let runs = labels
                                .entry(text.clone())
                                .or_insert_with(|| {
                                    engine
                                        .shape_label(&text, style_at(p, byte))
                                        .map(|l| (l.runs, l.width))
                                })
                                .clone();
                            let Some((mut runs, label_width)) = runs else {
                                continue;
                            };
                            // Shorter numbers sit where the paragraph's alignment puts them.
                            let spare = (b.width - label_width).max(0.0);
                            let shift = match st.align {
                                Align::Right => spare,
                                Align::Center => spare / 2.0,
                                Align::Left | Align::Justify => 0.0,
                            };
                            for r in &mut runs {
                                for gl in &mut r.glyphs {
                                    gl.x += left + b.x + shift;
                                    gl.y += dy + line.baseline;
                                }
                            }
                            if let Some(pg) = stacked.pages.get_mut(page) {
                                pg.labels.push(PlacedLabel {
                                    paragraph: i,
                                    byte,
                                    text,
                                    runs,
                                });
                            }
                        }
                    }
                    if let Some(pg) = stacked.pages.get_mut(page) {
                        pg.fragments.push(Fragment {
                            paragraph: i,
                            lines: 0..para.lines.len(),
                            top,
                            left,
                        });
                    }
                    y += para.height + st.space_after;
                }
            }
        }
        for (k, copies) in margin_first.iter().enumerate() {
            if let (Some(first), Some(slot)) =
                (copies.first(), stacked.first_line.get_mut(start + k))
            {
                *slot = *first;
            }
        }
        self.margin_first = margin_first;
    }

    /// Draws each footnote reference in the body as its number, where its line was placed.
    fn place_references(
        &self,
        engine: &mut LayoutEngine,
        inputs: &[(ParagraphInput, f32)],
        pages: &mut [Page],
    ) {
        for page in pages {
            let mut found = Vec::new();
            for f in &page.fragments {
                let (Some(para), Some((input, _))) =
                    (self.paragraph(f.paragraph), inputs.get(f.paragraph))
                else {
                    continue;
                };
                let Some(first) = para.lines.get(f.lines.start) else {
                    continue;
                };
                let dy = f.top - first.top;
                for line in &para.lines[f.lines.clone()] {
                    for b in &line.objects {
                        let Some(o) = self.objects.get(f.paragraph).and_then(|v| v.get(b.object))
                        else {
                            continue;
                        };
                        let ObjectMode::Field(FieldKind::Note(n)) = o.mode else {
                            continue;
                        };
                        let text = n.to_string();
                        let Some(label) = engine.shape_label(&text, style_at(input, o.byte)) else {
                            continue;
                        };
                        let mut runs = label.runs;
                        for r in &mut runs {
                            for gl in &mut r.glyphs {
                                gl.x += f.left + b.x;
                                gl.y += dy + line.baseline;
                            }
                        }
                        found.push(PlacedLabel {
                            paragraph: f.paragraph,
                            byte: o.byte,
                            text,
                            runs,
                        });
                    }
                }
            }
            page.labels.extend(found);
        }
    }

    /// Total height of footnote `k`'s paragraphs with their spacing.
    fn note_height(&self, k: usize) -> f32 {
        self.notes[k]
            .paragraphs
            .clone()
            .map(|j| {
                let st = self.note_styles[j];
                let h = self.cache.get(&self.note_keys[j]).map_or(0.0, |p| p.height);
                st.space_before + h + st.space_after
            })
            .sum()
    }

    /// Numbers the footnotes' paragraphs after the footer's and places each page's notes at
    /// its foot, under a rule a third of the text's width, in reference order.
    fn place_notes(&mut self, stacked: &mut Stacked) {
        let start = self.keys.len();
        self.note_range = start..start + self.note_keys.len();
        for j in 0..self.note_keys.len() {
            self.keys.push(self.note_keys[j]);
            self.styles.push(self.note_styles[j]);
            self.objects.push(self.note_objects[j].clone());
            self.in_cell.push(true);
            stacked.first_line.push(0);
        }
        let g = self.geometry;
        let (left, width) = (g.margin_left, g.content_width());
        let bottom = g.margin_top + g.content_height();
        for page in 0..stacked.pages.len() {
            let here: Vec<usize> = (0..self.notes.len())
                .filter(|&k| stacked.note_page.get(k).copied().flatten() == Some(page))
                .collect();
            if here.is_empty() {
                continue;
            }
            let total = NOTE_SEPARATOR + here.iter().map(|&k| self.note_height(k)).sum::<f32>();
            let mut y = bottom - total;
            stacked.pages[page].rules.push(Rule {
                x0: left,
                y0: y + NOTE_RULE_Y,
                x1: left + width / 3.0,
                y1: y + NOTE_RULE_Y,
            });
            y += NOTE_SEPARATOR;
            for &k in &here {
                for j in self.notes[k].paragraphs.clone() {
                    let i = start + j;
                    let st = self.style(i);
                    y += st.space_before;
                    let Some(para) = self.paragraph(i) else {
                        continue;
                    };
                    let Some(first) = para.lines.first() else {
                        continue;
                    };
                    let dy = y - first.top;
                    stacked.first_line[i] = stacked.lines.len();
                    for (n, line) in para.lines.iter().enumerate() {
                        let (lt, lb) = (dy + line.top, dy + line.bottom);
                        stacked.lines.push(PlacedLine {
                            page,
                            paragraph: i,
                            line: n,
                            top: lt,
                            bottom: lb,
                            left,
                            width,
                            region: Rect {
                                x: left,
                                y: lt,
                                width,
                                height: lb - lt,
                            },
                            zone: Zone::Footnote,
                        });
                    }
                    stacked.pages[page].fragments.push(Fragment {
                        paragraph: i,
                        lines: 0..para.lines.len(),
                        top: y,
                        left,
                    });
                    y += para.height + st.space_after;
                }
            }
        }
    }

    /// Pages from the last layout call.
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// Layout of paragraph `index` from the last call.
    pub fn paragraph(&self, index: usize) -> Option<&ParagraphLayout> {
        self.keys.get(index).and_then(|k| self.cache.get(k))
    }

    /// How many paragraphs the last call had to shape (the rest came from the cache).
    pub fn last_shaped_count(&self) -> usize {
        self.last_shaped
    }

    /// Every glyph on page `index`, positioned in points from the page's top-left corner.
    pub fn placed_glyphs(&self, index: usize) -> Vec<PlacedGlyph> {
        let mut out = Vec::new();
        let Some(page) = self.pages.get(index) else {
            return out;
        };
        for f in &page.fragments {
            let Some(para) = self.paragraph(f.paragraph) else {
                continue;
            };
            let Some(first) = para.lines.get(f.lines.start) else {
                continue;
            };
            let dy = f.top - first.top;
            let marker = para
                .marker
                .as_ref()
                .filter(|_| f.lines.start == 0)
                .map(|m| m.runs.as_slice())
                .unwrap_or_default();
            let runs = para.lines[f.lines.clone()]
                .iter()
                .flat_map(|line| &line.runs)
                .chain(marker);
            for run in runs {
                out.extend(run.glyphs.iter().map(|g| PlacedGlyph {
                    font: run.font,
                    face_index: run.face_index,
                    font_size: run.font_size,
                    id: g.id,
                    x: f.left + g.x,
                    y: dy + g.y,
                    color: run.color,
                }));
            }
        }
        for run in page.labels.iter().flat_map(|l| &l.runs) {
            out.extend(run.glyphs.iter().map(|g| PlacedGlyph {
                font: run.font,
                face_index: run.face_index,
                font_size: run.font_size,
                id: g.id,
                x: g.x,
                y: g.y,
                color: run.color,
            }));
        }
        out
    }

    /// Every highlight, underline and strikethrough on page `index`, highlights first (they go
    /// behind the text), in points from the page's top-left corner.
    pub fn placed_decorations(&self, index: usize) -> Vec<PlacedDecoration> {
        let mut out = Vec::new();
        let Some(page) = self.pages.get(index) else {
            return out;
        };
        for f in &page.fragments {
            let Some(para) = self.paragraph(f.paragraph) else {
                continue;
            };
            let Some(first) = para.lines.get(f.lines.start) else {
                continue;
            };
            let dy = f.top - first.top;
            for d in para.lines[f.lines.clone()]
                .iter()
                .flat_map(|l| &l.decorations)
            {
                out.push(PlacedDecoration {
                    kind: d.kind,
                    rect: Rect {
                        x: f.left + d.x,
                        y: dy + d.y,
                        width: d.width,
                        height: d.height,
                    },
                    color: d.color,
                });
            }
        }
        out.sort_by_key(|d| d.kind != DecorationKind::Highlight);
        out
    }

    fn stack(&self) -> Stacked {
        let g = self.geometry;
        let mut anchors: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (k, n) in self.notes.iter().enumerate() {
            anchors.entry(n.paragraph).or_default().push((n.byte, k));
        }
        let mut s = Stacker {
            out: Stacked {
                pages: vec![Page::default()],
                lines: Vec::new(),
                first_line: vec![0; self.keys.len()],
                wraps: Vec::new(),
                note_page: vec![None; self.notes.len()],
            },
            y: g.margin_top,
            g,
            notes_h: 0.0,
            has_notes: false,
            note_heights: (0..self.notes.len()).map(|k| self.note_height(k)).collect(),
            anchors,
        };
        for block in &self.plan {
            match block {
                Plan::Paragraph(i) => self.stack_paragraph(&mut s, *i),
                Plan::Table(rows) => self.stack_table(&mut s, rows),
            }
        }
        // A note whose reference wasn't found still needs a place to be edited.
        let last = s.page();
        for p in &mut s.out.note_page {
            p.get_or_insert(last);
        }
        self.place_inline_objects(&mut s.out.pages, 0..usize::MAX);
        s.out
    }

    /// Adds the in-line objects of paragraphs in `paragraphs` to their pages, where their lines
    /// were placed.
    fn place_inline_objects(&self, pages: &mut [Page], paragraphs: Range<usize>) {
        for page in pages {
            let mut found = Vec::new();
            for f in page
                .fragments
                .iter()
                .filter(|f| paragraphs.contains(&f.paragraph))
            {
                let Some(para) = self.paragraph(f.paragraph) else {
                    continue;
                };
                let Some(first) = para.lines.get(f.lines.start) else {
                    continue;
                };
                let dy = f.top - first.top;
                for line in &para.lines[f.lines.clone()] {
                    for b in &line.objects {
                        let Some(o) = self.objects.get(f.paragraph).and_then(|v| v.get(b.object))
                        else {
                            continue;
                        };
                        let (fw, _) = o.footprint();
                        let k = if fw > 0.0 { b.width / fw } else { 1.0 };
                        found.push(PlacedObject {
                            key: o.key,
                            paragraph: f.paragraph,
                            object: b.object,
                            layer: Layer::Text,
                            bounds: Rect {
                                x: f.left + b.x,
                                y: dy + b.top,
                                width: b.width,
                                height: b.height,
                            },
                            width: o.width * k,
                            height: o.height * k,
                            rotation: o.rotation,
                        });
                    }
                }
            }
            page.objects.extend(found);
        }
    }

    /// Where each body paragraph's lines must keep clear of wrapped objects, given where the
    /// last stacking put everything: each wrapped object (plus margin) on a page, in the
    /// coordinates of every paragraph piece on that page.
    fn exclusions(&self, stacked: &Stacked) -> Vec<Vec<Exclusion>> {
        let mut out = vec![Vec::new(); self.keys.len()];
        for (p, page) in stacked.pages.iter().enumerate() {
            let wraps: Vec<&Rect> = stacked
                .wraps
                .iter()
                .filter(|(q, _)| *q == p)
                .map(|(_, r)| r)
                .collect();
            if wraps.is_empty() {
                continue;
            }
            for f in &page.fragments {
                if self.in_cell.get(f.paragraph).copied().unwrap_or(true) {
                    continue;
                }
                let Some(para) = self.paragraph(f.paragraph) else {
                    continue;
                };
                let (Some(first), Some(last)) = (
                    para.lines.get(f.lines.start),
                    para.lines.get(f.lines.end - 1),
                ) else {
                    continue;
                };
                // Page y + shift = paragraph y, for this piece.
                let shift = first.top - f.top;
                // A piece continuing from an earlier page starts at its first line; the first
                // piece at the paragraph's top, above a first line that moved down.
                let floor = if f.lines.start == 0 { 0.0 } else { first.top };
                for r in &wraps {
                    let e = Exclusion {
                        // Never reaching up into the paragraph's lines on an earlier page.
                        top: (r.y + shift).max(floor),
                        bottom: r.y + r.height + shift,
                        left: r.x - f.left,
                        right: r.x + r.width - f.left,
                    };
                    if e.top < last.bottom && e.bottom > floor {
                        out[f.paragraph].push(e);
                    }
                }
            }
        }
        out
    }

    /// Places a body paragraph line by line, breaking pages between lines, with its objects
    /// that break text on lines of their own and its objects positioned on the page.
    fn stack_paragraph(&self, s: &mut Stacker, index: usize) {
        s.out.first_line[index] = s.out.lines.len();
        let Some(para) = self.paragraph(index) else {
            return;
        };
        let left = s.g.margin_left;
        let width = s.g.content_width();
        let style = self.style(index);
        let objects = self.objects.get(index).map_or(&[][..], Vec::as_slice);
        if style.page_break_before && s.y > s.g.margin_top {
            s.new_page();
        }
        let mut start = 0;
        // Page and y of the paragraph's first line: what positioned objects are measured from.
        let mut top: Option<(usize, f32)> = None;
        for (i, line) in para.lines.iter().enumerate() {
            let h = line.bottom - line.top;
            // Space before is dropped at the top of a page, so pages start flush with the
            // margin, as in Google Docs. So is a gap left where a line moved below a wrapped
            // object.
            let above = if i == 0 {
                0.0
            } else {
                para.lines[i - 1].bottom
            };
            let gap = (line.top - above).max(0.0);
            let mut pre = if i == 0 && s.y > s.g.margin_top {
                style.space_before
            } else {
                0.0
            };
            let last = i + 1 == para.lines.len();
            for (k, o) in objects.iter().enumerate() {
                if o.mode != ObjectMode::Break || !holds(line, o.byte, last) {
                    continue;
                }
                let (fw, fh) = o.footprint();
                let band = fh + 2.0 * BREAK_MARGIN;
                // A fragment's lines can't have gaps layout doesn't know of, so the lines so
                // far end one here.
                s.push_fragment(index, start..i, para, left);
                start = i;
                if s.needs_break(pre + band + h) {
                    s.new_page();
                    pre = 0.0;
                }
                s.y += pre + BREAK_MARGIN;
                pre = 0.0;
                let x = (left + o.x).clamp(0.0, (s.g.width - fw).max(0.0));
                let page = s.page();
                s.place_on(page, index, k, o, x, s.y, Layer::Text);
                s.y += fh + BREAK_MARGIN;
            }
            // The line's footnotes must fit on its page too.
            let notes = s.notes_on(index, line, last);
            if s.needs_break(pre + gap + h + s.notes_extra(&notes)) {
                s.push_fragment(index, start..i, para, left);
                s.new_page();
                start = i;
                if i == 0 {
                    top = Some((s.page(), s.y));
                }
            } else {
                s.y += pre;
                // The paragraph's top, not its first line's: that line may have moved below
                // an object measured from this very point.
                if i == 0 {
                    top = Some((s.page(), s.y));
                }
                s.y += gap;
            }
            s.out.lines.push(PlacedLine {
                page: s.page(),
                paragraph: index,
                line: i,
                top: s.y,
                bottom: s.y + h,
                left,
                width,
                region: Rect {
                    x: left,
                    y: s.y,
                    width,
                    height: h,
                },
                zone: Zone::Body,
            });
            s.y += h;
            s.add_notes(&notes);
        }
        s.push_fragment(index, start..para.lines.len(), para, left);
        // Space after may run past the bottom margin; the next block then starts a new page.
        s.y += style.space_after;

        let Some((page, y0)) = top else {
            return;
        };
        for (k, o) in objects.iter().enumerate() {
            let layer = match o.mode {
                ObjectMode::Inline | ObjectMode::Break | ObjectMode::Field(_) => continue,
                ObjectMode::Wrap => Layer::Text,
                ObjectMode::Behind => Layer::Behind,
                ObjectMode::Front => Layer::Front,
            };
            // Kept on the page, which may put it nearer the paragraph than asked.
            let (fw, fh) = o.footprint();
            let x = (left + o.x).clamp(0.0, (s.g.width - fw).max(0.0));
            let y = (y0 + o.y).clamp(0.0, (s.g.height - fh).max(0.0));
            let b = s.place_on(page, index, k, o, x, y, layer).bounds;
            if o.mode == ObjectMode::Wrap {
                s.out.wraps.push((
                    page,
                    Rect {
                        x: b.x - WRAP_MARGIN,
                        y: b.y - WRAP_MARGIN,
                        width: b.width + 2.0 * WRAP_MARGIN,
                        height: b.height + 2.0 * WRAP_MARGIN,
                    },
                ));
            }
        }
    }

    fn style(&self, index: usize) -> ParagraphStyle {
        self.styles.get(index).copied().unwrap_or_default()
    }

    /// Places a table row by row.
    ///
    /// A row that doesn't fit is split across pages at line boundaries, as in Google Docs: each
    /// cell fills the rest of the page and continues on the next, and the table's borders are
    /// drawn around each page's piece. A row with no line that fits in the remaining space moves
    /// to the next page whole. Nothing is placed below the bottom margin, except a single line
    /// taller than a whole page, which gets a page of its own.
    fn stack_table(&self, s: &mut Stacker, rows: &[Vec<Range<usize>>]) {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let col_width = s.g.content_width() / cols as f32;
        let text_width = cell_text_width(s.g.content_width(), cols);

        for row in rows {
            let cells: Vec<Vec<CellLine>> = row.iter().map(|cell| self.cell_lines(cell)).collect();
            let mut next = vec![0usize; cells.len()];

            while cells.iter().zip(&next).any(|(c, &n)| n < c.len()) {
                // Footnotes of lines placed so far on this page shorten it; those of this
                // piece's own lines are added once it's placed.
                let room = s.limit() - s.y - 2.0 * CELL_PADDING;
                let mut take: Vec<usize> = cells
                    .iter()
                    .zip(&next)
                    .map(|(c, &n)| lines_that_fit(&c[n..], room))
                    .collect();
                let progress = take.iter().any(|&k| k > 0);
                if !progress && s.y > s.g.margin_top {
                    s.new_page();
                    continue;
                }
                if !progress {
                    // A line taller than an empty page: place it anyway rather than loop.
                    for (k, (c, &n)) in take.iter_mut().zip(cells.iter().zip(&next)) {
                        *k = usize::from(n < c.len());
                    }
                }

                let top = s.y;
                let page = s.page();
                let mut piece_notes: Vec<usize> = Vec::new();
                let content = cells
                    .iter()
                    .zip(&next)
                    .zip(&take)
                    .map(|((c, &n), &k)| c[n..n + k].iter().map(CellLine::extent).sum::<f32>())
                    .fold(0.0, f32::max);
                let continues = cells
                    .iter()
                    .zip(&next)
                    .zip(&take)
                    .any(|((c, &n), &k)| n + k < c.len());
                // A piece that continues on the next page reaches down to the bottom margin,
                // as in Google Docs, so the table visibly runs off the page.
                let height = if continues {
                    (s.limit() - top).max(content + 2.0 * CELL_PADDING)
                } else {
                    content + 2.0 * CELL_PADDING
                };

                for (col, ((c, n), &k)) in cells.iter().zip(next.iter_mut()).zip(&take).enumerate()
                {
                    let x0 = s.g.margin_left + col as f32 * col_width;
                    let left = x0 + CELL_PADDING;
                    let region = Rect {
                        x: x0,
                        y: top,
                        width: col_width,
                        height,
                    };
                    let mut y = top + CELL_PADDING;
                    let placed = &c[*n..*n + k];
                    // One fragment per paragraph piece on this page: (paragraph, first line,
                    // top, line count).
                    let mut pieces: Vec<(usize, usize, f32, usize)> = Vec::new();
                    for l in placed {
                        if let Some(p) = self.paragraph(l.paragraph) {
                            if let Some(line) = p.lines.get(l.line) {
                                let last = l.line + 1 == p.lines.len();
                                piece_notes.extend(s.notes_on(l.paragraph, line, last));
                            }
                        }
                        y += l.before;
                        if l.line == 0 {
                            s.out.first_line[l.paragraph] = s.out.lines.len();
                        }
                        s.out.lines.push(PlacedLine {
                            page,
                            paragraph: l.paragraph,
                            line: l.line,
                            top: y,
                            bottom: y + l.height,
                            left,
                            width: text_width,
                            region,
                            zone: Zone::Body,
                        });
                        match pieces.last_mut() {
                            Some(p) if p.0 == l.paragraph => p.3 += 1,
                            _ => pieces.push((l.paragraph, l.line, y, 1)),
                        }
                        y += l.height + l.after;
                    }
                    if let Some(p) = s.out.pages.last_mut() {
                        for (paragraph, first, top, count) in pieces {
                            p.fragments.push(Fragment {
                                paragraph,
                                lines: first..first + count,
                                top,
                                left,
                            });
                        }
                    }
                    *n += k;
                }

                s.push_borders(top, top + height, cols, col_width);
                s.add_notes(&piece_notes);
                s.y = top + height;
                if cells.iter().zip(&next).any(|(c, &n)| n < c.len()) {
                    s.new_page();
                }
            }
        }
    }
}

/// One line of a table cell, with the paragraph spacing around it.
struct CellLine {
    paragraph: usize,
    line: usize,
    height: f32,
    /// Space above: the paragraph's space before, on its first line only.
    before: f32,
    /// Space below: the paragraph's space after, on its last line only.
    after: f32,
}

impl CellLine {
    fn extent(&self) -> f32 {
        self.before + self.height + self.after
    }
}

impl Paginator {
    /// A cell's lines in reading order; `cell` is its range of paragraph indices.
    fn cell_lines(&self, cell: &Range<usize>) -> Vec<CellLine> {
        let mut out = Vec::new();
        for i in cell.clone() {
            let Some(p) = self.paragraph(i) else {
                continue;
            };
            let style = self.style(i);
            let last = p.lines.len().saturating_sub(1);
            for (n, l) in p.lines.iter().enumerate() {
                out.push(CellLine {
                    paragraph: i,
                    line: n,
                    height: l.bottom - l.top,
                    before: if n == 0 { style.space_before } else { 0.0 },
                    after: if n == last { style.space_after } else { 0.0 },
                });
            }
        }
        out
    }
}

/// How many of `lines` fit, in order, within `room`. Space after a line doesn't need to fit.
fn lines_that_fit(lines: &[CellLine], room: f32) -> usize {
    let mut used = 0.0;
    lines
        .iter()
        .take_while(|l| {
            used += l.before + l.height;
            let fits = used <= room + 0.001;
            used += l.after;
            fits
        })
        .count()
}

struct Stacked {
    pages: Vec<Page>,
    lines: Vec<PlacedLine>,
    first_line: Vec<usize>,
    /// Wrapped objects plus their margin: `(page, area)`.
    wraps: Vec<(usize, Rect)>,
    /// The page each footnote goes on.
    note_page: Vec<Option<usize>>,
}

/// Whether `line` holds the placeholder at `byte`. The last line also holds a placeholder at
/// its very end.
fn holds(line: &Line, byte: usize, last: bool) -> bool {
    let r = &line.byte_range;
    r.start <= byte && (byte < r.end || (last && byte == r.end))
}

/// Running state while stacking blocks onto pages.
struct Stacker {
    out: Stacked,
    y: f32,
    g: PageGeometry,
    /// Height taken at the foot of this page by its footnotes, separator included.
    notes_h: f32,
    has_notes: bool,
    note_heights: Vec<f32>,
    /// For each paragraph holding footnote references: `(byte, note)`.
    anchors: HashMap<usize, Vec<(usize, usize)>>,
}

impl Stacker {
    fn page(&self) -> usize {
        self.out.pages.len() - 1
    }

    /// Where body text on this page must end: above its footnotes.
    fn limit(&self) -> f32 {
        self.g.margin_top + self.g.content_height() - self.notes_h
    }

    /// Whether something `h` tall must go on a new page. Only if something is already on this
    /// page: anything taller than a page goes on an empty page rather than moving on forever.
    fn needs_break(&self, h: f32) -> bool {
        self.y + h > self.limit() && self.y > self.g.margin_top
    }

    /// The footnotes referenced on `line` of `paragraph` that aren't placed yet.
    fn notes_on(&self, paragraph: usize, line: &Line, last: bool) -> Vec<usize> {
        self.anchors.get(&paragraph).map_or_else(Vec::new, |refs| {
            refs.iter()
                .filter(|&&(byte, k)| holds(line, byte, last) && self.out.note_page[k].is_none())
                .map(|&(_, k)| k)
                .collect()
        })
    }

    /// The room footnotes `notes` would take on this page, with the separator if they're
    /// its first.
    fn notes_extra(&self, notes: &[usize]) -> f32 {
        if notes.is_empty() {
            return 0.0;
        }
        let separator = if self.has_notes { 0.0 } else { NOTE_SEPARATOR };
        separator + notes.iter().map(|&k| self.note_heights[k]).sum::<f32>()
    }

    /// Puts footnotes `notes` on this page.
    fn add_notes(&mut self, notes: &[usize]) {
        if notes.is_empty() {
            return;
        }
        self.notes_h += self.notes_extra(notes);
        self.has_notes = true;
        let page = self.page();
        for &k in notes {
            self.out.note_page[k] = Some(page);
        }
    }

    /// Borders around one row piece: top, bottom and every column edge.
    fn push_borders(&mut self, top: f32, bottom: f32, cols: usize, col_width: f32) {
        let (x_start, x_end) = (
            self.g.margin_left,
            self.g.margin_left + self.g.content_width(),
        );
        if let Some(p) = self.out.pages.last_mut() {
            p.rules.push(Rule {
                x0: x_start,
                y0: top,
                x1: x_end,
                y1: top,
            });
            p.rules.push(Rule {
                x0: x_start,
                y0: bottom,
                x1: x_end,
                y1: bottom,
            });
            for c in 0..=cols {
                let x = x_start + c as f32 * col_width;
                p.rules.push(Rule {
                    x0: x,
                    y0: top,
                    x1: x,
                    y1: bottom,
                });
            }
        }
    }

    fn new_page(&mut self) {
        self.out.pages.push(Page::default());
        self.y = self.g.margin_top;
        self.notes_h = 0.0;
        self.has_notes = false;
    }

    /// Places an object with its bounding box's top-left corner at `(x, y)` on `page`.
    #[allow(clippy::too_many_arguments)]
    fn place_on(
        &mut self,
        page: usize,
        paragraph: usize,
        object: usize,
        o: &ObjectInput,
        x: f32,
        y: f32,
        layer: Layer,
    ) -> PlacedObject {
        let (fw, fh) = o.footprint();
        let placed = PlacedObject {
            key: o.key,
            paragraph,
            object,
            layer,
            bounds: Rect {
                x,
                y,
                width: fw,
                height: fh,
            },
            width: o.width,
            height: o.height,
            rotation: o.rotation,
        };
        if let Some(p) = self.out.pages.get_mut(page) {
            p.objects.push(placed);
        }
        placed
    }

    /// Adds the fragment of `lines` ending at the current y to the current page.
    fn push_fragment(
        &mut self,
        paragraph: usize,
        lines: Range<usize>,
        para: &ParagraphLayout,
        left: f32,
    ) {
        if lines.is_empty() {
            return;
        }
        let height = para.lines[lines.end - 1].bottom - para.lines[lines.start].top;
        let top = self.y - height;
        if let Some(page) = self.out.pages.last_mut() {
            page.fragments.push(Fragment {
                paragraph,
                lines,
                top,
                left,
            });
        }
    }
}

/// A paragraph's objects, footnote references sized to their numbers. In table cells (and
/// footnotes) everything else is laid out in line, whatever its mode.
fn size_references(
    engine: &mut LayoutEngine,
    p: &ParagraphInput,
    in_line: bool,
) -> Vec<ObjectInput> {
    p.objects
        .iter()
        .map(|o| match o.mode {
            ObjectMode::Field(FieldKind::Note(n)) => {
                let label = engine.shape_label(&n.to_string(), style_at(p, o.byte));
                ObjectInput {
                    width: label.as_ref().map_or(0.0, |l| l.width),
                    height: label.as_ref().map_or(0.0, |l| l.ascent),
                    rotation: 0.0,
                    ..*o
                }
            }
            ObjectMode::Field(_) => *o,
            _ if in_line => ObjectInput {
                mode: ObjectMode::Inline,
                ..*o
            },
            _ => *o,
        })
        .collect()
}

/// The style of the text at `byte` of a paragraph: the last span covering it, else its base.
fn style_at<'a>(p: &ParagraphInput<'a>, byte: usize) -> &'a TextStyle {
    p.spans
        .iter()
        .rev()
        .find(|s| s.byte_range.start <= byte && byte < s.byte_range.end)
        .map_or(p.base, |s| &s.style)
}

fn column_count(t: &TableInput) -> usize {
    t.rows.iter().map(Vec::len).max().unwrap_or(0)
}

/// Width available to a cell's text.
fn cell_text_width(content: f32, cols: usize) -> f32 {
    (content / cols.max(1) as f32 - 2.0 * CELL_PADDING).max(1.0)
}

fn cache_key(
    p: &ParagraphInput,
    width: f32,
    objects: &[ObjectInput],
    exclusions: &[Exclusion],
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.text.hash(&mut h);
    hash_style(p.base, &mut h);
    for s in p.spans {
        s.byte_range.hash(&mut h);
        hash_style(&s.style, &mut h);
    }
    // Space before/after is applied while stacking and doesn't change the paragraph's shape.
    p.style.align.hash(&mut h);
    p.style.line_spacing.to_bits().hash(&mut h);
    for v in [
        p.style.indent_start,
        p.style.indent_end,
        p.style.indent_first,
    ] {
        v.to_bits().hash(&mut h);
    }
    if let Some(m) = &p.marker {
        m.text.hash(&mut h);
        hash_style(m.style, &mut h);
    }
    width.to_bits().hash(&mut h);
    // Only what changes the lines: in-line objects' boxes and where the others sit in the text.
    for o in objects {
        o.byte.hash(&mut h);
        o.mode.hash(&mut h);
        if o.mode.in_line() {
            let (w, ht) = o.footprint();
            w.to_bits().hash(&mut h);
            ht.to_bits().hash(&mut h);
        }
    }
    for e in exclusions {
        for v in [e.top, e.bottom, e.left, e.right] {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

fn hash_style(s: &TextStyle, h: &mut impl Hasher) {
    s.font_family.hash(h);
    s.font_size.to_bits().hash(h);
    s.font_weight.to_bits().hash(h);
    s.italic.hash(h);
    s.color.hash(h);
    s.highlight.hash(h);
    s.underline.hash(h);
    s.strikethrough.hash(h);
    s.baseline_shift.to_bits().hash(h);
}
