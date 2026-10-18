//! Tables: structure, editing by flat position, and convergence under concurrent changes.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{Block, Document, Outline, TextRef};
use proptest::prelude::*;

fn sync(a: &Document, b: &Document) {
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
}

/// Paragraphs as text, tables as a grid of cell texts — easy to compare in assertions.
#[derive(Debug, PartialEq)]
enum Shape {
    P(String),
    T(Vec<Vec<String>>),
}

fn shape(d: &Document) -> Vec<Shape> {
    let o = d.outline();
    o.blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(i) => Shape::P(o.slots[*i].text.clone()),
            Block::Table(t) => Shape::T(
                t.cells
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|c| {
                                o.slots[c.clone()]
                                    .iter()
                                    .map(|s| s.text.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .collect()
                    })
                    .collect(),
            ),
        })
        .collect()
}

fn p(s: &str) -> Shape {
    Shape::P(s.into())
}

fn t(rows: &[&[&str]]) -> Shape {
    Shape::T(
        rows.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect(),
    )
}

fn table_id(o: &Outline) -> String {
    o.tables().next().unwrap().id.clone()
}

#[test]
fn inserting_mid_paragraph_splits_it_around_the_table() {
    let d = Document::new();
    d.insert(0, "abcd").unwrap();
    d.insert_table(2, 2, 3).unwrap();
    assert_eq!(
        shape(&d),
        [p("ab"), t(&[&["", "", ""], &["", "", ""]]), p("cd")]
    );
}

#[test]
fn table_in_empty_document_leaves_a_paragraph_after_it() {
    let d = Document::new();
    d.insert_table(0, 1, 2).unwrap();
    assert_eq!(shape(&d), [t(&[&["", ""]]), p("")]);
}

#[test]
fn typing_in_cells_and_moving_between_them() {
    let d = Document::new();
    d.insert(0, "intro").unwrap();
    d.insert_table(5, 2, 2).unwrap();
    let o = d.outline();
    let id = table_id(&o);

    let c00 = o.cell_start(&id, 0, 0).unwrap();
    let after = d.insert_at(&o, c00, "Name").unwrap();
    assert_eq!(after, c00 + 4);

    let o = d.outline();
    let c01 = o.next_cell(after).unwrap();
    assert_eq!(c01, o.cell_start(&id, 0, 1).unwrap());
    d.insert_at(&o, c01, "Age").unwrap();

    let o = d.outline();
    let c11 = o.cell_start(&id, 1, 1).unwrap();
    d.insert_at(&o, c11, "42\nyears").unwrap(); // Enter inside a cell = second paragraph

    assert_eq!(
        shape(&d),
        [
            p("intro"),
            t(&[&["Name", "Age"], &["", "42\nyears"]]),
            p("")
        ]
    );
}

#[test]
fn rows_and_columns_can_be_added_and_removed() {
    let d = Document::new();
    d.insert_table(0, 2, 2).unwrap();
    let o = d.outline();
    let id = table_id(&o);
    d.insert_at(&o, o.cell_start(&id, 0, 0).unwrap(), "a")
        .unwrap();
    let o = d.outline();
    d.insert_at(&o, o.cell_start(&id, 1, 1).unwrap(), "d")
        .unwrap();

    d.insert_row(&id, 1).unwrap();
    assert_eq!(shape(&d)[0], t(&[&["a", ""], &["", ""], &["", "d"]]));

    d.insert_column(&id, 0).unwrap();
    assert_eq!(
        shape(&d)[0],
        t(&[&["", "a", ""], &["", "", ""], &["", "", "d"]])
    );

    d.delete_row(&id, 1).unwrap();
    d.delete_column(&id, 0).unwrap();
    assert_eq!(shape(&d)[0], t(&[&["a", ""], &["", "d"]]));

    d.delete_table(&id).unwrap();
    assert_eq!(shape(&d), [p("")]);
}

#[test]
fn deleting_across_cells_clears_them_but_keeps_the_table() {
    let d = Document::new();
    d.insert_table(0, 1, 3).unwrap();
    let o = d.outline();
    let id = table_id(&o);
    for (col, s) in ["one", "two", "three"].iter().enumerate() {
        let o = d.outline();
        d.insert_at(&o, o.cell_start(&id, 0, col).unwrap(), s)
            .unwrap();
    }
    let o = d.outline();
    // From "o|ne" to "th|ree".
    let from = o.cell_start(&id, 0, 0).unwrap() + 1;
    let to = o.cell_start(&id, 0, 2).unwrap() + 2;
    let caret = d.delete_range_at(&o, from, to).unwrap();
    assert_eq!(caret, from);
    assert_eq!(shape(&d)[0], t(&[&["o", "", "ree"]]));
}

#[test]
fn deleting_from_body_into_a_table_removes_the_whole_table() {
    let d = Document::new();
    d.insert(0, "before\nafter").unwrap();
    d.insert_table(7, 2, 2).unwrap();
    let o = d.outline();
    let id = table_id(&o);
    // From "bef|ore" into the second cell.
    let caret = d
        .delete_range_at(&o, 3, o.cell_start(&id, 0, 1).unwrap())
        .unwrap();
    assert_eq!(shape(&d), [p("bef"), p("after")]);
    assert_eq!(caret, 3);
}

