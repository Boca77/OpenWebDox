//! Objects (images) in paragraphs: in line, breaking text, wrapped, behind and in front of
//! text. Bundled fonts only, so the numbers are the same on every machine.

use opendox_layout::{
    BlockInput, FontLibrary, Layer, LayoutEngine, ObjectInput, ObjectMode, Page, PageGeometry,
    Paginator, ParagraphInput, ParagraphStyle, PlacedGlyph, TableInput, TextStyle, BREAK_MARGIN,
    OBJECT_PLACEHOLDER, WRAP_MARGIN,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

fn engine() -> LayoutEngine {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    LayoutEngine::new(fonts)
}

fn style() -> TextStyle {
    TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    }
}

fn object(byte: usize, w: f32, h: f32, mode: ObjectMode) -> ObjectInput {
    ObjectInput {
        byte,
        key: 7,
        width: w,
        height: h,
        rotation: 0.0,
        mode,
        x: 0.0,
        y: 0.0,
    }
}

fn para<'a>(text: &'a str, base: &'a TextStyle, objects: &'a [ObjectInput]) -> ParagraphInput<'a> {
    ParagraphInput {
        text,
        base,
        spans: &[],
        style: ParagraphStyle::default(),
        objects,
        marker: None,
    }
}

fn long_text() -> String {
    "The rabbit-hole went straight on like a tunnel for some way, and then dipped suddenly down. "
        .repeat(8)
}

/// Glyphs drawn on page 0 whose baseline is between `y0` and `y1`.
fn glyphs_between(p: &Paginator, y0: f32, y1: f32) -> Vec<PlacedGlyph> {
    p.placed_glyphs(0)
        .into_iter()
        .filter(|g| g.y > y0 && g.y < y1)
        .collect()
}

fn line_count(pages: &[Page]) -> usize {
    pages
        .iter()
        .flat_map(|p| &p.fragments)
        .map(|f| f.lines.len())
        .sum()
}

#[test]
fn inline_object_takes_its_place_in_the_line() {
    let text = format!("ab{OBJECT_PLACEHOLDER}cd");
    let s = style();
    let objects = [object(2, 100.0, 50.0, ObjectMode::Inline)];
    let mut e = engine();
    let layout = e.layout_paragraph_with(
        &text,
        &s,
        &[],
        &ParagraphStyle::default(),
        468.0,
        &objects,
        &[],
    );
    let line = &layout.lines[0];
    assert!(
        line.bottom - line.top > 50.0,
        "line grows to the object: {line:?}"
    );
    // Before and after the object, a caret stop at each edge of its box.
    let before = line.caret_x(2);
    let after = line.caret_x(5);
    assert!((after - before - 100.0).abs() < 0.01, "{before} -> {after}");
    assert_eq!(line.objects.len(), 1);
    assert_eq!(line.objects[0].object, 0);
    assert!((line.objects[0].x - before).abs() < 0.01);
    // The object's bottom sits on the baseline.
    assert!((line.objects[0].top + 50.0 - line.baseline).abs() < 0.01);
    // The placeholder draws no glyph: a, b, c, d.
    let glyphs: usize = line.runs.iter().map(|r| r.glyphs.len()).sum();
    assert_eq!(glyphs, 4);
    // Clicks left and right of the object's middle land before and after it.
    assert_eq!(line.offset_at(before + 30.0), 2);
    assert_eq!(line.offset_at(before + 70.0), 5);

    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut e, &[para(&text, &s, &objects)]);
    let o = &pages[0].objects[0];
    assert_eq!((o.key, o.layer), (7, Layer::Text));
    assert!((o.bounds.x - (72.0 + before)).abs() < 0.01);
    assert_eq!((o.width, o.height), (100.0, 50.0));
}

#[test]
fn inline_object_wider_than_the_line_shrinks_to_fit() {
    let text = OBJECT_PLACEHOLDER.to_string();
    let s = style();
    let objects = [object(0, 936.0, 100.0, ObjectMode::Inline)];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &[para(&text, &s, &objects)]);
    let o = &pages[0].objects[0];
    assert!((o.bounds.width - 468.0).abs() < 0.01);
    assert!((o.width - 468.0).abs() < 0.01 && (o.height - 50.0).abs() < 0.01);
}

