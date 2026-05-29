//! Query matching for entries. Pure logic; the vault supplies the tags.

use crate::BibEntry;

/// Does `entry` (carrying `tags`) match `query`?
///
/// The query is whitespace-separated terms, all of which must match (AND). A
/// `tag:foo` term matches a tag equal to `foo` (case-insensitive); any other
/// term is a case-insensitive substring match over the cite key, the entry
/// type, and the field values. An empty query matches everything.
pub fn entry_matches(query: &str, entry: &BibEntry, tags: &[String]) -> bool {
    query
        .split_whitespace()
        .all(|term| term_matches(term, entry, tags))
}

fn term_matches(term: &str, entry: &BibEntry, tags: &[String]) -> bool {
    if let Some(want) = term.strip_prefix("tag:") {
        let want = want.to_lowercase();
        return tags.iter().any(|t| t.to_lowercase() == want);
    }
    let needle = term.to_lowercase();
    entry.citekey.to_lowercase().contains(&needle)
        || entry.entry_type().contains(&needle)
        || entry
            .fields
            .values()
            .any(|v| v.to_lowercase().contains(&needle))
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
        assert!(entry_matches("", &sample(), &[]));
        assert!(entry_matches("   ", &sample(), &[]));
    }

    #[test]
    fn substring_is_case_insensitive() {
        assert!(entry_matches("mathematical", &sample(), &[]));
        assert!(entry_matches("SHANNON", &sample(), &[]));
        assert!(!entry_matches("zzz", &sample(), &[]));
    }

    #[test]
    fn all_terms_must_match() {
        assert!(entry_matches("theory communication", &sample(), &[]));
        assert!(!entry_matches("theory zzz", &sample(), &[]));
    }

    #[test]
    fn matches_entry_type() {
        assert!(entry_matches("article", &sample(), &[]));
    }

    #[test]
    fn tag_terms() {
        let tags = vec!["nlp".to_string(), "llm".to_string()];
        assert!(entry_matches("tag:nlp", &sample(), &tags));
        assert!(entry_matches("tag:NLP", &sample(), &tags));
        assert!(!entry_matches("tag:vision", &sample(), &tags));
    }

    #[test]
    fn mixed_tag_and_text() {
        let tags = vec!["nlp".to_string()];
        assert!(entry_matches("tag:nlp shannon", &sample(), &tags));
        assert!(!entry_matches("tag:nlp missing", &sample(), &tags));
    }
}
