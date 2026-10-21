use std::ops::Range;

use parley::{
    Alignment, AlignmentOptions, FontStyle, FontWeight, InlineBox, InlineBoxKind, LayoutContext,
    LineHeight, OverflowWrap, PositionedLayoutItem, StyleProperty, VerticalAlign, YieldData,
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

/// The character standing in for an object (an image) in paragraph text.
pub const OBJECT_PLACEHOLDER: char = '\u{FFFC}';

/// How an object sits among the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ObjectMode {
    /// In the line like a large character; the line grows to fit it.
    #[default]
    Inline,
    /// Positioned on the page; lines beside it get shorter so text flows around it.
    Wrap,
    /// Positioned on lines of its own, before the line holding its placeholder.
    Break,
    /// Positioned on the page under the text, which ignores it.
    Behind,
    /// Positioned on the page over the text, which ignores it.
    Front,
}

/// An object (an image) anchored in a paragraph. Lengths in points.
///
/// Its placeholder character stays in the text and keeps its byte offsets, but takes no width
/// and draws nothing; an in-line object takes its place in the line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectInput {
    /// UTF-8 byte offset of the object's [`OBJECT_PLACEHOLDER`] in the paragraph text.
    pub byte: usize,
    /// Any number the caller wants back in [`crate::PlacedObject::key`].
    pub key: usize,
    /// Size before rotation.
    pub width: f32,
    pub height: f32,
    /// Degrees clockwise. Layout makes room for the rotated bounding box.
    pub rotation: f32,
    pub mode: ObjectMode,
    /// For objects positioned on the page: where the top-left corner of the bounding box goes,
    /// from the left margin and from the top of the paragraph. `Break` uses only `x`.
    pub x: f32,
    pub y: f32,
}

impl ObjectInput {
    /// Width and height of the rotated bounding box: the space the object takes.
    pub fn footprint(&self) -> (f32, f32) {
        let r = self.rotation.to_radians();
        let (s, c) = (r.sin().abs(), r.cos().abs());
        (
            self.width * c + self.height * s,
            self.width * s + self.height * c,
        )
    }
}

/// A rectangle a paragraph's lines must keep clear of (a wrapped image plus its margin), in
/// the paragraph's coordinates: y from the paragraph's top, x from its left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exclusion {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

/// The narrowest piece of line text is put on beside a wrapped object. Narrower than this,
/// the line moves below the object instead.
pub const MIN_WRAP_WIDTH: f32 = 36.0;

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
    /// Lines are stacked without gaps, except where a line moved below an exclusion.
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
    /// In-line objects on this line, left to right.
    pub objects: Vec<LineObject>,
}

