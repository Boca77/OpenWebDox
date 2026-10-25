//! Colours, decorations and links in both exports: PDF draws them as layout placed them and
//! makes links clickable; .docx carries them as run properties and hyperlinks.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::content::Content;
use lopdf::{Document as Pdf, Object};
use opendox_export::{docx, pdf, DocxOptions, PdfInfo, PdfLink};
use opendox_layout::{
    FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, ParagraphStyle, StyleSpan,
    TextStyle,
};
use opendox_model::{Document, FormatChange, Script};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

fn num(o: &Object) -> f32 {
    match o {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => panic!("not a number: {o:?}"),
    }
}

#[test]
fn pdf_draws_colours_and_decorations_and_links_open() {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let base = TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        ..TextStyle::default()
    };
    let text = "red marked link";
    let spans = [
        StyleSpan {
            byte_range: 0..3,
            style: TextStyle {
                color: Some(0xFF0000),
                ..base.clone()
            },
        },
        StyleSpan {
            byte_range: 4..10,
            style: TextStyle {
                highlight: Some(0xFFFF00),
                strikethrough: true,
                ..base.clone()
            },
        },
        StyleSpan {
            byte_range: 11..15,
            style: TextStyle {
                underline: true,
                color: Some(0x1155CC),
                ..base.clone()
            },
        },
    ];
    let input = ParagraphInput {
        text,
        base: &base,
        spans: &spans,
        style: ParagraphStyle::default(),
        objects: &[],
        marker: None,
    };
    let mut p = Paginator::new(PageGeometry::letter());
    p.layout(&mut engine, &[input]);
    let links = [PdfLink {
        paragraph: 0,
        bytes: 11..15,
        url: "https://example.org/x",
    }];
    let info = PdfInfo {
        links: &links,
        ..PdfInfo::default()
    };
    let bytes = pdf(&p, engine.fonts(), &[text], &info).unwrap();
    let doc = Pdf::load_mem(&bytes).unwrap();

    let page = *doc.get_pages().values().next().unwrap();
    let content = Content::decode(&doc.get_page_content(page)).unwrap();
    let fills: Vec<[f32; 3]> = content
        .operations
        .iter()
        .filter(|o| o.operator == "rg")
        .map(|o| {
            [
                num(&o.operands[0]),
                num(&o.operands[1]),
                num(&o.operands[2]),
            ]
        })
        .collect();
    assert!(fills.contains(&[1.0, 0.0, 0.0]), "red text: {fills:?}");
    assert!(fills.contains(&[1.0, 1.0, 0.0]), "yellow highlight");
    let rects = content
        .operations
        .iter()
        .filter(|o| o.operator == "re")
        .count();
    assert_eq!(rects, 3, "highlight, strikethrough, underline");
    // The highlight comes before any glyph is drawn.
    let first_re = content.operations.iter().position(|o| o.operator == "re");
    let first_tj = content.operations.iter().position(|o| o.operator == "Tj");
    assert!(first_re < first_tj);

    // One link annotation over "link" opening the URL.
    let annots = doc
        .get_page_annotations(page)
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(annots.len(), 1);
    let a = annots[0];
    assert_eq!(a.get(b"Subtype").unwrap().as_name().unwrap(), b"Link");
    let action = a.get(b"A").unwrap().as_dict().unwrap();
    assert_eq!(
        action.get(b"URI").unwrap().as_str().unwrap(),
        b"https://example.org/x"
    );
    let r: Vec<f32> = a
        .get(b"Rect")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(num)
        .collect();
    assert!(r[0] > 72.0 + 50.0 && r[2] > r[0] && r[3] > r[1], "{r:?}");
}

/// The text of `word/document.xml` in a .docx.
fn document_xml(bytes: &[u8]) -> (String, String) {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let (mut doc, mut rels) = (String::new(), String::new());
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
        match name.as_str() {
            "word/document.xml" => doc = String::from_utf8(raw).unwrap(),
            "word/_rels/document.xml.rels" => rels = String::from_utf8(raw).unwrap(),
            _ => {}
        }
        at = start + size;
    }
    (doc, rels)
}

#[test]
fn docx_carries_run_formats_and_hyperlinks() {
    let d = Document::new();
    d.insert(0, "under strike red mark up go").unwrap();
    let f = |a: usize, b: usize, c: FormatChange| {
        let o = d.outline();
        d.format_at(&o, a, b, &c).unwrap();
    };
    f(0, 5, FormatChange::Underline(true));
    f(6, 12, FormatChange::Strike(true));
    f(13, 16, FormatChange::Color(Some("#cc0000".into())));
    f(17, 21, FormatChange::Highlight(Some("#ffff00".into())));
    f(22, 24, FormatChange::Script(Script::Super));
    f(
        25,
        27,
        FormatChange::Link(Some("https://example.org/?a=1&b=2".into())),
    );
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let (doc, rels) = document_xml(&docx(&d, &options).unwrap());
    for want in [
        r#"<w:u w:val="single"/></w:rPr><w:t xml:space="preserve">under</w:t>"#,
        r#"<w:strike/></w:rPr><w:t xml:space="preserve">strike</w:t>"#,
        r#"<w:color w:val="CC0000"/></w:rPr><w:t xml:space="preserve">red</w:t>"#,
        r#"<w:shd w:val="clear" w:color="auto" w:fill="FFFF00"/></w:rPr><w:t xml:space="preserve">mark</w:t>"#,
        r#"<w:vertAlign w:val="superscript"/></w:rPr><w:t xml:space="preserve">up</w:t>"#,
        r#"<w:hyperlink r:id="rIdLink1" w:history="1"><w:r><w:rPr><w:color w:val="1155CC"/><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">go</w:t></w:r></w:hyperlink>"#,
    ] {
        assert!(doc.contains(want), "missing {want}\n{doc}");
    }
    assert!(rels.contains(
        r#"Id="rIdLink1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/?a=1&amp;b=2" TargetMode="External""#
    ));
}
