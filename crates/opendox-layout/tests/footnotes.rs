//! Footnotes: each note at the foot of the page holding its reference, under one short rule,
//! the reference drawn as its number, the reference's line moved on when its note doesn't fit,
//! and carets and Up/Down inside the notes. Bundled fonts only.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_layout::{
    BlockInput, FieldKind, FontLibrary, LayoutEngine, NoteInput, ObjectInput, ObjectMode,
    PageGeometry, Paginator, ParagraphInput, ParagraphStyle, StyleSpan, TableInput, TextPosition,
    TextStyle, Zone, NOTE_SEPARATOR, OBJECT_PLACEHOLDER,
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

fn para<'a>(
    text: &'a str,
    base: &'a TextStyle,
    spans: &'a [StyleSpan],
    objects: &'a [ObjectInput],
) -> ParagraphInput<'a> {
    ParagraphInput {
        text,
        base,
        spans,
        style: ParagraphStyle::default(),
        objects,
        marker: None,
    }
}

fn reference(byte: usize, n: u32) -> ObjectInput {
    ObjectInput {
        byte,
        key: usize::MAX,
        width: 0.0,
        height: 0.0,
        rotation: 0.0,
        mode: ObjectMode::Field(FieldKind::Note(n)),
        x: 0.0,
        y: 0.0,
    }
}

const GEOMETRY: PageGeometry = PageGeometry {
    width: 612.0,
    height: 300.0,
    margin_top: 72.0,
    margin_bottom: 72.0,
    margin_left: 72.0,
    margin_right: 72.0,
    header_distance: 36.0,
    footer_distance: 36.0,
};

/// Bottom of the text area.
const BOTTOM: f32 = 300.0 - 72.0;

/// "Paragraph i …" lines, with a reference placeholder in each paragraph of `refs`.
fn body(n: usize, refs: &[usize]) -> Vec<String> {
    (0..n)
        .map(|i| {
            if refs.contains(&i) {
                format!("Paragraph {i} has a note{OBJECT_PLACEHOLDER} in it.")
            } else {
                format!("Paragraph {i} with some words.")
            }
        })
        .collect()
}

/// Byte offset of the reference placeholder in a paragraph from [`body`].
fn ref_byte(text: &str) -> usize {
    text.find(OBJECT_PLACEHOLDER).unwrap()
}

/// Lays out `texts` with one note per referencing paragraph; note `k` has `lines[k]`
/// paragraphs.
fn layout(texts: &[String], refs: &[usize], lines: &[usize]) -> Paginator {
    let mut e = engine();
    let b = base();
    let objects: Vec<Vec<ObjectInput>> = texts
        .iter()
        .map(|t| match t.find(OBJECT_PLACEHOLDER) {
            Some(byte) => {
                let k = refs
                    .iter()
                    .position(|&r| t.starts_with(&format!("Paragraph {r} ")));
                vec![reference(byte, k.map_or(0, |k| k as u32 + 1))]
            }
            None => Vec::new(),
        })
        .collect();
    let blocks: Vec<BlockInput> = texts
        .iter()
        .zip(&objects)
        .map(|(t, o)| BlockInput::Paragraph(para(t, &b, &[], o)))
        .collect();
    let note_text: Vec<String> = (0..lines.iter().copied().max().unwrap_or(0))
        .map(|i| format!("Note line {i}."))
        .collect();
    let notes: Vec<NoteInput> = refs
        .iter()
        .zip(lines)
        .map(|(&r, &n)| NoteInput {
            paragraph: r,
            byte: ref_byte(&texts[r]),
            paragraphs: note_text[..n]
                .iter()
                .map(|t| para(t, &b, &[], &[]))
                .collect(),
        })
        .collect();
    let mut p = Paginator::new(GEOMETRY);
    p.layout_with_notes(&mut e, &blocks, &[], &[], &notes);
    p
}

fn page_of(p: &Paginator, paragraph: usize) -> usize {
    p.placed_lines()
        .iter()
        .find(|l| l.paragraph == paragraph)
        .unwrap()
        .page
}

