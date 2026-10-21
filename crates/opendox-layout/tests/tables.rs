//! Table layout: cell geometry, borders, pagination by row, hit testing and arrow keys.

use opendox_layout::{
    BlockInput, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, ParagraphStyle,
    TableInput, TextPosition, TextStyle, CELL_PADDING,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
/// Noto Sans at 12pt: ascent + descent, exact to the thousandth.
const LINE_H: f32 = 16.344;
const MARGIN: f32 = 72.0;
const CONTENT: f32 = 468.0;

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
    }
}

fn para<'a>(text: &'a str, s: &'a TextStyle) -> ParagraphInput<'a> {
    ParagraphInput {
        text,
        base: s,
        spans: &[],
        style: ParagraphStyle::default(),
        objects: &[],
    }
}

/// `before`, a table of single-paragraph cells, `after`. Paragraph indices: 0 = before,
/// 1.. = cells row by row, last = after.
fn doc(before: &str, cells: &[&[&str]], after: &str) -> Paginator {
    let s = style();
    let table = TableInput {
        rows: cells
            .iter()
            .map(|r| r.iter().map(|t| vec![para(t, &s)]).collect())
            .collect(),
    };
    let blocks = [
        BlockInput::Paragraph(para(before, &s)),
        BlockInput::Table(table),
        BlockInput::Paragraph(para(after, &s)),
    ];
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout_blocks(&mut engine(), &blocks);
    p
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn pos(paragraph: usize, byte: usize) -> TextPosition {
    TextPosition {
        paragraph,
        byte,
        upstream: false,
    }
}

#[test]
fn cells_sit_in_equal_columns_with_padding() {
    let p = doc("Before", &[&["a", "b"], &["c", "d"]], "After");
    let lines = p.placed_lines();
    assert_eq!(lines.len(), 6);
    let row_h = LINE_H + 2.0 * CELL_PADDING;
    let col_w = CONTENT / 2.0;

    // Cell (0,1): second column, first row, just under "Before".
    let b = lines[2];
    assert_eq!(b.paragraph, 2);
    assert!(approx(b.left, MARGIN + col_w + CELL_PADDING), "{b:?}");
    assert!(approx(b.top, MARGIN + LINE_H + CELL_PADDING), "{b:?}");
    assert!(approx(b.region.width, col_w) && approx(b.region.height, row_h));

    // "After" starts right below the second row.
    let after = lines[5];
    assert!(
        approx(after.top, MARGIN + LINE_H + 2.0 * row_h),
        "{after:?}"
    );
    assert!(approx(after.left, MARGIN));

    // Borders: per row a top, a bottom and 3 column edges.
    let rules = &p.pages()[0].rules;
    assert_eq!(rules.len(), 2 * 5);
    assert!(rules
        .iter()
        .any(|r| approx(r.x0, MARGIN + col_w) && r.x0 == r.x1));

    // Glyphs of "b" are drawn inside its cell.
    let glyphs = p.placed_glyphs(0);
    let in_cell_b = glyphs
        .iter()
        .filter(|g| {
            g.x >= b.region.x && g.x < b.region.x + b.region.width && g.y < b.bottom && g.y > b.top
        })
        .count();
    assert_eq!(in_cell_b, 1);
}

#[test]
fn long_cell_text_wraps_and_grows_the_row() {
    let long = "This cell has enough text to wrap onto several lines inside its column.";
    let p = doc("", &[&[long, "short"], &["x", "y"]], "");
    let cell_lines = p.paragraph(1).unwrap().lines.len();
    assert!(cell_lines >= 2, "{cell_lines}");
    for line in &p.paragraph(1).unwrap().lines {
        assert!(
            line.advance <= CONTENT / 2.0 - 2.0 * CELL_PADDING + 5.0,
            "{line:?}"
        );
    }
    // Row 2 starts below the tallest cell of row 1.
    let row2 = p.placed_lines().iter().find(|l| l.paragraph == 3).unwrap();
    let expected = MARGIN + LINE_H + cell_lines as f32 * LINE_H + 2.0 * CELL_PADDING;
    assert!(approx(row2.region.y, expected), "{row2:?}");
}

#[test]
fn clicks_anywhere_in_a_cell_go_to_that_cell() {
    let long = "This cell has enough text to wrap onto several lines inside its column.";
    let p = doc("", &[&[long, "short"]], "");
    let short = p
        .placed_lines()
        .iter()
        .find(|l| l.paragraph == 2)
        .copied()
        .unwrap();
    // Below "short"'s only line but still inside its (tall) cell.
    let hit = p
        .hit_test(
            0,
            short.left + 10.0,
            short.region.y + short.region.height - 2.0,
        )
        .unwrap();
    assert_eq!(hit.paragraph, 2);
    // In the padding left of the text.
    let hit = p
        .hit_test(0, short.region.x + 1.0, short.top + 3.0)
        .unwrap();
    assert_eq!((hit.paragraph, hit.byte), (2, 0));
}

#[test]
fn arrow_keys_move_through_table_columns() {
    let long = "This cell has enough text to wrap onto several lines inside its column.";
    let p = doc("Before", &[&["a", long], &["c", "d"]], "After");
    let col2_x = MARGIN + CONTENT * 0.75;

    // Down from "Before" under column 2 enters cell (0,1), first line, under the goal column.
    let down = p.move_vertical(pos(0, 0), col2_x, true).unwrap();
    assert_eq!(down.paragraph, 2);
    let rect = p.caret_rect(down).unwrap();
    assert!((rect.x - col2_x).abs() < 6.0, "{rect:?}");
    assert_eq!(
        p.placed_lines()
            .iter()
            .position(|l| l.paragraph == 2 && approx(l.top, rect.top)),
        Some(2)
    );

    // Down through the wrapped cell stays in it, then goes to (1,1) — not to cell (1,0), and
    // not into the short cell (0,0) beside it.
    let mut at = down;
    let lines = p.paragraph(2).unwrap().lines.len();
    for _ in 1..lines {
        at = p.move_vertical(at, col2_x, true).unwrap();
        assert_eq!(at.paragraph, 2);
    }
    at = p.move_vertical(at, col2_x, true).unwrap();
    assert_eq!(at.paragraph, 4, "cell (1,1)");
    at = p.move_vertical(at, col2_x, true).unwrap();
    assert_eq!(at.paragraph, 5, "paragraph after the table");

    // Up from "After" under column 1 lands in cell (1,0).
    let up = p.move_vertical(pos(5, 0), MARGIN + 10.0, false).unwrap();
    assert_eq!(up.paragraph, 3);
}

#[test]
fn rows_move_whole_to_the_next_page() {
    let s = style();
    let filler: Vec<String> = (0..37).map(|i| format!("Line {i}")).collect();
    let mut blocks: Vec<BlockInput> = filler
        .iter()
        .map(|t| BlockInput::Paragraph(para(t, &s)))
        .collect();
    // 37 lines use 604.6pt of 648; a 26.34pt row fits, the second doesn't.
    blocks.push(BlockInput::Table(TableInput {
        rows: vec![
            vec![vec![para("r1", &s)], vec![para("r1", &s)]],
            vec![vec![para("r2", &s)], vec![para("r2", &s)]],
        ],
    }));
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout_blocks(&mut engine(), &blocks).to_vec();
    assert_eq!(pages.len(), 2);
    let row2: Vec<_> = p
        .placed_lines()
        .iter()
        .filter(|l| l.paragraph >= 39)
        .collect();
    assert!(
        row2.iter().all(|l| l.page == 1),
        "second row moved together"
    );
    assert!(approx(row2[0].region.y, MARGIN));
    assert_eq!(pages[1].rules.len(), 5, "only row 2's borders on page 2");
}

#[test]
fn a_row_taller_than_the_page_splits_across_pages() {
    // The reported case: a short paragraph, then one row whose second cell holds far more
    // than a page of text. The row must start on page 1 and continue on page 2, never
    // crossing the bottom margin.
    let s = style();
    let tall: Vec<String> = (0..60)
        .map(|i| format!("Paragraph {i} in a tall cell"))
        .collect();
    let blocks = [
        BlockInput::Paragraph(para("nesto", &s)),
        BlockInput::Table(TableInput {
            rows: vec![vec![
                vec![para("short", &s)],
                tall.iter().map(|t| para(t, &s)).collect(),
            ]],
        }),
    ];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout_blocks(&mut engine(), &blocks).to_vec();
    assert_eq!(pages.len(), 2);
    let bottom = MARGIN + 648.0;
    let lines = p.placed_lines();

    // The table starts right under "nesto" on page 1.
    let first_cell = lines.iter().find(|l| l.paragraph == 1).unwrap();
    assert_eq!(first_cell.page, 0);
    assert!(approx(first_cell.region.y, MARGIN + LINE_H));

    // Nothing below the bottom margin, on either page; borders included.
    for l in lines {
        assert!(l.bottom <= bottom + 0.01, "{l:?}");
        assert!(l.region.y + l.region.height <= bottom + 0.01, "{l:?}");
    }
    for page in &pages {
        assert!(!page.rules.is_empty());
        assert!(page.rules.iter().all(|r| r.y0.max(r.y1) <= bottom + 0.01));
    }
    // Like Google Docs, the piece that continues reaches exactly to the bottom margin.
    let lowest = pages[0]
        .rules
        .iter()
        .map(|r| r.y0.max(r.y1))
        .fold(0.0, f32::max);
    assert!(approx(lowest, bottom), "{lowest}");

    // Every line of the tall cell is placed once, in order, and it continues at the top of
    // page 2.
    let tall_lines: Vec<_> = lines.iter().filter(|l| l.paragraph >= 2).collect();
    assert_eq!(tall_lines.len(), 60);
    let order: Vec<usize> = tall_lines.iter().map(|l| l.paragraph).collect();
    assert_eq!(order, (2..62).collect::<Vec<_>>());
    let first_on_page_2 = tall_lines.iter().find(|l| l.page == 1).unwrap();
    assert!(approx(first_on_page_2.region.y, MARGIN));
    assert!(
        tall_lines.iter().filter(|l| l.page == 0).count() > 30,
        "page 1 is used, not skipped"
    );

    // Arrow Down crosses the page break inside the cell.
    let last_on_page_1 = tall_lines.iter().rev().find(|l| l.page == 0).unwrap();
    let from = pos(last_on_page_1.paragraph, 0);
    let to = p
        .move_vertical(from, last_on_page_1.left + 1.0, true)
        .unwrap();
    assert_eq!(to.paragraph, first_on_page_2.paragraph);

    // Glyphs on page 2 are inside the page margins.
    for g in p.placed_glyphs(1) {
        assert!(g.y > MARGIN && g.y <= bottom, "{g:?}");
    }
}

#[test]
fn a_long_paragraph_inside_one_cell_splits_between_its_lines() {
    let s = style();
    let long = "word ".repeat(1500); // one paragraph, many wrapped lines
    let blocks = [BlockInput::Table(TableInput {
        rows: vec![vec![vec![para(&long, &s)]]],
    })];
    let mut p = Paginator::new(PageGeometry::letter());
    let pages = p.layout_blocks(&mut engine(), &blocks).to_vec();
    assert!(pages.len() >= 2);
    let n = p.paragraph(0).unwrap().lines.len();
    let placed: Vec<usize> = p.placed_lines().iter().map(|l| l.line).collect();
    assert_eq!(placed, (0..n).collect::<Vec<_>>());
    // One fragment per page for the paragraph, covering consecutive line ranges.
    let ranges: Vec<_> = pages
        .iter()
        .flat_map(|pg| pg.fragments.iter().map(|f| f.lines.clone()))
        .collect();
    assert_eq!(ranges.len(), pages.len());
    assert_eq!(ranges.first().unwrap().start, 0);
    assert_eq!(ranges.last().unwrap().end, n);
    for w in ranges.windows(2) {
        assert_eq!(w[0].end, w[1].start);
    }
}

#[test]
fn editing_a_cell_reshapes_only_that_cell() {
    let s = style();
    let mut eng = engine();
    let mut texts = ["one".to_string(), "two".to_string(), "three".to_string()];
    let mut p = Paginator::new(PageGeometry::letter());
    for (expected, edit) in [(3, false), (1, true)] {
        if edit {
            texts[1].push_str(" edited");
        }
        let blocks = [BlockInput::Table(TableInput {
            rows: vec![texts.iter().map(|t| vec![para(t, &s)]).collect()],
        })];
        p.layout_blocks(&mut eng, &blocks);
        assert_eq!(p.last_shaped_count(), expected);
    }
}
