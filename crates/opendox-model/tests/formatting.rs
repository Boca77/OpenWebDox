//! Rich text: character marks, paragraph formats through splits and joins, tables,
//! convergence under concurrent formatting, and local undo/redo.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{
    Align, Document, FormatChange, History, Outline, ParagraphChange, ParagraphFormat,
};
use proptest::prelude::*;

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn sync(a: &Document, b: &Document) {
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
}

/// `(text, bold)` pieces of each paragraph, for compact assertions.
fn bold_pieces(d: &Document) -> Vec<Vec<(String, bool)>> {
    let o = d.outline();
    o.slots
        .iter()
        .zip(d.formats(&o))
        .map(|(slot, f)| {
            let chars: Vec<char> = slot.text.chars().collect();
            let mut out: Vec<(String, bool)> = Vec::new();
            for (i, c) in chars.iter().enumerate() {
                let b = f.runs.iter().any(|r| r.range.contains(&i) && r.format.bold);
                match out.last_mut() {
                    Some((s, lb)) if *lb == b => s.push(*c),
                    _ => out.push((c.to_string(), b)),
                }
            }
            out
        })
        .collect()
}

fn aligns(d: &Document) -> Vec<Align> {
    d.formats(&d.outline())
        .iter()
        .map(|f| f.paragraph.align)
        .collect()
}

fn centre(d: &Document, pos: usize) {
    let o = d.outline();
    d.set_paragraph_format_at(&o, pos, pos, ParagraphChange::Align(Align::Center))
        .unwrap();
}

#[test]
fn bold_marks_a_range_and_continues_when_typing_at_its_end() {
    let d = doc("hello world");
    let o = d.outline();
    d.format_at(&o, 0, 5, &FormatChange::Bold(true)).unwrap();
    assert_eq!(
        bold_pieces(&d),
        vec![vec![("hello".into(), true), (" world".into(), false)]]
    );

    d.insert(5, "!").unwrap(); // at the end of the bold run
    d.insert(7, "_").unwrap(); // inside plain text
    assert_eq!(
        bold_pieces(&d),
        vec![vec![("hello!".into(), true), (" _world".into(), false)]]
    );

    let o = d.outline();
    d.format_at(&o, 1, 3, &FormatChange::Bold(false)).unwrap();
    assert_eq!(
        bold_pieces(&d),
        vec![vec![
            ("h".into(), true),
            ("el".into(), false),
            ("lo!".into(), true),
            (" _world".into(), false)
        ]]
    );
}

#[test]
fn typing_at_a_paragraph_start_takes_the_format_after_it() {
    let d = doc("plain\nbold");
    let o = d.outline();
    d.format_at(&o, 6, 10, &FormatChange::Bold(true)).unwrap();
    d.insert(6, "very ").unwrap();
    assert_eq!(bold_pieces(&d)[1], vec![("very bold".to_string(), true)]);
}

#[test]
fn size_font_and_italic_are_separate_marks() {
    let d = doc("abcdef");
    let o = d.outline();
    d.format_at(&o, 0, 4, &FormatChange::Size(Some(18.0)))
        .unwrap();
    d.format_at(&o, 2, 6, &FormatChange::Font(Some("Noto Serif".into())))
        .unwrap();
    d.format_at(&o, 3, 4, &FormatChange::Italic(true)).unwrap();
    let f = d.formats(&o);
    let at = |i: usize| {
        f[0].runs
            .iter()
            .find(|r| r.range.contains(&i))
            .unwrap()
            .format
            .clone()
    };
    assert_eq!(at(0).size, Some(18.0));
    assert_eq!(at(0).font, None);
    assert_eq!(at(3).size, Some(18.0));
    assert_eq!(at(3).font.as_deref(), Some("Noto Serif"));
    assert!(at(3).italic && !at(2).italic);
    assert_eq!(at(5).size, None);

    d.format_at(&o, 0, 6, &FormatChange::Size(None)).unwrap();
    assert!(d.formats(&o)[0]
        .runs
        .iter()
        .all(|r| r.format.size.is_none()));
}

#[test]
fn char_format_at_caret_and_selection() {
    let d = doc("ab\ncd");
    let o = d.outline();
    d.format_at(&o, 0, 2, &FormatChange::Bold(true)).unwrap();
    assert!(d.char_format_at(&o, 2, 2).bold, "after a bold char");
    assert!(
        d.char_format_at(&o, 0, 0).bold,
        "paragraph start: the char after"
    );
    assert!(!d.char_format_at(&o, 3, 3).bold);
    assert!(d.char_format_at(&o, 0, 2).bold, "all bold");
    assert!(!d.char_format_at(&o, 1, 4).bold, "mixed");
}