#[test]
fn a_note_sits_at_the_foot_of_its_references_page() {
    let texts = body(30, &[12]);
    let p = layout(&texts, &[12], &[1]);
    let note = texts.len();
    assert_eq!(p.note_paragraphs(), note..note + 1);
    let page = page_of(&p, 12);
    assert_eq!(page_of(&p, note), page);
    let line = p
        .placed_lines()
        .iter()
        .find(|l| l.paragraph == note)
        .unwrap();
    assert_eq!(line.zone, Zone::Footnote);
    assert!((line.bottom - BOTTOM).abs() < 0.01, "{line:?}");
    // One rule, above the note, starting at the margin and a third of the width long.
    let rules = &p.pages()[page].rules;
    assert_eq!(rules.len(), 1, "{rules:?}");
    let r = rules[0];
    assert!(r.y0 < line.top && r.y0 > line.top - NOTE_SEPARATOR, "{r:?}");
    assert_eq!(r.x0, 72.0);
    assert!((r.x1 - r.x0 - 468.0 / 3.0).abs() < 0.01);
    // The body on that page stops above the separator.
    let area = line.top - NOTE_SEPARATOR;
    for l in p
        .placed_lines()
        .iter()
        .filter(|l| l.page == page && l.zone == Zone::Body)
    {
        assert!(l.bottom <= area + 0.01, "{l:?} below {area}");
    }
    // No other page has notes or rules.
    for (n, pg) in p.pages().iter().enumerate().filter(|(n, _)| *n != page) {
        assert!(pg.rules.is_empty(), "page {n}");
    }
}

#[test]
fn the_reference_is_drawn_as_its_number_where_it_sits() {
    let texts = body(5, &[2]);
    let p = layout(&texts, &[2], &[1]);
    let labels = &p.pages()[0].labels;
    assert_eq!(labels.len(), 1, "{labels:?}");
    let l = &labels[0];
    assert_eq!(
        (l.paragraph, l.byte, l.text.as_str()),
        (2, ref_byte(&texts[2]), "1")
    );
    // At the reference's place in its line: after "note", before " in it."
    let line = p.placed_lines().iter().find(|l| l.paragraph == 2).unwrap();
    let para = p.paragraph(2).unwrap();
    let byte = ref_byte(&texts[2]);
    let x = line.left + para.lines[0].caret_x(byte);
    let gx = l.runs[0].glyphs[0].x;
    assert!((gx - x).abs() < 1.0, "label at {gx}, reference at {x}");
    // The reference takes room: the caret after it is further right by the number's width.
    let after = line.left + para.lines[0].caret_x(byte + OBJECT_PLACEHOLDER.len_utf8());
    assert!(after - x > 3.0, "{x}..{after}");
}

#[test]
fn a_reference_line_moves_on_when_its_note_does_not_fit() {
    // Without notes: the last body paragraph on page 0.
    let texts = body(30, &[]);
    let plain = layout(&texts, &[], &[]);
    let last = plain
        .placed_lines()
        .iter()
        .filter(|l| l.page == 0)
        .map(|l| l.paragraph)
        .max()
        .unwrap();
    // A three-line note referenced from that paragraph can't fit under it.
    let texts = body(30, &[last]);
    let p = layout(&texts, &[last], &[3]);
    assert_eq!(page_of(&p, last), 1, "the referencing line moved to page 1");
    assert_eq!(page_of(&p, texts.len()), 1, "and its note with it");
    assert!(p.pages()[0].rules.is_empty());
}

#[test]
fn notes_on_one_page_stack_in_order_under_one_rule() {
    let texts = body(6, &[1, 4]);
    let p = layout(&texts, &[1, 4], &[2, 1]);
    let n = texts.len();
    assert_eq!(p.note_paragraphs(), n..n + 3);
    let tops: Vec<f32> = (n..n + 3)
        .map(|i| {
            p.placed_lines()
                .iter()
                .find(|l| l.paragraph == i)
                .unwrap()
                .top
        })
        .collect();
    assert!(tops[0] < tops[1] && tops[1] < tops[2], "{tops:?}");
    assert_eq!(p.pages()[0].rules.len(), 1);
    let labels: Vec<&str> = p.pages()[0]
        .labels
        .iter()
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(labels, ["1", "2"]);
}

