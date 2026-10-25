//! Golden tests for pagination, using only bundled fonts.

use opendox_layout::{
    FontLibrary, LayoutEngine, Page, PageGeometry, Paginator, ParagraphInput, ParagraphStyle,
    TextStyle,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

fn engine() -> LayoutEngine {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    LayoutEngine::new(fonts)
}

fn style(size: f32) -> TextStyle {
    TextStyle {
        font_family: "Noto Sans".into(),
        font_size: size,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    }
}

fn inputs<'a>(texts: &'a [String], base: &'a TextStyle) -> Vec<ParagraphInput<'a>> {
    texts
        .iter()
        .map(|t| ParagraphInput {
            text: t,
            base,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        })
        .collect()
}

fn lines_per_page(pages: &[Page]) -> Vec<usize> {
    pages
        .iter()
        .map(|p| p.fragments.iter().map(|f| f.lines.len()).sum())
        .collect()
}

#[test]
fn letter_geometry() {
    let g = PageGeometry::letter();
    assert_eq!((g.content_width(), g.content_height()), (468.0, 648.0));
}

#[test]
fn fills_pages_with_whole_lines() {
    // 12pt Noto Sans lines are 16.34pt; 648 / 16.34 = 39.6, so 39 lines fit on a page.
    let texts: Vec<String> = (0..100).map(|i| format!("Paragraph {i}")).collect();
    let base = style(12.0);
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &inputs(&texts, &base));
    assert_eq!(lines_per_page(pages), [39, 39, 22]);

    // First fragment of every page starts at the top margin.
    for page in pages {
        assert_eq!(page.fragments[0].top, 72.0);
    }
}

#[test]
fn splits_a_long_paragraph_across_pages() {
    let base = style(12.0);
    let texts = vec![
        "Intro.".to_string(),
        "word ".repeat(2000), // far more than one page
        "Outro.".to_string(),
    ];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &inputs(&texts, &base)).to_vec();
    assert!(pages.len() >= 3, "{}", pages.len());

    // The long paragraph's fragments cover all its lines, in order, without overlap.
    let total = p.paragraph(1).unwrap().lines.len();
    let mut next = 0;
    for page in &pages {
        for f in page.fragments.iter().filter(|f| f.paragraph == 1) {
            assert_eq!(f.lines.start, next);
            next = f.lines.end;
        }
    }
    assert_eq!(next, total);

    // Every page except the last is full: no room for one more line.
    let line_h = 16.34;
    for page in &pages[..pages.len() - 1] {
        let last = page.fragments.last().unwrap();
        let para = p.paragraph(last.paragraph).unwrap();
        let used =
            last.top + para.lines[last.lines.end - 1].bottom - para.lines[last.lines.start].top;
        assert!(used <= 72.0 + 648.0 + 0.01, "overflows: {used}");
        assert!(used + line_h > 72.0 + 648.0, "underfilled: {used}");
    }
}

#[test]
fn every_line_is_placed_exactly_once_in_order() {
    let base = style(12.0);
    let texts: Vec<String> = (0..60).map(|i| "text ".repeat(i * 7 % 50 + 1)).collect();
    let mut p = Paginator::new(PageGeometry::a4());
    let pages = p.layout(&mut engine(), &inputs(&texts, &base)).to_vec();
    let placed: Vec<(usize, usize)> = pages
        .iter()
        .flat_map(|page| &page.fragments)
        .flat_map(|f| f.lines.clone().map(move |l| (f.paragraph, l)))
        .collect();
    let expected: Vec<(usize, usize)> = (0..texts.len())
        .flat_map(|i| (0..p.paragraph(i).unwrap().lines.len()).map(move |l| (i, l)))
        .collect();
    assert_eq!(placed, expected);
}

#[test]
fn line_taller_than_page_gets_its_own_page() {
    let base = style(12.0);
    let huge = style(800.0);
    let texts = ["before".to_string(), "X".to_string(), "after".to_string()];
    let paragraphs = [
        ParagraphInput {
            text: &texts[0],
            base: &base,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        },
        ParagraphInput {
            text: &texts[1],
            base: &huge,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        },
        ParagraphInput {
            text: &texts[2],
            base: &base,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        },
    ];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &paragraphs);
    let per_page: Vec<Vec<usize>> = pages
        .iter()
        .map(|pg| pg.fragments.iter().map(|f| f.paragraph).collect())
        .collect();
    assert_eq!(per_page, [vec![0], vec![1], vec![2]]);
}

#[test]
fn empty_document_has_one_page() {
    let base = style(12.0);
    let texts = vec![String::new()];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut engine(), &inputs(&texts, &base));
    assert_eq!(pages.len(), 1);
    assert_eq!(lines_per_page(pages), [1]);
}

#[test]
fn editing_one_paragraph_reshapes_only_that_paragraph() {
    let base = style(12.0);
    let mut eng = engine();
    let mut texts: Vec<String> = (0..50).map(|i| format!("Paragraph number {i}")).collect();
    let mut p = Paginator::new(PageGeometry::letter());

    p.layout(&mut eng, &inputs(&texts, &base));
    assert_eq!(p.last_shaped_count(), 50);

    p.layout(&mut eng, &inputs(&texts, &base));
    assert_eq!(p.last_shaped_count(), 0, "nothing changed");

    texts[20].push_str(" edited");
    p.layout(&mut eng, &inputs(&texts, &base));
    assert_eq!(p.last_shaped_count(), 1);

    // Inserting a paragraph shifts every index after it, but only the new one is shaped.
    texts.insert(0, "New first paragraph".into());
    p.layout(&mut eng, &inputs(&texts, &base));
    assert_eq!(p.last_shaped_count(), 1);
}

#[test]
fn changing_page_width_reshapes_everything() {
    let base = style(12.0);
    let mut eng = engine();
    let texts: Vec<String> = (0..10).map(|i| format!("Paragraph {i}")).collect();
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut eng, &inputs(&texts, &base));
    p.set_geometry(PageGeometry::a4());
    p.layout(&mut eng, &inputs(&texts, &base));
    assert_eq!(p.last_shaped_count(), 10);
}

#[test]
fn page_break_before_starts_a_new_page_but_never_an_empty_one() {
    let mut eng = engine();
    let base = style(12.0);
    let texts: Vec<String> = ["one", "two", "three"].map(String::from).to_vec();
    let mut paras = inputs(&texts, &base);
    let broken = ParagraphStyle {
        page_break_before: true,
        ..ParagraphStyle::default()
    };
    paras[1].style = broken;
    // Already at the top of a page: no extra page.
    paras[0].style = broken;
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout(&mut eng, &paras);
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages
            .iter()
            .map(|pg| pg.fragments.iter().map(|f| f.paragraph).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![0], vec![1, 2]]
    );
    assert_eq!(pages[1].fragments[0].top, 72.0);
}
