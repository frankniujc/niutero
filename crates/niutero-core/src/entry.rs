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

    /// Check that this entry can be serialized to valid, round-trippable
    /// BibTeX. The serializer assumes this holds (it writes the cite key and
    /// field names raw and wraps values in `{...}` without escaping), so the
    /// boundary that *constructs* entries from untrusted input — cite keys and
    /// values from the CLI or another tool — must call this before writing.
    ///
    /// Rejects: empty/illegal cite keys, non-alphanumeric entry types, illegal
    /// field names, and field values whose braces are unbalanced (which would
    /// corrupt the surrounding `.bib`).
    pub fn validate(&self) -> Result<(), String> {
        // Characters a BibTeX cite key (and our identifiers) must not contain.
        const FORBIDDEN: &[char] = &['{', '}', '(', ')', ',', '=', '@', '"', '#', '%', '~', '\\'];

        if self.citekey.is_empty() {
            return Err("cite key is empty".into());
        }
        if let Some(c) = self
            .citekey
            .chars()
            .find(|c| c.is_whitespace() || FORBIDDEN.contains(c))
        {
            return Err(format!(
                "cite key {:?} contains an illegal character {:?}",
                self.citekey, c
            ));
        }
        if self.entry_type.is_empty() || !self.entry_type.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(format!(
                "entry type {:?} must be non-empty and alphanumeric",
                self.entry_type
            ));
        }
        for (name, value) in &self.fields {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b':'))
            {
                return Err(format!("field name {name:?} is not a valid identifier"));
            }
            if !braces_balanced(value) {
                return Err(format!(
                    "field {name:?} has unbalanced braces (would corrupt the .bib): {value:?}"
                ));
            }
        }
        Ok(())
    }
}

/// True if `{` / `}` are balanced with no prefix dipping below zero — exactly
/// the condition for `{value}` to serialize as one well-formed group that
/// parses back to `value`.
fn braces_balanced(value: &str) -> bool {
    let mut depth: i32 = 0;
    for b in value.bytes() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
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

    #[test]
    fn validate_accepts_normal_and_tricky_balanced() {
        assert!(BibEntry::new("article", "niu-etal:2025")
            .with_field("title", "Hello {World} and \"quotes\" # $x$")
            .with_field("author", "Doe, John and Smith, A.")
            .validate()
            .is_ok());
    }

    #[test]
    fn validate_rejects_illegal_citekey() {
        for bad in ["x}", "a,b", "with space", "a@b", "", "a{b"] {
            assert!(
                BibEntry::new("misc", bad).validate().is_err(),
                "should reject citekey {bad:?}"
            );
        }
    }

    #[test]
    fn validate_rejects_bad_entry_type() {
        // entry_type is a public field, so it can be set to anything.
        let mut e = BibEntry::new("misc", "k");
        e.entry_type = "mis}c".into();
        assert!(e.validate().is_err());
        e.entry_type = String::new();
        assert!(e.validate().is_err());
    }

    #[test]
    fn validate_rejects_unbalanced_value() {
        assert!(BibEntry::new("misc", "k")
            .with_field("title", "x}")
            .validate()
            .is_err());
        assert!(BibEntry::new("misc", "k")
            .with_field("title", "a}b{c") // net zero but dips negative
            .validate()
            .is_err());
        assert!(BibEntry::new("misc", "k")
            .with_field("title", "{unclosed")
            .validate()
            .is_err());
    }

    #[test]
    fn validate_rejects_bad_field_name() {
        let mut e = BibEntry::new("misc", "k");
        e.fields.insert("bad=name".into(), "v".into());
        assert!(e.validate().is_err());
    }
}