#[test]
fn carets_and_up_down_work_inside_notes() {
    let texts = body(6, &[1, 4]);
    let p = layout(&texts, &[1, 4], &[2, 1]);
    let n = texts.len();
    let at = |paragraph| TextPosition {
        paragraph,
        byte: 0,
        upstream: false,
    };
    let caret = p.caret_rect(at(n + 1)).unwrap();
    let mid = (caret.top + caret.bottom) / 2.0;
    let hit = p.hit_test(0, caret.x + 2.0, mid).unwrap();
    assert_eq!(hit.paragraph, n + 1, "a click in a note lands in it");
    // Down goes to the next note paragraph, never back into the body; Up from the first note
    // line stops.
    let down = p.move_vertical(at(n + 1), caret.x, true).unwrap();
    assert_eq!(down.paragraph, n + 2);
    assert_eq!(p.move_vertical(at(n), caret.x, false), None);
    assert_eq!(p.move_vertical(at(n + 2), caret.x, true), None);
}

#[test]
fn a_reference_in_a_table_cell_puts_its_note_on_the_tables_page() {
    let mut e = engine();
    let b = base();
    let cell = format!("Cell note{OBJECT_PLACEHOLDER}");
    let objects = [reference(ref_byte(&cell), 1)];
    let fill = body(3, &[]);
    let mut blocks: Vec<BlockInput> = fill
        .iter()
        .map(|t| BlockInput::Paragraph(para(t, &b, &[], &[])))
        .collect();
    blocks.push(BlockInput::Table(TableInput {
        rows: vec![vec![
            vec![para(&cell, &b, &[], &objects)],
            vec![para("x", &b, &[], &[])],
        ]],
    }));
    let note = [para("In a cell.", &b, &[], &[])];
    let notes = [NoteInput {
        paragraph: 3,
        byte: ref_byte(&cell),
        paragraphs: note.to_vec(),
    }];
    let mut p = Paginator::new(GEOMETRY);
    p.layout_with_notes(&mut e, &blocks, &[], &[], &notes);
    assert_eq!(p.note_paragraphs(), 5..6);
    assert_eq!(page_of(&p, 5), page_of(&p, 3));
    assert!(p.pages()[0]
        .labels
        .iter()
        .any(|l| l.paragraph == 3 && l.text == "1"));
}

#[test]
fn layout_with_notes_is_repeatable_and_cached() {
    let texts = body(30, &[3, 20]);
    let mut e = engine();
    let b = base();
    let objects: Vec<Vec<ObjectInput>> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| match i {
            3 => vec![reference(ref_byte(t), 1)],
            20 => vec![reference(ref_byte(t), 2)],
            _ => Vec::new(),
        })
        .collect();
    let blocks: Vec<BlockInput> = texts
        .iter()
        .zip(&objects)
        .map(|(t, o)| BlockInput::Paragraph(para(t, &b, &[], o)))
        .collect();
    let note = [para("A note.", &b, &[], &[])];
    let notes: Vec<NoteInput> = [3, 20]
        .iter()
        .map(|&r| NoteInput {
            paragraph: r,
            byte: ref_byte(&texts[r]),
            paragraphs: note.to_vec(),
        })
        .collect();
    let mut p = Paginator::new(GEOMETRY);
    let first = p
        .layout_with_notes(&mut e, &blocks, &[], &[], &notes)
        .to_vec();
    let second = p
        .layout_with_notes(&mut e, &blocks, &[], &[], &notes)
        .to_vec();
    assert_eq!(first, second);
    assert_eq!(p.last_shaped_count(), 0, "nothing reshaped");
}
