//! Every glyph knows which text it draws, so exported PDFs can be searched and copied.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_layout::{FontLibrary, LayoutEngine, ParagraphLayout, TextStyle};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const ARABIC: &[u8] = include_bytes!("../../../fonts/noto/NotoSansArabic-Regular.ttf");

fn layout(text: &str) -> ParagraphLayout {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    fonts.register(ARABIC.to_vec());
    let style = TextStyle {
        font_family: "Noto Sans, Noto Sans Arabic".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    };
    LayoutEngine::new(fonts).layout_paragraph(text, &style, &[], 1000.0)
}

/// Each glyph's text, and the set of text bytes covered by some glyph.
fn check_cover(text: &str) -> (Vec<String>, usize) {
    let l = layout(text);
    let mut covered = vec![false; text.len()];
    let mut pieces = Vec::new();
    for run in l.lines.iter().flat_map(|l| &l.runs) {
        assert_eq!(run.glyph_text.len(), run.glyphs.len());
        for r in &run.glyph_text {
            assert!(
                !r.is_empty() && r.start >= run.byte_range.start && r.end <= run.byte_range.end
            );
            covered[r.clone()].iter_mut().for_each(|c| *c = true);
            pieces.push(text[r.clone()].to_string());
        }
    }
    (pieces, covered.iter().filter(|c| !**c).count())
}

#[test]
fn plain_text_maps_one_glyph_to_one_character() {
    let (pieces, uncovered) = check_cover("Hello, world");
    assert_eq!(pieces.concat(), "Hello, world");
    assert_eq!(uncovered, 0);
}

#[test]
fn ligatures_and_combining_marks_keep_all_their_text() {
    // "fi" and "ffl" may become single ligature glyphs; "é" built from e + U+0301 may be one or
    // two glyphs. Either way no character is lost.
    let text = "office waffle cafe\u{301}";
    let (pieces, uncovered) = check_cover(text);
    assert_eq!(uncovered, 0);
    let mut joined = pieces.clone();
    joined.dedup();
    assert_eq!(joined.concat(), text);
}

#[test]
fn right_to_left_text_is_covered_too() {
    let text = "hello مرحبا بالعالم end";
    let (_, uncovered) = check_cover(text);
    assert_eq!(uncovered, 0);
}