#[test]
fn formatting_reaches_across_paragraphs_and_into_cells() {
    let d = doc("before after");
    let t = d.insert_table(6, 1, 2).unwrap();
    let o = d.outline();
    let c0 = o.cell_start(&t, 0, 0).unwrap();
    d.insert_at(&o, c0, "cell").unwrap();
    let o = d.outline();
    d.format_at(&o, 0, o.len, &FormatChange::Bold(true))
        .unwrap();
    for (i, pieces) in bold_pieces(&d).iter().enumerate() {
        assert!(pieces.iter().all(|(_, b)| *b), "paragraph {i}: {pieces:?}");
    }
    // The cell's text really is marked: read it back from a fresh replica.
    let other = Document::new();
    other.import(&d.export_snapshot().unwrap()).unwrap();
    assert_eq!(bold_pieces(&other), bold_pieces(&d));
}

#[test]
fn paragraph_format_lives_on_middle_and_last_paragraphs() {
    let d = doc("one\ntwo\nthree");
    centre(&d, 5);
    centre(&d, 10);
    assert_eq!(aligns(&d), vec![Align::Left, Align::Center, Align::Center]);
    let o = d.outline();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::SpaceAfter(12.0))
        .unwrap();
    d.set_paragraph_format_at(&o, 8, 8, ParagraphChange::LineSpacing(2.0))
        .unwrap();
    let f = d.formats(&o);
    assert_eq!(f[0].paragraph.space_after, 12.0);
    assert_eq!(f[2].paragraph.line_spacing, 2.0);
    assert_eq!(
        d.paragraph_format_at(&o, 9),
        ParagraphFormat {
            align: Align::Center,
            line_spacing: 2.0,
            ..ParagraphFormat::default()
        }
    );
    // Back to the default removes the stored value.
    d.set_paragraph_format_at(&o, 9, 9, ParagraphChange::Align(Align::Left))
        .unwrap();
    assert_eq!(aligns(&d)[2], Align::Left);
}

#[test]
fn enter_keeps_the_format_on_both_halves() {
    let d = doc("first\nsecond");
    centre(&d, 0);
    d.insert(2, "\n").unwrap(); // mid-paragraph
    assert_eq!(aligns(&d), vec![Align::Center, Align::Center, Align::Left]);

    centre(&d, 10); // the last paragraph, stored beside the body
    let end = d.len();
    d.insert(end, "\n").unwrap(); // Enter at the very end
    d.insert(d.len(), "x\ny").unwrap(); // a paste with a break
    assert_eq!(
        aligns(&d),
        vec![
            Align::Center,
            Align::Center,
            Align::Center,
            Align::Center,
            Align::Center
        ]
    );
}

#[test]
fn joining_paragraphs_keeps_the_first_ones_format() {
    let d = doc("left\ncentred\nright");
    centre(&d, 6);
    let o = d.outline();
    d.set_paragraph_format_at(&o, 14, 14, ParagraphChange::Align(Align::Right))
        .unwrap();
    // Backspace at the start of "centred": joins it onto "left".
    let o = d.outline();
    d.delete_backward_at(&o, 5).unwrap();
    assert_eq!(d.text(), "leftcentred\nright");
    assert_eq!(aligns(&d), vec![Align::Left, Align::Right]);
    // Delete at the end of "leftcentred": the merged paragraph is the last one, kept left.
    let o = d.outline();
    d.delete_forward_at(&o, 11).unwrap();
    assert_eq!(aligns(&d), vec![Align::Left]);
}

#[test]
fn cell_paragraphs_have_formats_too() {
    let d = doc("");
    let t = d.insert_table(0, 1, 2).unwrap();
    let o = d.outline();
    let c1 = o.cell_start(&t, 0, 1).unwrap();
    d.insert_at(&o, c1, "a\nb").unwrap();
    let o = d.outline();
    let c1 = o.cell_start(&t, 0, 1).unwrap();
    // Both the first paragraph (held by its line break) and the last (held beside the cell).
    d.set_paragraph_format_at(&o, c1, c1 + 2, ParagraphChange::Align(Align::Right))
        .unwrap();
    let f = d.formats(&o);
    let cell_slots: Vec<usize> = (0..o.slots.len())
        .filter(|&i| o.slots[i].flat >= c1 && o.slots[i].flat <= c1 + 3)
        .collect();
    assert_eq!(cell_slots.len(), 2);
    for i in cell_slots {
        assert_eq!(f[i].paragraph.align, Align::Right);
    }
    // Deleting that column leaves no format behind on the other cell.
    d.delete_column(&t, 1).unwrap();
    assert!(d
        .formats(&d.outline())
        .iter()
        .all(|f| f.paragraph.align == Align::Left));
}

