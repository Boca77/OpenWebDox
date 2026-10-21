//! Images: the placeholder in the text, the image record, moving and deleting, undo, and
//! convergence when two people change one image at once.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{
    Align, Document, History, ImageChange, ImageData, ParagraphChange, Recolor, Wrap,
    IMAGE_PLACEHOLDER,
};

fn doc(id: u64, text: &str) -> Document {
    let d = Document::with_replica_id(id).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn sync(a: &Document, b: &Document) {
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
}

fn picture() -> ImageData {
    ImageData {
        bytes: vec![0x89, b'P', b'N', b'G', 1, 2, 3],
        mime: "image/png".into(),
        px_width: 40,
        px_height: 20,
    }
}

fn insert(d: &Document, pos: usize) -> String {
    d.insert_image(&d.outline(), pos, &picture(), 80.0, 40.0)
        .unwrap()
}

#[test]
fn image_sits_in_its_paragraph_as_one_character() {
    let d = doc(1, "ab\ncd");
    let id = insert(&d, 1);
    let o = d.outline();
    assert_eq!(o.slots[0].text, format!("a{IMAGE_PLACEHOLDER}b"));
    assert_eq!(o.slots.len(), 2, "an image doesn't split its paragraph");
    let refs = d.image_refs(&o);
    assert_eq!(refs[0].len(), 1);
    assert_eq!(
        (refs[0][0].offset, refs[0][0].id.as_str()),
        (1, id.as_str())
    );
    assert!(refs[1].is_empty());
    assert_eq!(d.image_position(&o, &id), Some(1));

    let p = d.image_props(&id).unwrap();
    assert_eq!((p.width, p.height, p.wrap), (80.0, 40.0, Wrap::Inline));
    assert_eq!(d.image_data(&id).unwrap(), picture());
}

#[test]
fn paragraph_format_ignores_image_placeholders() {
    // The image character is the same one tables use; it must not end the paragraph.
    let d = doc(1, "ab\ncd");
    insert(&d, 1);
    let o = d.outline();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Align(Align::Center))
        .unwrap();
    let o = d.outline();
    assert_eq!(d.paragraph_format_at(&o, 0).align, Align::Center);
    assert_eq!(d.paragraph_format_at(&o, 3).align, Align::Center);
    assert_eq!(d.paragraph_format_at(&o, 4).align, Align::Left);
    let f = d.formats(&o);
    assert_eq!(f[0].paragraph.align, Align::Center);
    assert_eq!(f[1].paragraph.align, Align::Left);
}

#[test]
fn pasted_placeholders_are_dropped() {
    let d = doc(1, "ab");
    let at = d
        .insert_at(&d.outline(), 1, &format!("x{IMAGE_PLACEHOLDER}y"))
        .unwrap();
    assert_eq!(d.text(), "axyb");
    assert_eq!(at, 3);
}

#[test]
fn properties_change_and_reach_other_replicas() {
    let a = doc(1, "text");
    let b = doc(2, "");
    let id = insert(&a, 4);
    sync(&a, &b);
    assert_eq!(b.image_data(&id).unwrap(), picture());

    a.set_image_many(
        &id,
        vec![
            ImageChange::Size(120.0, 60.0),
            ImageChange::Rotation(-90.0),
            ImageChange::Wrap(Wrap::Wrap),
            ImageChange::Offset(10.0, -5.0),
            ImageChange::Recolor(Recolor::Sepia),
            ImageChange::Brightness(150.0),
            ImageChange::Transparency(25.0),
            ImageChange::Alt("A rabbit".into()),
        ],
    )
    .unwrap();
    sync(&a, &b);
    let p = b.image_props(&id).unwrap();
    assert_eq!((p.width, p.height), (120.0, 60.0));
    assert_eq!(p.rotation, 270.0, "rotation is normalised");
    assert_eq!(p.wrap, Wrap::Wrap);
    assert_eq!((p.x, p.y), (10.0, -5.0));
    assert_eq!(p.recolor, Recolor::Sepia);
    assert_eq!(p.brightness, 100.0, "clamped");
    assert_eq!(p.transparency, 25.0);
    assert_eq!(p.alt, "A rabbit");
}

