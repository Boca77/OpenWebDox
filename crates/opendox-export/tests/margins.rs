//! Headers and footers in both exports: the PDF shows each page's number (extractable as
//! text); the .docx has header and footer parts with Word's PAGE and NUMPAGES fields.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::Document as Pdf;
use opendox_export::{docx, pdf, DocxOptions, PdfInfo};
use opendox_layout::{
    BlockInput, FieldKind, FontLibrary, LayoutEngine, ObjectInput, ObjectMode, PageGeometry,
    Paginator, ParagraphInput, ParagraphStyle, TextStyle, OBJECT_PLACEHOLDER,
};
use opendox_model::{Document, FieldKind as ModelField};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

#[test]
fn pdf_pages_show_their_own_numbers() {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let base = TextStyle {
        font_family: "Noto Sans".into(),
        ..TextStyle::default()
    };
    let body: Vec<String> = (0..30).map(|i| format!("Line {i}")).collect();
    let para = |text: &'static str, objects: &'static [ObjectInput]| ParagraphInput {
        text,
        base: Box::leak(Box::new(base.clone())),
        spans: &[],
        style: ParagraphStyle::default(),
        objects,
        marker: None,
    };
    let blocks: Vec<BlockInput> = body
        .iter()
        .map(|t| BlockInput::Paragraph(para(Box::leak(t.clone().into_boxed_str()), &[])))
        .collect();
    let footer_text: &'static str =
        Box::leak(format!("Page {OBJECT_PLACEHOLDER} of {OBJECT_PLACEHOLDER}").into_boxed_str());
    let field = |byte, kind| ObjectInput {
        byte,
        key: usize::MAX,
        width: 0.0,
        height: 0.0,
        rotation: 0.0,
        mode: ObjectMode::Field(kind),
        x: 0.0,
        y: 0.0,
    };
    let fields: &'static [ObjectInput] = Box::leak(Box::new([
        field(5, FieldKind::PageNumber),
        field(12, FieldKind::PageCount),
    ]));
    let footer = [para(footer_text, fields)];
    let mut p = Paginator::new(PageGeometry {
        height: 250.0,
        ..PageGeometry::letter()
    });
    p.layout_document(&mut engine, &blocks, &[], &footer);
    let pages = p.pages().len();
    assert!(pages >= 3);
    let mut texts: Vec<&str> = body.iter().map(String::as_str).collect();
    texts.push(footer_text);
    let bytes = pdf(&p, engine.fonts(), &texts, &PdfInfo::default()).unwrap();
    let doc = Pdf::load_mem(&bytes).unwrap();
    for n in 1..=pages {
        let text: String = doc
            .extract_text(&[n as u32])
            .unwrap()
            .split_whitespace()
            .collect();
        assert!(
            text.contains(&format!("Page{n}of{pages}")),
            "page {n}: {text}"
        );
    }
}

#[test]
fn docx_header_and_footer_parts_with_word_fields() {
    let d = Document::new();
    d.insert(0, "body").unwrap();
    d.set_header(true).unwrap();
    d.set_footer(true).unwrap();
    let o = d.outline();
    let h = o.slots[o.header.start].flat;
    d.insert_at(&o, h, "Report").unwrap();
    let o = d.outline();
    let f = o.slots[o.footer.start].flat;
    let after = d.insert_at(&o, f, "Page ").unwrap();
    let o = d.outline();
    d.insert_field_at(&o, after, ModelField::PageNumber)
        .unwrap();
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let bytes = docx(&d, &options).unwrap();
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let mut files: Vec<(String, String)> = Vec::new();
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
        files.push((name, String::from_utf8(raw).unwrap()));
        at = start + size;
    }
    let get = |n: &str| files.iter().find(|(f, _)| f == n).map(|(_, s)| s.clone());
    let header = get("word/header1.xml").unwrap();
    let footer = get("word/footer1.xml").unwrap();
    let document = get("word/document.xml").unwrap();
    assert!(
        header.starts_with(r#"<?xml"#) && header.contains("<w:hdr ") && header.contains(">Report<")
    );
    assert!(
        footer.contains(r#"<w:fldSimple w:instr=" PAGE ">"#),
        "{footer}"
    );
    assert!(!footer.contains('\u{FFFC}'));
    assert!(document.contains(
        r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdheader"/><w:footerReference w:type="default" r:id="rIdfooter"/><w:pgSz"#
    ));
    assert!(!document.contains("Report"), "the header isn't in the body");
    let rels = get("word/_rels/document.xml.rels").unwrap();
    assert!(rels.contains(r#"Id="rIdheader""#) && rels.contains(r#"Target="footer1.xml""#));
    assert!(get("[Content_Types].xml")
        .unwrap()
        .contains(r#"PartName="/word/header1.xml""#));
}