#[test]
fn concurrent_formatting_merges() {
    let a = Document::with_replica_id(1).unwrap();
    let b = Document::with_replica_id(2).unwrap();
    a.insert(0, "shared text").unwrap();
    sync(&a, &b);
    let oa = a.outline();
    a.format_at(&oa, 0, 6, &FormatChange::Bold(true)).unwrap();
    b.insert(6, "INSERTED ").unwrap(); // at the end of A's bold range: not covered
    let ob = b.outline();
    b.set_paragraph_format_at(&ob, 0, 0, ParagraphChange::Align(Align::Center))
        .unwrap();
    sync(&a, &b);
    assert_eq!(bold_pieces(&a), bold_pieces(&b));
    assert_eq!(aligns(&a), vec![Align::Center]);
    assert_eq!(aligns(&b), vec![Align::Center]);
}

// --- undo ------------------------------------------------------------------------------------

#[test]
fn undo_and_redo_restore_text_and_selection() {
    let d = doc("");
    let mut h = History::new(&d);
    let o = d.outline();
    h.before_edit(&d, &o, 0, 0, true);
    d.insert(0, "hello").unwrap();
    let o = d.outline();
    h.before_edit(&d, &o, 1, 4, false);
    d.format_at(&o, 1, 4, &FormatChange::Bold(true)).unwrap();

    // Undo the bold: text stays, selection back to 1..4.
    let o = d.outline();
    let (a, f) = h.undo(&d, &o, 4, 4).unwrap().unwrap().unwrap();
    let o = d.outline();
    assert_eq!(
        (d.resolve_at(&o, &a), d.resolve_at(&o, &f)),
        (Some(1), Some(4))
    );
    assert!(bold_pieces(&d)[0].iter().all(|(_, b)| !b));
    assert_eq!(d.text(), "hello");

    // Undo the typing.
    h.undo(&d, &o, 1, 4).unwrap().unwrap();
    assert_eq!(d.text(), "");
    assert!(!h.can_undo());
    assert!(h.undo(&d, &d.outline(), 0, 0).unwrap().is_none());

    // Redo both.
    h.redo(&d, &d.outline(), 0, 0).unwrap().unwrap();
    assert_eq!(d.text(), "hello");
    h.redo(&d, &d.outline(), 0, 0).unwrap().unwrap();
    assert_eq!(bold_pieces(&d)[0][1], ("ell".into(), true));
    assert!(!h.can_redo());
}

#[test]
fn quick_typing_undoes_as_one_step_and_checkpoints_split_it() {
    let d = doc("");
    let mut h = History::new(&d);
    for (i, c) in "abc".chars().enumerate() {
        h.before_edit(&d, &d.outline(), i, i, true);
        d.insert(i, &c.to_string()).unwrap();
    }
    h.before_edit(&d, &d.outline(), 3, 3, false);
    d.insert(3, "d").unwrap();
    h.undo(&d, &d.outline(), 4, 4).unwrap();
    assert_eq!(d.text(), "abc");
    h.undo(&d, &d.outline(), 3, 3).unwrap();
    assert_eq!(d.text(), "");
}

#[test]
fn undo_only_reverts_local_edits() {
    let a = Document::with_replica_id(1).unwrap();
    let b = Document::with_replica_id(2).unwrap();
    let mut h = History::new(&a);
    a.insert(0, "mine").unwrap();
    sync(&a, &b);
    b.insert(4, " theirs").unwrap();
    b.insert(0, ">").unwrap();
    sync(&a, &b);
    h.undo(&a, &a.outline(), 0, 0).unwrap().unwrap();
    assert_eq!(a.text(), "> theirs");
    sync(&a, &b);
    assert_eq!(b.text(), "> theirs");
    h.redo(&a, &a.outline(), 0, 0).unwrap().unwrap();
    assert_eq!(a.text(), ">mine theirs");
}

#[test]
fn undo_reverts_table_commands_and_paragraph_formats() {
    let d = doc("text");
    let mut h = History::new(&d);
    h.before_edit(&d, &d.outline(), 4, 4, false);
    let t = d.insert_table(4, 2, 2).unwrap();
    h.before_edit(&d, &d.outline(), 4, 4, false);
    d.insert_row(&t, 0).unwrap();
    h.before_edit(&d, &d.outline(), 0, 0, false);
    centre(&d, 0);
    h.undo(&d, &d.outline(), 0, 0).unwrap();
    assert_eq!(aligns(&d)[0], Align::Left);
    h.undo(&d, &d.outline(), 0, 0).unwrap();
    assert_eq!(d.outline().table(&t).unwrap().rows(), 2);
    h.undo(&d, &d.outline(), 0, 0).unwrap();
    assert_eq!(d.outline().tables().count(), 0);
    assert_eq!(d.text(), "text");
}

