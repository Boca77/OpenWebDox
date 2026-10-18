use std::ops::Range;

use parley::{
    Alignment, AlignmentOptions, FontStyle, FontWeight, LayoutContext, LineHeight, OverflowWrap,
    PositionedLayoutItem, StyleProperty,
};

use crate::fonts::{FontId, FontLibrary};

/// How a stretch of text looks.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// A CSS-style family list, e.g. `"Noto Sans, Noto Sans Arabic"`. Later families are used
    /// for characters the earlier ones lack.
    pub font_family: String,
    /// Font size in points.
    pub font_size: f32,
    /// CSS weight: 400 regular, 700 bold.
    pub font_weight: f32,
    /// Uses the family's italic face. Needs one registered: a family without an italic face
    /// is drawn upright (glyphs are never slanted synthetically).
    pub italic: bool,
}

/// Horizontal placement of a paragraph's lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    /// Lines stretched to the full width, except the paragraph's last line.
    Justify,
}

/// Paragraph-level formatting. Lengths in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphStyle {
    pub align: Align,
    /// Multiple of the fonts' natural line height (1.0 single, 2.0 double), as in Google Docs.
    pub line_spacing: f32,
    pub space_before: f32,
    pub space_after: f32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            align: Align::Left,
            line_spacing: 1.0,
            space_before: 0.0,
            space_after: 0.0,
        }
    }
}

/// A style applied to part of a paragraph, overriding the paragraph's base style.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleSpan {
    pub byte_range: Range<usize>,
    pub style: TextStyle,
}

/// One paragraph broken into lines. Coordinates are in points, relative to the paragraph's
/// top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLayout {
    pub lines: Vec<Line>,
    /// Width of the widest line.
    pub width: f32,
    /// Total height: the bottom of the last line.
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub byte_range: Range<usize>,
    pub top: f32,
    pub bottom: f32,
    /// Y of the baseline glyphs sit on.
    pub baseline: f32,
    /// Width of the line's content, including trailing whitespace that hangs past the edge.
    pub advance: f32,
    /// Glyph runs in visual (left-to-right) order.
    pub runs: Vec<GlyphRun>,
    /// Every place a caret can sit on this line, left to right. Never empty.
    pub carets: Vec<CaretStop>,
}

/// A caret position on a line: a grapheme boundary and its x coordinate.
///
/// In mixed-direction text one offset can appear twice (once at each visual edge where the
/// directions meet); lookups by offset use the first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretStop {
    pub byte: usize,
    pub x: f32,
}

impl Line {
    /// X of the caret at `byte`, or of the nearest stop before it.
    pub fn caret_x(&self, byte: usize) -> f32 {
        if let Some(stop) = self.carets.iter().find(|s| s.byte == byte) {
            return stop.x;
        }
        self.carets
            .iter()
            .filter(|s| s.byte <= byte)
            .max_by_key(|s| s.byte)
            .or(self.carets.first())
            .map_or(0.0, |s| s.x)
    }

    /// The caret offset closest to `x`.
    pub fn offset_at(&self, x: f32) -> usize {
        self.carets
            .iter()
            .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map_or(self.byte_range.start, |s| s.byte)
    }
}

/// Glyphs that share one font and size.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: FontId,
    /// Face index within the font file (non-zero only for font collections).
    pub face_index: u32,
    pub font_size: f32,
    pub byte_range: Range<usize>,
    pub rtl: bool,
    pub glyphs: Vec<Glyph>,
}

/// A glyph placed at its baseline origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// Glyph id within the font, not a character.
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// Shapes and line-breaks paragraphs. Keep one per document; it reuses allocations between calls.
pub struct LayoutEngine {
    fonts: FontLibrary,
    cx: LayoutContext<()>,
}

impl LayoutEngine {
    pub fn new(fonts: FontLibrary) -> Self {
        Self {
            fonts,
            cx: LayoutContext::new(),
        }
    }

    pub fn fonts(&self) -> &FontLibrary {
        &self.fonts
    }

    pub fn fonts_mut(&mut self) -> &mut FontLibrary {
        &mut self.fonts
    }

    /// Lays out `text` (one paragraph, no `\n`) to fit `width` points. Spans may overlap; later
    /// spans win. Words wider than the line are broken rather than allowed to overflow.
    pub fn layout_paragraph(
        &mut self,
        text: &str,
        base: &TextStyle,
        spans: &[StyleSpan],
        width: f32,
    ) -> ParagraphLayout {
        self.layout_paragraph_styled(text, base, spans, &ParagraphStyle::default(), width)
    }

