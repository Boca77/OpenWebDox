//! Footnotes: a reference in body or cell text and the note's own region after everything
//! else, numbered by reference order; editing in notes, region bounds, deleting, undo,
//! anchors and sync.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{
    Align, Document, History, ModelError, ParagraphChange, Region, TextRef, IMAGE_PLACEHOLDER,
};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

/// Flat position of the start of note `k` (0-based, in number order).
fn note_start(d: &Document, k: usize) -> usize {
    let o = d.outline();
    o.slots[o.footnotes[k].slots.start].flat
}

fn note_text(d: &Document, k: usize) -> String {
    let o = d.outline();
    d.text_in(&TextRef::Footnote(o.footnotes[k].id.clone()))
        .unwrap()
}

#[test]
fn a_footnote_is_a_reference_and_a_region_after_the_body() {
    let d = doc("Hello world");
    let o = d.outline();
    let id = d.insert_footnote_at(&o, 5).unwrap();
    assert_eq!(d.text(), format!("Hello{IMAGE_PLACEHOLDER} world"));
    let o = d.outline();
    assert_eq!(o.footnotes.len(), 1);
    let n = &o.footnotes[0];
    assert_eq!((n.id.as_str(), n.reference), (id.as_str(), 5));
    assert_eq!(n.slots.len(), 1);
    let start = note_start(&d, 0);
    assert_eq!(start, o.len + 1);
    assert_eq!(o.region(start), Region::Footnote);
    assert_eq!(o.footnote(&id).unwrap().slots, n.slots);
    assert_eq!(o.footnote_at(start).unwrap().id, id);
    assert!(o.footnote_at(3).is_none());
    // Typing goes into the note's own text, Enter makes a second paragraph.
    d.insert_at(&o, start, "A note.\nMore.").unwrap();
    assert_eq!(note_text(&d, 0), "A note.\nMore.");
    assert_eq!(d.text(), format!("Hello{IMAGE_PLACEHOLDER} world"));
    assert_eq!(d.outline().footnotes[0].slots.len(), 2);
}

#[test]
fn notes_are_numbered_by_where_their_references_are() {
    let d = doc("one two three");
    let o = d.outline();
    let late = d.insert_footnote_at(&o, 13).unwrap();
    let o = d.outline();
    let early = d.insert_footnote_at(&o, 3).unwrap();
    let o = d.outline();
    let ids: Vec<&str> = o.footnotes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, [early.as_str(), late.as_str()]);
    assert_eq!(o.footnotes[0].reference, 3);
    assert_eq!(o.footnotes[1].reference, 14);
    // Each note's text follows the one before it.
    let a = note_start(&d, 0);
    d.insert_at(&o, a, "first").unwrap();
    assert_eq!(note_start(&d, 1), a + 6);
}

#[test]
fn a_footnote_can_be_referenced_from_a_table_cell() {
    let d = doc("ab");
    let t = d.insert_table(2, 1, 2).unwrap();
    let o = d.outline();
    let cell = o.cell_start(&t, 0, 1).unwrap();
    d.insert_at(&o, cell, "x").unwrap();
    let o = d.outline();
    d.insert_footnote_at(&o, cell + 1).unwrap();
    let o = d.outline();
    assert_eq!(o.footnotes.len(), 1);
    assert_eq!(o.footnotes[0].reference, cell + 1);
    assert_eq!(o.region(note_start(&d, 0)), Region::Footnote);
}

#[test]
fn footnotes_go_only_in_body_and_cell_text() {
    let d = doc("body");
    d.set_header(true).unwrap();
    let o = d.outline();
    let header = o.slots[o.header.start].flat;
    assert!(matches!(
        d.insert_footnote_at(&o, header),
        Err(ModelError::FootnoteOutsideBody)
    ));
    d.insert_footnote_at(&o, 2).unwrap();
    let o = d.outline();
    let note = note_start(&d, 0);
    assert!(matches!(
        d.insert_footnote_at(&o, note),
        Err(ModelError::FootnoteOutsideBody)
    ));
}

