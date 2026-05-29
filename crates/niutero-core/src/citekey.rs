//! Citation-key generation from a small pattern mini-language.
//!
//! A pattern is literal text interspersed with `{token}`s. Recognized tokens:
//!
//! * `{auth}` — the first author's surname.
//! * `{year}` — the publication year (its digits).
//! * `{title.N}` — the next `N` words of the title (default 1).
//! * `{title-content-word.N}` — like `{title}`, but skips common stop-words.
//!
//! **Casing follows the token's own casing**: `{title}` → lower, `{Title}` →
//! Title-case (first letter up, rest preserved, so `SAEs` stays `SAEs`),
//! `{TITLE}` → UPPER. The same applies to `{auth}`/`{Auth}`/`{AUTH}`.
//!
//! Title tokens share a left-to-right cursor over the title's words, so
//! `{title.1}{Title.2}` takes word 1, then words 2–3. For
//! `Vaswani, Ashish ... 2017 ... Attention Is All You Need`, the default
//! `{auth}{year}{title.1}{Title.2}` yields `vaswani2017attentionIsAll`.
//!
//! Words are reduced to their ASCII alphanumerics (LaTeX-safe keys); an
//! unrecognized `{token}` is emitted verbatim so a typo is visible, not silent.

use crate::BibEntry;

/// A compiled citation-key pattern. Parse once, render per entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPattern {
    tokens: Vec<Token>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Lower,
    Title,
    Upper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Auth(Case),
    Year,
    /// `n` title words, `content` = skip stop-words.
    Title {
        n: usize,
        case: Case,
        content: bool,
    },
}

impl Default for KeyPattern {
    fn default() -> Self {
        Self::parse(Self::DEFAULT)
    }
}

impl KeyPattern {
    /// The built-in pattern, matching the design's default.
    pub const DEFAULT: &'static str = "{auth}{year}{title.1}{Title.2}";

    /// Compile a pattern string. Infallible: literal text passes through and an
    /// unrecognized `{token}` is preserved verbatim (so a typo shows up in the
    /// generated key rather than vanishing).
    pub fn parse(pattern: &str) -> Self {
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let bytes = pattern.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(close) = pattern[i + 1..].find('}') {
                    let inner = &pattern[i + 1..i + 1 + close];
                    match Token::from_spec(inner) {
                        Some(tok) => {
                            if !literal.is_empty() {
                                tokens.push(Token::Literal(std::mem::take(&mut literal)));
                            }
                            tokens.push(tok);
                        }
                        // Unknown token: keep it (with braces) as literal text.
                        None => literal.push_str(&pattern[i..i + 2 + close]),
                    }
                    i += 2 + close;
                    continue;
                }
                // No closing brace: the rest is literal.
                literal.push_str(&pattern[i..]);
                break;
            }
            literal.push(pattern[i..].chars().next().unwrap());
            i += pattern[i..].chars().next().unwrap().len_utf8();
        }
        if !literal.is_empty() {
            tokens.push(Token::Literal(literal));
        }
        KeyPattern { tokens }
    }

    /// Generate the key for `entry`. The result is *not* guaranteed unique
    /// across a library — the caller disambiguates collisions.
    pub fn render(&self, entry: &BibEntry) -> String {
        let surname = first_author_surname(entry.get("author").unwrap_or(""));
        let year: String = entry
            .get("year")
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        let words = title_words(entry.get("title").unwrap_or(""));

        let mut out = String::new();
        let mut cursor = 0; // shared position into `words` for all title tokens
        for tok in &self.tokens {
            match tok {
                Token::Literal(s) => out.push_str(s),
                Token::Auth(case) => out.push_str(&apply_case(&surname, *case)),
                Token::Year => out.push_str(&year),
                Token::Title { n, case, content } => {
                    let mut taken = 0;
                    while taken < *n && cursor < words.len() {
                        let w = &words[cursor];
                        cursor += 1;
                        if *content && is_stop_word(w) {
                            continue; // skip stop-words without consuming the count
                        }
                        out.push_str(&apply_case(w, *case));
                        taken += 1;
                    }
                }
            }
        }
        out
    }
}

impl Token {
    /// Parse the text between `{` and `}`. Returns `None` for an unknown name.
    fn from_spec(inner: &str) -> Option<Token> {
        // Split a trailing numeric `.N` index off the name.
        let (name, n) = match inner.rsplit_once('.') {
            Some((nm, idx)) => match idx.parse::<usize>() {
                Ok(n) => (nm, Some(n)),
                Err(_) => (inner, None),
            },
            None => (inner, None),
        };
        let case = detect_case(name);
        match name.to_ascii_lowercase().as_str() {
            "auth" => Some(Token::Auth(case)),
            "year" => Some(Token::Year),
            "title" => Some(Token::Title {
                n: n.unwrap_or(1),
                case,
                content: false,
            }),
            "title-content-word" => Some(Token::Title {
                n: n.unwrap_or(1),
                case,
                content: true,
            }),
            _ => None,
        }
    }
}

