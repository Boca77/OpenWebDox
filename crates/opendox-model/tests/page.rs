//! Page setup and page breaks: stored in the document, merged per value, validated; Ctrl+Enter
//! as one undoable change whose break stays with the right paragraph through later edits.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{Document, History, PageSetup};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn breaks(d: &Document) -> Vec<bool> {
    d.formats(&d.outline())
        .iter()
        .map(|f| f.paragraph.page_break_before)
        .collect()
}

#[test]
fn page_setup_defaults_to_letter_and_round_trips() {
    let d = doc("");
    assert_eq!(d.page_setup(), PageSetup::default());
    let a4 = PageSetup {
        width: 595.28,
        height: 841.89,
        margin_left: 54.0,
        ..PageSetup::default()
    };
    d.set_page_setup(a4).unwrap();
    let back = d.page_setup();
    assert!((back.width - 595.28).abs() < 0.01 && (back.height - 841.89).abs() < 0.01);
    assert_eq!(back.margin_left, 54.0);
}

#[test]
fn page_setup_without_room_for_text_is_refused() {
    let d = doc("");
    let bad = PageSetup {
        margin_left: 300.0,
        margin_right: 300.0,
        ..PageSetup::default()
    };
    assert!(d.set_page_setup(bad).is_err());
    assert_eq!(d.page_setup(), PageSetup::default());
}

#[test]
fn concurrent_margin_changes_both_survive() {
    let a = doc("x");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    a.set_page_setup(PageSetup {
        margin_left: 36.0,
        ..PageSetup::default()
    })
    .unwrap();
    b.set_page_setup(PageSetup {
        margin_top: 108.0,
        ..PageSetup::default()
    })
    .unwrap();
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    for d in [&a, &b] {
        let p = d.page_setup();
        assert_eq!((p.margin_left, p.margin_top), (36.0, 108.0));
    }
}

#[test]
fn ctrl_enter_breaks_before_the_second_half_in_one_undo_step() {
    let d = doc("before after");
    let mut h = History::new(&d);
    let o = d.outline();
    h.before_edit(&d, &o, 7, 7, false);
    let caret = d.insert_page_break_at(&o, 7).unwrap();
    assert_eq!(caret, 8);
    assert_eq!(d.text(), "before \nafter");
    assert_eq!(breaks(&d), [false, true]);
    h.undo(&d, &d.outline(), caret, caret).unwrap();
    assert_eq!(d.text(), "before after");
    assert_eq!(breaks(&d), [false]);
}

#[test]
fn enter_inside_a_paragraph_after_a_break_keeps_the_break_on_the_first_half() {
    let d = doc("one\ntwo three\nfour");
    let o = d.outline();
    // At the start of "two three": an empty paragraph stays behind, the break goes before it.
    d.insert_page_break_at(&o, 4).unwrap();
    assert_eq!(d.text(), "one\n\ntwo three\nfour");
    assert_eq!(breaks(&d), [false, false, true, false]);
    let o = d.outline();
    let start = o.slots[2].flat;
    d.insert_at(&o, start + 3, "\n").unwrap(); // "two" | " three"
    assert_eq!(breaks(&d), [false, false, true, false, false]);
}