// --- convergence -----------------------------------------------------------------------------

const REPLICAS: usize = 3;

#[derive(Debug, Clone)]
enum Op {
    Insert {
        replica: usize,
        at: f64,
        text: usize,
    },
    Delete {
        replica: usize,
        from: f64,
        to: f64,
    },
    Format {
        replica: usize,
        from: f64,
        to: f64,
        change: usize,
    },
    Paragraph {
        replica: usize,
        at: f64,
        change: usize,
    },
    Deliver {
        update: f64,
        to: usize,
    },
}

const TEXTS: &[&str] = &["a", "xyz", " ", "\n", "é", "👨‍👩‍👧", "p\nq"];

fn change(i: usize) -> FormatChange {
    match i {
        0 => FormatChange::Bold(true),
        1 => FormatChange::Bold(false),
        2 => FormatChange::Italic(true),
        3 => FormatChange::Size(Some(18.0)),
        4 => FormatChange::Size(None),
        _ => FormatChange::Font(Some("Noto Serif".into())),
    }
}

fn paragraph_change(i: usize) -> ParagraphChange {
    match i {
        0 => ParagraphChange::Align(Align::Center),
        1 => ParagraphChange::Align(Align::Left),
        2 => ParagraphChange::LineSpacing(1.5),
        _ => ParagraphChange::SpaceBefore(6.0),
    }
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..REPLICAS, 0.0..=1.0, 0..TEXTS.len())
            .prop_map(|(replica, at, text)| Op::Insert { replica, at, text }),
        1 => (0..REPLICAS, 0.0..=1.0, 0.0..=1.0)
            .prop_map(|(replica, from, to)| Op::Delete { replica, from, to }),
        2 => (0..REPLICAS, 0.0..=1.0, 0.0..=1.0, 0..6usize)
            .prop_map(|(replica, from, to, change)| Op::Format { replica, from, to, change }),
        1 => (0..REPLICAS, 0.0..=1.0, 0..4usize)
            .prop_map(|(replica, at, change)| Op::Paragraph { replica, at, change }),
        2 => (0.0..1.0, 0..REPLICAS).prop_map(|(update, to)| Op::Deliver { update, to }),
    ]
}

/// A caret position `frac` of the way through the document's caret positions.
fn caret_at(o: &Outline, frac: f64) -> usize {
    let mut all = vec![o.snap(0)];
    let mut p = 0;
    loop {
        let n = o.move_right(p);
        if n == p {
            break;
        }
        all.push(n);
        p = n;
    }
    all[((all.len() - 1) as f64 * frac).round() as usize]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn formats_converge(ops in prop::collection::vec(op(), 1..60)) {
        let docs: Vec<Document> = (0..REPLICAS)
            .map(|i| Document::with_replica_id(i as u64 + 1).unwrap())
            .collect();
        let mut updates: Vec<Vec<u8>> = Vec::new();
        for op in ops {
            let mut edit = |replica: usize, f: &dyn Fn(&Document, &Outline)| {
                let d = &docs[replica];
                let before = d.version();
                f(d, &d.outline());
                updates.push(d.export_updates_since(&before).unwrap());
            };
            match op {
                Op::Insert { replica, at, text } => edit(replica, &|d, o| {
                    d.insert_at(o, caret_at(o, at), TEXTS[text]).unwrap();
                }),
                Op::Delete { replica, from, to } => edit(replica, &|d, o| {
                    d.delete_range_at(o, caret_at(o, from), caret_at(o, to)).unwrap();
                }),
                Op::Format { replica, from, to, change: c } => edit(replica, &|d, o| {
                    d.format_at(o, caret_at(o, from), caret_at(o, to), &change(c)).unwrap();
                }),
                Op::Paragraph { replica, at, change: c } => edit(replica, &|d, o| {
                    let p = caret_at(o, at);
                    d.set_paragraph_format_at(o, p, p, paragraph_change(c)).unwrap();
                }),
                Op::Deliver { update, to } => {
                    if !updates.is_empty() {
                        let i = (updates.len() as f64 * update) as usize;
                        docs[to].import(&updates[i]).unwrap();
                    }
                }
            }
        }
        for d in &docs {
            for u in &updates {
                d.import(u).unwrap();
            }
        }
        let expected = docs[0].formats(&docs[0].outline());
        for d in &docs[1..] {
            prop_assert_eq!(d.text(), docs[0].text());
            prop_assert_eq!(d.formats(&d.outline()), expected.clone());
        }
    }
}
