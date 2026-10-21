//! .docx export: package parts, text, character and paragraph formats, tables, page setup.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use opendox_export::{docx, DocxOptions};
use opendox_layout::PageGeometry;
use opendox_model::{Align, Document, FormatChange, ParagraphChange};

fn options() -> DocxOptions<'static> {
    DocxOptions {
        default_font: "Noto Sans, Noto Sans Arabic",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: Some("Alice & friends"),
    }
}

/// Reads every file of a ZIP (deflated or stored), in order.
fn unzip(bytes: &[u8]) -> Vec<(String, String)> {
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

fn part(files: &[(String, String)], name: &str) -> String {
    files.iter().find(|(n, _)| n == name).unwrap().1.clone()
}

/// A heading (bold serif 18pt, centred, space after), a paragraph with italic and a tab,
/// a 2×2 table, and a final paragraph.
fn sample() -> Document {
    let d = Document::new();
    d.insert(0, "Chapter <one>\nOnce\tupon a time & more")
        .unwrap();
    let o = d.outline();
    d.format_at(&o, 0, 13, &FormatChange::Bold(true)).unwrap();
    d.format_at(&o, 0, 13, &FormatChange::Font(Some("Noto Serif".into())))
        .unwrap();
    d.format_at(&o, 0, 13, &FormatChange::Size(Some(18.0)))
        .unwrap();
    d.format_at(&o, 14, 18, &FormatChange::Italic(true))
        .unwrap();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::Align(Align::Center))
        .unwrap();
    d.set_paragraph_format_at(&o, 0, 0, ParagraphChange::SpaceAfter(12.0))
        .unwrap();
    d.set_paragraph_format_at(&o, 15, 15, ParagraphChange::Align(Align::Justify))
        .unwrap();
    d.set_paragraph_format_at(&o, 15, 15, ParagraphChange::LineSpacing(1.5))
        .unwrap();
    let t = d.insert_table(d.len(), 2, 2).unwrap();
    for (r, c, text) in [(0, 0, "a1"), (0, 1, "b1"), (1, 0, "a2"), (1, 1, "b2")] {
        let o = d.outline();
        d.insert_at(&o, o.cell_start(&t, r, c).unwrap(), text)
            .unwrap();
    }
    d.insert(d.len(), "The end").unwrap();
    d
}

#[test]
fn package_has_the_parts_word_needs() {
    let files = unzip(&docx(&sample(), &options()).unwrap());
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for want in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/document.xml",
        "word/styles.xml",
        "word/_rels/document.xml.rels",
        "docProps/core.xml",
    ] {
        assert!(names.contains(&want), "missing {want}");
    }
    assert!(part(&files, "docProps/core.xml").contains("<dc:title>Alice &amp; friends</dc:title>"));
    let styles = part(&files, "word/styles.xml");
    assert!(
        styles.contains(r#"w:ascii="Noto Sans""#) && styles.contains(r#"w:cs="Noto Sans Arabic""#)
    );
    assert!(styles.contains(r#"<w:sz w:val="22"/>"#), "11pt default");
}

#[test]
fn text_and_formats_become_runs_and_paragraph_properties() {
    let xml = part(
        &unzip(&docx(&sample(), &options()).unwrap()),
        "word/document.xml",
    );
    // Heading: centred, 12pt after, bold serif 18pt, text escaped.
    assert!(xml.contains(r#"<w:spacing w:before="0" w:after="240" w:line="240" w:lineRule="auto"/><w:jc w:val="center"/>"#));
    assert!(xml.contains(r#"<w:rFonts w:ascii="Noto Serif" w:hAnsi="Noto Serif""#));
    assert!(xml.contains(r#"<w:b/><w:bCs/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr><w:t xml:space="preserve">Chapter &lt;one&gt;</w:t>"#));
    // Second paragraph: justified at 1.5 lines, italic "Once", a tab, plain rest.
    assert!(xml.contains(r#"w:line="360" w:lineRule="auto"/><w:jc w:val="both"/>"#));
    assert!(xml.contains(r#"<w:r><w:rPr><w:i/><w:iCs/></w:rPr><w:t xml:space="preserve">Once</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve">upon a time &amp; more</w:t></w:r>"#));
}

#[test]
fn tables_become_bordered_grids() {
    let xml = part(
        &unzip(&docx(&sample(), &options()).unwrap()),
        "word/document.xml",
    );
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert_eq!(xml.matches("<w:tr>").count(), 2);
    assert_eq!(xml.matches("<w:tc>").count(), 4);
    // Two equal columns of the 468pt content width.
    assert_eq!(xml.matches(r#"<w:gridCol w:w="4680"/>"#).count(), 2);
    assert!(xml.contains(r#"<w:insideV w:val="single" w:sz="6""#));
    let cells: Vec<usize> = ["a1", "b1", "a2", "b2"]
        .iter()
        .map(|t| xml.find(&format!(">{t}<")).unwrap())
        .collect();
    assert!(
        cells.windows(2).all(|w| w[0] < w[1]),
        "cells in reading order"
    );
    assert!(xml.find(">The end<").unwrap() > cells[3]);
    // Letter page with 1 inch margins.
    assert!(xml.contains(r#"<w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440""#));
}

#[test]
fn adjacent_tables_stay_separate_and_a_final_table_gets_a_paragraph() {
    let d = Document::new();
    d.insert_table(0, 1, 1).unwrap();
    let end = d.len();
    d.insert_table(end, 1, 1).unwrap();
    let xml = part(&unzip(&docx(&d, &options()).unwrap()), "word/document.xml");
    assert_eq!(xml.matches("<w:tbl>").count(), 2);
    assert!(
        xml.contains("</w:tbl><w:p/><w:tbl>") || xml.contains("</w:tbl><w:p></w:p><w:tbl>"),
        "{xml}"
    );
    assert!(!xml.contains("</w:tbl><w:sectPr>"));
}

#[test]
fn export_is_deterministic() {
    let d = sample();
    assert_eq!(docx(&d, &options()).unwrap(), docx(&d, &options()).unwrap());
}
