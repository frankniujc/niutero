//! Offline duplicate detection: group entries that look like the same work.
//!
//! Two entries are "likely duplicates" when they share a normalized signature —
//! first-author surname + year + title (alphanumerics only, case-folded). That
//! catches the common cases (a paper imported twice, an arXiv copy and the
//! published version with the same title/author/year) without a fuzzy match.

use std::collections::HashMap;

use crate::citekey::first_author_surname;
use crate::BibEntry;

/// Groups of cite keys that look like duplicates of one another. Each group has
/// ≥ 2 keys (in input order); groups are ordered by first appearance. An entry
/// with no title or no author is never grouped (too little to match on).
pub fn duplicate_groups(entries: &[BibEntry]) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for e in entries {
        let Some(sig) = signature(e) else {
            continue;
        };
        match index.get(&sig) {
            Some(&i) => groups[i].push(e.citekey.clone()),
            None => {
                index.insert(sig, groups.len());
                groups.push(vec![e.citekey.clone()]);
            }
        }
    }
    groups.into_iter().filter(|g| g.len() > 1).collect()
}

/// `None` if the entry lacks the title/author needed to match on.
fn signature(e: &BibEntry) -> Option<String> {
    let title = alnum_fold(e.get("title")?);
    let surname = first_author_surname(e.get("author")?);
    if title.is_empty() || surname.is_empty() {
        return None;
    }
    // Keep any disambiguating letter ("2020a" != "2020b" — distinct works) while
    // still folding punctuation/case.
    let year = alnum_fold(e.get("year").unwrap_or(""));
    Some(format!("{surname}\u{1f}{year}\u{1f}{title}"))
}

fn alnum_fold(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: &str, author: &str, year: &str, title: &str) -> BibEntry {
        BibEntry::new("article", key)
            .with_field("author", author)
            .with_field("year", year)
            .with_field("title", title)
    }

    #[test]
    fn groups_entries_with_the_same_signature() {
        let entries = [
            e("a1", "Vaswani, Ashish", "2017", "Attention Is All You Need"),
            e("a2", "Vaswani, A.", "2017", "attention is all you need!"), // punctuation/case differ
            e("b", "Devlin, Jacob", "2019", "BERT"),
        ];
        let groups = duplicate_groups(&entries);
        assert_eq!(groups, vec![vec!["a1".to_string(), "a2".to_string()]]);
    }

    #[test]
    fn different_year_or_author_is_not_a_duplicate() {
        let entries = [
            e("a", "Smith, J", "2020", "A Study"),
            e("b", "Smith, J", "2021", "A Study"), // different year
            e("c", "Jones, K", "2020", "A Study"), // different author
        ];
        assert!(duplicate_groups(&entries).is_empty());
    }

    #[test]
    fn year_disambiguation_suffix_keeps_works_distinct() {
        // 2020a / 2020b mark DIFFERENT works — must not be grouped.
        let entries = [
            e("a", "Smith, J", "2020a", "A Study"),
            e("b", "Smith, J", "2020b", "A Study"),
        ];
        assert!(duplicate_groups(&entries).is_empty());
    }

    #[test]
    fn entries_missing_title_or_author_are_skipped() {
        let entries = [
            BibEntry::new("misc", "x").with_field("title", "T"), // no author
            BibEntry::new("misc", "y").with_field("title", "T"), // no author
        ];
        assert!(duplicate_groups(&entries).is_empty());
    }
}
