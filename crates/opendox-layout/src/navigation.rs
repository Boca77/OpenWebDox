//! Turning page coordinates into text positions and back: clicks, carets, arrow keys and
//! selection highlights.

use std::cmp::Ordering;

use crate::pagination::{Paginator, CELL_PADDING};
use crate::paragraph::Line;

/// A rectangle in points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn dx(&self, x: f32) -> f32 {
        (self.x - x).max(x - (self.x + self.width)).max(0.0)
    }

    fn dy(&self, y: f32) -> f32 {
        (self.y - y).max(y - (self.y + self.height)).max(0.0)
    }
}

/// A line as placed on a page. Coordinates are points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedLine {
    pub page: usize,
    /// Paragraph index (body and cell paragraphs together, in reading order).
    pub paragraph: usize,
    /// Index of the line within its paragraph.
    pub line: usize,
    pub top: f32,
    pub bottom: f32,
    /// X where the paragraph's text starts.
    pub left: f32,
    /// Width available to the text.
    pub width: f32,
    /// Area a click selects this line from: the line itself for body text, the whole cell for
    /// text in a table.
    pub region: Rect,
}

impl PlacedLine {
    fn rect(&self) -> Rect {
        Rect {
            x: self.left,
            y: self.top,
            width: self.width,
            height: self.bottom - self.top,
        }
    }
}

/// A caret position in the document.
///
/// Where a paragraph wraps, the end of one line and the start of the next are the same offset;
/// `upstream` picks the end of the earlier line (where the caret lands after clicking past
/// the end of a wrapped line, or pressing End).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    pub paragraph: usize,
    /// UTF-8 byte offset into the paragraph text.
    pub byte: usize,
    pub upstream: bool,
}

/// Where to draw a caret, in points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretRect {
    pub page: usize,
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

/// One highlighted box of a selection, in points from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRect {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Width shown for a selected paragraph break, so selecting across paragraphs is visible.
const PARAGRAPH_BREAK_WIDTH: f32 = 4.0;

const EPSILON: f32 = 0.01;

impl Paginator {
    /// Every line in document order with its page position.
    pub fn placed_lines(&self) -> &[PlacedLine] {
        &self.lines
    }

    /// The text position nearest to point `(x, y)` on `page`, or `None` if the page is empty or
    /// doesn't exist. Points in the margins snap to the nearest line and line end; points
    /// anywhere in a table cell go to that cell.
    pub fn hit_test(&self, page: usize, x: f32, y: f32) -> Option<TextPosition> {
        let on_page = || self.lines.iter().filter(move |l| l.page == page);
        let in_region = on_page().filter(|l| l.region.dx(x) == 0.0 && l.region.dy(y) == 0.0);
        let placed = in_region
            .min_by(|a, b| a.rect().dy(y).total_cmp(&b.rect().dy(y)))
            .or_else(|| {
                on_page().min_by(|a, b| {
                    let (ra, rb) = (a.rect(), b.rect());
                    ra.dy(y)
                        .total_cmp(&rb.dy(y))
                        .then(a.region.dx(x).total_cmp(&b.region.dx(x)))
                })
            })?;
        let line = self.line(placed)?;
        Some(self.position_on(placed, line, x))
    }

    /// Where to draw the caret for `pos`.
    pub fn caret_rect(&self, pos: TextPosition) -> Option<CaretRect> {
        let placed = &self.lines[self.line_index(pos)?];
        let line = self.line(placed)?;
        Some(CaretRect {
            page: placed.page,
            x: placed.left + line.caret_x(pos.byte),
            top: placed.top,
            bottom: placed.bottom,
        })
    }

    /// The position one line up or down from `pos`, as close as possible to `goal_x` (points
    /// from the page's left edge). `None` at the first or last line of the document.
    ///
    /// Lines in the same column are preferred, so moving through a table stays in a column and
    /// moving out of one lands under the caret.
    pub fn move_vertical(
        &self,
        pos: TextPosition,
        goal_x: f32,
        down: bool,
    ) -> Option<TextPosition> {
        let cur = &self.lines[self.line_index(pos)?];
        let beyond = |l: &&PlacedLine| {
            if down {
                l.page > cur.page || (l.page == cur.page && l.top >= cur.bottom - EPSILON)
            } else {
                l.page < cur.page || (l.page == cur.page && l.bottom <= cur.top + EPSILON)
            }
        };
        // Nearest first: the next line down has the smallest (page, top); the next line up has
        // the largest (page, bottom).
        let nearer = |a: &&PlacedLine, b: &&PlacedLine| -> Ordering {
            if down {
                (a.page, a.top)
                    .partial_cmp(&(b.page, b.top))
                    .unwrap_or(Ordering::Equal)
            } else {
                (b.page, b.bottom)
                    .partial_cmp(&(a.page, a.bottom))
                    .unwrap_or(Ordering::Equal)
            }
        };
        let in_column = |l: &&PlacedLine| l.region.dx(goal_x) == 0.0;
        let target = self
            .lines
            .iter()
            .filter(beyond)
            .filter(in_column)
            .min_by(nearer)
            .or_else(|| {
                self.lines.iter().filter(beyond).min_by(|a, b| {
                    nearer(a, b).then(a.region.dx(goal_x).total_cmp(&b.region.dx(goal_x)))
                })
            })?;
        let line = self.line(target)?;
        Some(self.position_on(target, line, goal_x))
    }

