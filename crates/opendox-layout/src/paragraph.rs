use std::ops::Range;

use parley::{
    Alignment, AlignmentOptions, FontWeight, LayoutContext, OverflowWrap, PositionedLayoutItem,
    StyleProperty,
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
        // Quantization off: snapping metrics to whole units would round 16.34pt lines to 17pt
        // and drift page breaks.
        let mut b = self.cx.ranged_builder(&mut self.fonts.cx, text, 1.0, false);
        b.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        b.push_default(StyleProperty::FontFamily(base.font_family.as_str().into()));
        b.push_default(StyleProperty::FontSize(base.font_size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(base.font_weight)));
        for span in spans {
            let r = span.byte_range.clone();
            let s = &span.style;
            b.push(
                StyleProperty::FontFamily(s.font_family.as_str().into()),
                r.clone(),
            );
            b.push(StyleProperty::FontSize(s.font_size), r.clone());
            b.push(StyleProperty::FontWeight(FontWeight::new(s.font_weight)), r);
        }
        let mut layout = b.build(text);
        layout.break_all_lines(Some(width));
        layout.align(Alignment::Start, AlignmentOptions::default());

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
