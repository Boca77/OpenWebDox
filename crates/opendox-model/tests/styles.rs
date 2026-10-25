//! Paragraph styles: setting them, Enter after a heading, joining, and the style table.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{Document, ParagraphChange, StyleName};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn styles(d: &Document) -> Vec<StyleName> {
    d.formats(&d.outline())
        .iter()
        .map(|f| f.paragraph.style)
        .collect()
}

fn set(d: &Document, pos: usize, s: StyleName) {
    let o = d.outline();
    d.set_paragraph_format_at(&o, pos, pos, ParagraphChange::Style(s))
        .unwrap();
}

const H1: StyleName = StyleName::Heading(1);
const N: StyleName = StyleName::Normal;

#[test]
fn enter_at_the_end_of_a_heading_starts_normal_text() {
    let d = doc("Intro\nbody");
    set(&d, 0, H1);
    let o = d.outline();
    d.insert_at(&o, 5, "\n").unwrap();
    assert_eq!(styles(&d), [H1, N, N]);
    // The last paragraph's format lives beside the body: the same holds there.
    let d = doc("Only");
    set(&d, 0, StyleName::Title);
    d.insert_at(&d.outline(), 4, "\n").unwrap();
    assert_eq!(styles(&d), [StyleName::Title, N]);
}

#[test]
fn enter_inside_a_heading_keeps_both_halves_headings() {
    let d = doc("Intro text");
    set(&d, 0, H1);
    d.insert_at(&d.outline(), 5, "\n").unwrap();
    assert_eq!(styles(&d), [H1, H1]);
}

#[test]
fn joining_keeps_the_first_paragraphs_style() {
    let d = doc("Head\nbody");
    set(&d, 0, H1);
    let o = d.outline();
    d.delete_backward_at(&o, 5).unwrap();
    assert_eq!(d.text(), "Headbody");
    assert_eq!(styles(&d), [H1]);
}

#[test]
fn styles_are_stored_by_name_and_converge() {
    let a = doc("one\ntwo");
    let b = Document::with_replica_id(2).unwrap();
    b.import(&a.export_snapshot().unwrap()).unwrap();
    set(&a, 0, StyleName::Heading(2));
    set(&b, 4, StyleName::Subtitle);
    b.import(&a.export_updates_since(&b.version()).unwrap())
        .unwrap();
    a.import(&b.export_updates_since(&a.version()).unwrap())
        .unwrap();
    assert_eq!(styles(&a), [StyleName::Heading(2), StyleName::Subtitle]);
    assert_eq!(styles(&a), styles(&b));
}

#[test]
fn the_style_table_matches_google_docs_defaults() {
    let h1 = StyleName::Heading(1).look();
    assert_eq!(
        (h1.size, h1.space_before, h1.space_after),
        (Some(20.0), 20.0, 6.0)
    );
    assert_eq!(StyleName::Title.look().size, Some(26.0));
    assert_eq!(StyleName::Subtitle.look().color, Some(0x666666));
    assert!(StyleName::Heading(6).look().italic);
    assert_eq!(N.look().size, None);
    assert_eq!(StyleName::Title.outline_level(), Some(0));
    assert_eq!(StyleName::Heading(3).outline_level(), Some(3));
    assert_eq!(StyleName::Subtitle.outline_level(), None);
}