#[test]
fn backspace_and_delete_stop_at_cell_and_table_edges() {
    let d = Document::new();
    d.insert(0, "ab\ncd").unwrap();
    d.insert_table(3, 1, 2).unwrap(); // between "ab" and "cd"
    let o = d.outline();
    let id = table_id(&o);
    let before = shape(&d);

    let c01 = o.cell_start(&id, 0, 1).unwrap();
    assert_eq!(
        d.delete_backward_at(&o, c01).unwrap(),
        c01,
        "start of a cell"
    );
    let cd = o.slots.last().unwrap().flat;
    assert_eq!(
        d.delete_backward_at(&o, cd).unwrap(),
        cd,
        "paragraph after a table"
    );
    assert_eq!(
        d.delete_forward_at(&o, 2).unwrap(),
        2,
        "paragraph before a table"
    );
    assert_eq!(shape(&d), before);

    // Inside one container, Backspace joins paragraphs as usual.
    let o = d.outline();
    d.insert_at(&o, o.cell_start(&id, 0, 0).unwrap(), "x\ny")
        .unwrap();
    let o = d.outline();
    let y = o.cell_start(&id, 0, 0).unwrap() + 2;
    d.delete_backward_at(&o, y).unwrap();
    assert_eq!(shape(&d)[1], t(&[&["xy", ""]]));
}

#[test]
fn text_typed_next_to_a_table_never_becomes_part_of_it() {
    let a = Document::with_replica_id(1).unwrap();
    let b = Document::with_replica_id(2).unwrap();
    a.insert_table(0, 1, 1).unwrap();
    sync(&a, &b);
    // B types right after the placeholder (body index 1), on the table's own line. It becomes
    // a paragraph after the table; the empty paragraph that was already there stays below it.
    b.insert_in(&TextRef::Body, 1, "x").unwrap();
    sync(&a, &b);
    assert_eq!(shape(&a), [t(&[&[""]]), p("x"), p("")]);
    assert_eq!(shape(&a), shape(&b));
}

#[test]
fn concurrent_row_and_column_inserts_line_up() {
    let a = Document::with_replica_id(1).unwrap();
    let b = Document::with_replica_id(2).unwrap();
    a.insert_table(0, 2, 2).unwrap();
    sync(&a, &b);
    let id = table_id(&a.outline());

    a.insert_column(&id, 2).unwrap(); // A adds a third column
    b.insert_row(&id, 2).unwrap(); // B adds a third row, not knowing about it
    let ob = b.outline();
    b.insert_at(&ob, ob.cell_start(&id, 2, 0).unwrap(), "new row")
        .unwrap();
    sync(&a, &b);

    assert_eq!(shape(&a), shape(&b));
    let grid = &shape(&a)[0];
    // Rectangular: B's row reads an empty cell for A's column.
    assert_eq!(
        *grid,
        t(&[&["", "", ""], &["", "", ""], &["new row", "", ""]])
    );

    // Typing into that cell creates it on demand.
    let o = a.outline();
    a.insert_at(&o, o.cell_start(&id, 2, 2).unwrap(), "filled")
        .unwrap();
    sync(&a, &b);
    assert_eq!(
        shape(&b)[0],
        t(&[&["", "", ""], &["", "", ""], &["new row", "", "filled"]])
    );
}

#[test]
fn deleting_a_table_wins_over_concurrent_cell_edits() {
    let a = Document::with_replica_id(1).unwrap();
    let b = Document::with_replica_id(2).unwrap();
    a.insert(0, "x").unwrap();
    a.insert_table(1, 1, 1).unwrap();
    sync(&a, &b);
    let id = table_id(&a.outline());
    a.delete_table(&id).unwrap();
    let ob = b.outline();
    b.insert_at(&ob, ob.cell_start(&id, 0, 0).unwrap(), "lost")
        .unwrap();
    sync(&a, &b);
    assert_eq!(shape(&a), shape(&b));
    assert!(a.outline().tables().next().is_none());
}

// --- property test ------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Type {
        r: usize,
        at: f64,
        s: usize,
    },
    DeleteRange {
        r: usize,
        a: f64,
        b: f64,
    },
    InsertTable {
        r: usize,
        at: f64,
        rows: usize,
        cols: usize,
    },
    AddRow {
        r: usize,
        t: f64,
        at: f64,
    },
    AddCol {
        r: usize,
        t: f64,
        at: f64,
    },
    DelRow {
        r: usize,
        t: f64,
        at: f64,
    },
    DelCol {
        r: usize,
        t: f64,
        at: f64,
    },
    DelTable {
        r: usize,
        t: f64,
    },
    Sync {
        from: usize,
        to: usize,
    },
}

const SNIPPETS: &[&str] = &["a", "word ", "\n", "é", "👨‍👩‍👧", "日本"];
const N: usize = 3;

