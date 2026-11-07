//! Natural, Unicode-aware filename comparison.
//!
//! This is deliberately a small, dependency-light implementation rather than a
//! locale-dependent collation library. It compares runs of ASCII digits
//! numerically, and all other text using canonical Unicode normalization plus
//! case-insensitive primary comparison and a case-sensitive tie-break.

use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Num {
        significant: String,
        leading_zeros: usize,
    },
    Text {
        folded: String,
        original: String,
    },
}

impl Token {
    fn first_char(&self) -> char {
        match self {
            Token::Num { significant, .. } => significant.chars().next().unwrap_or('0'),
            Token::Text { folded, .. } => folded.chars().next().unwrap_or('\0'),
        }
    }
}

fn tokens(value: &str) -> Vec<Token> {
    let normalized: String = value.nfc().collect();
    let chars: Vec<char> = normalized.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            let trimmed = run.trim_start_matches('0');
            let significant = if trimmed.is_empty() {
                "0".to_owned()
            } else {
                trimmed.to_owned()
            };
            let leading_zeros = run.len() - trimmed.len();
            result.push(Token::Num {
                significant,
                leading_zeros,
            });
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_ascii_digit() {
                i += 1;
            }
            let original: String = chars[start..i].iter().collect();
            let folded = original.to_lowercase();
            result.push(Token::Text { folded, original });
        }
    }

    result
}

fn cmp_num(a_sig: &str, a_zeros: usize, b_sig: &str, b_zeros: usize) -> Ordering {
    // Comparing the digit count first avoids integer overflow on absurdly long
    // numeric runs, while still giving numeric ordering.
    a_sig
        .len()
        .cmp(&b_sig.len())
        .then_with(|| a_sig.cmp(b_sig))
        .then_with(|| a_zeros.cmp(&b_zeros))
}

/// Compare two filenames naturally.
///
/// `file2` sorts before `file10`. Comparison is case-insensitive first, then
/// case-sensitive as a deterministic tie-break. Canonical Unicode
/// normalization makes equivalent composed/decomposed names compare equally in
/// their primary form.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let ta = tokens(a);
    let tb = tokens(b);

    for (x, y) in ta.iter().zip(tb.iter()) {
        let order = match (x, y) {
            (
                Token::Num {
                    significant: asig,
                    leading_zeros: az,
                },
                Token::Num {
                    significant: bsig,
                    leading_zeros: bz,
                },
            ) => cmp_num(asig, *az, bsig, *bz),
            (
                Token::Text {
                    folded: af,
                    original: ao,
                },
                Token::Text {
                    folded: bf,
                    original: bo,
                },
            ) => af.cmp(bf).then_with(|| ao.cmp(bo)),
            _ => x.first_char().cmp(&y.first_char()),
        };

        if order != Ordering::Equal {
            return order;
        }
    }

    ta.len().cmp(&tb.len()).then_with(|| {
        let an: String = a.nfc().collect();
        let bn: String = b.nfc().collect();
        an.cmp(&bn)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn numbers_are_natural() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file10", "file2"), Ordering::Greater);
        assert_eq!(natural_cmp("a1b2", "a1b10"), Ordering::Less);
        assert_eq!(natural_cmp("2.txt", "10.txt"), Ordering::Less);
    }

    #[test]
    fn equal_values_with_leading_zeroes_are_stable() {
        assert_eq!(natural_cmp("file1", "file001"), Ordering::Less);
        assert_eq!(natural_cmp("file001", "file1"), Ordering::Greater);
        assert_eq!(natural_cmp("file01", "file01"), Ordering::Equal);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(
            natural_cmp(
                "x99999999999999999999999999999999999999",
                "x100000000000000000000000000000000000000"
            ),
            Ordering::Less
        );
    }

    #[test]
    fn case_is_primary_insensitive_and_then_deterministic() {
        assert_eq!(natural_cmp("apple", "Banana"), Ordering::Less);
        assert_eq!(natural_cmp("Apple", "apple"), Ordering::Less);
        assert_eq!(natural_cmp("apple", "Apple"), Ordering::Greater);
    }

    #[test]
    fn unicode_is_handled_without_panicking() {
        assert_eq!(natural_cmp("café", "cafe"), Ordering::Greater);
        assert_eq!(natural_cmp("日本語2", "日本語10"), Ordering::Less);
        assert_eq!(natural_cmp("é", "e"), Ordering::Greater);
        // Canonically equivalent spellings compare equal in the primary form.
        assert_eq!(natural_cmp("caf\u{e9}", "cafe\u{301}"), Ordering::Equal);
    }

    #[test]
    fn punctuation_and_prefixes_are_sensible() {
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "a-1"), Ordering::Less);
        assert_eq!(natural_cmp("a-1", "a1"), Ordering::Greater);
        assert_eq!(natural_cmp("a b", "a-b"), Ordering::Less);
    }
}
