//! Copy and paste with formatting: what a copy takes, how a paste places paragraphs and
//! formats, lists becoming lists of their own, one undo step, JSON and HTML.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_model::{
    Align, Clip, Document, FormatChange, History, ListKind, ParagraphChange, StyleName,
};

fn doc(text: &str) -> Document {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, text).unwrap();
    d
}

fn para(d: &Document, i: usize) -> usize {
    d.outline().slots[i].flat
}

/// "Heading\nSome bold text\n• one\n• two"
fn source() -> Document {
    let d = doc("Heading\nSome bold text\none\ntwo");
    let o = d.outline();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Style(StyleName::Heading(1)))
        .unwrap();
    let b = para(&d, 1) + 5;
    d.format_at(&o, b, b + 4, &FormatChange::Bold(true))
        .unwrap();
    d.format_at(&o, b, b + 4, &FormatChange::Color(Some("#cc0000".into())))
        .unwrap();
    d.set_list_at(
        &d.outline(),
        para(&d, 2),
        para(&d, 3),
        Some(ListKind::Bullet),
    )
    .unwrap();
    d
}

#[test]
fn copy_takes_text_runs_and_paragraph_formats_cut_to_the_range() {
    let d = source();
    let o = d.outline();
    // From "ing" in the heading to "on" in the first bullet.
    let clip = d.copy_at(&o, 4, para(&d, 2) + 2);
    let texts: Vec<&str> = clip.paragraphs.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["ing", "Some bold text", "on"]);
    assert_eq!(clip.paragraphs[0].style.as_deref(), Some("h1"));
    let run = &clip.paragraphs[1].runs[0];
    assert_eq!((run.start, run.end, run.bold), (5, 9, true));
    assert_eq!(run.color.as_deref(), Some("#cc0000"));
    assert_eq!(clip.paragraphs[2].list.as_ref().unwrap().kind, "bullet");
    assert_eq!(clip.plain_text(), "ing\nSome bold text\non");
}

#[test]
fn a_clip_survives_json() {
    let d = source();
    let clip = d.copy_at(&d.outline(), 0, d.outline().len);
    assert_eq!(Clip::from_json(&clip.to_json()).unwrap(), clip);
    assert!(Clip::from_json("{not json").is_err());
}

#[test]
fn pasting_into_another_document_keeps_everything() {
    let src = source();
    let clip = src.copy_at(&src.outline(), 0, src.outline().len);
    let dst = Document::with_replica_id(2).unwrap();
    let end = dst.paste_at(&dst.outline(), 0, &clip).unwrap();
    assert_eq!(dst.text(), src.text());
    assert_eq!(end, src.outline().len);
    let (a, b) = (src.formats(&src.outline()), dst.formats(&dst.outline()));
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.runs, y.runs);
        assert_eq!(x.paragraph.style, y.paragraph.style);
        assert_eq!(
            x.paragraph.list.map(|l| l.kind),
            y.paragraph.list.map(|l| l.kind)
        );
    }
    // The two pasted bullets are one list, a new one.
    let ids: Vec<u64> = b[2..]
        .iter()
        .map(|f| f.paragraph.list.unwrap().id)
        .collect();
    assert_eq!(ids[0], ids[1]);
}

#[test]
fn a_single_paragraph_paste_keeps_the_paragraph_it_lands_in() {
    let src = source();
    let clip = src.copy_at(&src.outline(), para(&src, 1), para(&src, 1) + 9); // "Some bold"
    let dst = doc("centred line");
    let o = dst.outline();
    dst.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Align(Align::Center))
        .unwrap();
    dst.paste_at(&dst.outline(), 8, &clip).unwrap();
    assert_eq!(dst.text(), "centred Some boldline");
    let f = &dst.formats(&dst.outline())[0];
    assert_eq!(f.paragraph.align, Align::Center);
    let bold: Vec<_> = f.runs.iter().filter(|r| r.format.bold).collect();
    assert_eq!(bold[0].range, 13..17);
}

#[test]
fn multi_paragraph_paste_formats_only_whole_paragraphs() {
    let src = source();
    let clip = src.copy_at(&src.outline(), 0, para(&src, 2) + 3); // Heading … "one"
    let dst = doc("start end");
    dst.paste_at(&dst.outline(), 6, &clip).unwrap();
    assert_eq!(dst.text(), "start Heading\nSome bold text\noneend");
    let f = dst.formats(&dst.outline());
    // The paste began mid-paragraph: "start Heading" stays normal text.
    assert_eq!(f[0].paragraph.style, StyleName::Normal);
    // "oneend" mixes pasted and existing text: it keeps the destination's format.
    assert_eq!(f[2].paragraph.list, None);
}

#[test]
fn pasted_text_never_takes_the_format_around_it() {
    let dst = doc("bold");
    let o = dst.outline();
    dst.format_at(&o, 0, 4, &FormatChange::Bold(true)).unwrap();
    let clip = Clip::from_json(r#"{"paragraphs":[{"text":"plain "}]}"#).unwrap();
    dst.paste_at(&dst.outline(), 0, &clip).unwrap();
    let f = &dst.formats(&dst.outline())[0];
    assert!(f.runs.iter().all(|r| r.range.start >= 6));
}

#[test]
fn a_paste_is_one_undo_step() {
    let src = source();
    let clip = src.copy_at(&src.outline(), 0, src.outline().len);
    let dst = doc("x");
    let mut h = History::new(&dst);
    let o = dst.outline();
    h.before_edit(&dst, &o, 1, 1, false);
    dst.paste_at(&o, 1, &clip).unwrap();
    h.undo(&dst, &dst.outline(), 1, 1).unwrap();
    assert_eq!(dst.text(), "x");
}

#[test]
fn html_has_tags_lists_and_the_lossless_clip() {
    let d = source();
    let clip = d.copy_at(&d.outline(), 0, d.outline().len);
    let html = clip.to_html();
    assert!(html.contains("<h1>Heading</h1>"), "{html}");
    assert!(
        html.contains(r#"<b><span style="color:#cc0000;">bold</span></b>"#),
        "{html}"
    );
    assert!(html.contains("<ul><li>one</li><li>two</li></ul>"), "{html}");
    assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count());
    assert!(html.contains("data-opendox-clip=\""));
    assert!(!html.contains("<script"));
}

#[test]
fn nested_and_mixed_lists_close_properly() {
    let clip = Clip::from_json(
        r#"{"paragraphs":[
            {"text":"a","list":{"kind":"number","level":0,"id":"1"}},
            {"text":"b","list":{"kind":"bullet","level":1,"id":"1"}},
            {"text":"c","list":{"kind":"number","level":0,"id":"1"}},
            {"text":"d"}]}"#,
    )
    .unwrap();
    let html = clip.to_html();
    assert!(
        html.contains("<ol><li>a</li><ul><li>b</li></ul><li>c</li></ol><p>d</p>"),
        "{html}"
    );
}
