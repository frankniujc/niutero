use crate::BibEntry;

/// An ordered collection of entries — the bibliographic content of a
/// `references.bib`. The verbatim `@string` / `@preamble` / `@comment` blocks
/// are not modeled here; they are tracked by the bib layer's item stream so
/// they round-trip untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    pub entries: Vec<BibEntry>,
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    /// First entry with this (exact, case-sensitive) cite key.
    pub fn get(&self, citekey: &str) -> Option<&BibEntry> {
        self.entries.iter().find(|e| e.citekey == citekey)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<Vec<BibEntry>> for Library {
    fn from(entries: Vec<BibEntry>) -> Self {
        Self { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_citekey() {
        let lib = Library::from(vec![
            BibEntry::new("article", "a"),
            BibEntry::new("book", "b"),
        ]);
        assert_eq!(lib.len(), 2);
        assert_eq!(lib.get("b").unwrap().entry_type(), "book");
        assert!(lib.get("missing").is_none());
    }
}
