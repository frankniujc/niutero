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

/// Content identity: are these two entries the same *work*? True on DOI
/// equality (trimmed, case-folded, doi.org prefix stripped), URL equality
/// (trimmed), or a normalized-title match ([`alnum_fold`], both non-empty).
/// Title-only on purpose: an author-less metadata capture must still match its
/// stored twin. The connector uses this to tell "same paper re-captured" from
/// "different paper whose citekey merely collides".
pub fn same_work(a: &BibEntry, b: &BibEntry) -> bool {
    fn doi_of(e: &BibEntry) -> Option<String> {
        // Case-fold first so `DOI:`/`https://DOI.org/` spellings strip too
        // (DOIs themselves are case-insensitive).
        let d = e.get("doi")?.trim().to_ascii_lowercase();
        let d = d
            .strip_prefix("https://doi.org/")
            .or_else(|| d.strip_prefix("http://doi.org/"))
            .or_else(|| d.strip_prefix("https://dx.doi.org/"))
            .or_else(|| d.strip_prefix("doi:"))
            .unwrap_or(&d);
        (!d.is_empty()).then(|| d.to_string())
    }
    fn url_of(e: &BibEntry) -> Option<String> {
        let u = e.get("url")?.trim();
        (!u.is_empty()).then(|| u.to_string())
    }
    fn title_of(e: &BibEntry) -> Option<String> {
        let t = alnum_fold(e.get("title")?);
        (!t.is_empty()).then_some(t)
    }
    if let (Some(x), Some(y)) = (doi_of(a), doi_of(b)) {
        if x == y {
            return true;
        }
    }
    if let (Some(x), Some(y)) = (url_of(a), url_of(b)) {
        if x == y {
            return true;
        }
    }
    matches!((title_of(a), title_of(b)), (Some(x), Some(y)) if x == y)
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

    #[test]
    fn same_work_by_doi_in_any_spelling() {
        let a = BibEntry::new("misc", "a").with_field("doi", "10.1145/1234.5678");
        let b = BibEntry::new("misc", "b").with_field("doi", "https://doi.org/10.1145/1234.5678");
        let c = BibEntry::new("misc", "c").with_field("doi", "DOI:10.1145/1234.5678");
        assert!(same_work(&a, &b));
        assert!(same_work(&a, &c));
    }

    #[test]
    fn same_work_by_title_despite_braces_and_case() {
        let a = BibEntry::new("misc", "a").with_field("title", "{{Attention}} Is All You Need");
        let b = BibEntry::new("misc", "b").with_field("title", "attention is all you need!");
        assert!(same_work(&a, &b));
    }

    #[test]
    fn same_work_by_url() {
        let a = BibEntry::new("misc", "a").with_field("url", "https://openreview.net/forum?id=X1");
        let b = BibEntry::new("misc", "b")
            .with_field("title", "Completely Different")
            .with_field("url", "https://openreview.net/forum?id=X1");
        assert!(same_work(&a, &b));
    }

    #[test]
    fn different_papers_with_a_shared_key_shape_are_not_same_work() {
        let a = BibEntry::new("misc", "a")
            .with_field("title", "Learning to Compress Prompts")
            .with_field("doi", "10.1/x");
        let b = BibEntry::new("misc", "b")
            .with_field("title", "Learning to Compress Videos")
            .with_field("doi", "10.1/y");
        assert!(!same_work(&a, &b));
        // and with no doi/url at all, distinct titles stay distinct
        let c = BibEntry::new("misc", "c").with_field("title", "Alpha");
        let d = BibEntry::new("misc", "d").with_field("title", "Beta");
        assert!(!same_work(&c, &d));
    }
}