#[test]
fn rotated_object_takes_its_bounding_box() {
    let text = format!("x{OBJECT_PLACEHOLDER}");
    let s = style();
    let mut o = object(1, 100.0, 40.0, ObjectMode::Inline);
    o.rotation = 90.0;
    assert_eq!(o.footprint().0.round(), 40.0);
    assert_eq!(o.footprint().1.round(), 100.0);
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &[para(&text, &s, &[o])]);
    let placed = &pages[0].objects[0];
    assert_eq!(placed.bounds.width.round(), 40.0);
    assert_eq!(placed.bounds.height.round(), 100.0);
    assert_eq!(
        (placed.width, placed.height, placed.rotation),
        (100.0, 40.0, 90.0)
    );
}

#[test]
fn object_breaking_text_gets_lines_of_its_own() {
    let text = format!("{OBJECT_PLACEHOLDER}After the picture.");
    let s = style();
    let mut o = object(0, 200.0, 100.0, ObjectMode::Break);
    o.x = 50.0;
    let mut p = Paginator::new(PageGeometry::letter());
    let os = [o];
    let placed = p.layout(
        &mut engine(),
        &[para("Before.", &s, &[]), para(&text, &s, &os)],
    )[0]
    .objects[0];
    let first = p.placed_lines()[0];
    let second = p.placed_lines()[1];
    assert!((placed.bounds.y - (first.bottom + BREAK_MARGIN)).abs() < 0.01);
    assert!(
        (placed.bounds.x - 122.0).abs() < 0.01,
        "50pt from the margin"
    );
    assert!(
        (second.top - (placed.bounds.y + 100.0 + BREAK_MARGIN)).abs() < 0.01,
        "text continues below: {second:?}"
    );
    // The text itself is laid out at full width, as if the object weren't there.
    let g = glyphs_between(&p, second.top, second.bottom);
    assert!((g[0].x - 72.0).abs() < 1.0);
}

#[test]
fn wrapped_object_shortens_the_lines_beside_it() {
    let text = format!("{OBJECT_PLACEHOLDER}{}", long_text());
    let s = style();
    let objects = [object(0, 200.0, 100.0, ObjectMode::Wrap)];
    let mut p = Paginator::new(PageGeometry::letter());
    let plain = line_count(p.layout(&mut engine(), &[para(&text, &s, &[])]));
    let pages = p.layout(&mut engine(), &[para(&text, &s, &objects)]);
    assert!(
        line_count(pages) > plain,
        "narrower lines need more of them"
    );
    let placed = pages[0].objects[0];
    assert_eq!(placed.layer, Layer::Text);
    assert!((placed.bounds.x - 72.0).abs() < 0.01 && (placed.bounds.y - 72.0).abs() < 0.01);

    // Lines beside the object start right of it (plus margin); lines below start at the margin.
    let clear = placed.bounds.x + placed.bounds.width + WRAP_MARGIN;
    let bottom = placed.bounds.y + placed.bounds.height + WRAP_MARGIN;
    for l in p.placed_lines() {
        let g = glyphs_between(&p, l.top, l.bottom);
        let x = g.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
        if l.top < bottom {
            assert!(x >= clear - 0.5, "line at {} starts at {x}", l.top);
        } else {
            assert!((x - 72.0).abs() < 1.0, "line at {} starts at {x}", l.top);
        }
    }
    // Carets follow the text, so clicking beside the object finds it.
    let hit = p.hit_test(0, clear + 1.0, 80.0).unwrap();
    assert_eq!(hit.byte, 3, "just after the placeholder");
}

#[test]
fn wrapped_object_reaches_the_following_paragraphs() {
    let tall = format!("{OBJECT_PLACEHOLDER}Short.");
    let text = long_text();
    let s = style();
    let objects = [object(0, 150.0, 200.0, ObjectMode::Wrap)];
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(
        &mut engine(),
        &[para(&tall, &s, &objects), para(&text, &s, &[])],
    );
    let second = p
        .placed_lines()
        .iter()
        .find(|l| l.paragraph == 1)
        .copied()
        .unwrap();
    let g = glyphs_between(&p, second.top, second.bottom);
    assert!(
        g[0].x > 72.0 + 150.0,
        "next paragraph wraps too: {}",
        g[0].x
    );
}

