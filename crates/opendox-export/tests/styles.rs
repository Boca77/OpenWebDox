//! Paragraph styles in both exports: .docx uses Word's heading styles (with outline levels);
//! the PDF gets bookmarks nested by heading level, each opening its heading's page.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::{Document as Pdf, Object, ObjectId};
use opendox_export::{docx, pdf, DocxOptions, PdfHeading, PdfInfo};
use opendox_layout::{
    FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, ParagraphStyle, TextStyle,
};
use opendox_model::{Document, ParagraphChange, StyleName};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

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
fn docx_uses_word_heading_styles() {
    let d = Document::new();
    d.insert(0, "Report\nIntro\nbody").unwrap();
    let set = |pos: usize, s: StyleName| {
        let o = d.outline();
        d.set_paragraph_format_at(&o, pos, pos, ParagraphChange::Style(s))
            .unwrap();
    };
    set(0, StyleName::Title);
    set(7, StyleName::Heading(1));
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let files = parts(&docx(&d, &options).unwrap());
    let get = |n: &str| files.iter().find(|(f, _)| f == n).unwrap().1.clone();
    let document = get("word/document.xml");
    let styles = get("word/styles.xml");
    assert!(document.contains(r#"<w:pPr><w:pStyle w:val="Title"/>"#));
    // The heading's style spacing (20pt before, 6pt after) is written, as the editor uses it.
    assert!(document
        .contains(r#"<w:pPr><w:pStyle w:val="Heading1"/><w:spacing w:before="400" w:after="120""#));
    assert!(styles.contains(
        r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/>"#
    ));
    assert!(styles.contains(r#"<w:outlineLvl w:val="0"/>"#));
    assert!(
        styles.contains(r#"<w:sz w:val="40"/>"#),
        "Heading 1 at 20pt"
    );
    assert!(styles.contains(r#"w:styleId="Heading6""#));
}

#[test]
fn docx_page_break_before_a_paragraph() {
    let d = Document::new();
    d.insert(0, "one two").unwrap();
    let o = d.outline();
    d.insert_page_break_at(&o, 4).unwrap();
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let files = parts(&docx(&d, &options).unwrap());
    let document = &files
        .iter()
        .find(|(f, _)| f == "word/document.xml")
        .unwrap()
        .1;
    assert!(document.contains(r#"<w:p><w:pPr><w:pageBreakBefore/><w:spacing"#));
    assert_eq!(document.matches("pageBreakBefore").count(), 1);
}

/// Follows `/Dest`'s page reference to its 0-based page number.
fn page_number(pdf: &Pdf, item: &lopdf::Dictionary) -> usize {
    let dest = item.get(b"Dest").unwrap().as_array().unwrap();
    let page = dest[0].as_reference().unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().values().copied().collect();
    pages.iter().position(|p| *p == page).unwrap()
}

#[test]
fn pdf_bookmarks_nest_by_heading_level() {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let base = TextStyle {
        font_family: "Noto Sans".into(),
        ..TextStyle::default()
    };
    let filler = "Text that fills the page. ".repeat(120);
    let texts = [
        "The Title",
        "Part one",
        filler.as_str(),
        "Section 1.1",
        "Part two",
    ];
    let inputs: Vec<ParagraphInput> = texts
        .iter()
        .map(|text| ParagraphInput {
            text,
            base: &base,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &[],
            marker: None,
        })
        .collect();
    let mut p = Paginator::new(PageGeometry {
        height: 400.0,
        ..PageGeometry::letter()
    });
    p.layout(&mut engine, &inputs);
    let headings = [
        PdfHeading {
            paragraph: 0,
            level: 0,
            title: texts[0],
        },
        PdfHeading {
            paragraph: 1,
            level: 1,
            title: texts[1],
        },
        PdfHeading {
            paragraph: 3,
            level: 2,
            title: texts[3],
        },
        PdfHeading {
            paragraph: 4,
            level: 1,
            title: texts[4],
        },
    ];
    let info = PdfInfo {
        headings: &headings,
        ..PdfInfo::default()
    };
    let bytes = pdf(&p, engine.fonts(), &texts, &info).unwrap();
    let doc = Pdf::load_mem(&bytes).unwrap();
    let catalog = doc.catalog().unwrap();
    let outlines = doc
        .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
        .unwrap();
    assert_eq!(outlines.get(b"Count").unwrap().as_i64().unwrap(), 4);
    let deref = |o: &Object| doc.get_dictionary(o.as_reference().unwrap()).unwrap();
    let title = |d: &lopdf::Dictionary| {
        String::from_utf8_lossy(d.get(b"Title").unwrap().as_str().unwrap()).into_owned()
    };
    // One top-level item, the title, holding both parts; part one holds the section.
    let top = deref(outlines.get(b"First").unwrap());
    assert_eq!(title(top), "The Title");
    assert!(top.get(b"Next").is_err());
    assert_eq!(top.get(b"Count").unwrap().as_i64().unwrap(), 3);
    let part1 = deref(top.get(b"First").unwrap());
    let part2 = deref(top.get(b"Last").unwrap());
    assert_eq!(
        (title(part1), title(part2)),
        ("Part one".into(), "Part two".into())
    );
    let section = deref(part1.get(b"First").unwrap());
    assert_eq!(title(section), "Section 1.1");
    // Each opens the page its heading is on: the filler pushes later headings onward.
    assert_eq!(page_number(&doc, top), 0);
    assert!(page_number(&doc, section) > 0);
    assert_eq!(
        page_number(&doc, part2),
        p.placed_lines()
            .iter()
            .find(|l| l.paragraph == 4)
            .unwrap()
            .page
    );
}
