use std::collections::VecDeque;

pub const ALPHABET: &[char] = &[
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 'u', 'i', 'o', 'p', 't', 'y',
    'n', 'm', 'b', 'v', 'c', 'x', 'z',
];

const RESERVE: usize = 6;

pub struct LabelAllocator {
    free: VecDeque<String>,
}

impl Default for LabelAllocator {
    fn default() -> LabelAllocator {
        LabelAllocator::new()
    }
}

impl LabelAllocator {
    pub fn new() -> LabelAllocator {
        LabelAllocator {
            free: ALPHABET.iter().map(|c| c.to_string()).collect(),
        }
    }

    pub fn allocate(&mut self) -> String {
        if self.free.len() <= RESERVE {
            self.expand();
        }
        self.free
            .pop_front()
            .expect("expand keeps the label pool non-empty")
    }

    fn expand(&mut self) {
        let prefixes: Vec<String> = self.free.drain(..).collect();
        for c in ALPHABET {
            for prefix in &prefixes {
                self.free.push_back(format!("{prefix}{c}"));
            }
        }
    }
}

pub fn generate(n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    if n <= ALPHABET.len() {
        return ALPHABET.iter().take(n).map(|c| c.to_string()).collect();
    }

    let mut len = 1usize;
    let mut capacity = ALPHABET.len();
    while capacity < n {
        len += 1;
        capacity *= ALPHABET.len();
    }

    (0..n).map(|i| nth_label(i, len)).collect()
}

fn nth_label(mut index: usize, len: usize) -> String {
    let base = ALPHABET.len();
    let mut chars = vec!['a'; len];
    for slot in (0..len).rev() {
        chars[slot] = ALPHABET[index % base];
        index /= base;
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_for_zero() {
        assert!(generate(0).is_empty());
    }

    #[test]
    fn single_chars_under_alphabet() {
        let labels = generate(3);
        assert_eq!(labels, vec!["a", "s", "d"]);
    }

    #[test]
    fn full_alphabet_stays_single_char() {
        let labels = generate(ALPHABET.len());
        assert_eq!(labels.len(), ALPHABET.len());
        assert!(labels.iter().all(|l| l.chars().count() == 1));
    }

    #[test]
    fn multi_char_is_fixed_length_and_prefix_free() {
        let n = ALPHABET.len() + 5;
        let labels = generate(n);
        assert_eq!(labels.len(), n);
        let len = labels[0].chars().count();
        assert!(len >= 2);
        assert!(labels.iter().all(|l| l.chars().count() == len));
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn home_row_comes_first() {
        let labels = generate(2);
        assert_eq!(labels[0], "a");
        assert_eq!(labels[1], "s");
    }

    #[test]
    fn allocator_hands_out_single_chars_first() {
        let mut alloc = LabelAllocator::new();
        let first: Vec<String> = (0..20).map(|_| alloc.allocate()).collect();
        assert!(first.iter().all(|l| l.chars().count() == 1));
        assert_eq!(first[0], "a");
        assert_eq!(first[1], "s");
    }

    #[test]
    fn allocator_labels_are_prefix_free_and_unique() {
        let mut alloc = LabelAllocator::new();
        let labels: Vec<String> = (0..300).map(|_| alloc.allocate()).collect();
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a), "{a} is a prefix of {b}");
                }
            }
        }
    }

    #[test]
    fn allocator_gives_consecutive_multi_char_labels_different_first_letters() {
        let mut alloc = LabelAllocator::new();
        for _ in 0..20 {
            alloc.allocate();
        }
        let overflow: Vec<String> = (0..12).map(|_| alloc.allocate()).collect();
        assert!(overflow.iter().all(|l| l.chars().count() == 2));
        for pair in overflow.windows(2) {
            assert_ne!(
                pair[0].chars().next(),
                pair[1].chars().next(),
                "adjacent labels {} and {} share a first letter",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn allocator_keeps_expanding_past_two_char_capacity() {
        let mut alloc = LabelAllocator::new();
        let labels: Vec<String> = (0..500).map(|_| alloc.allocate()).collect();
        let unique: std::collections::HashSet<&String> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }
}
