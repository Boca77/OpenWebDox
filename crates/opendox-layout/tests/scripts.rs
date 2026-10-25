//! Emoji and CJK through fallback fonts: drawn with real glyphs instead of the missing-glyph
//! box. Emoji use the bundled Noto Emoji; the CJK test runs when `npm run fetch-fonts` has
//! downloaded the CJK fonts (too big for the repo) and says so when it skips.

use opendox_layout::{FontId, FontLibrary, LayoutEngine, ParagraphLayout, TextStyle};

const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");
const EMOJI: &[u8] = include_bytes!("../../../fonts/noto/NotoEmoji-Regular.ttf");

fn style(family: &str) -> TextStyle {
    TextStyle {
        font_family: family.into(),
        ..TextStyle::default()
    }
}

fn glyphs(l: &ParagraphLayout) -> Vec<(FontId, u32)> {
    l.lines
        .iter()
        .flat_map(|l| &l.runs)
        .flat_map(|r| r.glyphs.iter().map(move |g| (r.font, g.id)))
        .collect()
}

#[test]
fn emoji_get_real_glyphs_and_a_zwj_family_is_one() {
    let mut fonts = FontLibrary::new();
    let sans = fonts.register(REGULAR.to_vec());
    let emoji = fonts.register(EMOJI.to_vec());
    let mut e = LayoutEngine::new(fonts);
    let text = "hi 😀 👨‍👩‍👧";
    let without = e.layout_paragraph(text, &style("Noto Sans"), &[], 400.0);
    assert!(
        glyphs(&without).iter().any(|g| g.1 == 0),
        "boxes without an emoji font"
    );
    let with = e.layout_paragraph(text, &style("Noto Sans, Noto Emoji"), &[], 400.0);
    let g = glyphs(&with);
    assert!(g.iter().all(|g| g.1 != 0), "{g:?}");
    let emoji_glyphs: Vec<_> = g.iter().filter(|g| g.0 == emoji).collect();
    // 😀 and the family (5 chars joined by ZWJs) are one glyph each.
    assert_eq!(emoji_glyphs.len(), 2, "{g:?}");
    assert!(g.iter().filter(|g| g.0 == sans).count() >= 3);
    // The emoji's line is no taller than Noto Sans text: no jump when emoji are typed.
    let plain = e.layout_paragraph("hi", &style("Noto Sans, Noto Emoji"), &[], 400.0);
    assert!(
        with.lines[0].bottom - with.lines[0].top
            <= plain.lines[0].bottom - plain.lines[0].top + 0.5
    );
}

#[test]
fn cjk_text_uses_the_cjk_fonts_when_present() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts/noto/");
    let (Ok(sc), Ok(kr)) = (
        std::fs::read(format!("{dir}NotoSansSC-Regular.otf")),
        std::fs::read(format!("{dir}NotoSansKR-Regular.otf")),
    ) else {
        eprintln!("skipped: CJK fonts not downloaded (npm run fetch-fonts)");
        return;
    };
    let mut fonts = FontLibrary::new();
    fonts.register(REGULAR.to_vec());
    let sc = fonts.register(sc);
    let kr = fonts.register(kr);
    let mut e = LayoutEngine::new(fonts);
    let l = e.layout_paragraph(
        "你好，世界 안녕하세요",
        &style("Noto Sans, Noto Sans SC, Noto Sans KR"),
        &[],
        400.0,
    );
    let g = glyphs(&l);
    assert!(g.iter().all(|g| g.1 != 0), "{g:?}");
    assert!(g.iter().any(|g| g.0 == sc) && g.iter().any(|g| g.0 == kr));
    // CJK breaks between characters: a narrow paragraph wraps without overflowing.
    let narrow = e.layout_paragraph(
        "我能吞下玻璃而不伤身体我能吞下玻璃而不伤身体",
        &style("Noto Sans, Noto Sans SC"),
        &[],
        60.0,
    );
    assert!(narrow.lines.len() > 3);
    assert!(narrow.lines.iter().all(|l| l.advance <= 60.0 + 0.5));
}
