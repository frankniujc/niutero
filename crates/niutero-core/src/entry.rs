use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// One bibliographic entry.
///
/// `fields` preserves insertion (source) order, which is what lets the
/// serializer stay byte-stable across save cycles. BibTeX treats entry types
/// and field names case-insensitively, so both are normalized to lowercase
/// here; field *values* are kept verbatim (the text between the delimiters,
/// inner braces included).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibEntry {
    pub citekey: String,
    pub entry_type: String,
    pub fields: IndexMap<String, String>,
}

impl BibEntry {
    /// A new entry with no fields. `entry_type` is lowercased; `citekey` is
    /// kept exactly as given (cite keys are case-sensitive in LaTeX).
    pub fn new(entry_type: impl Into<String>, citekey: impl Into<String>) -> Self {
        Self {
            citekey: citekey.into(),
            entry_type: entry_type.into().to_ascii_lowercase(),
            fields: IndexMap::new(),
        }
    }

    /// Builder-style field insert. See [`set`](Self::set).
    pub fn with_field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.set(name, value);
        self
    }

    /// Insert or replace a field. The name is lowercased; the value is stored
    /// verbatim. An existing field keeps its position; a new one is appended.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.fields
            .insert(name.into().to_ascii_lowercase(), value.into());
    }

    /// Look up a field value by (case-insensitive) name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Remove a field, preserving the order of the remaining fields.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.fields.shift_remove(&name.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_is_lowercased_citekey_is_not() {
        let e = BibEntry::new("InProceedings", "Niu2025");
        assert_eq!(e.entry_type, "inproceedings");
        assert_eq!(e.citekey, "Niu2025");
    }

    #[test]
    fn field_names_are_case_insensitive() {
        let e = BibEntry::new("article", "k")
            .with_field("Title", "Hello")
            .with_field("YEAR", "2025");
        assert_eq!(e.get("title"), Some("Hello"));
        assert_eq!(e.get("TITLE"), Some("Hello"));
        assert_eq!(e.get("year"), Some("2025"));
    }

    #[test]
    fn set_replaces_in_place_preserving_order() {
        let mut e = BibEntry::new("article", "k")
            .with_field("title", "A")
            .with_field("year", "2020");
        e.set("TITLE", "B");
        assert_eq!(e.get("title"), Some("B"));
        // order unchanged: title still first
        let keys: Vec<_> = e.fields.keys().cloned().collect();
        assert_eq!(keys, vec!["title", "year"]);
    }

    #[test]
    fn remove_preserves_remaining_order() {
        let mut e = BibEntry::new("article", "k")
            .with_field("a", "1")
            .with_field("b", "2")
            .with_field("c", "3");
        assert_eq!(e.remove("B"), Some("2".to_string()));
        let keys: Vec<_> = e.fields.keys().cloned().collect();
        assert_eq!(keys, vec!["a", "c"]);
    }

    #[test]
    fn values_are_kept_verbatim() {
        let e = BibEntry::new("article", "k").with_field("title", "Hello {World} $x$");
        assert_eq!(e.get("title"), Some("Hello {World} $x$"));
    }
}
