//! Indents and list markers: lines between the indents, first-line and hanging indents, the
//! marker on the first line's baseline, and markers through pagination. Bundled fonts only.

use opendox_layout::{
    Exclusion, FontId, FontLibrary, LayoutEngine, Marker, PageGeometry, Paginator, ParagraphInput,
    ParagraphLayout, ParagraphStyle, TextStyle, MARKER_GAP,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const SYMBOLS: &[u8] = include_bytes!("../../../fonts/noto/NotoSansSymbols2-Regular.ttf");

const TEXT: &str = "The quick brown fox jumps over the lazy dog and keeps running through \
                    the field until the sun goes down behind the hills far away.";

fn engine() -> (LayoutEngine, FontId) {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let symbols = fonts.register(SYMBOLS.to_vec());
    (LayoutEngine::new(fonts), symbols)
}

fn style() -> TextStyle {
    TextStyle {
        font_family: "Noto Sans, Noto Sans Symbols 2".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    }
}

fn indented(start: f32, end: f32, first: f32) -> ParagraphStyle {
    ParagraphStyle {
        indent_start: start,
        indent_end: end,
        indent_first: first,
        ..ParagraphStyle::default()
    }
}

fn lay(text: &str, para: &ParagraphStyle, width: f32, marker: Option<&str>) -> ParagraphLayout {
    let (mut e, _) = engine();
    let s = style();
    let marker = marker.map(|text| Marker { text, style: &s });
    e.layout_list_item(text, &s, &[], para, width, &[], &[], marker)
}

fn ranges(l: &ParagraphLayout) -> Vec<std::ops::Range<usize>> {
    l.lines.iter().map(|l| l.byte_range.clone()).collect()
}

fn first_x(line: &opendox_layout::Line) -> f32 {
    line.runs[0].glyphs[0].x
}

#[test]
fn start_indent_breaks_like_a_narrower_paragraph_shifted_right() {
    let plain = lay(TEXT, &ParagraphStyle::default(), 164.0, None);
    let ind = lay(TEXT, &indented(36.0, 0.0, 0.0), 200.0, None);
    assert!(plain.lines.len() >= 3);
    assert_eq!(ranges(&plain), ranges(&ind));
    for (a, b) in plain.lines.iter().zip(&ind.lines) {
        assert!((first_x(b) - first_x(a) - 36.0).abs() < 0.01);
        assert!((b.carets[0].x - a.carets[0].x - 36.0).abs() < 0.01);
        assert_eq!(a.baseline, b.baseline);
    }
}

#[test]
fn end_indent_narrows_lines_from_the_right() {
    let plain = lay(TEXT, &ParagraphStyle::default(), 164.0, None);
    let ind = lay(TEXT, &indented(0.0, 36.0, 0.0), 200.0, None);
    assert_eq!(ranges(&plain), ranges(&ind));
    assert!(ind.lines.iter().all(|l| l.advance <= 164.0 + 0.01));
}

#[test]
fn first_line_indent_moves_only_the_first_line() {
    let l = lay(TEXT, &indented(0.0, 0.0, 24.0), 200.0, None);
    assert!((first_x(&l.lines[0]) - 24.0).abs() < 0.01);
    assert!(l.lines[1..].iter().all(|line| first_x(line).abs() < 0.01));
    // Hanging: the first line out to the left of the others, never past the edge.
    let h = lay(TEXT, &indented(36.0, 0.0, -18.0), 200.0, None);
    assert!((first_x(&h.lines[0]) - 18.0).abs() < 0.01);
    assert!(h.lines[1..]
        .iter()
        .all(|line| (first_x(line) - 36.0).abs() < 0.01));
    let past = lay(TEXT, &indented(10.0, 0.0, -50.0), 200.0, None);
    assert!(first_x(&past.lines[0]).abs() < 0.01);
}

#[test]
fn marker_sits_in_the_hanging_indent_on_the_first_baseline() {
    let (_, symbols) = engine();
    let l = lay(TEXT, &indented(36.0, 0.0, -18.0), 200.0, Some("●"));
    let m = l.marker.as_ref().unwrap();
    assert_eq!(m.text, "●");
    assert_eq!(m.x, 18.0);
    let g = m.runs[0].glyphs[0];
    assert!((g.x - 18.0).abs() < 0.01);
    assert_eq!(g.y, l.lines[0].baseline);
    // Drawn from the symbols font: Noto Sans has no ●.
    assert_eq!(m.runs[0].font, symbols);
    assert!(m.width > 0.0 && 18.0 + m.width + MARKER_GAP <= 36.0);
    // Text starts at the indent on every line, the first included.
    assert!(l
        .lines
        .iter()
        .all(|line| (first_x(line) - 36.0).abs() < 0.01));
}

#[test]
fn a_wide_marker_pushes_the_first_line_text_right() {
    let l = lay("item", &indented(36.0, 0.0, -18.0), 200.0, Some("1000."));
    let m = l.marker.as_ref().unwrap();
    let want = 18.0 + m.width + MARKER_GAP;
    assert!(want > 36.0);
    assert!((first_x(&l.lines[0]) - want).abs() < 0.01);
}

#[test]
fn empty_list_item_still_has_its_marker_and_a_caret_at_the_indent() {
    let l = lay("", &indented(36.0, 0.0, -18.0), 200.0, Some("1."));
    assert_eq!(l.lines.len(), 1);
    assert!(l.marker.is_some());
    assert!((l.lines[0].carets[0].x - 36.0).abs() < 0.01);
}

#[test]
fn centred_lines_centre_between_the_indents() {
    let para = ParagraphStyle {
        align: opendox_layout::Align::Center,
        ..indented(40.0, 0.0, 0.0)
    };
    let l = lay("mid", &para, 200.0, None);
    let line = &l.lines[0];
    let left = first_x(line);
    let right = left + line.advance;
    assert!(((left - 40.0) - (200.0 - right)).abs() < 0.5);
}

#[test]
fn wrapping_around_an_object_stays_inside_the_indent() {
    let (mut e, _) = engine();
    let s = style();
    let ex = [Exclusion {
        top: 0.0,
        bottom: 40.0,
        left: 120.0,
        right: 200.0,
    }];
    let l = e.layout_paragraph_with(TEXT, &s, &[], &indented(36.0, 0.0, 0.0), 200.0, &[], &ex);
    for line in &l.lines {
        assert!(first_x(line) >= 36.0 - 0.01);
        if line.top < 40.0 {
            assert!(first_x(line) + line.advance <= 120.0 + 0.5);
        }
    }
}

#[test]
fn marker_is_placed_once_on_the_page_holding_the_first_line() {
    let (mut e, _) = engine();
    let s = style();
    let long = TEXT.repeat(30);
    let para = indented(36.0, 0.0, -18.0);
    let input = ParagraphInput {
        text: &long,
        base: &s,
        spans: &[],
        style: para,
        objects: &[],
        marker: Some(Marker {
            text: "●",
            style: &s,
        }),
    };
    let geometry = PageGeometry {
        height: 300.0,
        ..PageGeometry::letter()
    };
    let mut p = Paginator::new(geometry);
    let pages = p.layout(&mut e, &[input]).len();
    assert!(pages >= 2);
    let para_glyphs: usize = p.paragraph(0).unwrap().lines[..p.pages()[0].fragments[0].lines.end]
        .iter()
        .flat_map(|l| &l.runs)
        .map(|r| r.glyphs.len())
        .sum();
    assert_eq!(p.placed_glyphs(0).len(), para_glyphs + 1);
    let marker = p.placed_glyphs(0)[..]
        .iter()
        .find(|g| (g.x - (72.0 + 18.0)).abs() < 0.01)
        .copied();
    assert!(marker.is_some(), "marker at margin + 18pt");
    let second: usize = p.pages()[1]
        .fragments
        .iter()
        .flat_map(|f| &p.paragraph(0).unwrap().lines[f.lines.clone()])
        .flat_map(|l| &l.runs)
        .map(|r| r.glyphs.len())
        .sum();
    assert_eq!(p.placed_glyphs(1).len(), second);
}

#[test]
fn changing_the_marker_relays_out_the_paragraph() {
    let (mut e, _) = engine();
    let s = style();
    let para = indented(36.0, 0.0, -18.0);
    let mut p = Paginator::new(PageGeometry::letter());
    for (text, shaped) in [("1.", 1), ("1.", 0), ("2.", 1)] {
        let input = ParagraphInput {
            text: "item",
            base: &s,
            spans: &[],
            style: para,
            objects: &[],
            marker: Some(Marker { text, style: &s }),
        };
        p.layout(&mut e, &[input]);
        assert_eq!(p.last_shaped_count(), shaped, "marker {text}");
    }
}