fn op() -> impl Strategy<Value = Op> {
    let f = || 0.0..1.0f64;
    prop_oneof![
        4 => (0..N, f(), 0..SNIPPETS.len()).prop_map(|(r, at, s)| Op::Type { r, at, s }),
        1 => (0..N, f(), f()).prop_map(|(r, a, b)| Op::DeleteRange { r, a, b }),
        1 => (0..N, f(), 1..3usize, 1..3usize)
            .prop_map(|(r, at, rows, cols)| Op::InsertTable { r, at, rows, cols }),
        1 => (0..N, f(), f()).prop_map(|(r, t, at)| Op::AddRow { r, t, at }),
        1 => (0..N, f(), f()).prop_map(|(r, t, at)| Op::AddCol { r, t, at }),
        1 => (0..N, f(), f()).prop_map(|(r, t, at)| Op::DelRow { r, t, at }),
        1 => (0..N, f(), f()).prop_map(|(r, t, at)| Op::DelCol { r, t, at }),
        1 => (0..N, f()).prop_map(|(r, t)| Op::DelTable { r, t }),
        3 => (0..N, 0..N).prop_map(|(from, to)| Op::Sync { from, to }),
    ]
}

/// A real caret position (inside some paragraph), chosen by fraction.
fn caret(o: &Outline, frac: f64) -> usize {
    let mut all = Vec::new();
    for s in &o.slots {
        let mut off = s.flat;
        all.push(off);
        loop {
            let next = o.move_right(off);
            if next == off || next > s.flat_end() || next == s.flat_end() + 1 {
                break;
            }
            if o.slot_at(next).map(|(i, _)| &o.slots[i]) != Some(s) {
                break;
            }
            all.push(next);
            off = next;
        }
    }
    all[((all.len() - 1) as f64 * frac) as usize]
}

fn pick<T: Clone>(items: &[T], frac: f64) -> Option<T> {
    (!items.is_empty()).then(|| items[((items.len() - 1) as f64 * frac).round() as usize].clone())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn replicas_with_tables_converge(ops in prop::collection::vec(op(), 1..60)) {
        let docs: Vec<Document> =
            (0..N).map(|i| Document::with_replica_id(i as u64 + 1).unwrap()).collect();
        for op in ops {
            match op {
                Op::Type { r, at, s } => {
                    let o = docs[r].outline();
                    docs[r].insert_at(&o, caret(&o, at), SNIPPETS[s]).unwrap();
                }
                Op::DeleteRange { r, a, b } => {
                    let o = docs[r].outline();
                    docs[r].delete_range_at(&o, caret(&o, a), caret(&o, b)).unwrap();
                }
                Op::InsertTable { r, at, rows, cols } => {
                    let o = docs[r].outline();
                    let loc = o.locate(caret(&o, at));
                    if loc.container == TextRef::Body {
                        docs[r].insert_table(loc.offset, rows, cols).unwrap();
                    }
                }
                Op::AddRow { r, t, at } | Op::AddCol { r, t, at }
                | Op::DelRow { r, t, at } | Op::DelCol { r, t, at } => {
                    let o = docs[r].outline();
                    let tables: Vec<_> = o.tables().cloned().collect();
                    let Some(tb) = pick(&tables, t) else { continue };
                    let rows: Vec<usize> = (0..=tb.rows()).collect();
                    let cols: Vec<usize> = (0..=tb.columns).collect();
                    let d = &docs[r];
                    match op {
                        Op::AddRow { .. } => d.insert_row(&tb.id, pick(&rows, at).unwrap()).unwrap(),
                        Op::AddCol { .. } => d.insert_column(&tb.id, pick(&cols, at).unwrap()).unwrap(),
                        Op::DelRow { .. } if tb.rows() > 1 => {
                            d.delete_row(&tb.id, pick(&rows[..tb.rows()], at).unwrap()).unwrap()
                        }
                        Op::DelCol { .. } if tb.columns > 1 => {
                            d.delete_column(&tb.id, pick(&cols[..tb.columns], at).unwrap()).unwrap()
                        }
                        _ => {}
                    }
                }
                Op::DelTable { r, t } => {
                    let o = docs[r].outline();
                    let ids: Vec<String> = o.tables().map(|t| t.id.clone()).collect();
                    if let Some(id) = pick(&ids, t) {
                        docs[r].delete_table(&id).unwrap();
                    }
                }
                Op::Sync { from, to } => {
                    let bytes = docs[from].export_updates_since(&docs[to].version()).unwrap();
                    docs[to].import(&bytes).unwrap();
                }
            }
            // Every replica always has a well-formed outline: rectangular tables, valid slots.
            for d in &docs {
                let o = d.outline();
                for tb in o.tables() {
                    prop_assert!(tb.cells.iter().all(|r| r.len() == tb.columns));
                }
                prop_assert!(!o.slots.is_empty());
            }
        }
        for i in 0..N {
            for j in 0..N {
                if i != j {
                    sync(&docs[i], &docs[j]);
                }
            }
        }
        let expected = docs[0].outline();
        for d in &docs[1..] {
            prop_assert_eq!(&d.outline(), &expected);
        }
    }
}
