//! Lists in both exports: PDF draws markers where layout put them and they extract as text;
//! .docx carries Word numbering with one definition per list and the indents.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::Document as Pdf;
use opendox_export::{docx, pdf, DocxOptions, PdfInfo};
use opendox_layout::{
    FontLibrary, LayoutEngine, Marker, PageGeometry, Paginator, ParagraphInput, ParagraphStyle,
    TextStyle,
};
use opendox_model::{Document, ListKind, ParagraphChange};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const SYMBOLS: &[u8] = include_bytes!("../../../fonts/noto/NotoSansSymbols2-Regular.ttf");

#[test]
fn pdf_draws_markers_and_they_extract_before_their_items() {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    fonts.register(SYMBOLS.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let base = TextStyle {
        font_family: "Noto Sans, Noto Sans Symbols 2".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
        ..TextStyle::default()
    };
    let style = ParagraphStyle {
        indent_start: 36.0,
        indent_first: -18.0,
        ..ParagraphStyle::default()
    };
    let texts = ["Apples", "Pears"];
    let markers = ["1.", "●"];
    let inputs: Vec<ParagraphInput> = texts
        .iter()
        .zip(markers)
        .map(|(text, m)| ParagraphInput {
            text,
            base: &base,
            spans: &[],
            style,
            objects: &[],
            marker: Some(Marker {
                text: m,
                style: &base,
            }),
        })
        .collect();
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut engine, &inputs);
    let bytes = pdf(&p, engine.fonts(), &texts, &PdfInfo::default()).unwrap();
    let doc = Pdf::load_mem(&bytes).unwrap();
    let text = doc.extract_text(&[1]).unwrap();
    let squashed: String = text.split_whitespace().collect();
    assert!(squashed.contains("1.Apples"), "{text:?}");
    assert!(squashed.contains("●Pears"), "{text:?}");
    // Two fonts: the body text and the bullet from the symbols font.
    let fonts = doc
        .objects
        .values()
        .filter(|o| {
            o.as_dict().is_ok_and(|d| {
                d.get(b"Subtype")
                    .is_ok_and(|s| s.as_name().is_ok_and(|n| n == b"Type0"))
            })
        })
        .count();
    assert_eq!(fonts, 2);
}

fn options() -> DocxOptions<'static> {
    DocxOptions {
        default_font: "Noto Sans, Noto Sans Arabic",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    }
}

/// Reads every file of a ZIP (deflated or stored).
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

fn part(files: &[(String, String)], name: &str) -> Option<String> {
    files
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, s)| s.clone())
}

/// `<w:numPr>` of each paragraph as `(level, numId)`, `None` for plain paragraphs.
fn num_prs(document: &str) -> Vec<Option<(u32, u32)>> {
    document
        .split("<w:p>")
        .skip(1)
        .map(|p| {
            let p = p.split("</w:p>").next().unwrap();
            let attr = |tag: &str| -> Option<u32> {
                let at = p.find(tag)? + tag.len();
                let rest = &p[at..];
                rest[..rest.find('"')?].parse().ok()
            };
            Some((attr(r#"<w:ilvl w:val=""#)?, attr(r#"<w:numId w:val=""#)?))
        })
        .collect()
}

#[test]
fn docx_lists_become_word_numbering() {
    let d = Document::new();
    d.insert(0, "one\nnested\ntwo\nbreak\nfresh\nbullet\ntask\ndone")
        .unwrap();
    let flat = |i: usize| d.outline().slots[i].flat;
    let o = d.outline();
    d.set_list_at(&o, 0, flat(2), Some(ListKind::Number))
        .unwrap();
    d.indent_at(&d.outline(), flat(1), flat(1), true).unwrap();
    d.set_list_at(&d.outline(), flat(4), flat(4), Some(ListKind::Number))
        .unwrap();
    d.set_list_at(&d.outline(), flat(5), flat(5), Some(ListKind::Bullet))
        .unwrap();
    d.set_list_at(&d.outline(), flat(6), flat(7), Some(ListKind::Check))
        .unwrap();
    let p = flat(7);
    d.set_paragraph_format_at(&d.outline(), p, p, ParagraphChange::Checked(true))
        .unwrap();

    let files = unzip(&docx(&d, &options()).unwrap());
    let document = part(&files, "word/document.xml").unwrap();
    let numbering = part(&files, "word/numbering.xml").unwrap();
    assert!(part(&files, "[Content_Types].xml")
        .unwrap()
        .contains(r#"PartName="/word/numbering.xml""#));
    assert!(part(&files, "word/_rels/document.xml.rels")
        .unwrap()
        .contains(r#"Target="numbering.xml""#));

    // The first list (with its nested item), a new numbered list after the plain paragraph,
    // bullets, and the checklist split into unticked and ticked.
    assert_eq!(
        num_prs(&document),
        vec![
            Some((0, 1)),
            Some((1, 1)),
            Some((0, 1)),
            None,
            Some((0, 2)),
            Some((0, 3)),
            Some((0, 4)),
            Some((0, 5)),
        ]
    );
    assert_eq!(numbering.matches("<w:abstractNum ").count(), 5);
    assert_eq!(numbering.matches("<w:num ").count(), 5);
    // Number formats by level, and list text with Word's level placeholders.
    for want in [
        r#"<w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>"#,
        r#"<w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2."/>"#,
        r#"<w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%3."/>"#,
        r#"<w:lvlText w:val="●"/>"#,
        r#"<w:lvlText w:val="○"/>"#,
        r#"<w:lvlText w:val="☐"/>"#,
        r#"<w:lvlText w:val="☑"/>"#,
        r#"w:ascii="Noto Sans Symbols 2""#,
    ] {
        assert!(numbering.contains(want), "missing {want}");
    }
    // Indents: level 0 text at 0.5 in with the marker hanging 0.25 in; level 1 at 1 in.
    assert!(document.contains(r#"<w:ind w:left="720" w:right="0" w:hanging="360"/>"#));
    assert!(document.contains(r#"<w:ind w:left="1440" w:right="0" w:hanging="360"/>"#));
}

#[test]
fn docx_without_lists_has_no_numbering_part() {
    let d = Document::new();
    d.insert(0, "plain").unwrap();
    let files = unzip(&docx(&d, &options()).unwrap());
    assert!(part(&files, "word/numbering.xml").is_none());
}

#[test]
fn docx_paragraph_indents_without_a_list() {
    let d = Document::new();
    d.insert(0, "indented").unwrap();
    let o = d.outline();
    for c in [
        ParagraphChange::IndentStart(36.0),
        ParagraphChange::IndentEnd(18.0),
        ParagraphChange::IndentFirst(12.0),
    ] {
        d.set_paragraph_format_at(&o, 0, 0, c).unwrap();
    }
    let files = unzip(&docx(&d, &options()).unwrap());
    let document = part(&files, "word/document.xml").unwrap();
    assert!(document.contains(r#"<w:ind w:left="720" w:right="360" w:firstLine="240"/>"#));
}
