use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::paragraph::{LayoutEngine, ParagraphLayout, StyleSpan, TextStyle};

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
}

/// A page and the pieces of paragraphs placed on it, top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub fragments: Vec<Fragment>,
}

/// The lines of one paragraph that landed on one page.
///
/// To place a line on the page: `x = margin_left + glyph.x`,
/// `y = fragment.top + (line.top - first_line.top)` where `first_line` is
/// `paragraph.lines[fragment.lines.start]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// Index of the paragraph in the input.
    pub paragraph: usize,
    /// Which of the paragraph's lines are on this page.
    pub lines: Range<usize>,
    /// Y of the fragment's first line top, measured from the top edge of the page.
    pub top: f32,
}

/// Lays out a whole document onto pages, caching paragraph layouts between calls.
///
/// Paragraphs are cached by content (text, styles, width), not by position, so inserting or
/// deleting a paragraph doesn't invalidate the others. Restacking lines into pages is plain
/// arithmetic and is redone in full on every call.
pub struct Paginator {
    geometry: PageGeometry,
    cache: HashMap<u64, ParagraphLayout>,
    keys: Vec<u64>,
    pages: Vec<Page>,
    last_shaped: usize,
}

impl Paginator {
    pub fn new(geometry: PageGeometry) -> Self {
        Self {
            geometry,
            cache: HashMap::new(),
            keys: Vec::new(),
            pages: Vec::new(),
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

    /// Lays out `paragraphs` and splits them into pages.
    pub fn layout(&mut self, engine: &mut LayoutEngine, paragraphs: &[ParagraphInput]) -> &[Page] {
        let width = self.geometry.content_width();
        let mut fresh = HashMap::with_capacity(paragraphs.len());
        self.keys.clear();
        self.last_shaped = 0;

        for p in paragraphs {
            let key = cache_key(p, width);
            self.keys.push(key);
            if fresh.contains_key(&key) {
                continue;
            }
            let layout = match self.cache.remove(&key) {
                Some(l) => l,
                None => {
                    self.last_shaped += 1;
                    engine.layout_paragraph(p.text, p.base, p.spans, width)
                }
            };
            fresh.insert(key, layout);
        }
        // Dropping entries not used this pass keeps memory proportional to the document.
        self.cache = fresh;
        self.pages = self.stack();
        &self.pages
    }

    /// Pages from the last [`Paginator::layout`] call.
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

    fn stack(&self) -> Vec<Page> {
        let g = &self.geometry;
        let bottom_limit = g.margin_top + g.content_height();
        let mut pages = vec![Page {
            fragments: Vec::new(),
        }];
        let mut y = g.margin_top;

        for (index, key) in self.keys.iter().enumerate() {
            let Some(para) = self.cache.get(key) else {
                continue;
            };
            let mut start = 0;
            for (i, line) in para.lines.iter().enumerate() {
                let h = line.bottom - line.top;
                // Break only if something is already on the page: a line taller than a whole
                // page is placed on an empty page rather than pushed forward forever.
                if y + h > bottom_limit && y > g.margin_top {
                    push_fragment(&mut pages, index, start..i, y, para);
                    pages.push(Page {
                        fragments: Vec::new(),
                    });
                    y = g.margin_top;
                    start = i;
                }
                y += h;
            }
            push_fragment(&mut pages, index, start..para.lines.len(), y, para);
        }
        pages
    }
}

/// Adds the fragment ending at `bottom` to the last page; its top is derived from line heights.
fn push_fragment(
    pages: &mut [Page],
    paragraph: usize,
    lines: Range<usize>,
    bottom: f32,
    para: &ParagraphLayout,
) {
    if lines.is_empty() {
        return;
    }
    let height = para.lines[lines.end - 1].bottom - para.lines[lines.start].top;
    if let Some(page) = pages.last_mut() {
        page.fragments.push(Fragment {
            paragraph,
            lines,
            top: bottom - height,
        });
    }
}

fn cache_key(p: &ParagraphInput, width: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.text.hash(&mut h);
    hash_style(p.base, &mut h);
    for s in p.spans {
        s.byte_range.hash(&mut h);
        hash_style(&s.style, &mut h);
    }
    width.to_bits().hash(&mut h);
    h.finish()
}

fn hash_style(s: &TextStyle, h: &mut impl Hasher) {
    s.font_family.hash(h);
    s.font_size.to_bits().hash(h);
    s.font_weight.to_bits().hash(h);
}
