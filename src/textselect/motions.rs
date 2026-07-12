#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharRect {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub h: f64,
}

pub fn nearest_in_band(rects: &[CharRect], head: &CharRect, down: bool) -> Option<usize> {
    let line_h = head.h.max(1.0);
    rects
        .iter()
        .filter_map(|r| {
            let rank = ((r.y - head.y) / line_h).round() as i64;
            let on_target_side = if down { rank >= 1 } else { rank <= -1 };
            if on_target_side {
                Some((rank.unsigned_abs(), (r.x - head.x).abs(), r.index))
            } else {
                None
            }
        })
        .min_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map(|(_, _, index)| index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Blank,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Blank
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

pub fn char_left(head: usize) -> usize {
    head.saturating_sub(1)
}

pub fn char_right(chars: &[char], head: usize) -> usize {
    let last = chars.len().saturating_sub(1);
    (head + 1).min(last)
}

pub fn next_word_start(chars: &[char], head: usize) -> usize {
    let n = chars.len();
    if n == 0 {
        return 0;
    }
    let mut i = head;
    if class(chars[i]) != Class::Blank {
        let start = class(chars[i]);
        while i < n && class(chars[i]) == start {
            i += 1;
        }
    }
    while i < n && class(chars[i]) == Class::Blank {
        i += 1;
    }
    if i >= n {
        n - 1
    } else {
        i
    }
}

pub fn word_end(chars: &[char], head: usize) -> usize {
    let n = chars.len();
    if n == 0 {
        return 0;
    }
    let mut i = head + 1;
    while i < n && class(chars[i]) == Class::Blank {
        i += 1;
    }
    if i >= n {
        return n - 1;
    }
    let cls = class(chars[i]);
    while i + 1 < n && class(chars[i + 1]) == cls {
        i += 1;
    }
    i
}

pub fn prev_word_start(chars: &[char], head: usize) -> usize {
    if head == 0 || chars.is_empty() {
        return 0;
    }
    let mut i = head - 1;
    while i > 0 && class(chars[i]) == Class::Blank {
        i -= 1;
    }
    if class(chars[i]) == Class::Blank {
        return 0;
    }
    let cls = class(chars[i]);
    while i > 0 && class(chars[i - 1]) == cls {
        i -= 1;
    }
    i
}

pub fn line_start(chars: &[char], head: usize) -> usize {
    let mut i = head.min(chars.len());
    while i > 0 && chars[i - 1] != '\n' {
        i -= 1;
    }
    i
}

pub fn first_non_blank(chars: &[char], head: usize) -> usize {
    let start = line_start(chars, head);
    let mut i = start;
    while i < chars.len() && chars[i] != '\n' && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

pub fn line_end(chars: &[char], head: usize) -> usize {
    let start = line_start(chars, head);
    let n = chars.len();
    let mut i = head.min(n);
    while i < n && chars[i] != '\n' {
        i += 1;
    }
    if i > start {
        i - 1
    } else {
        start
    }
}

pub fn line_down(chars: &[char], head: usize) -> usize {
    let col = head - line_start(chars, head);
    let end = line_end_index(chars, head);
    let n = chars.len();
    if end >= n {
        return head;
    }
    let next_start = end + 1;
    let next_end = line_end_index(chars, next_start);
    let last = next_end.saturating_sub(1);
    (next_start + col).min(last.max(next_start))
}

pub fn line_up(chars: &[char], head: usize) -> usize {
    let start = line_start(chars, head);
    let col = head - start;
    if start == 0 {
        return head;
    }
    let prev_end = start - 1;
    let prev_start = line_start(chars, prev_end);
    (prev_start + col).min(prev_end)
}

pub fn find_forward(chars: &[char], head: usize, target: char) -> usize {
    let n = chars.len();
    let mut i = head + 1;
    while i < n {
        if chars[i] == target {
            return i;
        }
        i += 1;
    }
    head
}

pub fn till_forward(chars: &[char], head: usize, target: char) -> usize {
    let found = find_forward(chars, head, target);
    if found > head {
        found - 1
    } else {
        head
    }
}

pub fn char_index_to_utf16(chars: &[char], idx: usize) -> usize {
    chars[..idx.min(chars.len())]
        .iter()
        .map(|c| c.len_utf16())
        .sum()
}

pub fn selection_range(chars: &[char], anchor: usize, head: usize) -> (usize, usize) {
    let lo = anchor.min(head);
    let hi = anchor.max(head);
    let location = char_index_to_utf16(chars, lo);
    let end = char_index_to_utf16(chars, hi + 1);
    (location, end - location)
}

fn line_end_index(chars: &[char], head: usize) -> usize {
    let n = chars.len();
    let mut i = head.min(n);
    while i < n && chars[i] != '\n' {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cv(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn two_line_rects() -> Vec<CharRect> {
        let mut rects = Vec::new();
        for i in 0..5 {
            rects.push(CharRect {
                index: i,
                x: (i as f64) * 10.0,
                y: 0.0,
                h: 10.0,
            });
        }
        for i in 0..5 {
            rects.push(CharRect {
                index: 5 + i,
                x: (i as f64) * 10.0,
                y: 12.0,
                h: 10.0,
            });
        }
        rects
    }

    #[test]
    fn nearest_in_band_moves_down_preserving_column() {
        let rects = two_line_rects();
        let head = rects[2];
        assert_eq!(nearest_in_band(&rects, &head, true), Some(7));
    }

    #[test]
    fn nearest_in_band_moves_up_preserving_column() {
        let rects = two_line_rects();
        let head = rects[7];
        assert_eq!(nearest_in_band(&rects, &head, false), Some(2));
    }

    #[test]
    fn nearest_in_band_none_when_no_adjacent_line() {
        let rects = two_line_rects();
        let head = rects[7];
        assert_eq!(nearest_in_band(&rects, &head, true), None);
    }

    #[test]
    fn nearest_in_band_snaps_to_closest_x_on_shorter_line() {
        let mut rects = vec![CharRect {
            index: 0,
            x: 80.0,
            y: 0.0,
            h: 10.0,
        }];
        rects.push(CharRect {
            index: 1,
            x: 0.0,
            y: 12.0,
            h: 10.0,
        });
        rects.push(CharRect {
            index: 2,
            x: 20.0,
            y: 12.0,
            h: 10.0,
        });
        let head = rects[0];
        assert_eq!(nearest_in_band(&rects, &head, true), Some(2));
    }

    #[test]
    fn char_left_clamps_at_zero() {
        assert_eq!(char_left(0), 0);
        assert_eq!(char_left(3), 2);
    }

    #[test]
    fn char_right_clamps_at_last_char() {
        let c = cv("abc");
        assert_eq!(char_right(&c, 0), 1);
        assert_eq!(char_right(&c, 2), 2);
    }

    #[test]
    fn next_word_start_crosses_space() {
        let c = cv("hello world");
        assert_eq!(next_word_start(&c, 0), 6);
    }

    #[test]
    fn next_word_start_on_last_word_goes_to_last_char() {
        let c = cv("hello world");
        assert_eq!(next_word_start(&c, 6), 10);
    }

    #[test]
    fn next_word_start_treats_punct_as_its_own_word() {
        let c = cv("foo.bar");
        assert_eq!(next_word_start(&c, 0), 3);
        assert_eq!(next_word_start(&c, 3), 4);
    }

    #[test]
    fn word_end_lands_on_last_char_of_current_word() {
        let c = cv("hello world");
        assert_eq!(word_end(&c, 0), 4);
    }

    #[test]
    fn word_end_jumps_to_next_word_when_already_at_end() {
        let c = cv("hello world");
        assert_eq!(word_end(&c, 4), 10);
    }

    #[test]
    fn prev_word_start_from_mid_second_word() {
        let c = cv("hello world");
        assert_eq!(prev_word_start(&c, 8), 6);
    }

    #[test]
    fn prev_word_start_crosses_space_to_previous_word() {
        let c = cv("hello world");
        assert_eq!(prev_word_start(&c, 6), 0);
    }

    #[test]
    fn line_start_and_end_within_multiline() {
        let c = cv("ab\ncde\nf");
        assert_eq!(line_start(&c, 4), 3);
        assert_eq!(line_end(&c, 4), 5);
        assert_eq!(line_start(&c, 0), 0);
        assert_eq!(line_end(&c, 0), 1);
    }

    #[test]
    fn first_non_blank_skips_leading_spaces() {
        let c = cv("  hi\n  yo");
        assert_eq!(first_non_blank(&c, 0), 2);
        assert_eq!(first_non_blank(&c, 6), 7);
    }

    #[test]
    fn line_down_preserves_column() {
        let c = cv("abcd\nef\nghij");
        assert_eq!(line_down(&c, 2), 6);
    }

    #[test]
    fn line_down_clamps_column_to_shorter_line() {
        let c = cv("abcd\nef\nghij");
        assert_eq!(line_down(&c, 3), 6);
    }

    #[test]
    fn line_up_preserves_column() {
        let c = cv("abcd\nefgh");
        assert_eq!(line_up(&c, 7), 2);
    }

    #[test]
    fn find_forward_lands_on_target() {
        let c = cv("hello world");
        assert_eq!(find_forward(&c, 0, 'o'), 4);
        assert_eq!(find_forward(&c, 4, 'o'), 7);
    }

    #[test]
    fn find_forward_no_match_keeps_head() {
        let c = cv("hello");
        assert_eq!(find_forward(&c, 0, 'z'), 0);
    }

    #[test]
    fn till_forward_lands_before_target() {
        let c = cv("hello world");
        assert_eq!(till_forward(&c, 0, ' '), 4);
    }

    #[test]
    fn utf16_conversion_counts_surrogate_pairs() {
        let c = cv("a\u{1F600}b");
        assert_eq!(char_index_to_utf16(&c, 0), 0);
        assert_eq!(char_index_to_utf16(&c, 1), 1);
        assert_eq!(char_index_to_utf16(&c, 2), 3);
        assert_eq!(char_index_to_utf16(&c, 3), 4);
    }

    #[test]
    fn selection_range_is_inclusive_of_head_char() {
        let c = cv("hello world");
        assert_eq!(selection_range(&c, 0, 4), (0, 5));
        assert_eq!(selection_range(&c, 6, 10), (6, 5));
    }

    #[test]
    fn selection_range_in_utf16_units() {
        let c = cv("a\u{1F600}b");
        assert_eq!(selection_range(&c, 0, 2), (0, 4));
    }
}