    /// [`LayoutEngine::layout_paragraph`] with alignment and line spacing. Space before and
    /// after the paragraph is left to pagination.
    pub fn layout_paragraph_styled(
        &mut self,
        text: &str,
        base: &TextStyle,
        spans: &[StyleSpan],
        para: &ParagraphStyle,
        width: f32,
    ) -> ParagraphLayout {
        // Quantization off: snapping metrics to whole units would round 16.34pt lines to 17pt
        // and drift page breaks.
        let mut b = self.cx.ranged_builder(&mut self.fonts.cx, text, 1.0, false);
        b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        b.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(
            para.line_spacing,
        )));
        push_default_style(&mut b, base);
        for span in spans {
            push_style(&mut b, &span.style, span.byte_range.clone());
        }
        let mut layout = b.build(text);
        layout.break_all_lines(Some(width));
        let alignment = match para.align {
            // Start, not Left: right-to-left paragraphs start on the right.
            Align::Left => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::End,
            Align::Justify => Alignment::Justify,
        };
        // Aligns within the break width (`width`), not the widest line.
        layout.align(alignment, AlignmentOptions::default());

        let lines: Vec<Line> = layout
            .lines()
            .map(|line| {
                let m = line.metrics();
                let runs = line
                    .items()
                    .filter_map(|item| match item {
                        PositionedLayoutItem::GlyphRun(g) => Some(g),
                        PositionedLayoutItem::InlineBox(_) => None,
                    })
                    .filter_map(|g| {
                        let run = g.run();
                        let font = run.font();
                        // Every font parley can pick came from our library, so this lookup only
                        // fails if that invariant breaks; skip the run rather than crash.
                        let id = self.fonts.id_of(&font.data)?;
                        Some(GlyphRun {
                            font: id,
                            face_index: font.index,
                            font_size: run.font_size(),
                            byte_range: run.text_range(),
                            rtl: run.is_rtl(),
                            glyphs: g
                                .positioned_glyphs()
                                .map(|gl| Glyph {
                                    id: gl.id,
                                    x: gl.x,
                                    y: gl.y,
                                })
                                .collect(),
                        })
                    })
                    .collect();
                Line {
                    byte_range: line.text_range(),
                    top: m.block_min_coord,
                    bottom: m.block_max_coord,
                    baseline: m.baseline,
                    advance: m.advance,
                    runs,
                    carets: caret_stops(&line),
                }
            })
            .collect();

        ParagraphLayout {
            width: layout.width(),
            height: lines.last().map_or(0.0, |l| l.bottom),
            lines,
        }
    }
}

fn push_default_style(b: &mut parley::RangedBuilder<'_, ()>, s: &TextStyle) {
    b.push_default(StyleProperty::FontFamily(s.font_family.as_str().into()));
    b.push_default(StyleProperty::FontSize(s.font_size));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(s.font_weight)));
    b.push_default(StyleProperty::FontStyle(font_style(s)));
}

fn push_style(b: &mut parley::RangedBuilder<'_, ()>, s: &TextStyle, r: Range<usize>) {
    b.push(
        StyleProperty::FontFamily(s.font_family.as_str().into()),
        r.clone(),
    );
    b.push(StyleProperty::FontSize(s.font_size), r.clone());
    b.push(
        StyleProperty::FontWeight(FontWeight::new(s.font_weight)),
        r.clone(),
    );
    b.push(StyleProperty::FontStyle(font_style(s)), r);
}

fn font_style(s: &TextStyle) -> FontStyle {
    if s.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }
}

/// Walks the line's clusters in visual order, recording the offset at each cluster edge.
fn caret_stops(line: &parley::Line<'_, ()>) -> Vec<CaretStop> {
    let mut stops: Vec<CaretStop> = Vec::new();
    let mut x = line.metrics().offset;
    let mut push = |byte: usize, x: f32| {
        if stops.last().is_none_or(|s| s.byte != byte || s.x != x) {
            stops.push(CaretStop { byte, x });
        }
    };
    for run in line.runs() {
        for cluster in run.visual_clusters() {
            let r = cluster.text_range();
            let right = x + cluster.advance();
            // A right-to-left cluster starts (logically) at its right edge.
            let (left_byte, right_byte) = if cluster.is_rtl() {
                (r.end, r.start)
            } else {
                (r.start, r.end)
            };
            push(left_byte, x);
            push(right_byte, right);
            x = right;
        }
    }
    if stops.is_empty() {
        stops.push(CaretStop {
            byte: line.text_range().start,
            x: line.metrics().offset,
        });
    }
    stops
}
