#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    pub target: usize,
    pub start: usize,
    pub len: usize,
}

fn char_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

pub fn find_matches(texts: &[Vec<char>], query: &str) -> Vec<TextMatch> {
    let needle: Vec<char> = query.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (target, chars) in texts.iter().enumerate() {
        if needle.len() > chars.len() {
            continue;
        }
        let mut start = 0;
        while start + needle.len() <= chars.len() {
            let matched = chars[start..start + needle.len()]
                .iter()
                .zip(needle.iter())
                .all(|(&c, &n)| char_eq_ci(c, n));
            if matched {
                out.push(TextMatch {
                    target,
                    start,
                    len: needle.len(),
                });
            }
            start += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[&str]) -> Vec<Vec<char>> {
        items.iter().map(|s| s.chars().collect()).collect()
    }

    #[test]
    fn empty_query_finds_nothing() {
        let t = texts(&["hello"]);
        assert_eq!(find_matches(&t, ""), vec![]);
    }

    #[test]
    fn finds_single_occurrence() {
        let t = texts(&["hello world"]);
        assert_eq!(
            find_matches(&t, "world"),
            vec![TextMatch {
                target: 0,
                start: 6,
                len: 5
            }]
        );
    }

    #[test]
    fn is_case_insensitive() {
        let t = texts(&["Fix the PR-123 bug"]);
        assert_eq!(
            find_matches(&t, "pr-123"),
            vec![TextMatch {
                target: 0,
                start: 8,
                len: 6
            }]
        );
    }

    #[test]
    fn finds_multiple_occurrences_in_one_text() {
        let t = texts(&["ab ab"]);
        assert_eq!(
            find_matches(&t, "ab"),
            vec![
                TextMatch {
                    target: 0,
                    start: 0,
                    len: 2
                },
                TextMatch {
                    target: 0,
                    start: 3,
                    len: 2
                },
            ]
        );
    }

    #[test]
    fn spans_multiple_texts() {
        let t = texts(&["nothing here", "the cat", "a cat sat"]);
        assert_eq!(
            find_matches(&t, "cat"),
            vec![
                TextMatch {
                    target: 1,
                    start: 4,
                    len: 3
                },
                TextMatch {
                    target: 2,
                    start: 2,
                    len: 3
                },
            ]
        );
    }

    #[test]
    fn query_length_is_in_chars_not_bytes() {
        let t = texts(&["x \u{1F600}y z"]);
        assert_eq!(
            find_matches(&t, "\u{1F600}y"),
            vec![TextMatch {
                target: 0,
                start: 2,
                len: 2
            }]
        );
    }
}
