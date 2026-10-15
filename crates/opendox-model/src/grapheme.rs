//! Grapheme-cluster boundaries expressed as char indices.

use unicode_segmentation::UnicodeSegmentation;

/// Char indices of every grapheme boundary in `text`, including 0 and the char length.
///
/// O(n) over the whole string. Fine while documents are small; once layout caches paragraphs
/// this should run per paragraph instead.
pub(crate) fn boundaries(text: &str) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.len() + 1);
    let mut pos = 0;
    out.push(0);
    for g in text.graphemes(true) {
        pos += g.chars().count();
        out.push(pos);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_has_a_boundary_between_every_char() {
        assert_eq!(boundaries("abc"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn combining_mark_stays_with_its_letter() {
        // "e" + COMBINING ACUTE ACCENT is one grapheme made of two chars.
        assert_eq!(boundaries("e\u{301}x"), vec![0, 2, 3]);
    }

    #[test]
    fn zwj_emoji_is_one_grapheme() {
        // Family emoji: 5 chars (3 people joined by 2 zero-width joiners).
        assert_eq!(boundaries("a👨‍👩‍👧b"), vec![0, 1, 6, 7]);
    }

    #[test]
    fn crlf_is_one_grapheme() {
        assert_eq!(boundaries("a\r\nb"), vec![0, 1, 3, 4]);
    }

    #[test]
    fn empty_text_has_one_boundary() {
        assert_eq!(boundaries(""), vec![0]);
    }
}
