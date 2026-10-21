use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::fonts::FontId;
use crate::navigation::{PlacedLine, Rect};
use crate::paragraph::{LayoutEngine, ParagraphLayout, ParagraphStyle, StyleSpan, TextStyle};

/// Space between a cell's border and its text, in points.
pub const CELL_PADDING: f32 = 5.0;

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

/// A page: the pieces of paragraphs placed on it, and the table borders to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub fragments: Vec<Fragment>,
    pub rules: Vec<Rule>,
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
        for block in blocks {
            match block {
                BlockInput::Paragraph(p) => {
                    inputs.push((*p, content));
                    self.plan.push(Plan::Paragraph(inputs.len() - 1));
                }
                BlockInput::Table(t) => {
                    let cols = column_count(t);
                    let width = cell_text_width(content, cols);
                    let rows = t
                        .rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .take(cols)
                                .map(|cell| {
                                    let start = inputs.len();
                                    inputs.extend(cell.iter().map(|p| (*p, width)));
                                    start..inputs.len()
                                })
                                .collect()
                        })
                        .collect();
                    self.plan.push(Plan::Table(rows));
                }
            }
        }

        let mut fresh = HashMap::with_capacity(inputs.len());
        self.keys.clear();
        self.styles.clear();
        self.last_shaped = 0;
        for (p, width) in &inputs {
            let key = cache_key(p, *width);
            self.keys.push(key);
            self.styles.push(p.style);
            if fresh.contains_key(&key) {
                continue;
            }
            let layout = match self.cache.remove(&key) {
                Some(l) => l,
                None => {
                    self.last_shaped += 1;
                    engine.layout_paragraph_styled(p.text, p.base, p.spans, &p.style, *width)
                }
            };
            fresh.insert(key, layout);
        }
        // Dropping entries not used this pass keeps memory proportional to the document.
        self.cache = fresh;
        let stacked = self.stack();
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
                pages: vec![Page {
                    fragments: Vec::new(),
                    rules: Vec::new(),
                }],
                lines: Vec::new(),
                first_line: vec![0; self.keys.len()],
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
        s.out
    }

    /// Places a body paragraph line by line, breaking pages between lines.
    fn stack_paragraph(&self, s: &mut Stacker, index: usize) {
        s.out.first_line[index] = s.out.lines.len();
        let Some(para) = self.paragraph(index) else {
            return;
        };
        let left = s.g.margin_left;
        let width = s.g.content_width();
        let style = self.style(index);
        let mut start = 0;
        for (i, line) in para.lines.iter().enumerate() {
            let h = line.bottom - line.top;
            // Space before is dropped at the top of a page, so pages start flush with the
            // margin, as in Google Docs.
            let before = if i == 0 && s.y > s.g.margin_top {
                style.space_before
            } else {
                0.0
            };
            if s.needs_break(before + h) {
                s.push_fragment(index, start..i, para, left);
                s.new_page();
                start = i;
            } else {
                s.y += before;
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
        self.out.pages.push(Page {
            fragments: Vec::new(),
            rules: Vec::new(),
        });
        self.y = self.g.margin_top;
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

fn cache_key(p: &ParagraphInput, width: f32) -> u64 {
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
    h.finish()
}

fn hash_style(s: &TextStyle, h: &mut impl Hasher) {
    s.font_family.hash(h);
    s.font_size.to_bits().hash(h);
    s.font_weight.to_bits().hash(h);
    s.italic.hash(h);
}
