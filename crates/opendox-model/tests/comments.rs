//! Comments: ranges that follow edits (typing at their edges stays outside), overlap,
//! replies, resolving, deleting the commented text, and merging between replicas.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{Document, TextRef};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn add(d: &Document, a: usize, b: usize, text: &str) -> String {
    let o = d.outline();
    d.add_comment(&o, a, b, "Ada", text, 1.0).unwrap()
}

fn ranges(d: &Document) -> Vec<(usize, usize)> {
    d.comments(&d.outline())
        .iter()
        .map(|c| (c.start, c.end))
        .collect()
}

fn type_at(d: &Document, pos: usize, s: &str) {
    let o = d.outline();
    d.insert_at(&o, pos, s).unwrap();
}

#[test]
fn a_comment_covers_its_range_and_needs_text() {
    let d = doc("the quick fox");
    let o = d.outline();
    assert!(d.add_comment(&o, 4, 4, "Ada", "empty", 1.0).is_err());
    let id = add(&d, 4, 9, "which fox?");
    let c = &d.comments(&d.outline())[0];
    assert_eq!((c.id.as_str(), c.start, c.end), (id.as_str(), 4, 9));
    assert_eq!(
        (c.author.as_str(), c.text.as_str(), c.resolved),
        ("Ada", "which fox?", false)
    );
}

#[test]
fn typing_at_the_edges_stays_outside_and_inside_grows_it() {
    let d = doc("the quick fox");
    add(&d, 4, 9, "quick");
    type_at(&d, 9, "!"); // right after "quick"
    assert_eq!(ranges(&d), [(4, 9)]);
    type_at(&d, 4, ">"); // right before
    assert_eq!(ranges(&d), [(5, 10)]);
    type_at(&d, 7, "xx"); // inside
    assert_eq!(ranges(&d), [(5, 12)]);
    type_at(&d, 0, "so "); // before everything
    assert_eq!(ranges(&d), [(8, 15)]);
}

#[test]
fn deleting_the_commented_text_leaves_an_empty_comment() {
    let d = doc("keep this gone keep");
    add(&d, 10, 14, "gone?");
    // Deleting the last character shrinks it.
    let o = d.outline();
    d.delete_range_at(&o, 13, 14).unwrap();
    assert_eq!(ranges(&d), [(10, 13)]);
    // Deleting all of it leaves the comment, empty, where the text was.
    let o = d.outline();
    d.delete_range_at(&o, 10, 13).unwrap();
    assert_eq!(ranges(&d), [(10, 10)]);
    assert_eq!(d.comments(&d.outline()).len(), 1);
}

#[test]
fn comments_overlap_and_list_in_reading_order() {
    let d = doc("abcdefghij");
    add(&d, 5, 9, "second");
    add(&d, 2, 7, "first");
    let texts: Vec<String> = d
        .comments(&d.outline())
        .into_iter()
        .map(|c| c.text)
        .collect();
    assert_eq!(texts, ["first", "second"]);
}

#[test]
fn replies_resolving_editing_and_deleting() {
    let d = doc("text");
    let id = add(&d, 0, 4, "hm");
    d.reply_to_comment(&id, "Grace", "agreed", 2.0).unwrap();
    d.resolve_comment(&id, true).unwrap();
    let c = &d.comments(&d.outline())[0];
    assert!(c.resolved);
    assert_eq!(c.replies.len(), 1);
    assert_eq!(
        (c.replies[0].author.as_str(), c.replies[0].text.as_str()),
        ("Grace", "agreed")
    );
    // A reply reopens it.
    d.reply_to_comment(&id, "Ada", "one more thing", 3.0)
        .unwrap();
    assert!(!d.comments(&d.outline())[0].resolved);
    d.edit_comment(&id, "hmm").unwrap();
    assert_eq!(d.comments(&d.outline())[0].text, "hmm");
    d.delete_comment(&id).unwrap();
    assert!(d.comments(&d.outline()).is_empty());
    assert!(d.delete_comment(&id).is_err());
}

#[test]
fn comments_in_cells_and_the_header() {
    let d = doc("body");
    d.insert_table(4, 1, 1).unwrap();
    let o = d.outline();
    let cell = o
        .slots
        .iter()
        .find(|s| s.container != TextRef::Body)
        .unwrap()
        .flat;
    d.insert_at(&o, cell, "cell text").unwrap();
    add(&d, cell, cell + 4, "in a cell");
    d.set_header(true).unwrap();
    let o = d.outline();
    let h = o.slots[o.header.start].flat;
    d.insert_at(&o, h, "header").unwrap();
    add(&d, h, h + 6, "in the header");
    let h = {
        let o = d.outline();
        o.slots[o.header.start].flat
    };
    assert_eq!(ranges(&d), [(cell, cell + 4), (h, h + 6)]);
}

#[test]
fn comments_and_replies_from_two_people_merge() {
    let a = doc("shared words");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    let id = add(&a, 0, 6, "from A");
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    // Concurrently: B replies and adds another comment, A resolves its comment.
    b.reply_to_comment(&id, "B", "reply", 2.0).unwrap();
    let ob = b.outline();
    b.add_comment(&ob, 7, 12, "B", "from B", 3.0).unwrap();
    a.resolve_comment(&id, true).unwrap();
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    for d in [&a, &b] {
        let cs = d.comments(&d.outline());
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].replies.len(), 1);
        assert_eq!(cs[1].text, "from B");
    }
    // Resolved and replied at the same time: the reply's reopening and the resolve were
    // concurrent; both replicas agree on one outcome.
    assert_eq!(
        a.comments(&a.outline())[0].resolved,
        b.comments(&b.outline())[0].resolved
    );
}
