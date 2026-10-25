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

/// The UAX #29 word segments of `text` as char ranges, each flagged whether it's a word
/// (holds a letter or digit) rather than spaces or punctuation.
pub(crate) fn word_segments(text: &str) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for seg in text.split_word_bounds() {
        let n = seg.chars().count();
        out.push((pos..pos + n, seg.chars().any(char::is_alphanumeric)));
        pos += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_follow_uax29() {
        let segs: Vec<(usize, usize, bool)> = word_segments("can't stop, 3.5 é")
            .into_iter()
            .map(|(r, w)| (r.start, r.end, w))
            .collect();
        // "can't" is one word, "3.5" one number, punctuation and spaces are not words.
        assert_eq!(segs[0], (0, 5, true));
        assert!(segs.contains(&(12, 15, true)));
        assert!(segs.contains(&(10, 11, false)));
    }

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
