//! Headers and footers: placed on every page at their distances from the edges, page number
//! and page count fields filled in per page, carets and Up/Down on the page clicked, and the
//! body untouched by them. Bundled fonts only.

use opendox_layout::{
    Align, FieldKind, FontLibrary, LayoutEngine, ObjectInput, ObjectMode, PageGeometry, Paginator,
    ParagraphInput, ParagraphStyle, TextPosition, TextStyle, OBJECT_PLACEHOLDER,
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
        ..TextStyle::default()
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

fn field(byte: usize, kind: FieldKind) -> ObjectInput {
    ObjectInput {
        byte,
        key: 0,
        width: 0.0,
        height: 0.0,
        rotation: 0.0,
        mode: ObjectMode::Field(kind),
        x: 0.0,
        y: 0.0,
    }
}

/// "Page ￼ of ￼": the field placeholders at bytes 5 and 12.
fn footer_text() -> String {
    format!("Page {OBJECT_PLACEHOLDER} of {OBJECT_PLACEHOLDER}")
}

struct Doc {
    p: Paginator,
    body: Vec<String>,
}

/// Enough body text for `pages` pages of a short page, with a header and footer.
fn doc(paragraphs: usize) -> Doc {
    let mut e = engine();
    let b = base();
    let body: Vec<String> = (0..paragraphs)
        .map(|i| format!("Paragraph {i} with some words to fill the line."))
        .collect();
    let blocks: Vec<ParagraphInput> = body.iter().map(|t| para(t, &b, &[])).collect();
    let blocks: Vec<_> = blocks
        .into_iter()
        .map(opendox_layout::BlockInput::Paragraph)
        .collect();
    let footer = footer_text();
    let fields = [
        field(5, FieldKind::PageNumber),
        field(5 + 3 + 4, FieldKind::PageCount),
    ];
    let header = [para("Report header", &b, &[])];
    let footer = [para(&footer, &b, &fields)];
    let mut p = Paginator::new(PageGeometry {
        height: 300.0,
        ..PageGeometry::letter()
    });
    p.layout_document(&mut e, &blocks, &header, &footer);
    Doc { p, body }
}

#[test]
fn header_and_footer_are_on_every_page_at_their_distances() {
    let d = doc(40);
    let pages = d.p.pages().len();
    assert!(pages >= 3, "{pages}");
    let body = d.body.len();
    assert_eq!(d.p.header_paragraphs(), body..body + 1);
    assert_eq!(d.p.footer_paragraphs(), body + 1..body + 2);
    for (n, page) in d.p.pages().iter().enumerate() {
        let header = page.fragments.iter().find(|f| f.paragraph == body).unwrap();
        assert_eq!(header.top, 36.0, "page {n}");
        let footer = page
            .fragments
            .iter()
            .find(|f| f.paragraph == body + 1)
            .unwrap();
        let line =
            d.p.placed_lines()
                .iter()
                .find(|l| l.page == n && l.paragraph == body + 1)
                .unwrap();
        assert!((line.bottom - (300.0 - 36.0)).abs() < 0.01, "{line:?}");
        assert_eq!(footer.left, 72.0);
    }
}

#[test]
fn fields_show_each_pages_number_and_the_count() {
    let d = doc(40);
    let count = d.p.pages().len().to_string();
    for (n, page) in d.p.pages().iter().enumerate() {
        let texts: Vec<&str> = page.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, [(n + 1).to_string().as_str(), count.as_str()]);
        // Label glyphs are part of what's drawn on the page.
        let label_glyphs: usize = page
            .labels
            .iter()
            .flat_map(|l| &l.runs)
            .map(|r| r.glyphs.len())
            .sum();
        assert!(label_glyphs >= 2);
    }
}

#[test]
fn the_body_is_laid_out_as_without_a_header() {
    let d = doc(40);
    let mut e = engine();
    let b = base();
    let blocks: Vec<_> = d
        .body
        .iter()
        .map(|t| opendox_layout::BlockInput::Paragraph(para(t, &b, &[])))
        .collect();
    let mut plain = Paginator::new(*d.p.geometry());
    plain.layout_blocks(&mut e, &blocks);
    let body_lines = |p: &Paginator| -> Vec<(usize, usize, f32)> {
        p.placed_lines()
            .iter()
            .filter(|l| l.paragraph < d.body.len())
            .map(|l| (l.page, l.paragraph, l.top))
            .collect()
    };
    assert_eq!(body_lines(&d.p), body_lines(&plain));
}

#[test]
fn a_field_is_one_caret_step_and_carets_use_the_page_clicked() {
    let d = doc(40);
    let footer = d.body.len() + 1;
    let line = &d.p.paragraph(footer).unwrap().lines[0];
    let bytes: Vec<usize> = line.carets.iter().map(|c| c.byte).collect();
    // No caret inside the field's 3-byte placeholder.
    assert!(!bytes.contains(&6) && !bytes.contains(&7));
    assert!(bytes.contains(&5) && bytes.contains(&8));

    let pos = TextPosition {
        paragraph: footer,
        byte: 0,
        upstream: false,
    };
    assert_eq!(d.p.caret_rect(pos).unwrap().page, 0);
    d.p.set_margin_page(2);
    assert_eq!(d.p.caret_rect(pos).unwrap().page, 2);
    // A click in page 1's footer lands in the footer.
    let hit = d.p.hit_test(1, 80.0, 300.0 - 40.0).unwrap();
    assert_eq!(hit.paragraph, footer);
}

#[test]
fn up_and_down_stay_in_the_header() {
    let d = doc(40);
    let header = d.body.len();
    d.p.set_margin_page(1);
    let pos = TextPosition {
        paragraph: header,
        byte: 3,
        upstream: false,
    };
    assert_eq!(d.p.move_vertical(pos, 100.0, true), None);
    assert_eq!(d.p.move_vertical(pos, 100.0, false), None);
}

#[test]
fn right_aligned_numbers_end_at_the_margin() {
    let mut e = engine();
    let b = base();
    let blocks: Vec<_> = (0..30)
        .map(|_| opendox_layout::BlockInput::Paragraph(para("x", &b, &[])))
        .collect();
    let text = OBJECT_PLACEHOLDER.to_string();
    let fields = [field(0, FieldKind::PageNumber)];
    let mut right = para(&text, &b, &fields);
    right.style = ParagraphStyle {
        align: Align::Right,
        ..ParagraphStyle::default()
    };
    let mut p = Paginator::new(PageGeometry {
        height: 200.0,
        ..PageGeometry::letter()
    });
    p.layout_document(&mut e, &blocks, &[], &[right]);
    assert!(p.pages().len() >= 10, "two-digit page numbers");
    let edge = 612.0 - 72.0;
    for page in p.pages() {
        let last = page.labels[0]
            .runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .map(|g| g.x)
            .fold(0.0, f32::max);
        // The last digit starts within a digit's width of the right margin.
        assert!(last < edge && last > edge - 10.0, "{last}");
    }
}