#[test]
fn editing_stays_inside_one_note() {
    let d = doc("ab cd");
    let o = d.outline();
    d.insert_footnote_at(&o, 2).unwrap();
    let o = d.outline();
    d.insert_footnote_at(&o, 6).unwrap();
    let o = d.outline();
    d.insert_at(&o, note_start(&d, 0), "one").unwrap();
    let o = d.outline();
    d.insert_at(&o, note_start(&d, 1), "two").unwrap();
    let o = d.outline();
    let (a, b) = (note_start(&d, 0), note_start(&d, 1));
    assert_eq!(
        o.region_bounds(a + 1),
        (a, a + 3),
        "select-all stays in the note"
    );
    // A selection from one note into the next deletes only the first note's part.
    d.delete_range_at(&o, a + 1, b + 2).unwrap();
    assert_eq!(
        (note_text(&d, 0), note_text(&d, 1)),
        ("o".into(), "two".into())
    );
    // Backspace at a note's start joins nothing.
    let o = d.outline();
    let b = note_start(&d, 1);
    d.delete_backward_at(&o, b).unwrap();
    assert_eq!(
        (note_text(&d, 0), note_text(&d, 1)),
        ("o".into(), "two".into())
    );
    // A note's paragraphs have formats like any others.
    let o = d.outline();
    d.set_paragraph_format_at(&o, b, b, ParagraphChange::Align(Align::Center))
        .unwrap();
    let o = d.outline();
    assert_eq!(d.paragraph_format_at(&o, b).align, Align::Center);
    assert_eq!(d.paragraph_format_at(&o, 0).align, Align::Left);
}

#[test]
fn deleting_the_reference_removes_the_note_and_undo_brings_it_back() {
    let d = doc("text");
    let mut h = History::new(&d);
    let o = d.outline();
    h.before_edit(&d, &o, 4, 4, false);
    let id = d.insert_footnote_at(&o, 4).unwrap();
    let o = d.outline();
    let start = note_start(&d, 0);
    h.before_edit(&d, &o, start, start, false);
    d.insert_at(&o, start, "kept").unwrap();
    let o = d.outline();
    h.before_edit(&d, &o, 5, 5, false);
    d.delete_backward_at(&o, 5).unwrap();
    assert_eq!(d.text(), "text");
    assert!(d.outline().footnotes.is_empty());
    h.undo(&d, &d.outline(), 4, 4).unwrap();
    let o = d.outline();
    assert_eq!(o.footnotes.len(), 1);
    assert_eq!(o.footnotes[0].id, id);
    assert_eq!(note_text(&d, 0), "kept");
    // delete_footnote does the same from the note's id.
    d.delete_footnote(&o, &id).unwrap();
    assert_eq!(d.text(), "text");
    assert!(matches!(
        d.delete_footnote(&d.outline(), &id),
        Err(ModelError::NoSuchFootnote)
    ));
}

#[test]
fn anchors_follow_text_in_notes() {
    let d = doc("ab");
    let o = d.outline();
    d.insert_footnote_at(&o, 1).unwrap();
    let o = d.outline();
    let start = note_start(&d, 0);
    d.insert_at(&o, start, "note").unwrap();
    let o = d.outline();
    let anchor = d.anchor_at(&o, start + 2).unwrap();
    d.insert_at(&o, start, "a ").unwrap();
    let o = d.outline();
    assert_eq!(d.resolve_at(&o, &anchor), Some(start + 4));
}

#[test]
fn footnotes_sync_between_replicas() {
    let a = doc("shared");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    let o = a.outline();
    a.insert_footnote_at(&o, 6).unwrap();
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    // Both type into the note at once.
    let start = note_start(&a, 0);
    a.insert_at(&a.outline(), start, "A").unwrap();
    b.insert_at(&b.outline(), start, "B").unwrap();
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    assert_eq!(note_text(&a, 0), note_text(&b, 0));
    assert_eq!(note_text(&a, 0).len(), 2);
    assert_eq!(a.outline(), b.outline());
}
