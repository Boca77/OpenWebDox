//! Lists: making paragraphs list items, numbering by level and list, Enter/join behaviour,
//! indenting, checklists, undo, and convergence under concurrent list edits.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{
    list_markers, Document, History, ListKind, Outline, ParagraphChange, MAX_LIST_LEVEL,
};
use proptest::prelude::*;

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn markers(d: &Document) -> Vec<Option<String>> {
    list_markers(&d.formats(&d.outline()))
}

fn m(s: &[&str]) -> Vec<Option<String>> {
    s.iter()
        .map(|s| (!s.is_empty()).then(|| (*s).to_owned()))
        .collect()
}

/// Flat position of the start of paragraph `i`.
fn para(d: &Document, i: usize) -> usize {
    d.outline().slots[i].flat
}

fn set_list(d: &Document, a: usize, b: usize, kind: Option<ListKind>) {
    let o = d.outline();
    d.set_list_at(&o, a, b, kind).unwrap();
}

fn indent(d: &Document, a: usize, b: usize, deeper: bool) {
    let o = d.outline();
    d.indent_at(&o, a, b, deeper).unwrap();
}

#[test]
fn bullets_over_a_selection_make_one_list() {
    let d = doc("one\ntwo\nthree\nafter");
    set_list(&d, 1, para(&d, 2) + 1, Some(ListKind::Bullet));
    assert_eq!(markers(&d), m(&["●", "●", "●", ""]));
    let f = d.formats(&d.outline());
    let ids: Vec<u64> = f[..3]
        .iter()
        .map(|f| f.paragraph.list.unwrap().id)
        .collect();
    assert!(ids.iter().all(|&i| i == ids[0] && i != 0));
}

#[test]
fn numbers_count_per_level_and_restart_below_a_shallower_item() {
    let d = doc("a\nb\nc\nd\ne\nf");
    set_list(&d, 0, d.len(), Some(ListKind::Number));
    // a, [b, c], d, [e, [f]]
    indent(&d, para(&d, 1), para(&d, 2), true);
    indent(&d, para(&d, 4), para(&d, 5), true);
    indent(&d, para(&d, 5), para(&d, 5), true);
    assert_eq!(markers(&d), m(&["1.", "a.", "b.", "2.", "a.", "i."]));
}

#[test]
fn bullets_change_shape_by_level() {
    let d = doc("a\nb\nc\nd");
    set_list(&d, 0, d.len(), Some(ListKind::Bullet));
    for (i, depth) in [(1, 1), (2, 2), (3, 3)] {
        for _ in 0..depth {
            indent(&d, para(&d, i), para(&d, i), true);
        }
    }
    assert_eq!(markers(&d), m(&["●", "○", "■", "●"]));
}

#[test]
fn enter_continues_the_list_and_join_keeps_the_first_item() {
    let d = doc("first\nsecond");
    set_list(&d, 0, d.len(), Some(ListKind::Number));
    let o = d.outline();
    d.insert_at(&o, 3, "\n").unwrap(); // "fir" | "st"
    assert_eq!(markers(&d), m(&["1.", "2.", "3."]));
    // Backspace at the start of "st" joins it back.
    let o = d.outline();
    d.delete_backward_at(&o, para(&d, 1)).unwrap();
    assert_eq!(d.text(), "first\nsecond");
    assert_eq!(markers(&d), m(&["1.", "2."]));
}

#[test]
fn separate_lists_number_separately_but_continue_across_plain_paragraphs() {
    let d = doc("a\nb\nplain\nc\nd");
    set_list(&d, 0, para(&d, 1), Some(ListKind::Number));
    // Right after a plain paragraph: a new list.
    set_list(&d, para(&d, 3), para(&d, 4), Some(ListKind::Number));
    assert_eq!(markers(&d), m(&["1.", "2.", "", "1.", "2."]));

    // Numbering applied directly after an item continues that list…
    let d = doc("a\nb\nc");
    set_list(&d, 0, para(&d, 1), Some(ListKind::Number));
    set_list(&d, para(&d, 2), para(&d, 2), Some(ListKind::Number));
    assert_eq!(markers(&d), m(&["1.", "2.", "3."]));

    // …and taking an item out of the middle leaves one list numbered around it.
    set_list(&d, para(&d, 1), para(&d, 1), None);
    assert_eq!(markers(&d), m(&["1.", "", "2."]));
}

