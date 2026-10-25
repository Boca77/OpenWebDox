//! Underline, strikethrough, colours, superscript/subscript and links: storing them, how they
//! spread to typed text (links never do at their edges), and selections that mix them.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{CharFormat, Document, FormatChange, Script};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn format(d: &Document, a: usize, b: usize, change: FormatChange) {
    let o = d.outline();
    d.format_at(&o, a, b, &change).unwrap();
}

/// The format of char `i` of the first paragraph.
fn at(d: &Document, i: usize) -> CharFormat {
    let o = d.outline();
    d.char_format_at(&o, i, i + 1)
}

fn type_at(d: &Document, pos: usize, s: &str) {
    let o = d.outline();
    d.insert_at(&o, pos, s).unwrap();
}

#[test]
fn each_format_is_its_own_mark() {
    let d = doc("abcdef");
    format(&d, 0, 6, FormatChange::Underline(true));
    format(&d, 1, 3, FormatChange::Strike(true));
    format(&d, 2, 4, FormatChange::Color(Some("#cc0000".into())));
    format(&d, 3, 5, FormatChange::Highlight(Some("#ffff00".into())));
    format(&d, 4, 5, FormatChange::Script(Script::Super));
    format(&d, 5, 6, FormatChange::Script(Script::Sub));
    let f = at(&d, 3);
    assert!(f.underline && !f.strike);
    assert_eq!(f.color.as_deref(), Some("#cc0000"));
    assert_eq!(f.highlight.as_deref(), Some("#ffff00"));
    assert_eq!(at(&d, 4).script, Script::Super);
    assert_eq!(at(&d, 5).script, Script::Sub);
    assert_eq!(at(&d, 0).script, Script::Normal);

    // Removing one leaves the others.
    format(&d, 0, 6, FormatChange::Color(None));
    let f = at(&d, 3);
    assert_eq!(f.color, None);
    assert!(f.underline);
    assert_eq!(f.highlight.as_deref(), Some("#ffff00"));
}

#[test]
fn underline_and_colour_continue_when_typing_at_their_end() {
    let d = doc("ab cd");
    format(&d, 0, 2, FormatChange::Underline(true));
    format(&d, 0, 2, FormatChange::Color(Some("#cc0000".into())));
    type_at(&d, 2, "X");
    let f = at(&d, 2);
    assert!(f.underline);
    assert_eq!(f.color.as_deref(), Some("#cc0000"));
}

#[test]
fn links_never_grow_at_their_edges_but_take_text_typed_inside() {
    let d = doc("see docs here");
    let url = "https://example.org/docs";
    format(&d, 4, 8, FormatChange::Link(Some(url.into())));
    // After the link's end.
    type_at(&d, 8, "!");
    assert_eq!(at(&d, 8).link, None);
    // Inside it.
    type_at(&d, 6, "o");
    assert_eq!(at(&d, 6).link.as_deref(), Some(url));
    // Before its start.
    type_at(&d, 4, "[");
    assert_eq!(at(&d, 4).link, None);
    assert_eq!(at(&d, 5).link.as_deref(), Some(url));
}

#[test]
fn typing_at_a_paragraph_start_takes_the_next_format_but_not_its_link() {
    let d = doc("first\nlinked");
    format(
        &d,
        6,
        12,
        FormatChange::Link(Some("https://example.org".into())),
    );
    format(&d, 6, 12, FormatChange::Underline(true));
    type_at(&d, 6, "A");
    let f = at(&d, 6);
    assert!(f.underline, "copies the following character's look");
    assert_eq!(f.link, None);
}

#[test]
fn a_selection_is_underlined_only_if_all_of_it_is() {
    let d = doc("abcd");
    format(&d, 0, 2, FormatChange::Underline(true));
    format(&d, 0, 4, FormatChange::Strike(true));
    let o = d.outline();
    let f = d.char_format_at(&o, 0, 4);
    assert!(!f.underline);
    assert!(f.strike);
    assert!(d.char_format_at(&o, 0, 2).underline);
}

#[test]
fn links_merge_from_two_replicas() {
    let a = doc("one two");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    format(
        &a,
        0,
        3,
        FormatChange::Link(Some("https://a.example".into())),
    );
    format(
        &b,
        4,
        7,
        FormatChange::Link(Some("https://b.example".into())),
    );
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    for d in [&a, &b] {
        assert_eq!(at(d, 1).link.as_deref(), Some("https://a.example"));
        assert_eq!(at(d, 5).link.as_deref(), Some("https://b.example"));
        assert_eq!(at(d, 3).link, None);
    }
}
