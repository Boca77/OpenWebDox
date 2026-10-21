use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::fonts::FontId;
use crate::navigation::{PlacedLine, Rect};
use crate::paragraph::{
    Exclusion, LayoutEngine, Line, ObjectInput, ObjectMode, ParagraphLayout, ParagraphStyle,
    StyleSpan, TextStyle,
};

/// Space between a cell's border and its text, in points.
pub const CELL_PADDING: f32 = 5.0;

/// Space kept between a wrapped object and the text beside it, in points (1/8 in, as in
/// Google Docs).
pub const WRAP_MARGIN: f32 = 9.0;

/// Space above and below an object that breaks text, in points.
pub const BREAK_MARGIN: f32 = 9.0;

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
}

/// A table as input to pagination: rows of cells, each cell a list of paragraphs. Columns
/// share the content width equally.
#[derive(Debug, Clone)]
pub struct TableInput<'a> {
    pub rows: Vec<Vec<Vec<ParagraphInput<'a>>>>,
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
            last_shaped: 0,
        }
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
                    self.objects.push(p.objects.to_vec());
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
                                let inline = p.objects.iter().map(|o| ObjectInput {
                                    mode: ObjectMode::Inline,
                                    ..*o
                                });
                                self.objects.push(inline.collect());
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
                        engine.layout_paragraph_with(
                            p.text,
                            p.base,
                            p.spans,
                            &p.style,
                            *width,
                            objects,
                            &exclusions[i],
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
        // Dropping entries not used this call keeps memory proportional to the document.
        let used: HashSet<u64> = self.keys.iter().copied().collect();
        self.cache.retain(|k, _| used.contains(k));
        self.pages = stacked.pages;
        self.lines = stacked.lines;
        self.first_line = stacked.first_line;
        &self.pages
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
            for line in &para.lines[f.lines.clone()] {
                for run in &line.runs {
                    out.extend(run.glyphs.iter().map(|g| PlacedGlyph {
                        font: run.font,
                        face_index: run.face_index,
                        font_size: run.font_size,
                        id: g.id,
                        x: f.left + g.x,
                        y: dy + g.y,
                    }));
                }
            }
        }
        out
    }

    fn stack(&self) -> Stacked {
        let g = self.geometry;
        let mut s = Stacker {
            out: Stacked {
                pages: vec![Page::default()],
                lines: Vec::new(),
                first_line: vec![0; self.keys.len()],
                wraps: Vec::new(),
            },
            y: g.margin_top,
            g,
        };
        for block in &self.plan {
            match block {
                Plan::Paragraph(i) => self.stack_paragraph(&mut s, *i),
                Plan::Table(rows) => self.stack_table(&mut s, rows),
            }
        }
        self.place_inline_objects(&mut s.out.pages);
        s.out
    }

    /// Adds every in-line object to its page, where its line was placed.
    fn place_inline_objects(&self, pages: &mut [Page]) {
        for page in pages {
            let mut found = Vec::new();
            for f in &page.fragments {
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
            if s.needs_break(pre + gap + h) {
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
            });
            s.y += h;
        }
        s.push_fragment(index, start..para.lines.len(), para, left);
        // Space after may run past the bottom margin; the next block then starts a new page.
        s.y += style.space_after;

        let Some((page, y0)) = top else {
            return;
        };
        for (k, o) in objects.iter().enumerate() {
            let layer = match o.mode {
                ObjectMode::Inline | ObjectMode::Break => continue,
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
        let limit = s.g.margin_top + s.g.content_height();

        for row in rows {
            let cells: Vec<Vec<CellLine>> = row.iter().map(|cell| self.cell_lines(cell)).collect();
            let mut next = vec![0usize; cells.len()];

            while cells.iter().zip(&next).any(|(c, &n)| n < c.len()) {
                let room = limit - s.y - 2.0 * CELL_PADDING;
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
                    (limit - top).max(content + 2.0 * CELL_PADDING)
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
}

impl Stacker {
    fn page(&self) -> usize {
        self.out.pages.len() - 1
    }

    /// Whether something `h` tall must go on a new page. Only if something is already on this
    /// page: anything taller than a page goes on an empty page rather than moving on forever.
    fn needs_break(&self, h: f32) -> bool {
        self.y + h > self.g.margin_top + self.g.content_height() && self.y > self.g.margin_top
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
    width.to_bits().hash(&mut h);
    // Only what changes the lines: in-line objects' boxes and where the others sit in the text.
    for o in objects {
        o.byte.hash(&mut h);
        o.mode.hash(&mut h);
        if o.mode == ObjectMode::Inline {
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
}
