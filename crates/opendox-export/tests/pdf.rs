//! PDF export, read back with lopdf: pages, glyph positions, borders, embedded subset fonts,
//! extractable text, determinism.
#![allow(clippy::unwrap_used)] // test helpers, not only #[test] fns

use lopdf::content::Content;
use lopdf::{Document as Pdf, Object};
use opendox_export::{pdf, PdfInfo};
use opendox_layout::{
    BlockInput, FontLibrary, LayoutEngine, PageGeometry, Paginator, ParagraphInput, ParagraphStyle,
    StyleSpan, TableInput, TextStyle,
};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Bold.ttf");
const ARABIC: &[u8] = include_bytes!("../../../fonts/noto/NotoSansArabic-Regular.ttf");

const ALICE: &str = "Alice was beginning to get very tired of sitting by her sister on the bank, \
                     and of having nothing to do: once or twice she had peeped into the book her \
                     sister was reading, but it had no pictures or conversations in it.";

struct Doc {
    paginator: Paginator,
    fonts: FontLibrary,
    texts: Vec<String>,
}

fn style() -> TextStyle {
    TextStyle {
        font_family: "Noto Sans, Noto Sans Arabic".into(),
        font_size: 12.0,
        font_weight: 400.0,
        italic: false,
    }
}

/// A heading with a bold word, `body` paragraphs of Alice, a 2×2 table, and Arabic.
fn doc(body: usize) -> Doc {
    let mut fonts = FontLibrary::new();
    for f in [REGULAR, BOLD, ARABIC] {
        fonts.register(f.to_vec());
    }
    let mut engine = LayoutEngine::new(fonts);
    let base = style();
    let bold = [StyleSpan {
        byte_range: 0..7,
        style: TextStyle {
            font_weight: 700.0,
            ..style()
        },
    }];
    let mut texts: Vec<String> = vec!["Chapter one: down the rabbit-hole".into()];
    texts.extend((0..body).map(|_| ALICE.to_string()));
    texts.extend(["cell a", "cell b", "cell c", "cell d"].map(String::from));
    texts.push("Arabic: مرحبا بالعالم".into());
    let para = |i: usize, spans: &'static [StyleSpan]| ParagraphInput {
        text: &texts[i],
        base: &base,
        spans,
        style: ParagraphStyle::default(),
    };
    let bold: &'static [StyleSpan] = Box::leak(Box::new(bold));
    let mut blocks = vec![BlockInput::Paragraph(para(0, bold))];
    blocks.extend((1..=body).map(|i| BlockInput::Paragraph(para(i, &[]))));
    let c = body + 1;
    blocks.push(BlockInput::Table(TableInput {
        rows: vec![
            vec![vec![para(c, &[])], vec![para(c + 1, &[])]],
            vec![vec![para(c + 2, &[])], vec![para(c + 3, &[])]],
        ],
    }));
    blocks.push(BlockInput::Paragraph(para(c + 4, &[])));
    let mut paginator = Paginator::new(PageGeometry::letter());
    paginator.layout_blocks(&mut engine, &blocks);
    let fonts = std::mem::replace(engine.fonts_mut(), FontLibrary::new());
    Doc {
        paginator,
        fonts,
        texts,
    }
}

fn export(d: &Doc) -> Vec<u8> {
    let texts: Vec<&str> = d.texts.iter().map(String::as_str).collect();
    pdf(
        &d.paginator,
        &d.fonts,
        &texts,
        &PdfInfo {
            title: Some("Test"),
        },
    )
    .unwrap()
}

fn num(o: &Object) -> f32 {
    match o {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => panic!("not a number: {o:?}"),
    }
}

/// `(x, y)` of every glyph shown, and line segments drawn, per page.
fn drawn(pdf: &Pdf) -> Vec<(Vec<(f32, f32)>, usize)> {
    pdf.get_pages()
        .values()
        .map(|&id| {
            let content = Content::decode(&pdf.get_page_content(id)).unwrap();
            let mut at = (0.0, 0.0);
            let mut glyphs = Vec::new();
            let mut lines = 0;
            for op in &content.operations {
                match op.operator.as_str() {
                    "Tm" => at = (num(&op.operands[4]), num(&op.operands[5])),
                    "Tj" => glyphs.push(at),
                    "l" => lines += 1,
                    _ => {}
                }
            }
            (glyphs, lines)
        })
        .collect()
}