/// An in-line object on a line: its bounding box, in paragraph coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineObject {
    /// Index into the paragraph's objects.
    pub object: usize,
    pub x: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
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
    /// For each glyph, the paragraph text it draws (byte range), for copy and search in
    /// exported PDFs. A ligature's glyph covers all the characters it joins; the extra glyphs
    /// of a character drawn with several share its range.
    pub glyph_text: Vec<Range<usize>>,
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
        self.layout_paragraph_with(text, base, spans, para, width, &[], &[])
    }

    /// [`LayoutEngine::layout_paragraph_styled`] with objects (images) in the text, and areas
    /// the lines must keep clear of. A line beside exclusions uses the widest gap they leave;
    /// if no gap is at least [`MIN_WRAP_WIDTH`] wide, the line moves below them.
    #[allow(clippy::too_many_arguments)]
    pub fn layout_paragraph_with(
        &mut self,
        text: &str,
        base: &TextStyle,
        spans: &[StyleSpan],
        para: &ParagraphStyle,
        width: f32,
        objects: &[ObjectInput],
        exclusions: &[Exclusion],
    ) -> ParagraphLayout {
        // Objects whose byte really holds a placeholder, with their index in `objects`.
        let objects: Vec<(usize, &ObjectInput)> = objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                text.get(o.byte..)
                    .is_some_and(|t| t.starts_with(OBJECT_PLACEHOLDER))
            })
            .collect();
        // Placeholders keep their bytes but become zero-width spaces: the same length in
        // UTF-8 (3 bytes), no ink, and a break opportunity after an object.
        let swapped: String;
        let text = if objects.is_empty() {
            text
        } else {
            let mut s = String::with_capacity(text.len());
            for (i, c) in text.char_indices() {
                let object = c == OBJECT_PLACEHOLDER && objects.iter().any(|(_, o)| o.byte == i);
                s.push(if object { '\u{200B}' } else { c });
            }
            swapped = s;
            swapped.as_str()
        };
        let placeholders: Vec<usize> = objects.iter().map(|(_, o)| o.byte).collect();

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
        for (i, o) in &objects {
            if o.mode != ObjectMode::Inline {
                continue;
            }
            let (w, h) = inline_size(o, width);
            b.push_inline_box(InlineBox {
                id: *i as u64,
                kind: InlineBoxKind::InFlow,
                index: o.byte,
                width: w,
                height: h,
                baseline: None,
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        let mut layout = b.build(text);
        if exclusions.is_empty() {
            layout.break_all_lines(Some(width));
        } else {
            let guess = base.font_size * 1.4 * para.line_spacing;
            break_around(&mut layout, width, guess, exclusions);
        }
        let alignment = match para.align {
            // Start, not Left: right-to-left paragraphs start on the right.
            Align::Left => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::End,
            Align::Justify => Alignment::Justify,
        };
        // Aligns each line within its own break width (`width`, or what exclusions left).
        layout.align(alignment, AlignmentOptions::default());

        let lines: Vec<Line> = layout
            .lines()
            .map(|line| {
                let m = line.metrics();
                let mut boxes: Vec<LineObject> = Vec::new();
                let runs = line
                    .items()
                    .filter_map(|item| match item {
                        PositionedLayoutItem::GlyphRun(g) => Some(g),
                        PositionedLayoutItem::InlineBox(b) => {
                            boxes.push(LineObject {
                                object: b.id as usize,
                                x: b.x,
                                top: b.y,
                                width: b.width,
                                height: b.height,
                            });
                            None
                        }
                    })
                    .filter_map(|g| {
                        let run = g.run();
                        let font = run.font();
                        // Every font parley can pick came from our library, so this lookup only
                        // fails if that invariant breaks; skip the run rather than crash.
                        let id = self.fonts.id_of(&font.data)?;
                        let all: Vec<Glyph> = g
                            .positioned_glyphs()
                            .map(|gl| Glyph {
                                id: gl.id,
                                x: gl.x,
                                y: gl.y,
                            })
                            .collect();
                        let texts = glyph_text(run, all.len());
                        // Placeholders draw nothing, whatever glyph the font gave them.
                        let (glyphs, glyph_text): (Vec<Glyph>, Vec<Range<usize>>) = all
                            .into_iter()
                            .zip(texts)
                            .filter(|(_, t)| !placeholders.contains(&t.start))
                            .unzip();
                        (!glyphs.is_empty()).then(|| GlyphRun {
                            font: id,
                            face_index: font.index,
                            font_size: run.font_size(),
                            byte_range: run.text_range(),
                            rtl: run.is_rtl(),
                            glyphs,
                            glyph_text,
                        })
                    })
                    .collect();
                let box_stops: Vec<(usize, f32, f32)> = boxes
                    .iter()
                    .filter_map(|b| Some((objects.get(b.object)?.1.byte, b.x, b.width)))
                    .collect();
                // Parley numbered boxes in push order; callers know objects by their index.
                for b in &mut boxes {
                    b.object = objects.get(b.object).map_or(usize::MAX, |(i, _)| *i);
                }
                Line {
                    byte_range: line.text_range(),
                    top: m.block_min_coord,
                    bottom: m.block_max_coord,
                    baseline: m.baseline,
                    advance: m.advance,
                    runs,
                    carets: caret_stops(&line, &box_stops, &placeholders),
                    objects: boxes,
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

/// The box an in-line object takes on its line: its bounding box, scaled down to the line
/// width if wider, since an object that can't fit would overflow the page.
pub fn inline_size(o: &ObjectInput, line_width: f32) -> (f32, f32) {
    let (w, h) = o.footprint();
    let k = if w > line_width && w > 0.0 {
        line_width / w
    } else {
        1.0
    };
    (w * k, h * k)
}

/// Breaks lines one at a time, giving each line the horizontal gap the exclusions leave at its
/// height. A line's height is only known once it's broken, so each line is first broken with
/// the previous line's height as a guess and broken again if its real extent meets different
/// exclusions.
fn break_around(
    layout: &mut parley::Layout<()>,
    width: f32,
    first_guess: f32,
    exclusions: &[Exclusion],
) {
    fn place(lines: &mut parley::BreakLines<'_, ()>, (x, w): (f32, f32)) {
        let st = lines.state_mut();
        st.set_line_x(x);
        st.set_line_max_advance(w);
    }
    let mut lines = layout.break_lines();
    lines.state_mut().set_layout_max_advance(width);
    let mut guess = first_guess;
    // Bounded: every pass commits a line or moves the next line's top down past an exclusion.
    for _ in 0..10_000 {
        let y = lines.committed_y() as f32;
        let span = match free_span(y, y + guess, width, exclusions) {
            Span::Below(bottom) => {
                lines.state_mut().set_line_y(f64::from(bottom));
                continue;
            }
            Span::At(x, w) => (x, w),
        };
        place(&mut lines, span);
        let Some(data) = lines.break_next() else {
            break;
        };
        let YieldData::LineBreak(d) = data else {
            continue;
        };
        let (top, bottom) = (d.line_y_start as f32, d.line_y_end as f32);
        guess = (bottom - top).max(1.0);
        match free_span(top, bottom, width, exclusions) {
            Span::At(x, w) if (x, w) == span => {}
            Span::At(x, w) => {
                // Broken once more with the gap at its real height, and kept either way.
                if lines.revert() {
                    place(&mut lines, (x, w));
                    if lines.break_next().is_none() {
                        break;
                    }
                }
            }
            Span::Below(b) => {
                if lines.revert() {
                    lines.state_mut().set_line_y(f64::from(b));
                }
            }
        }
        if lines.is_done() {
            break;
        }
    }
    lines.finish();
}

enum Span {
    /// The line goes at this x with this much width.
    At(f32, f32),
    /// No gap is wide enough: the line moves down to this y.
    Below(f32),
}

/// The widest stretch of `0..width` that no exclusion covers between `top` and `bottom`.
fn free_span(top: f32, bottom: f32, width: f32, exclusions: &[Exclusion]) -> Span {
    let hits: Vec<&Exclusion> = exclusions
        .iter()
        .filter(|e| top < e.bottom && bottom > e.top && e.left < width && e.right > 0.0)
        .collect();
    if hits.is_empty() {
        return Span::At(0.0, width);
    }
    let mut blocked: Vec<(f32, f32)> = hits
        .iter()
        .map(|e| (e.left.max(0.0), e.right.min(width)))
        .collect();
    blocked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best = (0.0f32, 0.0f32);
    let mut x = 0.0f32;
    for (l, r) in blocked {
        if l - x > best.1 {
            best = (x, l - x);
        }
        x = x.max(r);
    }
    if width - x > best.1 {
        best = (x, width - x);
    }
    if best.1 >= MIN_WRAP_WIDTH {
        return Span::At(best.0, best.1);
    }
    let lowest = hits.iter().map(|e| e.bottom).fold(f32::INFINITY, f32::min);
    // Never stay put: that would retry the same line forever.
    Span::Below(lowest.max(top + 0.01))
}

/// The text each glyph of `run` draws, in the visual order parley positions glyphs in.
fn glyph_text(run: &parley::Run<'_, ()>, glyphs: usize) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::with_capacity(glyphs);
    // A cluster with no glyphs is part of a ligature drawn by its logically preceding
    // cluster: the previous one in visual order, or the next one in right-to-left runs.
    let mut pending: Option<Range<usize>> = None;
    let join = |a: Range<usize>, b: &Range<usize>| a.start.min(b.start)..a.end.max(b.end);
    for cluster in run.visual_clusters() {
        let mut r = cluster.text_range();
        let n = cluster.glyphs().count();
        if n == 0 {
            match out.last_mut() {
                Some(last) if !run.is_rtl() => *last = join(last.clone(), &r),
                _ => pending = Some(pending.map_or(r.clone(), |p| join(p, &r))),
            }
            continue;
        }
        if let Some(p) = pending.take() {
            r = join(r, &p);
        }
        out.extend(std::iter::repeat_n(r, n));
    }
    if out.len() != glyphs {
        // Never seen; parley documents both walks as producing the same glyphs. If they ever
        // disagree, attribute the whole run to every glyph rather than mislabel them.
        return vec![run.text_range(); glyphs];
    }
    out
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
///
/// `boxes` are the line's in-line objects as `(placeholder byte, x, width)`: the caret before
/// one sits at its left edge and the caret after it at its right edge. `placeholders` holds the
/// byte of every object placeholder; they take no width of their own.
fn caret_stops(
    line: &parley::Line<'_, ()>,
    boxes: &[(usize, f32, f32)],
    placeholders: &[usize],
) -> Vec<CaretStop> {
    let start_x = line.metrics().offset + line.metrics().inline_min_coord;
    fn push(stops: &mut Vec<CaretStop>, byte: usize, x: f32) {
        if stops.last().is_none_or(|s| s.byte != byte || s.x != x) {
            stops.push(CaretStop { byte, x });
        }
    }
    let mut stops: Vec<CaretStop> = Vec::new();
    let after = |byte: usize| byte + OBJECT_PLACEHOLDER.len_utf8();
    let mut pending: Vec<&(usize, f32, f32)> = boxes.iter().collect();
    let mut x = start_x;
    for run in line.runs() {
        for cluster in run.visual_clusters() {
            // Objects sit between clusters: step over any box that starts here.
            while let Some(k) = pending.iter().position(|b| b.1 <= x + 0.01) {
                let (byte, bx, w) = *pending.remove(k);
                push(&mut stops, byte, bx);
                x = bx + w;
                push(&mut stops, after(byte), x);
            }
            let r = cluster.text_range();
            let right = x + cluster.advance();
            // A right-to-left cluster starts (logically) at its right edge.
            let (left_byte, right_byte) = if cluster.is_rtl() {
                (r.end, r.start)
            } else {
                (r.start, r.end)
            };
            // An in-line object's placeholder already has both its stops from the box.
            if !boxes.iter().any(|b| b.0 == r.start) {
                // A placeholder takes no width, so its left edge would tie with its right
                // one; keeping only the right makes clicks there land after it.
                if !placeholders.contains(&r.start) {
                    push(&mut stops, left_byte, x);
                }
                push(&mut stops, right_byte, right);
            }
            x = right;
        }
    }
    for (byte, bx, w) in pending {
        push(&mut stops, *byte, *bx);
        push(&mut stops, after(*byte), bx + w);
    }
    if stops.is_empty() {
        stops.push(CaretStop {
            byte: line.text_range().start,
            x: start_x,
        });
    }
    stops
}