#[test]
fn a_different_kind_after_a_list_starts_its_own_list() {
    let d = doc("a\nb");
    set_list(&d, 0, 0, Some(ListKind::Bullet));
    set_list(&d, para(&d, 1), para(&d, 1), Some(ListKind::Number));
    let f = d.formats(&d.outline());
    assert_ne!(
        f[0].paragraph.list.unwrap().id,
        f[1].paragraph.list.unwrap().id
    );
    assert_eq!(markers(&d), m(&["●", "1."]));
}

#[test]
fn outdenting_level_zero_leaves_the_list_and_plain_paragraphs_indent_by_steps() {
    let d = doc("item\nplain");
    set_list(&d, 0, 0, Some(ListKind::Bullet));
    indent(&d, 0, 0, false);
    assert_eq!(markers(&d), m(&["", ""]));

    let p = para(&d, 1);
    indent(&d, p, p, true);
    indent(&d, p, p, true);
    let o = d.outline();
    assert_eq!(d.paragraph_format_at(&o, p).indent_start, 72.0);
    indent(&d, p, p, false);
    indent(&d, p, p, false);
    indent(&d, p, p, false);
    assert_eq!(d.paragraph_format_at(&d.outline(), p).indent_start, 0.0);
}

#[test]
fn levels_stop_at_the_deepest() {
    let d = doc("x");
    set_list(&d, 0, 0, Some(ListKind::Bullet));
    for _ in 0..20 {
        indent(&d, 0, 0, true);
    }
    let l = d.paragraph_format_at(&d.outline(), 0).list.unwrap();
    assert_eq!(l.level, MAX_LIST_LEVEL);
}

#[test]
fn checklist_items_tick_and_keep_their_tick_through_enter() {
    let d = doc("milk\neggs");
    set_list(&d, 0, d.len(), Some(ListKind::Check));
    let o = d.outline();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Checked(true))
        .unwrap();
    assert_eq!(markers(&d), m(&["☑", "☐"]));
    // Ticks mean nothing on other kinds.
    set_list(&d, 0, 0, Some(ListKind::Bullet));
    let o = d.outline();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Checked(true))
        .unwrap();
    assert!(!d.paragraph_format_at(&d.outline(), 0).list.unwrap().checked);
}

#[test]
fn enter_after_a_ticked_item_starts_an_unticked_one() {
    let d = doc("done\nlast");
    set_list(&d, 0, d.len(), Some(ListKind::Check));
    for p in [0, para(&d, 1)] {
        let o = d.outline();
        d.set_paragraph_format_at(&o, p, p, ParagraphChange::Checked(true))
            .unwrap();
    }
    // At the end of a middle item and of the last one (whose format sits beside the body).
    let o = d.outline();
    d.insert_at(&o, 4, "\n").unwrap();
    let end = d.len();
    d.insert_at(&d.outline(), end, "\n").unwrap();
    assert_eq!(markers(&d), m(&["☑", "☐", "☑", "☐"]));
    // In the middle, both halves are the item that was ticked.
    let o = d.outline();
    d.insert_at(&o, 2, "\n").unwrap();
    assert_eq!(markers(&d)[..2], m(&["☑", "☑"])[..]);
}

