#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Hit(usize),
    Pending,
    NoMatch,
}

pub fn classify(labels: &[String], typed: &str) -> MatchResult {
    let mut last_match: Option<usize> = None;
    let mut count = 0usize;
    for (i, label) in labels.iter().enumerate() {
        if label.starts_with(typed) {
            count += 1;
            last_match = Some(i);
            if count > 1 {
                return MatchResult::Pending;
            }
        }
    }
    match (count, last_match) {
        (0, _) => MatchResult::NoMatch,
        (1, Some(i)) => MatchResult::Hit(i),
        _ => MatchResult::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Vec<String> {
        vec!["aa".into(), "as".into(), "sa".into()]
    }

    #[test]
    fn empty_prefix_is_pending() {
        assert_eq!(classify(&labels(), ""), MatchResult::Pending);
    }

    #[test]
    fn unique_prefix_hits_before_full_length() {
        assert_eq!(classify(&labels(), "s"), MatchResult::Hit(2));
    }

    #[test]
    fn ambiguous_prefix_is_pending() {
        assert_eq!(classify(&labels(), "a"), MatchResult::Pending);
    }

    #[test]
    fn full_label_hits() {
        assert_eq!(classify(&labels(), "as"), MatchResult::Hit(1));
    }

    #[test]
    fn unknown_prefix_is_no_match() {
        assert_eq!(classify(&labels(), "z"), MatchResult::NoMatch);
    }
}
