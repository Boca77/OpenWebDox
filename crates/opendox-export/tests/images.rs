//! Images in exports: the PDF draws each picture where layout placed it (with transparency and
//! the colour treatment); the .docx carries it as a Word picture.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::content::Content;
use lopdf::{Document as Pdf, Object};
use opendox_export::{adjust, docx, pdf, DocxOptions, ExportImage, PdfInfo};
use opendox_layout::{
    FontLibrary, LayoutEngine, ObjectInput, ObjectMode, PageGeometry, Paginator, ParagraphInput,
    ParagraphStyle, TextStyle, OBJECT_PLACEHOLDER,
};
use opendox_model::{Document, ImageChange, ImageData, Recolor, Wrap};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

/// A 2 x 1 PNG: one opaque red pixel, one half-transparent blue one.
fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, 2, 1);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut w = e.write_header().unwrap();
    w.write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128])
        .unwrap();
    w.finish().unwrap();
    out
}

fn num(o: &Object) -> f32 {
    match o {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => panic!("not a number: {o:?}"),
    }
}

#[test]
fn pdf_draws_the_picture_where_layout_put_it() {
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let base = TextStyle {
        font_family: "Noto Sans".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
    };
    let text = format!("Picture: {OBJECT_PLACEHOLDER}");
    let objects = [ObjectInput {
        byte: 9,
        key: 0,
        width: 100.0,
        height: 50.0,
        rotation: 0.0,
        mode: ObjectMode::Inline,
        x: 0.0,
        y: 0.0,
    }];
    let mut paginator = Paginator::new(PageGeometry::letter());
    paginator.layout(
        &mut engine,
        &[ParagraphInput {
            text: &text,
            base: &base,
            spans: &[],
            style: ParagraphStyle::default(),
            objects: &objects,
        }],
    );
    let placed = paginator.pages()[0].objects[0];
    let data = png();
    let images = [Some(ExportImage {
        data: &data,
        mime: "image/png",
        recolor: Recolor::Grayscale,
        brightness: 0.0,
        contrast: 0.0,
        transparency: 25.0,
        alt: "",
    })];
    let bytes = pdf(
        &paginator,
        engine.fonts(),
        &[&text],
        &PdfInfo {
            title: None,
            images: &images,
        },
    )
    .unwrap();
    let doc = Pdf::load_mem(&bytes).unwrap();

    // One image XObject, 2 x 1, with an alpha mask for the half-transparent pixel.
    let image = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find(|s| {
            s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()) == Some(b"Image")
                && s.dict.has(b"SMask")
        })
        .expect("an image with a soft mask");
    assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 2);
    let mut image = image.clone();
    image.decompress().unwrap();
    let red = adjust([255, 0, 0], Recolor::Grayscale, 0.0, 0.0);
    assert_eq!(&image.content[..3], &red, "grayscale applied");

    // Drawn with the transform that puts the unit square on the placed box, see-through.
    let page = *doc.get_pages().values().next().unwrap();
    let content = Content::decode(&doc.get_page_content(page)).unwrap();
    let cm = content
        .operations
        .iter()
        .find(|op| op.operator == "cm")
        .unwrap();
    let m: Vec<f32> = cm.operands.iter().map(num).collect();
    let h = 792.0;
    assert!(
        (m[0] - 100.0).abs() < 0.01 && (m[3] - 50.0).abs() < 0.01,
        "{m:?}"
    );
    assert!((m[4] - placed.bounds.x).abs() < 0.01);
    assert!((m[5] - (h - placed.bounds.y - 50.0)).abs() < 0.01);
    assert!(content.operations.iter().any(|op| op.operator == "gs"));
    assert!(content.operations.iter().any(|op| op.operator == "Do"));
}

/// The files of a .docx: names and raw bytes.
fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
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
        out.push((name, raw));
        at = start + size;
    }
    out
}

#[test]
fn docx_carries_pictures_inline_and_anchored() {
    let d = Document::with_replica_id(1).unwrap();
    d.insert(0, "One picture here and another there.").unwrap();
    let data = ImageData {
        bytes: png(),
        mime: "image/png".into(),
        px_width: 2,
        px_height: 1,
    };
    let a = d.insert_image(&d.outline(), 12, &data, 40.0, 20.0).unwrap();
    let b = d.insert_image(&d.outline(), 30, &data, 80.0, 40.0).unwrap();
    d.set_image_many(
        &b,
        vec![
            ImageChange::Wrap(Wrap::Wrap),
            ImageChange::Offset(36.0, 18.0),
            ImageChange::Rotation(90.0),
            ImageChange::Alt("A <tiny> picture".into()),
            ImageChange::Brightness(20.0),
        ],
    )
    .unwrap();
    let _ = a;
    let options = DocxOptions {
        default_font: "Noto Sans",
        default_size: 11.0,
        page: PageGeometry::letter(),
        title: None,
    };
    let files = unzip(&docx(&d, &options).unwrap());
    let part = |name: &str| {
        files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
            .unwrap_or_else(|| panic!("no {name}"))
    };
    assert_eq!(part("word/media/image1.png"), png());
    assert_eq!(part("word/media/image2.png"), png());
    let rels = String::from_utf8(part("word/_rels/document.xml.rels")).unwrap();
    assert!(rels.contains(r#"Id="rIdImg2""#) && rels.contains("media/image2.png"));
    let types = String::from_utf8(part("[Content_Types].xml")).unwrap();
    assert!(types.contains(r#"Extension="png""#));

    let doc = String::from_utf8(part("word/document.xml")).unwrap();
    assert_eq!(doc.matches("<wp:inline").count(), 1);
    assert_eq!(doc.matches("<wp:anchor").count(), 1);
    assert!(
        doc.contains(r#"<wp:extent cx="508000" cy="254000"/>"#),
        "40 x 20 pt in EMU"
    );
    assert!(doc.contains(r#"<wp:wrapSquare wrapText="largest"/>"#));
    assert!(doc.contains(r#"<a:xfrm rot="5400000">"#), "90 degrees");
    assert!(doc.contains(r#"descr="A &lt;tiny&gt; picture""#));
    assert!(doc.contains(r#"<a:lum bright="20000" contrast="0"/>"#));
    // The placeholder characters don't leak into the text.
    assert!(!doc.contains('\u{FFFC}'));
    assert!(doc.contains(r#"<w:t xml:space="preserve">One picture </w:t>"#));
}
