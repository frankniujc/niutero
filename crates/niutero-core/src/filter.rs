//! Query matching for entries. Pure logic; the caller supplies the sidecar
//! facets (tags / status / stars), so this crate stays free of any IO.

use crate::BibEntry;

/// The sidecar-derived facets a query can match against, alongside the entry's
/// own bibliographic fields. The engine fills these from the vault; keeping
/// them as plain primitives lets `niutero-core` avoid depending on the vault.
#[derive(Default)]
pub struct Facets<'a> {
    pub tags: &'a [String],
    /// Effective reading-status name (`reading` / `done`); `None` == unread.
    pub status: Option<&'a str>,
    /// Star rating 1–5; `None` == unrated (compared as 0).
    pub stars: Option<u8>,
}

impl<'a> Facets<'a> {
    /// Facets carrying only tags (status / stars unset) — the common case.
    pub fn tags(tags: &'a [String]) -> Self {
        Self {
            tags,
            ..Self::default()
        }
    }
}

/// Does `entry` (with its sidecar `facets`) match `query`?
///
/// The query is whitespace-separated terms, all of which must match (AND):
/// * `tag:foo` — a tag equal to `foo` (case-insensitive);
/// * `status:unread|reading|done` — the reading status (absent == unread);
/// * `stars:N` / `stars:>=N` / `stars:>N` / `stars:<=N` / `stars:<N` — the
///   rating (absent == 0);
/// * any other term — a case-insensitive substring over the cite key, the
///   entry type, and the field values.
///
/// An empty query matches everything.
pub fn entry_matches(query: &str, entry: &BibEntry, facets: &Facets) -> bool {
    query
        .split_whitespace()
        .all(|term| term_matches(term, entry, facets))
}

fn term_matches(term: &str, entry: &BibEntry, facets: &Facets) -> bool {
    if let Some(want) = term.strip_prefix("tag:") {
        let want = want.to_lowercase();
        return facets.tags.iter().any(|t| t.to_lowercase() == want);
    }
    if let Some(want) = term.strip_prefix("status:") {
        // An absent status is "unread", so `status:unread` matches unset entries.
        return facets.status.unwrap_or("unread").eq_ignore_ascii_case(want);
    }
    if let Some(spec) = term.strip_prefix("stars:") {
        return stars_matches(spec, facets.stars.unwrap_or(0));
    }
    let needle = term.to_lowercase();
    entry.citekey.to_lowercase().contains(&needle)
        || entry.entry_type().contains(&needle)
        || entry
            .fields
            .values()
            .any(|v| v.to_lowercase().contains(&needle))
}

/// Match a `stars:` comparison spec (`N`, `>=N`, `>N`, `<=N`, `<N`) against a
/// rating. An unparseable spec never matches, so a typo filters nothing *in*.
fn stars_matches(spec: &str, stars: u8) -> bool {
    let (op, num) = if let Some(n) = spec.strip_prefix(">=") {
        (">=", n)
    } else if let Some(n) = spec.strip_prefix("<=") {
        ("<=", n)
    } else if let Some(n) = spec.strip_prefix('>') {
        (">", n)
    } else if let Some(n) = spec.strip_prefix('<') {
        ("<", n)
    } else {
        ("==", spec)
    };
    let Ok(n) = num.trim().parse::<u8>() else {
        return false;
    };
    match op {
        ">=" => stars >= n,
        "<=" => stars <= n,
        ">" => stars > n,
        "<" => stars < n,
        _ => stars == n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BibEntry {
        BibEntry::new("article", "Shannon1948")
            .with_field("title", "A Mathematical Theory of Communication")
            .with_field("author", "Shannon, C. E.")
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(entry_matches("", &sample(), &Facets::default()));
        assert!(entry_matches("   ", &sample(), &Facets::default()));
    }

    #[test]
    fn substring_is_case_insensitive() {
        assert!(entry_matches("mathematical", &sample(), &Facets::default()));
        assert!(entry_matches("SHANNON", &sample(), &Facets::default()));
        assert!(!entry_matches("zzz", &sample(), &Facets::default()));
    }

    #[test]
    fn all_terms_must_match() {
        assert!(entry_matches(
            "theory communication",
            &sample(),
            &Facets::default()
        ));
        assert!(!entry_matches("theory zzz", &sample(), &Facets::default()));
    }

    #[test]
    fn matches_entry_type() {
        assert!(entry_matches("article", &sample(), &Facets::default()));
    }

    #[test]
    fn tag_terms() {
        let tags = vec!["nlp".to_string(), "llm".to_string()];
        assert!(entry_matches("tag:nlp", &sample(), &Facets::tags(&tags)));
        assert!(entry_matches("tag:NLP", &sample(), &Facets::tags(&tags)));
        assert!(!entry_matches(
            "tag:vision",
            &sample(),
            &Facets::tags(&tags)
        ));
    }

    #[test]
    fn mixed_tag_and_text() {
        let tags = vec!["nlp".to_string()];
        assert!(entry_matches(
            "tag:nlp shannon",
            &sample(),
            &Facets::tags(&tags)
        ));
        assert!(!entry_matches(
            "tag:nlp missing",
            &sample(),
            &Facets::tags(&tags)
        ));
    }

    #[test]
    fn status_terms_treat_absent_as_unread() {
        let reading = Facets {
            status: Some("reading"),
            ..Default::default()
        };
        assert!(entry_matches("status:reading", &sample(), &reading));
        assert!(entry_matches("status:READING", &sample(), &reading));
        assert!(!entry_matches("status:done", &sample(), &reading));
        // No status set == unread.
        assert!(entry_matches(
            "status:unread",
            &sample(),
            &Facets::default()
        ));
        assert!(!entry_matches(
            "status:reading",
            &sample(),
            &Facets::default()
        ));
    }

    #[test]
    fn stars_comparisons() {
        let four = Facets {
            stars: Some(4),
            ..Default::default()
        };
        assert!(entry_matches("stars:4", &sample(), &four));
        assert!(entry_matches("stars:>=4", &sample(), &four));
        assert!(entry_matches("stars:>3", &sample(), &four));
        assert!(entry_matches("stars:<=5", &sample(), &four));
        assert!(!entry_matches("stars:5", &sample(), &four));
        assert!(!entry_matches("stars:<4", &sample(), &four));
        // Absent rating compares as 0.
        assert!(entry_matches("stars:0", &sample(), &Facets::default()));
        assert!(entry_matches("stars:<1", &sample(), &Facets::default()));
        assert!(!entry_matches("stars:>=1", &sample(), &Facets::default()));
        // A malformed spec matches nothing.
        assert!(!entry_matches("stars:lots", &sample(), &four));
    }
}
