//! Data the renderer consumes: glyph outlines and glyphs placed on pages.

use opendox_layout::{
    FontId, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, ParagraphStyle,
    TextStyle,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

fn setup() -> (LayoutEngine, FontId, TextStyle) {
    let mut fonts = FontLibrary::new();
    let id = fonts.register(REGULAR.to_vec());
    let style = TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    };
    (LayoutEngine::new(fonts), id, style)
}

fn glyph_ids(engine: &mut LayoutEngine, style: &TextStyle, text: &str) -> Vec<u32> {
    let l = engine.layout_paragraph(text, style, &[], 1000.0);
    l.lines[0].runs[0].glyphs.iter().map(|g| g.id).collect()
}

#[test]
fn outline_of_a_letter_is_a_closed_path() {
    let (mut engine, font, style) = setup();
    let h = glyph_ids(&mut engine, &style, "H")[0];
    let o = engine.fonts().glyph_outline(font, 0, h).unwrap();
    assert_eq!(o.units_per_em, 1000);
    assert!(o.svg_path.starts_with('M'), "{}", o.svg_path);
    assert!(o.svg_path.ends_with('Z'), "{}", o.svg_path);
    // "H" sits on the baseline and rises above it: y values are <= 0 after the flip.
    assert!(o.svg_path.contains(" -"), "{}", o.svg_path);
}

#[test]
fn space_has_an_empty_outline() {
    let (mut engine, font, style) = setup();
    let space = glyph_ids(&mut engine, &style, "a b")[1];
    let o = engine.fonts().glyph_outline(font, 0, space).unwrap();
    assert!(o.svg_path.is_empty());
}

#[test]
fn unknown_font_or_glyph_has_no_outline() {
    let (engine, font, _) = setup();
    assert!(engine.fonts().glyph_outline(FontId(99), 0, 1).is_none());
    assert!(engine.fonts().glyph_outline(font, 0, 1_000_000).is_none());
}

#[test]
fn placed_glyphs_sit_inside_the_page_margins() {
    let (mut engine, _, style) = setup();
    let texts: Vec<String> = (0..80).map(|i| format!("Line {i} of text")).collect();
    let inputs: Vec<ParagraphInput> = texts
        .iter()
        .map(|t| ParagraphInput {
            text: t,
            base: &style,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        })
        .collect();
    let g = PageGeometry::letter();
    let mut p = Paginator::new(g);
    let pages = p.layout(&mut engine, &inputs).len();
    assert_eq!(pages, 3);

    for page in 0..pages {
        let glyphs = p.placed_glyphs(page);
        assert!(!glyphs.is_empty());
        let first = glyphs[0];
        // First glyph: at the left margin, on the first line's baseline (72 + 12.83).
        assert!((first.x - g.margin_left).abs() < 1.0, "{first:?}");
        assert!((first.y - (g.margin_top + 12.83)).abs() < 0.01, "{first:?}");
        for gl in &glyphs {
            assert!(
                gl.x >= g.margin_left && gl.x <= g.width - g.margin_right,
                "{gl:?}"
            );
            assert!(
                gl.y > g.margin_top && gl.y <= g.height - g.margin_bottom,
                "{gl:?}"
            );
        }
    }
    assert!(p.placed_glyphs(pages).is_empty());
}
