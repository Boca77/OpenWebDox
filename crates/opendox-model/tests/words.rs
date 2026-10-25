//! Word-wise movement and word ranges (Ctrl+Left/Right, double-click), by UAX #29 words.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::Document;

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

/// Every stop Ctrl+Right makes from 0 to the end.
fn rights(d: &Document) -> Vec<usize> {
    let o = d.outline();
    let mut out = Vec::new();
    let mut p = 0;
    loop {
        let n = o.word_right(p);
        if n == p {
            break;
        }
        out.push(n);
        p = n;
    }
    out
}

fn lefts(d: &Document) -> Vec<usize> {
    let o = d.outline();
    let mut out = Vec::new();
    let mut p = o.len;
    loop {
        let n = o.word_left(p);
        if n == p {
            break;
        }
        out.push(n);
        p = n;
    }
    out
}

#[test]
fn ctrl_right_stops_at_word_starts_then_the_paragraph_end() {
    // "one, two" | "three": word starts 0, 5; paragraph end 8; next paragraph 9, its end 14.
    let d = doc("one, two\nthree");
    assert_eq!(rights(&d), [5, 8, 9, 14]);
}

#[test]
fn ctrl_left_stops_at_word_starts_and_paragraph_edges() {
    let d = doc("one, two\nthree");
    assert_eq!(lefts(&d), [9, 8, 5, 0]);
}

#[test]
fn apostrophes_numbers_and_accents_stay_inside_words() {
    let d = doc("don't pay 3.50 for café");
    assert_eq!(rights(&d), [6, 10, 15, 19, 23]);
}

#[test]
fn double_click_takes_the_word_or_the_spaces() {
    let d = doc("hello   world");
    let o = d.outline();
    assert_eq!(o.word_at(2), 0..5);
    assert_eq!(o.word_at(6), 5..8);
    assert_eq!(o.word_at(13), 8..13, "at the end: the word before");
}

#[test]
fn emoji_and_cjk_words() {
    // Each Han character is its own word under UAX #29; the family emoji is one segment.
    let d = doc("你好 👨‍👩‍👧 ok");
    let o = d.outline();
    let fam = "你好 ".chars().count();
    assert_eq!(o.word_at(fam), fam..fam + 5);
    assert_eq!(o.word_right(0), 1);
}

#[test]
fn word_motion_stays_in_the_header() {
    let d = doc("body");
    d.set_header(true).unwrap();
    let o = d.outline();
    let h = o.slots[o.header.start].flat;
    d.insert_at(&o, h, "head text").unwrap();
    let o = d.outline();
    assert_eq!(o.word_left(h), h);
    assert_eq!(o.word_right(4), 4, "body end doesn't step into the header");
}
