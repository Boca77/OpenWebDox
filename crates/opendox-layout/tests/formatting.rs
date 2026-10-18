//! Rich-text and paragraph formatting: italic faces, alignment, justification, line spacing,
//! and space before/after paragraphs. Bundled fonts only.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_layout::{
    Align, BlockInput, FontId, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput,
    ParagraphLayout, ParagraphStyle, StyleSpan, TableInput, TextStyle,
};

const FILES: [&[u8]; 5] = [
    include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf"),
    include_bytes!("../../../fonts/noto/NotoSans-Bold.ttf"),
    include_bytes!("../../../fonts/noto/NotoSans-Italic.ttf"),
    include_bytes!("../../../fonts/noto/NotoSans-BoldItalic.ttf"),
    include_bytes!("../../../fonts/noto/NotoSerif-Regular.ttf"),
];
const REGULAR: FontId = FontId(0);
const ITALIC: FontId = FontId(2);
const BOLD_ITALIC: FontId = FontId(3);
const SERIF: FontId = FontId(4);
/// Noto Sans at 12pt: ascent + descent.
const LINE_H: f32 = 16.344;
const MARGIN: f32 = 72.0;
const CONTENT: f32 = 468.0;

const LONG: &str = "The rabbit-hole went straight on like a tunnel for some way, and then dipped \
                    suddenly down, so suddenly that Alice had not a moment to think about \
                    stopping herself before she found herself falling down a very deep well.";

fn engine() -> LayoutEngine {
    let mut fonts = FontLibrary::new();
    for f in FILES {
        fonts.register(f.to_vec());
    }
    LayoutEngine::new(fonts)
}

fn style() -> TextStyle {
    TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
    }
}

fn styled(text: &str, align: Align, line_spacing: f32, width: f32) -> ParagraphLayout {
    let para = ParagraphStyle {
        align,
        line_spacing,
        ..ParagraphStyle::default()
    };
    engine().layout_paragraph_styled(text, &style(), &[], &para, width)
}

fn fonts_used(l: &ParagraphLayout) -> Vec<FontId> {
    let mut v: Vec<FontId> = l
        .lines
        .iter()
        .flat_map(|line| line.runs.iter().map(|r| r.font))
        .collect();
    v.dedup();
    v
}

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() < tol
}

#[test]
fn italic_and_bold_italic_pick_their_own_faces() {
    let span = |range: std::ops::Range<usize>, weight: f32| StyleSpan {
        byte_range: range,
        style: TextStyle {
            italic: true,
            font_weight: weight,
            ..style()
        },
    };
    let text = "plain slanted heavy";
    let l = engine().layout_paragraph(
        text,
        &style(),
        &[span(6..13, 400.0), span(14..19, 700.0)],
        1000.0,
    );
    // The spaces between the spans stay regular.
    assert_eq!(fonts_used(&l), vec![REGULAR, ITALIC, REGULAR, BOLD_ITALIC]);
}

#[test]
fn font_family_spans_switch_typeface() {
    let serif = StyleSpan {
        byte_range: 0..5,
        style: TextStyle {
            font_family: "Noto Serif".into(),
            ..style()
        },
    };
    let l = engine().layout_paragraph("serif sans", &style(), &[serif], 1000.0);
    assert_eq!(fonts_used(&l), vec![SERIF, REGULAR]);
}

#[test]
fn center_and_right_alignment_move_glyphs_and_carets_together() {
    let left = styled("Centre me", Align::Left, 1.0, 400.0);
    let advance = left.lines[0].advance;
    for (align, offset) in [
        (Align::Center, (400.0 - advance) / 2.0),
        (Align::Right, 400.0 - advance),
    ] {
        let l = styled("Centre me", align, 1.0, 400.0);
        let line = &l.lines[0];
        assert!(
            approx(line.carets[0].x, offset, 0.01),
            "{align:?}: {}",
            line.carets[0].x
        );
        assert!(approx(line.runs[0].glyphs[0].x, offset, 0.01), "{align:?}");
        // The caret after the last character sits at the end of the text.
        assert!(approx(line.caret_x(9), offset + advance, 0.01), "{align:?}");
    }
}

