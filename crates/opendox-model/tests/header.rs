//! Headers and footers: their own regions after the body, editing in them, fields, region
//! boundaries for moving and deleting, anchors, removal, and sync.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{Document, FieldKind, Region, TextRef};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

/// Flat position of the start of the header (or footer).
fn start(d: &Document, footer: bool) -> usize {
    let o = d.outline();
    let r = if footer { o.footer } else { o.header };
    o.slots[r.start].flat
}

#[test]
fn a_header_is_a_region_after_the_body() {
    let d = doc("body");
    assert!(!d.has_header());
    assert!(d.outline().header.is_empty());
    d.set_header(true).unwrap();
    d.set_footer(true).unwrap();
    let o = d.outline();
    assert_eq!(o.len, 4);
    assert_eq!(o.header.len(), 1);
    assert_eq!(start(&d, false), 5);
    assert_eq!(start(&d, true), 6);
    assert_eq!(o.region(4), Region::Body);
    assert_eq!(o.region(5), Region::Header);
    assert_eq!(o.region(6), Region::Footer);
    // Typing goes into the header's own text.
    let h = start(&d, false);
    d.insert_at(&o, h, "Title\nsecond").unwrap();
    assert_eq!(d.text(), "body");
    assert_eq!(d.text_in(&TextRef::Header).unwrap(), "Title\nsecond");
    let o = d.outline();
    assert_eq!(o.header.len(), 2);
    assert_eq!(o.region(start(&d, true)), Region::Footer);
}

#[test]
fn moving_and_deleting_stay_in_their_region() {
    let d = doc("ab");
    d.set_header(true).unwrap();
    let o = d.outline();
    let h = start(&d, false);
    d.insert_at(&o, h, "hd").unwrap();
    let o = d.outline();
    assert_eq!(o.move_right(2), 2, "body end doesn't step into the header");
    assert_eq!(o.move_left(h), h, "header start doesn't step into the body");
    // A selection from the body into the header deletes only the body part.
    d.delete_range_at(&o, 1, h + 1).unwrap();
    assert_eq!(d.text(), "a");
    assert_eq!(d.text_in(&TextRef::Header).unwrap(), "hd");
    // Backspace at the header's start is a no-op.
    let o = d.outline();
    let h = start(&d, false);
    d.delete_backward_at(&o, h).unwrap();
    assert_eq!(d.text_in(&TextRef::Header).unwrap(), "hd");
    assert_eq!(d.text(), "a");
}

#[test]
fn fields_go_only_in_headers_and_footers() {
    let d = doc("body");
    d.set_footer(true).unwrap();
    let o = d.outline();
    assert!(d.insert_field_at(&o, 2, FieldKind::PageNumber).is_err());
    let f = start(&d, true);
    let after = d.insert_at(&o, f, "Page  of ").unwrap();
    let o = d.outline();
    let after_page = d.insert_field_at(&o, f + 5, FieldKind::PageNumber).unwrap();
    let o = d.outline();
    d.insert_field_at(&o, after + 1, FieldKind::PageCount)
        .unwrap();
    assert_eq!(after_page, f + 6);
    let o = d.outline();
    let formats = d.formats(&o);
    let fields = &formats[o.footer.start].fields;
    assert_eq!(fields.len(), 2);
    assert_eq!(
        (fields[0].offset, fields[0].kind),
        (5, FieldKind::PageNumber)
    );
    assert_eq!(fields[1].kind, FieldKind::PageCount);
    // A field is one character: Backspace removes it whole.
    d.delete_backward_at(&o, after_page).unwrap();
    let o = d.outline();
    assert_eq!(d.formats(&o)[o.footer.start].fields.len(), 1);
    // Typing just after a field isn't a field.
    assert!(d.formats(&o)[o.footer.start]
        .runs
        .iter()
        .all(|r| r.range.len() <= 1));
}

#[test]
fn removing_a_header_removes_its_text() {
    let d = doc("body");
    d.set_header(true).unwrap();
    let o = d.outline();
    d.insert_at(&o, start(&d, false), "draft").unwrap();
    d.set_header(false).unwrap();
    assert!(!d.has_header());
    assert!(d.outline().header.is_empty());
    d.set_header(true).unwrap();
    assert_eq!(d.text_in(&TextRef::Header).unwrap(), "");
}

#[test]
fn anchors_in_the_footer_follow_edits() {
    let d = doc("body");
    d.set_footer(true).unwrap();
    let o = d.outline();
    let f = start(&d, true);
    d.insert_at(&o, f, "footer").unwrap();
    let o = d.outline();
    let anchor = d.anchor_at(&o, f + 3).unwrap();
    // Text added before it, in the footer and in the body.
    d.insert_at(&o, f, "my ").unwrap();
    let o = d.outline();
    d.insert_at(&o, 0, "the ").unwrap();
    let o = d.outline();
    assert_eq!(d.resolve_at(&o, &anchor), Some(start(&d, true) + 6));
}

#[test]
fn header_text_and_the_header_itself_sync() {
    let a = doc("body");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    a.set_header(true).unwrap();
    let o = a.outline();
    a.insert_at(&o, start(&a, false), "From A").unwrap();
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    assert!(b.has_header());
    assert_eq!(b.text_in(&TextRef::Header).unwrap(), "From A");
}