#[test]
fn lists_work_in_table_cells_and_the_last_body_paragraph() {
    let d = doc("a\nb");
    d.insert_table(2, 1, 1).unwrap();
    let o = d.outline();
    // The cell's only paragraph and the body's last one keep formats beside their container.
    let cell = o
        .slots
        .iter()
        .position(|s| s.container != o.slots[0].container);
    let cell = o.slots[cell.unwrap()].flat;
    d.insert_at(&o, cell, "x\ny").unwrap();
    let cell = d.outline().slots[1].flat;
    set_list(&d, cell, cell + 3, Some(ListKind::Number));
    let last = d.outline().slots.last().unwrap().flat;
    set_list(&d, last, last, Some(ListKind::Bullet));
    let got = markers(&d);
    assert_eq!(got.last().unwrap().as_deref(), Some("●"));
    assert_eq!(got[1..3], m(&["1.", "2."])[..]);
}

#[test]
fn undo_removes_a_list_in_one_step() {
    let d = doc("a\nb");
    let mut h = History::new(&d);
    let o = d.outline();
    h.before_edit(&d, &o, 0, 3, false);
    d.set_list_at(&o, 0, 3, Some(ListKind::Number)).unwrap();
    assert_eq!(markers(&d), m(&["1.", "2."]));
    h.undo(&d, &d.outline(), 0, 3).unwrap();
    assert_eq!(markers(&d), m(&["", ""]));
}

#[test]
fn concurrent_toggles_on_different_items_both_survive() {
    let a = doc("one\ntwo\nthree");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    set_list(&a, 0, 0, Some(ListKind::Number));
    set_list(&b, para(&b, 2), para(&b, 2), Some(ListKind::Bullet));
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    assert_eq!(markers(&a), m(&["1.", "", "●"]));
    assert_eq!(markers(&a), markers(&b));
}

// --- convergence --------------------------------------------------------------------------

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
    List {
        replica: usize,
        from: f64,
        to: f64,
        kind: usize,
    },
    Indent {
        replica: usize,
        at: f64,
        deeper: bool,
    },
    Tick {
        replica: usize,
        at: f64,
    },
    Deliver {
        update: f64,
        to: usize,
    },
}

const TEXTS: &[&str] = &["a", "xyz", "\n", "é", "p\nq", "\n\n"];

fn kind(i: usize) -> Option<ListKind> {
    [
        None,
        Some(ListKind::Bullet),
        Some(ListKind::Number),
        Some(ListKind::Check),
    ][i]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..REPLICAS, 0.0..=1.0, 0..TEXTS.len())
            .prop_map(|(replica, at, text)| Op::Insert { replica, at, text }),
        1 => (0..REPLICAS, 0.0..=1.0, 0.0..=1.0)
            .prop_map(|(replica, from, to)| Op::Delete { replica, from, to }),
        2 => (0..REPLICAS, 0.0..=1.0, 0.0..=1.0, 0..4usize)
            .prop_map(|(replica, from, to, kind)| Op::List { replica, from, to, kind }),
        1 => (0..REPLICAS, 0.0..=1.0, any::<bool>())
            .prop_map(|(replica, at, deeper)| Op::Indent { replica, at, deeper }),
        1 => (0..REPLICAS, 0.0..=1.0).prop_map(|(replica, at)| Op::Tick { replica, at }),
        2 => (0.0..1.0, 0..REPLICAS).prop_map(|(update, to)| Op::Deliver { update, to }),
    ]
}

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
    fn lists_converge(ops in prop::collection::vec(op(), 1..60)) {
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
                Op::List { replica, from, to, kind: k } => edit(replica, &|d, o| {
                    d.set_list_at(o, caret_at(o, from), caret_at(o, to), kind(k)).unwrap();
                }),
                Op::Indent { replica, at, deeper } => edit(replica, &|d, o| {
                    let p = caret_at(o, at);
                    d.indent_at(o, p, p, deeper).unwrap();
                }),
                Op::Tick { replica, at } => edit(replica, &|d, o| {
                    let p = caret_at(o, at);
                    d.set_paragraph_format_at(o, p, p, ParagraphChange::Checked(true)).unwrap();
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
            let got = d.formats(&d.outline());
            prop_assert_eq!(list_markers(&got), list_markers(&expected));
            prop_assert_eq!(got, expected.clone());
        }
    }
}
