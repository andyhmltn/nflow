pub const ALPHABET: &[char] = &[
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 'u', 'i', 'o', 'p', 't', 'y',
    'n', 'm', 'b', 'v', 'c', 'x', 'z',
];

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
}
