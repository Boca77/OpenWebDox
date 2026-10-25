//! Text colour, highlights, underlines, strikethroughs and baseline shifts: where layout puts
//! them, and that splitting a run by colour keeps glyphs, positions and text mapping intact.

use opendox_layout::{
    DecorationKind, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput,
    ParagraphLayout, ParagraphStyle, StyleSpan, TextStyle,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

fn engine() -> LayoutEngine {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    LayoutEngine::new(fonts)
}

fn base() -> TextStyle {
    TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        ..TextStyle::default()
    }
}

fn span(range: std::ops::Range<usize>, style: TextStyle) -> StyleSpan {
    StyleSpan {
        byte_range: range,
        style,
    }
}

fn lay(text: &str, spans: &[StyleSpan]) -> ParagraphLayout {
    engine().layout_paragraph(text, &base(), spans, 400.0)
}

const TEXT: &str = "plain colored plain";

#[test]
fn colour_splits_runs_without_moving_glyphs_or_their_text() {
    let plain = lay(TEXT, &[]);
    let red = span(
        6..13,
        TextStyle {
            color: Some(0xCC0000),
            ..base()
        },
    );
    let coloured = lay(TEXT, &[red]);
    let glyphs = |l: &ParagraphLayout| -> Vec<(u32, f32, std::ops::Range<usize>)> {
        l.lines[0]
            .runs
            .iter()
            .flat_map(|r| {
                r.glyphs
                    .iter()
                    .zip(&r.glyph_text)
                    .map(|(g, t)| (g.id, g.x, t.clone()))
            })
            .collect()
    };
    assert_eq!(glyphs(&plain), glyphs(&coloured));
    let runs = &coloured.lines[0].runs;
    assert_eq!(runs.len(), 3);
    assert_eq!(
        runs.iter().map(|r| r.color).collect::<Vec<_>>(),
        [None, Some(0xCC0000), None]
    );
    assert_eq!(runs[1].byte_range, 6..13);
    assert!(coloured.lines[0].decorations.is_empty());
}

#[test]
fn underline_sits_below_the_baseline_and_strikethrough_above_it() {
    let l = lay(
        TEXT,
        &[span(
            0..5,
            TextStyle {
                underline: true,
                strikethrough: true,
                ..base()
            },
        )],
    );
    let line = &l.lines[0];
    let find = |k: DecorationKind| *line.decorations.iter().find(|d| d.kind == k).unwrap();
    let u = find(DecorationKind::Underline);
    let s = find(DecorationKind::Strikethrough);
    assert!(u.y > line.baseline && u.y < line.bottom, "{u:?} {line:?}");
    assert!(s.y < line.baseline && s.y > line.top, "{s:?}");
    assert!(u.height >= 0.5 && s.height >= 0.5);
    // Both cover "plain" exactly: from its first glyph to the start of the next run.
    let next = line.runs[1].glyphs[0].x;
    assert!((u.x - line.runs[0].glyphs[0].x).abs() < 0.01);
    assert!((u.x + u.width - next).abs() < 0.01);
    assert_eq!(u.color, None);
}

#[test]
fn highlight_spans_the_text_height_and_keeps_its_colour() {
    let l = lay(
        TEXT,
        &[span(
            6..13,
            TextStyle {
                highlight: Some(0xFFFF00),
                ..base()
            },
        )],
    );
    let line = &l.lines[0];
    let h = line.decorations[0];
    assert_eq!(h.kind, DecorationKind::Highlight);
    assert_eq!(h.color, Some(0xFFFF00));
    assert!(h.y < line.baseline && h.y + h.height > line.baseline);
    assert!(h.y >= line.top - 0.01 && h.y + h.height <= line.bottom + 0.01);
}

#[test]
fn superscript_raises_glyphs_without_changing_carets() {
    let plain = lay("x2", &[]);
    let raised = TextStyle {
        font_size: 12.0,
        baseline_shift: 4.0,
        ..base()
    };
    let sup = lay("x2", &[span(1..2, raised)]);
    let y = |l: &ParagraphLayout, run: usize| l.lines[0].runs[run].glyphs[0].y;
    assert!((y(&sup, 1) - (y(&plain, 0) - 4.0)).abs() < 0.01);
    assert_eq!(y(&sup, 0), y(&plain, 0));
    assert_eq!(sup.lines[0].carets, plain.lines[0].carets);
}

#[test]
fn decorations_and_colours_reach_the_page() {
    let mut e = engine();
    let b = base();
    let spans = [span(
        0..5,
        TextStyle {
            underline: true,
            highlight: Some(0x00FF00),
            color: Some(0x0000FF),
            ..base()
        },
    )];
    let input = ParagraphInput {
        text: TEXT,
        base: &b,
        spans: &spans,
        style: ParagraphStyle::default(),
        objects: &[],
        marker: None,
    };
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut e, &[input]);
    let d = p.placed_decorations(0);
    assert_eq!(d.len(), 2);
    assert_eq!(
        d[0].kind,
        DecorationKind::Highlight,
        "highlights come first"
    );
    assert!(d[0].rect.x >= 72.0);
    let glyphs = p.placed_glyphs(0);
    assert_eq!(glyphs[0].color, Some(0x0000FF));
    assert_eq!(glyphs.last().unwrap().color, None);
}

#[test]
fn styling_changes_relayout_only_the_styled_paragraph() {
    let mut e = engine();
    let b = base();
    let red = [span(
        0..5,
        TextStyle {
            color: Some(0xFF0000),
            ..base()
        },
    )];
    let input = |spans: &'static [StyleSpan]| ParagraphInput {
        text: TEXT,
        base: &b,
        spans,
        style: ParagraphStyle::default(),
        objects: &[],
        marker: None,
    };
    let red: &'static [StyleSpan] = Box::leak(Box::new(red));
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut e, &[input(&[]), input(&[])]);
    p.layout(&mut e, &[input(red), input(&[])]);
    assert_eq!(p.last_shaped_count(), 1);
    assert_eq!(p.placed_glyphs(0)[0].color, Some(0xFF0000));
}
