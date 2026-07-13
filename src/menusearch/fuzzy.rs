//! Fuzzy subsequence matcher with fzf-style scoring.
//!
//! The matcher finds the best-scoring subsequence alignment of `query` inside
//! `text` (both compared case-insensitively) and reports the matched character
//! positions so the overlay can highlight them. Scoring rewards word-boundary
//! matches, contiguous runs, and early matches; it penalises skipped gaps.

#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyMatch {
    pub score: i64,
    pub positions: Vec<usize>,
}

const SCORE_MATCH: i64 = 16;
const SCORE_GAP: i64 = -1;
const SCORE_LEADING_GAP: i64 = -1;
const BONUS_CONSECUTIVE: i64 = 8;
const BONUS_BOUNDARY: i64 = 10;
const BONUS_FIRST_CHAR: i64 = 6;

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_boundary(prev: Option<char>, cur: char) -> bool {
    match prev {
        None => true,
        Some(p) => {
            (!is_word_char(p) && is_word_char(cur))
                || (p.is_lowercase() && cur.is_uppercase())
                || (p.is_ascii_digit() && !cur.is_ascii_digit())
        }
    }
}

/// Returns the best fuzzy match of `query` inside `text`, or `None` if `query`
/// is not a subsequence of `text`. An empty query is treated as "matches
/// everything with zero score and no positions".
pub fn match_query(query: &str, text: &str) -> Option<FuzzyMatch> {
    let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();

    if q.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    if t.is_empty() || q.len() > t.len() {
        return None;
    }

    let n = q.len();
    let m = t.len();
    let neg_inf = i64::MIN / 4;

    // dp[i][j] = best score matching q[0..=i] ending with q[i] matched at t[j].
    let mut dp = vec![vec![neg_inf; m]; n];
    let mut back: Vec<Vec<Option<usize>>> = vec![vec![None; m]; n];

    for j in 0..m {
        if t[j] != q[0] {
            continue;
        }
        let bonus = if j == 0 {
            BONUS_FIRST_CHAR + BONUS_BOUNDARY
        } else {
            SCORE_LEADING_GAP * j as i64
                + if is_boundary(Some(t[j - 1]), t[j]) {
                    BONUS_BOUNDARY
                } else {
                    0
                }
        };
        dp[0][j] = SCORE_MATCH + bonus;
    }

    for i in 1..n {
        for j in i..m {
            if t[j] != q[i] {
                continue;
            }
            let mut best = neg_inf;
            let mut best_k = None;
            for (k, &prev) in dp[i - 1].iter().take(j).enumerate().skip(i - 1) {
                if prev == neg_inf {
                    continue;
                }
                let gap = (j - k - 1) as i64 * SCORE_GAP;
                let consec = if k + 1 == j { BONUS_CONSECUTIVE } else { 0 };
                let s = prev + gap + consec;
                if s > best {
                    best = s;
                    best_k = Some(k);
                }
            }
            if best == neg_inf {
                continue;
            }
            let boundary = if is_boundary(if j == 0 { None } else { Some(t[j - 1]) }, t[j]) {
                BONUS_BOUNDARY
            } else {
                0
            };
            back[i][j] = best_k;
            dp[i][j] = best + SCORE_MATCH + boundary;
        }
    }

    let last_row = &dp[n - 1];
    let (end, &best_score) = last_row
        .iter()
        .enumerate()
        .rev()
        .max_by_key(|(_, s)| *s)
        .filter(|(_, s)| **s != neg_inf)?;
    if best_score == neg_inf {
        return None;
    }

    let mut positions = Vec::with_capacity(n);
    positions.push(end);
    let mut i = n - 1;
    let mut j = end;
    while i > 0 {
        let k = back[i][j]?; // The previous match position.
        positions.push(k);
        i -= 1;
        j = k;
    }
    positions.reverse();

    Some(FuzzyMatch {
        score: best_score,
        positions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        let m = match_query("", "anything").unwrap();
        assert!(m.positions.is_empty());
        assert_eq!(m.score, 0);
    }

    #[test]
    fn exact_match_scores_high() {
        let m = match_query("save", "Save").unwrap();
        assert_eq!(m.positions, vec![0, 1, 2, 3]);
    }

    #[test]
    fn subsequence_matches_out_of_order_is_rejected() {
        assert!(match_query("abc", "cab").is_none());
    }

    #[test]
    fn subsequence_match_succeeds_in_order() {
        let m = match_query("fs", "File > Save").unwrap();
        assert_eq!(m.positions.len(), 2);
    }

    #[test]
    fn prefers_word_boundary_match() {
        // "sa" at the start of "Save" (a word boundary) should beat "sa"
        // buried mid-word inside "basalt" (no boundary, leading gap).
        let boundary = match_query("sa", "Save").unwrap();
        let interior = match_query("sa", "basalt").unwrap();
        assert!(boundary.score > interior.score);
    }

    #[test]
    fn prefers_contiguous_match() {
        let contig = match_query("save", "Save").unwrap();
        let spread = match_query("save", "s o m e a v e").unwrap();
        assert!(contig.score > spread.score);
    }

    #[test]
    fn no_match_when_query_longer_than_text() {
        assert!(match_query("toolong", "ab").is_none());
    }

    #[test]
    fn case_insensitive() {
        let m = match_query("FILE", "File > Save").unwrap();
        assert_eq!(m.positions, vec![0, 1, 2, 3]);
    }

    #[test]
    fn missing_char_is_no_match() {
        assert!(match_query("xyz", "File > Save").is_none());
    }
}