#[test]
fn concurrent_changes_to_different_properties_both_survive() {
    let a = doc(1, "text");
    let b = doc(2, "");
    let id = insert(&a, 0);
    sync(&a, &b);
    a.set_image(&id, ImageChange::Size(200.0, 100.0)).unwrap();
    b.set_image(&id, ImageChange::Wrap(Wrap::Behind)).unwrap();
    b.set_image(&id, ImageChange::Recolor(Recolor::Grayscale))
        .unwrap();
    // Same property on both: one value wins, the same everywhere.
    a.set_image(&id, ImageChange::Rotation(10.0)).unwrap();
    b.set_image(&id, ImageChange::Rotation(20.0)).unwrap();
    sync(&a, &b);
    let (pa, pb) = (a.image_props(&id).unwrap(), b.image_props(&id).unwrap());
    assert_eq!(pa, pb);
    assert_eq!(
        (pa.width, pa.wrap, pa.recolor),
        (200.0, Wrap::Behind, Recolor::Grayscale)
    );
    assert!(pa.rotation == 10.0 || pa.rotation == 20.0);
}

#[test]
fn typing_around_an_image_concurrently_converges() {
    let a = doc(1, "hello world");
    let b = doc(2, "");
    sync(&a, &b);
    let id = insert(&a, 5);
    b.insert(5, ",").unwrap();
    b.insert(0, ">").unwrap();
    sync(&a, &b);
    assert_eq!(a.text(), b.text());
    let o = b.outline();
    let at = b.image_position(&o, &id).unwrap();
    assert_eq!(o.slice(at - 5, at), "hello");
}

#[test]
fn move_within_and_across_paragraphs() {
    let d = doc(1, "abc\ndef");
    let id = insert(&d, 1);
    // Later in the same paragraph: "a[img]bc" -> "ab[img]c" (positions per the old outline).
    d.move_image(&d.outline(), &id, 3, vec![]).unwrap();
    assert_eq!(d.outline().slots[0].text, format!("ab{IMAGE_PLACEHOLDER}c"));
    // Into the next paragraph, changing wrap in the same edit.
    let o = d.outline();
    let to = o.slots[1].flat + 1;
    d.move_image(&o, &id, to, vec![ImageChange::Wrap(Wrap::Front)])
        .unwrap();
    let o = d.outline();
    assert_eq!(o.slots[0].text, "abc");
    assert_eq!(o.slots[1].text, format!("d{IMAGE_PLACEHOLDER}ef"));
    assert_eq!(d.image_props(&id).unwrap().wrap, Wrap::Front);
    // Earlier: back to the start of the document.
    d.move_image(&d.outline(), &id, 0, vec![]).unwrap();
    assert_eq!(d.outline().slots[0].text, format!("{IMAGE_PLACEHOLDER}abc"));
}

#[test]
fn delete_and_undo_bring_the_image_back() {
    let d = doc(1, "ab");
    let mut history = History::new(&d);
    let o = d.outline();
    history.before_edit(&d, &o, 1, 1, false);
    let id = d.insert_image(&o, 1, &picture(), 50.0, 50.0).unwrap();
    let o = d.outline();
    history.before_edit(&d, &o, 2, 2, false);
    d.set_image(&id, ImageChange::Wrap(Wrap::Wrap)).unwrap();
    let o = d.outline();
    history.before_edit(&d, &o, 2, 2, false);
    d.delete_image(&o, &id).unwrap();
    assert_eq!(d.text(), "ab");
    assert!(d.image_refs(&d.outline())[0].is_empty());

    history.undo(&d, &d.outline(), 1, 1).unwrap();
    let o = d.outline();
    assert_eq!(
        d.image_position(&o, &id),
        Some(1),
        "the image is back, mark and all"
    );
    assert_eq!(d.image_props(&id).unwrap().wrap, Wrap::Wrap);
    history.undo(&d, &o, 1, 1).unwrap();
    assert_eq!(d.image_props(&id).unwrap().wrap, Wrap::Inline);
}

#[test]
fn images_work_inside_table_cells() {
    let d = doc(1, "x");
    let t = d.insert_table(1, 1, 2).unwrap();
    let o = d.outline();
    let cell = o.cell_start(&t, 0, 1).unwrap();
    let id = insert(&d, cell);
    let o = d.outline();
    assert_eq!(d.image_position(&o, &id), Some(cell));
    let slot = o.slot_at(cell).unwrap().0;
    assert_eq!(d.image_refs(&o)[slot][0].id, id);
    d.delete_image(&o, &id).unwrap();
    assert_eq!(d.outline().slots[slot].text, "");
}