#[test]
fn justified_lines_fill_the_width_except_the_last() {
    let l = styled(LONG, Align::Justify, 1.0, 300.0);
    assert!(l.lines.len() >= 3);
    let (last, body) = l.lines.split_last().unwrap();
    for (i, line) in body.iter().enumerate() {
        // The caret before the trailing space (end of the last word) reaches the right edge:
        // caret stops include the stretched spaces, so clicks land where glyphs are drawn.
        let text_end = LONG[line.byte_range.clone()].trim_end().len() + line.byte_range.start;
        assert!(
            approx(line.caret_x(text_end), 300.0, 0.05),
            "line {i}: {}",
            line.caret_x(text_end)
        );
        // Word starts: the caret before each word and that word's first glyph line up.
        let glyph_xs: Vec<f32> = line
            .runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .map(|g| g.x)
            .collect();
        let text = &LONG[line.byte_range.clone()];
        for (off, _) in text
            .match_indices(' ')
            .filter(|(o, _)| o + 1 < text.trim_end().len())
        {
            let x = line.caret_x(line.byte_range.start + off + 1);
            assert!(
                glyph_xs.iter().any(|g| approx(*g, x, 0.01)),
                "line {i}: no glyph at caret {x}"
            );
        }
    }
    let end = last.byte_range.end;
    assert!(last.caret_x(end) < 299.0, "last line stays ragged");
}

#[test]
fn line_spacing_multiplies_line_height() {
    let single = styled(LONG, Align::Left, 1.0, 300.0);
    let double = styled(LONG, Align::Left, 2.0, 300.0);
    let h1 = single.lines[0].bottom - single.lines[0].top;
    let h2 = double.lines[0].bottom - double.lines[0].top;
    assert!(approx(h1, LINE_H, 0.001));
    assert!(approx(h2, 2.0 * LINE_H, 0.001), "{h2}");
    assert_eq!(single.lines.len(), double.lines.len());
    assert!(approx(double.height, 2.0 * single.height, 0.01));
}

fn para<'a>(text: &'a str, base: &'a TextStyle, before: f32, after: f32) -> ParagraphInput<'a> {
    ParagraphInput {
        text,
        base,
        spans: &[],
        style: ParagraphStyle {
            space_before: before,
            space_after: after,
            ..ParagraphStyle::default()
        },
    }
}

#[test]
fn space_before_and_after_separate_paragraphs() {
    let s = style();
    let blocks = [
        BlockInput::Paragraph(para("first", &s, 20.0, 6.0)),
        BlockInput::Paragraph(para("second", &s, 10.0, 0.0)),
    ];
    let mut p = Paginator::new(PageGeometry::letter());
    let mut e = engine();
    p.layout_blocks(&mut e, &blocks);
    let frags = &p.pages()[0].fragments;
    // Space before is dropped at the top of the page.
    assert!(approx(frags[0].top, MARGIN, 0.001));
    assert!(
        approx(frags[1].top, MARGIN + LINE_H + 6.0 + 10.0, 0.001),
        "{}",
        frags[1].top
    );

    // Changing only spacing restacks without reshaping anything.
    let blocks = [
        BlockInput::Paragraph(para("first", &s, 0.0, 30.0)),
        BlockInput::Paragraph(para("second", &s, 0.0, 0.0)),
    ];
    p.layout_blocks(&mut e, &blocks);
    assert_eq!(p.last_shaped_count(), 0);
    assert!(approx(
        p.pages()[0].fragments[1].top,
        MARGIN + LINE_H + 30.0,
        0.001
    ));
}

#[test]
fn space_before_is_dropped_on_a_new_page() {
    let s = style();
    // Fill page 1 exactly to the last line that fits, then a spaced paragraph.
    let fit = (648.0 / LINE_H).floor() as usize;
    let lines: Vec<String> = (0..fit).map(|i| format!("line {i}")).collect();
    let mut blocks: Vec<BlockInput> = lines
        .iter()
        .map(|t| BlockInput::Paragraph(para(t, &s, 0.0, 0.0)))
        .collect();
    blocks.push(BlockInput::Paragraph(para("next page", &s, 24.0, 0.0)));
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout_blocks(&mut engine(), &blocks);
    assert_eq!(p.pages().len(), 2);
    assert!(approx(p.pages()[1].fragments[0].top, MARGIN, 0.001));
}

#[test]
fn paragraph_spacing_applies_inside_table_cells() {
    let s = style();
    let cell = vec![para("a", &s, 0.0, 8.0), para("b", &s, 4.0, 0.0)];
    let table = TableInput {
        rows: vec![vec![cell, vec![para("c", &s, 0.0, 0.0)]]],
    };
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout_blocks(&mut engine(), &[BlockInput::Table(table)]);
    let page = &p.pages()[0];
    let tops: Vec<f32> = page.fragments.iter().map(|f| f.top).collect();
    let pad = opendox_layout::CELL_PADDING;
    assert!(approx(tops[0], MARGIN + pad, 0.001));
    assert!(
        approx(tops[1], MARGIN + pad + LINE_H + 8.0 + 4.0, 0.001),
        "{tops:?}"
    );
    // The row's bottom border is below the taller cell.
    let bottom = page
        .rules
        .iter()
        .map(|r| r.y0.max(r.y1))
        .fold(0.0, f32::max);
    assert!(
        approx(bottom, MARGIN + 2.0 * pad + 2.0 * LINE_H + 12.0, 0.001),
        "{bottom}"
    );
    let _ = CONTENT;
}