/// The casing the token's own spelling asks for.
fn detect_case(name: &str) -> Case {
    let has_upper = name.chars().any(|c| c.is_uppercase());
    let has_lower = name.chars().any(|c| c.is_lowercase());
    if has_upper && !has_lower {
        Case::Upper
    } else if name.chars().next().is_some_and(char::is_uppercase) {
        Case::Title
    } else {
        Case::Lower
    }
}

fn apply_case(s: &str, case: Case) -> String {
    match case {
        Case::Lower => s.to_lowercase(),
        Case::Upper => s.to_uppercase(),
        // Capitalize the first character, preserve the rest (keeps `SAEs`).
        Case::Title => {
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

/// The first author's surname, reduced to ASCII alphanumerics. Authors are
/// `"Last, First and Last2, First2"`; for `"Last, First"` the surname is
/// `Last`, and for `"First Last"` it is the final whitespace token.
fn first_author_surname(authors: &str) -> String {
    let first = authors.split(" and ").next().unwrap_or("").trim();
    let surname = match first.split_once(',') {
        Some((last, _)) => last.trim(),
        None => first.rsplit(char::is_whitespace).next().unwrap_or(first),
    };
    surname
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Title words, each reduced to its ASCII alphanumerics, empties dropped.
fn title_words(title: &str) -> Vec<String> {
    title
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn is_stop_word(word: &str) -> bool {
    const STOP: &[&str] = &[
        "a", "an", "the", "of", "for", "and", "or", "to", "in", "on", "with", "is", "are", "be",
        "by", "at", "as", "from", "that", "this", "it", "its", "we", "our", "you", "your", "all",
        "can", "do", "does", "via", "into", "but", "if",
    ];
    let lower = word.to_lowercase();
    STOP.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(author: &str, year: &str, title: &str) -> BibEntry {
        BibEntry::new("article", "old")
            .with_field("author", author)
            .with_field("year", year)
            .with_field("title", title)
    }

    #[test]
    fn default_pattern_matches_the_design_example() {
        let e = entry(
            "Vaswani, Ashish and Shazeer, Noam",
            "2017",
            "Attention Is All You Need",
        );
        assert_eq!(
            KeyPattern::default().render(&e),
            "vaswani2017attentionIsAll"
        );
    }

    #[test]
    fn casing_follows_the_token() {
        let e = entry("Arad, Dana", "2025", "SAEs Are Good");
        // {Title.3} preserves internal caps (SAEs) and capitalizes each word.
        assert_eq!(
            KeyPattern::parse("{auth}{year}{Title.3}").render(&e),
            "arad2025SAEsAreGood"
        );
        assert_eq!(KeyPattern::parse("{AUTH}{year}").render(&e), "ARAD2025");
        assert_eq!(
            KeyPattern::parse("{Auth}-{title.2}").render(&e),
            "Arad-saesare"
        );
    }

    #[test]
    fn content_words_skip_stopwords() {
        let e = entry(
            "Hong, Jie",
            "2025",
            "The Reasoning and Memorization Interplay",
        );
        // raw title takes "The"; content-word skips it.
        assert_eq!(
            KeyPattern::parse("{title-content-word.2}").render(&e),
            "reasoningmemorization"
        );
        assert_eq!(KeyPattern::parse("{title.2}").render(&e), "thereasoning");
    }

    #[test]
    fn literal_text_and_punctuation_pass_through() {
        let e = entry("Gao, Leo", "2025", "Scaling Sparse Autoencoders");
        assert_eq!(
            KeyPattern::parse("{auth}_{year}:{title.1}").render(&e),
            "gao_2025:scaling"
        );
    }

    #[test]
    fn missing_fields_yield_empty_parts() {
        let e = BibEntry::new("misc", "x"); // no author/year/title
        assert_eq!(KeyPattern::default().render(&e), "");
        let only_year = BibEntry::new("misc", "x").with_field("year", "{2020}");
        assert_eq!(KeyPattern::parse("{year}").render(&only_year), "2020");
    }

    #[test]
    fn surname_handles_comma_and_plain_forms() {
        let comma = entry("Dupré, Tom", "2020", "T");
        assert_eq!(KeyPattern::parse("{auth}").render(&comma), "dupr"); // ASCII-only
        let plain = entry("Tom Bricken", "2020", "T");
        assert_eq!(KeyPattern::parse("{auth}").render(&plain), "bricken");
    }

    #[test]
    fn unknown_token_is_kept_verbatim() {
        let e = entry("Arad, Dana", "2025", "T");
        assert_eq!(KeyPattern::parse("{auth}{bogus}").render(&e), "arad{bogus}");
        // an unterminated brace is literal too
        assert_eq!(KeyPattern::parse("{auth").render(&e), "{auth");
    }
}
