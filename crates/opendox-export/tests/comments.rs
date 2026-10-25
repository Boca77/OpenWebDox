//! Comments in .docx: comments.xml with author, date, text and replies; range marks in the
//! text; resolved comments left out.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_export::{docx, DocxOptions};
use opendox_layout::PageGeometry;
use opendox_model::Document;

fn parts(bytes: &[u8]) -> Vec<(String, String)> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    let mut at = 0;
    while u32_at(at) == 0x0403_4b50 {
        let method = u16_at(at + 8);
        let size = u32_at(at + 18);
        let name_len = u16_at(at + 26);
        let extra = u16_at(at + 28);
        let name = String::from_utf8(bytes[at + 30..at + 30 + name_len].to_vec()).unwrap();
        let start = at + 30 + name_len + extra;
        let data = &bytes[start..start + size];
        let raw = match method {
            8 => miniz_oxide::inflate::decompress_to_vec(data).unwrap(),
            _ => data.to_vec(),
        };
        out.push((name, String::from_utf8(raw).unwrap()));
        at = start + size;
    }
    out
}

#[test]
fn open_comments_become_word_comments() {
    let d = Document::new();
    d.insert(0, "Check this claim.\nAnd this one.").unwrap();
    let o = d.outline();
    // 2026-10-25T12:30:00Z
    let when = 1_792_931_400_000.0;
    let a = d
        .add_comment(&o, 6, 10, "Ada Lovelace", "Source?", when)
        .unwrap();
    d.reply_to_comment(&a, "Grace", "Added <one>", when + 60_000.0)
        .unwrap();
    // Spanning into the next paragraph, ending at its end.
    let b = d
        .add_comment(&o, 11, 31, "Alan", "Whole thing", when)
        .unwrap();
    let c = d.add_comment(&o, 0, 5, "Ken", "done", when).unwrap();
    d.resolve_comment(&c, true).unwrap();
    let _ = b;
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let files = parts(&docx(&d, &options).unwrap());
    let get = |n: &str| files.iter().find(|(f, _)| f == n).map(|(_, s)| s.clone());
    let comments = get("word/comments.xml").unwrap();
    let document = get("word/document.xml").unwrap();
    assert!(comments.contains(
        r#"<w:comment w:id="0" w:author="Ada Lovelace" w:date="2026-10-25T12:30:00Z" w:initials="AL">"#
    ), "{comments}");
    assert!(comments.contains(">Source?<"));
    assert!(comments.contains(">Grace: <") && comments.contains("Added &lt;one&gt;"));
    assert!(!comments.contains("done"), "resolved comments are left out");
    // "this" is marked: start before it, end and reference after it.
    assert!(document.contains(r#"<w:commentRangeStart w:id="0"/><w:r><w:t xml:space="preserve">this</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r>"#), "{document}");
    assert_eq!(document.matches("commentRangeStart").count(), 2);
    assert_eq!(document.matches("commentRangeEnd").count(), 2);
    assert!(get("[Content_Types].xml")
        .unwrap()
        .contains("/word/comments.xml"));
    assert!(get("word/_rels/document.xml.rels")
        .unwrap()
        .contains(r#"Target="comments.xml""#));
}

#[test]
fn no_comments_no_part() {
    let d = Document::new();
    d.insert(0, "plain").unwrap();
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let files = parts(&docx(&d, &options).unwrap());
    assert!(files.iter().all(|(f, _)| f != "word/comments.xml"));
}
