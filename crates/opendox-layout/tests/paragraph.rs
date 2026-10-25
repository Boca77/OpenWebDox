//! Golden tests for paragraph layout. They use only the fonts bundled in `fonts/noto`, so the
//! expected line breaks hold on every machine.

use opendox_layout::{FontId, FontLibrary, LayoutEngine, ParagraphLayout, StyleSpan, TextStyle};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Bold.ttf");
const ARABIC: &[u8] = include_bytes!("../../../fonts/noto/NotoSansArabic-Regular.ttf");

struct Fixture {
    engine: LayoutEngine,
    regular: FontId,
    bold: FontId,
    arabic: FontId,
}

fn fixture() -> Fixture {
    let mut fonts = FontLibrary::new();
    let regular = fonts.register(REGULAR.to_vec());
    let bold = fonts.register(BOLD.to_vec());
    let arabic = fonts.register(ARABIC.to_vec());
    Fixture {
        engine: LayoutEngine::new(fonts),
        regular,
        bold,
        arabic,
    }
}

fn style(size: f32) -> TextStyle {
    TextStyle {
        font_family: "Noto Sans, Noto Sans Arabic".into(),
        font_size: size,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    }
}

fn layout(text: &str, width: f32) -> ParagraphLayout {
    fixture()
        .engine
        .layout_paragraph(text, &style(12.0), &[], width)
}

fn line_texts<'a>(text: &'a str, l: &ParagraphLayout) -> Vec<&'a str> {
    l.lines
        .iter()
        .map(|line| &text[line.byte_range.clone()])
        .collect()
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn wraps_at_word_boundaries() {
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(3);
    let l = layout(&text, 200.0);
    assert_eq!(
        line_texts(&text, &l),
        [
            "The quick brown fox jumps over ",
            "the lazy dog. The quick brown fox ",
            "jumps over the lazy dog. The quick ",
            "brown fox jumps over the lazy dog. ",
        ]
    );
}

#[test]
fn line_height_comes_from_font_metrics_without_rounding() {
    let l = layout("Hello", 200.0);
    let line = &l.lines[0];
    // Noto Sans ascent + descent at 12pt = 16.34pt. Pixel snapping would give 17.
    assert!(approx(line.bottom - line.top, 16.34), "{line:?}");
    assert!(approx(line.baseline, 12.83), "{line:?}");
}

#[test]
fn breaks_words_wider_than_the_line() {
    let text = "Supercalifragilisticexpialidocious_and_even_longer end";
    let l = layout(text, 100.0);
    assert_eq!(
        line_texts(text, &l),
        [
            "Supercalifragilisti",
            "cexpialidocious_a",
            "nd_even_longer ",
            "end"
        ]
    );
    for line in &l.lines {
        assert!(line.advance <= 100.0 + 5.0, "{line:?}"); // only a hanging space may overrun
    }
}

#[test]
fn breaks_cjk_without_spaces() {
    // No CJK font is bundled, so glyphs are .notdef boxes, but breaking follows UAX #14.
    let text = "日本語のテキストは空白なしで改行されます";
    let l = layout(text, 100.0);
    assert_eq!(
        line_texts(text, &l),
        ["日本語のテキストは空白なし", "で改行されます"]
    );
}

#[test]
fn never_splits_emoji_or_combining_marks() {
    let text = "family 👨‍👩‍👧 and cafe\u{301} done";
    let l = layout(text, 80.0);
    assert_eq!(
        line_texts(text, &l),
        ["family 👨\u{200d}👩\u{200d}👧 ", "and cafe\u{301} done"]
    );
}

#[test]
fn mixed_sizes_grow_the_line() {
    let text = "small text BIG small text again";
    let mut f = fixture();
    let big = StyleSpan {
        byte_range: 11..14,
        style: style(30.0),
    };
    let l = f.engine.layout_paragraph(text, &style(12.0), &[big], 400.0);
    assert_eq!(l.lines.len(), 1);
    let line = &l.lines[0];
    // Tall enough for the 30pt run; baseline pushed down so the big text fits above it.
    assert!(approx(line.bottom - line.top, 40.86), "{line:?}");
    assert!(approx(line.baseline, 32.07), "{line:?}");
    let sizes: Vec<f32> = line.runs.iter().map(|r| r.font_size).collect();
    assert_eq!(sizes, [12.0, 30.0, 12.0]);
}

