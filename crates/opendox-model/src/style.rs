//! Paragraph styles: Normal text, Title, Subtitle and Heading 1–6, with Google Docs' default
//! looks. A paragraph stores only its style's name ([`crate::ParagraphFormat::style`]); what
//! the style looks like is this table, so every replica and every export agrees.

/// A paragraph style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum StyleName {
    #[default]
    Normal,
    Title,
    Subtitle,
    /// Heading level 1–6.
    Heading(u8),
}

/// How a style's text looks where the text has no format of its own. Lengths in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyleLook {
    /// Font size; `None` keeps the document's default size.
    pub size: Option<f32>,
    /// `0xRRGGBB`; `None` keeps the default colour.
    pub color: Option<u32>,
    pub italic: bool,
    /// Space before and after the paragraph, used where the paragraph sets none itself.
    pub space_before: f32,
    pub space_after: f32,
}

impl StyleName {
    /// Every style, in menu order.
    pub const ALL: [StyleName; 9] = [
        StyleName::Normal,
        StyleName::Title,
        StyleName::Subtitle,
        StyleName::Heading(1),
        StyleName::Heading(2),
        StyleName::Heading(3),
        StyleName::Heading(4),
        StyleName::Heading(5),
        StyleName::Heading(6),
    ];

    /// The stored name: `normal`, `title`, `subtitle`, `h1` … `h6`.
    pub fn key(self) -> &'static str {
        match self {
            StyleName::Normal => "normal",
            StyleName::Title => "title",
            StyleName::Subtitle => "subtitle",
            StyleName::Heading(1) => "h1",
            StyleName::Heading(2) => "h2",
            StyleName::Heading(3) => "h3",
            StyleName::Heading(4) => "h4",
            StyleName::Heading(5) => "h5",
            StyleName::Heading(_) => "h6",
        }
    }

    /// The style a stored name stands for; unknown names are `None`.
    pub fn parse(key: &str) -> Option<StyleName> {
        StyleName::ALL.into_iter().find(|s| s.key() == key)
    }

    /// The outline level a heading appears at (titles above headings), or `None` for text
    /// that isn't a heading.
    pub fn outline_level(self) -> Option<u8> {
        match self {
            StyleName::Normal | StyleName::Subtitle => None,
            StyleName::Title => Some(0),
            StyleName::Heading(n) => Some(n.clamp(1, 6)),
        }
    }

    /// Google Docs' default look for the style.
    pub fn look(self) -> StyleLook {
        const GREY: u32 = 0x666666;
        let look = |size: f32, color: Option<u32>, before: f32, after: f32| StyleLook {
            size: Some(size),
            color,
            italic: false,
            space_before: before,
            space_after: after,
        };
        match self {
            StyleName::Normal => StyleLook {
                size: None,
                color: None,
                italic: false,
                space_before: 0.0,
                space_after: 0.0,
            },
            StyleName::Title => look(26.0, None, 0.0, 3.0),
            StyleName::Subtitle => look(15.0, Some(GREY), 0.0, 16.0),
            StyleName::Heading(1) => look(20.0, None, 20.0, 6.0),
            StyleName::Heading(2) => look(16.0, None, 18.0, 6.0),
            StyleName::Heading(3) => look(14.0, Some(0x434343), 16.0, 4.0),
            StyleName::Heading(4) => look(12.0, Some(GREY), 14.0, 4.0),
            StyleName::Heading(5) => look(11.0, Some(GREY), 12.0, 4.0),
            StyleName::Heading(_) => StyleLook {
                italic: true,
                ..look(11.0, Some(GREY), 12.0, 4.0)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for s in StyleName::ALL {
            assert_eq!(StyleName::parse(s.key()), Some(s));
        }
        assert_eq!(StyleName::parse("h7"), None);
    }
}