#[test]
fn pages_and_glyphs_match_layout_exactly() {
    let d = doc(14);
    let bytes = export(&d);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages = d.paginator.pages();
    assert!(pages.len() >= 2, "test document should span pages");
    assert_eq!(pdf.get_pages().len(), pages.len());

    let h = d.paginator.geometry().height;
    for (i, (glyphs, lines)) in drawn(&pdf).iter().enumerate() {
        let expected = d.paginator.placed_glyphs(i);
        assert_eq!(glyphs.len(), expected.len(), "page {i} glyph count");
        for (g, e) in glyphs.iter().zip(&expected) {
            assert!(
                (g.0 - e.x).abs() < 0.01 && (g.1 - (h - e.y)).abs() < 0.01,
                "page {i}: {g:?} vs {e:?}"
            );
        }
        assert_eq!(*lines, pages[i].rules.len(), "page {i} borders");
    }

    // Letter size.
    let first = *pdf.get_pages().values().next().unwrap();
    let mb = pdf
        .get_dictionary(first)
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!((num(&mb[2]), num(&mb[3])), (612.0, 792.0));
}

#[test]
fn fonts_are_embedded_as_small_subsets() {
    let d = doc(2);
    let bytes = export(&d);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let mut embedded = 0;
    for obj in pdf.objects.values() {
        let Ok(dict) = obj.as_dict() else { continue };
        if dict.get(b"Type").and_then(|t| t.as_name()).ok() != Some(b"FontDescriptor".as_slice()) {
            continue;
        }
        let name =
            String::from_utf8_lossy(dict.get(b"FontName").unwrap().as_name().unwrap()).to_string();
        assert!(
            name.len() > 7 && name.as_bytes()[6] == b'+',
            "subset tag: {name}"
        );
        let file = dict.get(b"FontFile2").unwrap().as_reference().unwrap();
        let mut stream = pdf.get_object(file).unwrap().as_stream().unwrap().clone();
        stream.decompress().unwrap();
        assert!(
            stream.content.len() < REGULAR.len() / 4,
            "{name}: {} bytes",
            stream.content.len()
        );
        embedded += 1;
    }
    assert_eq!(embedded, 3, "regular, bold and Arabic");
    assert!(bytes.len() < 150_000, "whole file {} bytes", bytes.len());
}

#[test]
fn text_can_be_extracted() {
    let d = doc(1);
    let pdf = Pdf::load_mem(&export(&d)).unwrap();
    let text = pdf.extract_text(&[1]).unwrap();
    let squashed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for want in ["Chapter one", "rabbit-hole", "sister was reading", "cell d"] {
        assert!(squashed.contains(want), "{want:?} not in {squashed:?}");
    }
}

#[test]
fn export_is_deterministic() {
    assert_eq!(export(&doc(3)), export(&doc(3)));
}

#[test]
fn characters_drawn_with_several_glyphs_carry_their_text_once() {
    // No emoji font is bundled: the family emoji is three missing-glyph boxes, all glyph 0.
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let mut engine = LayoutEngine::new(fonts);
    let text = "a 👨‍👩‍👧 b";
    let base = style();
    let mut paginator = Paginator::new(PageGeometry::letter());
    paginator.layout(
        &mut engine,
        &[ParagraphInput {
            text,
            base: &base,
            spans: &[],
            style: ParagraphStyle::default(),
        }],
    );
    let fonts = std::mem::replace(engine.fonts_mut(), FontLibrary::new());
    let bytes = pdf(&paginator, &fonts, &[text], &PdfInfo::default()).unwrap();
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let id = *pdf.get_pages().values().next().unwrap();
    let ops = Content::decode(&pdf.get_page_content(id))
        .unwrap()
        .operations;
    let spans: Vec<String> = ops
        .iter()
        .filter(|op| op.operator == "BDC")
        .map(|op| {
            let dict = op.operands[1].as_dict().unwrap();
            let raw = dict.get(b"ActualText").unwrap().as_str().unwrap();
            // PDF text strings with non-Latin text are UTF-16BE with a byte-order mark.
            let units: Vec<u16> = raw[2..]
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units).unwrap()
        })
        .collect();
    assert_eq!(spans, vec!["👨‍👩‍👧".to_string()]);
    assert_eq!(ops.iter().filter(|op| op.operator == "EMC").count(), 1);
}
