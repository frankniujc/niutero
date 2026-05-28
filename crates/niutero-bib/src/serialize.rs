use crate::item::BibItem;
use niutero_core::BibEntry;

/// Serialize an item stream to canonical BibTeX. See the crate docs for the
/// exact format. Items are separated by a blank line; the result ends with a
/// single newline (empty input yields an empty string).
pub fn to_bibtex(items: &[BibItem]) -> String {
    let blocks: Vec<String> = items
        .iter()
        .map(|it| match it {
            BibItem::Entry(e) => entry_block(e),
            BibItem::Verbatim(v) => v.clone(),
        })
        .collect();
    join(blocks)
}

/// Serialize just entries (e.g. for `export`), with no verbatim blocks.
pub fn to_bibtex_entries(entries: &[BibEntry]) -> String {
    join(entries.iter().map(entry_block).collect())
}

fn join(blocks: Vec<String>) -> String {
    let mut s = blocks.join("\n\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

fn entry_block(e: &BibEntry) -> String {
    let mut s = String::new();
    s.push('@');
    s.push_str(&e.entry_type);
    s.push('{');
    s.push_str(&e.citekey);
    if e.fields.is_empty() {
        s.push_str("\n}");
        return s;
    }
    s.push_str(",\n");
    let last = e.fields.len() - 1;
    for (idx, (k, v)) in e.fields.iter().enumerate() {
        s.push_str("  ");
        s.push_str(k);
        s.push_str(" = {");
        s.push_str(v);
        s.push('}');
        if idx != last {
            s.push(',');
        }
        s.push('\n');
    }
    s.push('}');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap_order::*;

    // tiny helper so tests don't depend on IndexMap's builder ergonomics
    mod indexmap_order {
        use niutero_core::BibEntry;
        pub fn entry(typ: &str, key: &str, fields: &[(&str, &str)]) -> BibEntry {
            let mut e = BibEntry::new(typ, key);
            for (k, v) in fields {
                e.set(*k, *v);
            }
            e
        }
    }

    #[test]
    fn canonical_layout() {
        let e = entry(
            "inproceedings",
            "niu2025",
            &[("title", "Hello"), ("year", "2025")],
        );
        let out = to_bibtex_entries(&[e]);
        assert_eq!(
            out,
            "@inproceedings{niu2025,\n  title = {Hello},\n  year = {2025}\n}\n"
        );
    }

    #[test]
    fn no_fields_layout() {
        let e = entry("misc", "k", &[]);
        assert_eq!(to_bibtex_entries(&[e]), "@misc{k\n}\n");
    }

    #[test]
    fn two_entries_separated_by_blank_line() {
        let a = entry("misc", "a", &[("x", "1")]);
        let b = entry("misc", "b", &[("y", "2")]);
        let out = to_bibtex_entries(&[a, b]);
        assert_eq!(out, "@misc{a,\n  x = {1}\n}\n\n@misc{b,\n  y = {2}\n}\n");
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(to_bibtex(&[]), "");
        assert_eq!(to_bibtex_entries(&[]), "");
    }
}