#[test]
fn line_moves_below_an_object_too_wide_to_wrap_beside() {
    let text = format!("{OBJECT_PLACEHOLDER}{}", long_text());
    let s = style();
    let objects = [object(0, 440.0, 100.0, ObjectMode::Wrap)];
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut engine(), &[para(&text, &s, &objects)]);
    let first = p.placed_lines()[0];
    assert!(
        first.top >= 72.0 + 100.0 + WRAP_MARGIN - 0.01,
        "no line squeezed beside it: {first:?}"
    );
}

#[test]
fn objects_behind_and_in_front_leave_the_text_alone() {
    let text = format!("{OBJECT_PLACEHOLDER}{}", long_text());
    let s = style();
    let mut p = Paginator::new(PageGeometry::letter());
    // The text as it is with any object not in line (the placeholder draws nothing).
    let tiny = [object(0, 1.0, 1.0, ObjectMode::Behind)];
    p.layout(
        &mut engine(),
        &[para("Intro.", &s, &[]), para(&text, &s, &tiny)],
    );
    let plain = p.placed_glyphs(0);
    for (mode, layer) in [
        (ObjectMode::Behind, Layer::Behind),
        (ObjectMode::Front, Layer::Front),
    ] {
        let mut o = object(0, 300.0, 200.0, mode);
        o.x = 20.0;
        o.y = 30.0;
        let pages = p.layout(
            &mut engine(),
            &[para("Intro.", &s, &[]), para(&text, &s, &[o])],
        );
        let placed = pages[0].objects[0];
        assert_eq!(placed.layer, layer);
        let top = p.placed_lines()[1].top;
        assert!((placed.bounds.x - 92.0).abs() < 0.01);
        assert!((placed.bounds.y - (top + 30.0)).abs() < 0.01);
        assert_eq!(p.placed_glyphs(0), plain, "{mode:?} moved text");
    }
}

#[test]
fn positioned_objects_stay_on_the_page() {
    let text = OBJECT_PLACEHOLDER.to_string();
    let s = style();
    let mut o = object(0, 100.0, 100.0, ObjectMode::Front);
    o.x = 1000.0;
    o.y = -500.0;
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &[para(&text, &s, &[o])]);
    let b = pages[0].objects[0].bounds;
    assert_eq!((b.x, b.y), (512.0, 0.0));
}

#[test]
fn objects_in_table_cells_are_always_in_line() {
    let text = format!("a{OBJECT_PLACEHOLDER}");
    let s = style();
    let objects = [object(1, 60.0, 30.0, ObjectMode::Wrap)];
    let cell = vec![para(&text, &s, &objects)];
    let blocks = [BlockInput::Table(TableInput {
        rows: vec![vec![cell]],
    })];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout_blocks(&mut engine(), &blocks);
    let o = pages[0].objects[0];
    assert_eq!(o.layer, Layer::Text);
    let line = p.placed_lines()[0];
    assert!(
        line.bottom - line.top > 30.0,
        "in line: the cell's line grew"
    );
}

#[test]
fn layout_with_wrapped_objects_is_repeatable() {
    let text = format!("{OBJECT_PLACEHOLDER}{}", long_text());
    let s = style();
    let mut o = object(0, 180.0, 120.0, ObjectMode::Wrap);
    o.x = 250.0;
    o.y = 10.0;
    let os = [o];
    let input = [
        para("Intro line.", &s, &[]),
        para(&text, &s, &os),
        para(&text, &s, &[]),
    ];
    let mut a = Paginator::new(PageGeometry::letter());
    let pages_a = a.layout(&mut engine(), &input).to_vec();
    let mut b = Paginator::new(PageGeometry::letter());
    b.layout(&mut engine(), &input);
    // Laid out again from a warm cache, too.
    let pages_b = b.layout(&mut engine(), &input).to_vec();
    assert_eq!(pages_a, pages_b);
    assert_eq!(a.placed_glyphs(0), b.placed_glyphs(0));
    // Text right of the margin and left of the object: the left gap is the wider one.
    let placed = pages_a[0].objects[0];
    let l = a
        .placed_lines()
        .iter()
        .find(|l| l.paragraph == 1)
        .copied()
        .unwrap();
    let g = glyphs_between(&a, l.top, l.bottom);
    let max_x = g.iter().map(|g| g.x).fold(0.0, f32::max);
    assert!(max_x < placed.bounds.x - WRAP_MARGIN, "{max_x}");
}
