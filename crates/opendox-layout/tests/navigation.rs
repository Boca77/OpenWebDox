//! Hit testing, carets, vertical movement and selection rectangles.

use opendox_layout::{
    FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, TextPosition, TextStyle,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const ARABIC: &[u8] = include_bytes!("../../../fonts/noto/NotoSansArabic-Regular.ttf");

const LINE_H: f32 = 16.34;
const MARGIN: f32 = 72.0;

fn paginate(texts: &[&str]) -> Paginator {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    fonts.register(ARABIC.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let style = TextStyle {
        font_family: "Noto Sans, Noto Sans Arabic".into(),
        font_size: 12.0,
        font_weight: 400.0,
    };
    let inputs: Vec<ParagraphInput> = texts
        .iter()
        .map(|t| ParagraphInput {
            text: t,
            base: &style,
            spans: &[],
        })
        .collect();
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut engine, &inputs);
    p
}

fn pos(paragraph: usize, byte: usize, upstream: bool) -> TextPosition {
    TextPosition {
        paragraph,
        byte,
        upstream,
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

const WRAPPING: &str = "The quick brown fox jumps over the lazy dog and keeps running far past \
    the end of the first line so that this paragraph wraps onto several lines of text.";

#[test]
fn ltr_caret_stops_match_glyph_positions() {
    let p = paginate(&["Hello world"]);
    let line = &p.paragraph(0).unwrap().lines[0];
    let glyph_xs: Vec<f32> = line.runs[0].glyphs.iter().map(|g| g.x).collect();
    let stop_xs: Vec<f32> = line.carets.iter().map(|s| s.x).collect();
    // One stop before each glyph, plus one after the last.
    assert_eq!(stop_xs.len(), glyph_xs.len() + 1);
    for (s, g) in stop_xs.iter().zip(&glyph_xs) {
        assert!(approx(*s, *g), "{stop_xs:?} vs {glyph_xs:?}");
    }
    let bytes: Vec<usize> = line.carets.iter().map(|s| s.byte).collect();
    assert_eq!(bytes, (0..=11).collect::<Vec<_>>());
}

#[test]
fn clicking_a_caret_position_returns_it() {
    let p = paginate(&[WRAPPING, "Second paragraph."]);
    for placed in p.placed_lines() {
        let line = &p.paragraph(placed.paragraph).unwrap().lines[placed.line];
        for stop in &line.carets {
            let y = (placed.top + placed.bottom) / 2.0;
            let hit = p.hit_test(placed.page, MARGIN + stop.x, y).unwrap();
            assert_eq!((hit.paragraph, hit.byte), (placed.paragraph, stop.byte));
            let rect = p.caret_rect(hit).unwrap();
            assert!(approx(rect.x, MARGIN + stop.x), "{rect:?} {stop:?}");
            assert!(approx(rect.top, placed.top), "caret moved to another line");
        }
    }
}

#[test]
fn clicks_in_margins_snap_to_line_ends() {
    let p = paginate(&[WRAPPING]);
    let lines = p.placed_lines();
    assert!(lines.len() >= 2);
    let first = lines[0];
    let line0 = &p.paragraph(0).unwrap().lines[0];
    let mid = (first.top + first.bottom) / 2.0;

    // Left margin: start of the line.
    assert_eq!(p.hit_test(0, 5.0, mid), Some(pos(0, 0, false)));

    // Right margin of a wrapped line: its end, kept on this line (upstream).
    let end = p.hit_test(0, 600.0, mid).unwrap();
    assert_eq!(end, pos(0, line0.byte_range.end, true));
    assert!(approx(p.caret_rect(end).unwrap().top, first.top));

    // Right margin of the paragraph's last line: end of text, no ambiguity.
    let last = lines.last().unwrap();
    let hit = p
        .hit_test(0, 600.0, (last.top + last.bottom) / 2.0)
        .unwrap();
    assert_eq!(hit, pos(0, WRAPPING.len(), false));
}

#[test]
fn clicks_above_or_below_text_snap_to_first_or_last_line() {
    let p = paginate(&["First line.", "Last line."]);
    // Top margin.
    assert_eq!(p.hit_test(0, MARGIN, 10.0).unwrap().paragraph, 0);
    // Far below the last line, in the empty rest of the page.
    let hit = p.hit_test(0, MARGIN, 700.0).unwrap();
    assert_eq!(hit.paragraph, 1);
}

#[test]
fn hit_testing_respects_page_boundaries() {
    // 39 lines fit on a Letter page at 12pt; 45 one-line paragraphs spill onto page 2.
    let texts: Vec<String> = (0..45).map(|i| format!("Paragraph {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let p = paginate(&refs);
    assert_eq!(p.pages().len(), 2);

    // Bottom margin of page 1 → last line on page 1; top margin of page 2 → first on page 2.
    assert_eq!(p.hit_test(0, MARGIN, 780.0).unwrap().paragraph, 38);
    assert_eq!(p.hit_test(1, MARGIN, 5.0).unwrap().paragraph, 39);

    // Arrow down from the last line of page 1 lands on page 2.
    let next = p.move_vertical(pos(38, 0, false), MARGIN, true).unwrap();
    assert_eq!(next.paragraph, 39);
    assert_eq!(p.caret_rect(next).unwrap().page, 1);
    assert!(approx(p.caret_rect(next).unwrap().top, MARGIN));

    // A page that doesn't exist has nothing to hit.
    assert_eq!(p.hit_test(2, MARGIN, MARGIN), None);
}

#[test]
fn wrap_point_affinity_picks_the_line() {
    let p = paginate(&[WRAPPING]);
    let line0 = &p.paragraph(0).unwrap().lines[0];
    let wrap = line0.byte_range.end;

    let up = p.caret_rect(pos(0, wrap, true)).unwrap();
    let down = p.caret_rect(pos(0, wrap, false)).unwrap();
    assert!(approx(up.top, MARGIN), "upstream stays on line 1");
    assert!(
        approx(down.top, MARGIN + LINE_H),
        "downstream starts line 2"
    );
    assert!(approx(down.x, MARGIN));
    assert!(up.x > MARGIN + 300.0);
}

#[test]
fn vertical_movement_keeps_the_goal_column() {
    let p = paginate(&[WRAPPING]);
    let goal = MARGIN + 150.0;
    let start = p.hit_test(0, goal, MARGIN + 1.0).unwrap();
    let down = p.move_vertical(start, goal, true).unwrap();
    let rect = p.caret_rect(down).unwrap();
    assert!(approx(rect.top, MARGIN + LINE_H));
    assert!((rect.x - goal).abs() < 6.0, "{rect:?}"); // within half a glyph
    let back = p.move_vertical(down, goal, false).unwrap();
    assert_eq!(back, start);

    // No line above the first or below the last.
    assert_eq!(p.move_vertical(start, goal, false), None);
    let last = p.placed_lines().len() - 1;
    let bottom = p
        .hit_test(0, goal, p.placed_lines()[last].top + 1.0)
        .unwrap();
    assert_eq!(p.move_vertical(bottom, goal, true), None);
}

#[test]
fn home_and_end_go_to_visual_line_edges() {
    let p = paginate(&[WRAPPING]);
    let lines = &p.paragraph(0).unwrap().lines;
    assert_eq!(lines.len(), 2);

    // A wrapped line: End stays on it (upstream).
    let inside_first = pos(0, 3, false);
    assert_eq!(p.line_start(inside_first), Some(pos(0, 0, false)));
    assert_eq!(
        p.line_end(inside_first),
        Some(pos(0, lines[0].byte_range.end, true))
    );

    // The paragraph's last line: End is the end of the text, no ambiguity.
    let inside_last = pos(0, lines[1].byte_range.start + 3, false);
    assert_eq!(
        p.line_start(inside_last),
        Some(pos(0, lines[1].byte_range.start, false))
    );
    assert_eq!(p.line_end(inside_last), Some(pos(0, WRAPPING.len(), false)));
}

#[test]
fn selection_rects_cover_lines_and_paragraph_breaks() {
    let p = paginate(&[WRAPPING, "Second paragraph."]);
    let lines0 = p.paragraph(0).unwrap().lines.len();

    // Within one line: one box exactly between the two carets.
    let r = p.selection_rects(pos(0, 4, false), pos(0, 9, false));
    assert_eq!(r.len(), 1);
    let (a, b) = (
        p.caret_rect(pos(0, 4, false)).unwrap(),
        p.caret_rect(pos(0, 9, false)).unwrap(),
    );
    assert!(
        approx(r[0].x, a.x) && approx(r[0].width, b.x - a.x),
        "{r:?}"
    );

    // Across the paragraph break: one box per line, plus the break marker on the last line
    // of paragraph 0.
    let r = p.selection_rects(pos(0, 4, false), pos(1, 6, false));
    assert_eq!(r.len(), lines0 + 1);
    let last_of_p0 = &r[lines0 - 1];
    let end_x = p.caret_rect(pos(0, WRAPPING.len(), false)).unwrap().x;
    assert!(
        approx(last_of_p0.x + last_of_p0.width, end_x + 4.0),
        "{r:?}"
    );

    // Order doesn't matter; empty selections draw nothing.
    assert_eq!(r, p.selection_rects(pos(1, 6, false), pos(0, 4, false)));
    assert!(p
        .selection_rects(pos(0, 4, false), pos(0, 4, true))
        .is_empty());
}

#[test]
fn empty_paragraph_has_a_caret_at_the_margin() {
    let p = paginate(&["Above", "", "Below"]);
    let hit = p.hit_test(0, 300.0, MARGIN + LINE_H * 1.5).unwrap();
    assert_eq!(hit, pos(1, 0, false));
    let rect = p.caret_rect(hit).unwrap();
    assert!(approx(rect.x, MARGIN));
    assert!(approx(rect.top, MARGIN + LINE_H));
}

#[test]
fn rtl_paragraph_starts_at_the_right() {
    let text = "مرحبا بالعالم";
    let p = paginate(&[text]);
    let start = p.caret_rect(pos(0, 0, false)).unwrap();
    let end = p.caret_rect(pos(0, text.len(), false)).unwrap();
    assert!(start.x > end.x, "start {start:?} end {end:?}");
    // Clicking just left of the start caret lands next to it, not at the far end.
    let hit = p.hit_test(0, start.x - 1.0, MARGIN + 5.0).unwrap();
    assert_eq!(hit.byte, 0);
}