#[test]
fn arabic_uses_fallback_font_and_is_rtl() {
    let text = "مرحبا بالعالم هذا نص عربي طويل جدا";
    let mut f = fixture();
    let l = f.engine.layout_paragraph(text, &style(12.0), &[], 100.0);
    assert_eq!(
        line_texts(text, &l),
        ["مرحبا بالعالم هذا نص ", "عربي طويل جدا"]
    );
    let runs: Vec<_> = l.lines.iter().flat_map(|line| &line.runs).collect();
    let summary: Vec<_> = runs
        .iter()
        .map(|r| (&text[r.byte_range.clone()], r.font, r.rtl))
        .collect();
    // Letters fall back to the Arabic font; spaces come from Noto Sans, first in the family list.
    assert!(runs.iter().all(|r| r.rtl), "{summary:?}");
    for (s, font, _) in &summary {
        let expected = if s.trim().is_empty() {
            f.regular
        } else {
            f.arabic
        };
        assert_eq!(*font, expected, "{summary:?}");
    }
    assert_ne!(f.arabic, f.regular);
}

#[test]
fn bold_span_uses_bold_font() {
    let text = "plain bold plain";
    let mut f = fixture();
    let bold = StyleSpan {
        byte_range: 6..10,
        style: TextStyle {
            font_weight: 700.0,
            ..style(12.0)
        },
    };
    let l = f
        .engine
        .layout_paragraph(text, &style(12.0), &[bold], 400.0);
    let fonts: Vec<FontId> = l.lines[0].runs.iter().map(|r| r.font).collect();
    assert_eq!(fonts, [f.regular, f.bold, f.regular]);
}

#[test]
fn empty_paragraph_still_has_one_line() {
    let l = layout("", 200.0);
    assert_eq!(l.lines.len(), 1);
    assert!(approx(l.height, 16.34), "{l:?}");
}

#[test]
fn lines_stack_without_gaps() {
    let text = "word ".repeat(100);
    let l = layout(&text, 150.0);
    assert!(l.lines.len() > 5);
    assert_eq!(l.lines[0].top, 0.0);
    for pair in l.lines.windows(2) {
        assert!(approx(pair[0].bottom, pair[1].top), "{pair:?}");
    }
    assert_eq!(l.height, l.lines.last().unwrap().bottom);
}

#[test]
fn glyph_positions_scale_exactly_with_font_size() {
    // Layout is in points with no hinting, so doubling the size doubles every advance.
    let mut f = fixture();
    let small = f
        .engine
        .layout_paragraph("Typography", &style(12.0), &[], 1000.0);
    let large = f
        .engine
        .layout_paragraph("Typography", &style(24.0), &[], 1000.0);
    assert!(approx(large.lines[0].advance, small.lines[0].advance * 2.0));
    let xs = |l: &ParagraphLayout| -> Vec<f32> {
        l.lines[0].runs[0].glyphs.iter().map(|g| g.x).collect()
    };
    for (a, b) in xs(&small).iter().zip(xs(&large)) {
        assert!(approx(a * 2.0, b));
    }
}

#[test]
fn layout_is_deterministic() {
    let text = "Same input, same output, every time. ".repeat(20);
    let a = layout(&text, 300.0);
    let b = layout(&text, 300.0);
    assert_eq!(a, b);
}

#[test]
fn glyph_runs_cover_line_text_and_reference_registered_fonts() {
    let text = "Hello مرحبا world";
    let mut f = fixture();
    let l = f.engine.layout_paragraph(text, &style(12.0), &[], 1000.0);
    let line = &l.lines[0];
    for run in &line.runs {
        assert!(f.engine.fonts().data(run.font).is_some());
        assert!(!run.glyphs.is_empty());
        assert!(line.byte_range.start <= run.byte_range.start);
        assert!(run.byte_range.end <= line.byte_range.end);
    }
    // Visual order: glyph x positions increase across runs even with RTL in the middle.
    let firsts: Vec<f32> = line.runs.iter().map(|r| r.glyphs[0].x).collect();
    assert!(firsts.windows(2).all(|w| w[0] < w[1]), "{firsts:?}");
}