    /// Start of the visual line containing `pos` (Home).
    pub fn line_start(&self, pos: TextPosition) -> Option<TextPosition> {
        let placed = &self.lines[self.line_index(pos)?];
        let line = self.line(placed)?;
        Some(TextPosition {
            paragraph: placed.paragraph,
            byte: line.byte_range.start,
            upstream: false,
        })
    }

    /// End of the visual line containing `pos` (End).
    pub fn line_end(&self, pos: TextPosition) -> Option<TextPosition> {
        let placed = &self.lines[self.line_index(pos)?];
        let line = self.line(placed)?;
        Some(TextPosition {
            paragraph: placed.paragraph,
            byte: line.byte_range.end,
            upstream: !self.is_last_line(placed),
        })
    }

    /// Highlight boxes covering the text between `a` and `b` (in either order).
    pub fn selection_rects(&self, a: TextPosition, b: TextPosition) -> Vec<SelectionRect> {
        let (start, end) = if (a.paragraph, a.byte) <= (b.paragraph, b.byte) {
            (a, b)
        } else {
            (b, a)
        };
        let mut out = Vec::new();
        if (start.paragraph, start.byte) == (end.paragraph, end.byte) {
            return out;
        }
        // Selection boundaries ignore affinity: a range that ends at a wrap point covers the
        // earlier line, one that starts there begins on the later line.
        let first = self.line_index(TextPosition {
            upstream: false,
            ..start
        });
        let last = self.line_index(TextPosition {
            upstream: true,
            ..end
        });
        let (Some(first), Some(last)) = (first, last) else {
            return out;
        };
        for placed in &self.lines[first..=last] {
            let Some(line) = self.line(placed) else {
                continue;
            };
            let from = if placed.paragraph == start.paragraph {
                start.byte.max(line.byte_range.start)
            } else {
                line.byte_range.start
            };
            let to = if placed.paragraph == end.paragraph {
                end.byte.min(line.byte_range.end)
            } else {
                line.byte_range.end
            };
            let (x0, x1) = (line.caret_x(from), line.caret_x(to));
            let mut width = (x1 - x0).abs();
            // The selection continues past this paragraph's end: show the break as selected.
            if self.is_last_line(placed) && placed.paragraph < end.paragraph {
                width += PARAGRAPH_BREAK_WIDTH.min(CELL_PADDING);
            }
            if width > 0.0 {
                out.push(SelectionRect {
                    page: placed.page,
                    x: placed.left + x0.min(x1),
                    y: placed.top,
                    width,
                    height: placed.bottom - placed.top,
                });
            }
        }
        out
    }

    fn line(&self, placed: &PlacedLine) -> Option<&Line> {
        self.paragraph(placed.paragraph)?.lines.get(placed.line)
    }

    fn is_last_line(&self, placed: &PlacedLine) -> bool {
        self.paragraph(placed.paragraph)
            .is_none_or(|p| placed.line + 1 >= p.lines.len())
    }

    /// Index into `lines` of the line holding `pos`, using `upstream` at wrap points.
    fn line_index(&self, pos: TextPosition) -> Option<usize> {
        let first = *self.first_line.get(pos.paragraph)?;
        let para = self.paragraph(pos.paragraph)?;
        let mut found = None;
        for (i, line) in para.lines.iter().enumerate() {
            let r = &line.byte_range;
            if r.start <= pos.byte && pos.byte <= r.end {
                found = Some(i);
                // At a wrap point both lines match; upstream keeps the earlier one.
                if pos.upstream || pos.byte < r.end {
                    break;
                }
            }
        }
        // Past the end (stale position): clamp to the paragraph's last line.
        let line = found.unwrap_or(para.lines.len().saturating_sub(1));
        Some(first + line)
    }

    /// The caret position on `line` nearest to page x coordinate `x`.
    fn position_on(&self, placed: &PlacedLine, line: &Line, x: f32) -> TextPosition {
        let byte = line.offset_at(x - placed.left);
        TextPosition {
            paragraph: placed.paragraph,
            byte,
            upstream: byte == line.byte_range.end && !self.is_last_line(placed),
        }
    }
}
