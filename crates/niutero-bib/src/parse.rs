use crate::item::BibItem;
use niutero_core::BibEntry;

/// Parse `.bib` source into an ordered item stream. Tolerant: malformed input
/// is captured verbatim rather than dropped, and never panics.
pub fn parse(src: &str) -> Vec<BibItem> {
    Parser {
        src,
        b: src.as_bytes(),
        i: 0,
    }
    .run()
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn run(mut self) -> Vec<BibItem> {
        let mut items = Vec::new();
        let n = self.b.len();
        while self.i < n {
            // Free text up to the next '@' is captured verbatim (trimmed).
            let start = self.i;
            while self.i < n && self.b[self.i] != b'@' {
                self.i += 1;
            }
            if self.i > start {
                let txt = self.src[start..self.i].trim();
                if !txt.is_empty() {
                    items.push(BibItem::Verbatim(txt.to_string()));
                }
            }
            if self.i >= n {
                break;
            }
            self.item(&mut items);
        }
        items
    }

    /// Precondition: `self.b[self.i] == b'@'`.
    fn item(&mut self, items: &mut Vec<BibItem>) {
        let at = self.i;
        self.i += 1; // consume '@'
        let type_start = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let typ = self.src[type_start..self.i].to_ascii_lowercase();
        self.skip_ws();

        // No opening delimiter: malformed. Capture what we have, stay tolerant.
        if self.i >= self.b.len() || (self.b[self.i] != b'{' && self.b[self.i] != b'(') {
            let txt = self.src[at..self.i].trim();
            if !txt.is_empty() {
                items.push(BibItem::Verbatim(txt.to_string()));
            }
            return;
        }

        match typ.as_str() {
            "string" | "preamble" | "comment" => {
                let end = self.scan_block(self.i);
                let raw = self.src[at..end].trim();
                items.push(BibItem::Verbatim(raw.to_string()));
                self.i = end;
            }
            _ => items.push(BibItem::Entry(self.entry(typ))),
        }
    }

    /// Precondition: `self.b[self.i]` is the opening `{` or `(`.
    fn entry(&mut self, typ: String) -> BibEntry {
        let open = self.b[self.i];
        let close = if open == b'{' { b'}' } else { b')' };
        self.i += 1; // consume opener
        self.skip_ws();

        // Cite key: up to the first ',' or the closer.
        let key_start = self.i;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c == b',' || c == close {
                break;
            }
            self.i += 1;
        }
        let citekey = self.src[key_start..self.i].trim().to_string();
        let mut entry = BibEntry::new(typ, citekey);

        loop {
            if self.i >= self.b.len() {
                break; // tolerant: unterminated entry
            }
            let c = self.b[self.i];
            if c == close {
                self.i += 1;
                break;
            }
            if c == b',' || c.is_ascii_whitespace() {
                self.i += 1;
                continue;
            }
            // Field name: up to '=', or a separator if there is no '='.
            let name_start = self.i;
            while self.i < self.b.len() {
                let d = self.b[self.i];
                if d == b'=' || d == b',' || d == close {
                    break;
                }
                self.i += 1;
            }
            let name = self.src[name_start..self.i].trim().to_string();
            if self.i >= self.b.len() || self.b[self.i] != b'=' {
                continue; // no value; drop the stray token, keep going
            }
            self.i += 1; // consume '='
            let raw = self.read_raw_value(close);
            if !name.is_empty() {
                entry.set(name, canonical_value(&raw));
            }
        }
        entry
    }

    /// Read the raw right-hand side of a field, from after `=` up to (but not
    /// consuming) the top-level `,` or the entry's closer. Braces nest and a
    /// top-level `"`-string suppresses terminators inside it, so commas/braces
    /// inside a value do not end it.
    fn read_raw_value(&mut self, close: u8) -> String {
        self.skip_ws();
        let start = self.i;
        let n = self.b.len();
        let mut brace = 0i32;
        let mut in_quote = false;
        while self.i < n {
            let c = self.b[self.i];
            if in_quote {
                match c {
                    b'"' if brace == 0 => in_quote = false,
                    b'{' => brace += 1,
                    b'}' if brace > 0 => brace -= 1,
                    _ => {}
                }
                self.i += 1;
                continue;
            }
            match c {
                b'{' => brace += 1,
                b'}' => {
                    if brace == 0 {
                        break; // the entry's closer (when close == '}')
                    }
                    brace -= 1;
                }
                // A `"` is a string delimiter only at the value's base level;
                // inside `{...}` it is a literal character (e.g. Na{\"i}ve).
                b'"' if brace == 0 => in_quote = true,
                b',' if brace == 0 => break,
                _ if c == close && brace == 0 => break,
                _ => {}
            }
            self.i += 1;
        }
        self.src[start..self.i].trim().to_string()
    }

    /// Given the index of an opening `{`/`(`, return the index just past the
    /// matching closer. `d` counts `{}` nesting inside the block; a `"` is a
    /// string delimiter only at `d == 0`, and inside a string nested `{}` still
    /// balance (so a literal `}` in `"a}b"` does not end the block). Tolerant of
    /// an unterminated block (returns end-of-input).
    fn scan_block(&self, open_idx: usize) -> usize {
        let close = if self.b[open_idx] == b'{' { b'}' } else { b')' };
        let n = self.b.len();
        let mut i = open_idx + 1;
        let mut d = 0i32;
        let mut in_quote = false;
        while i < n {
            let c = self.b[i];
            if in_quote {
                match c {
                    b'"' if d == 0 => in_quote = false,
                    b'{' => d += 1,
                    b'}' if d > 0 => d -= 1,
                    _ => {}
                }
                i += 1;
                continue;
            }
            if c == close && d == 0 {
                return i + 1;
            }
            match c {
                b'{' => d += 1,
                b'}' if d > 0 => d -= 1,
                b'"' if d == 0 => in_quote = true,
                _ => {}
            }
            i += 1;
        }
        n
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
}

/// Strip a single outer `{...}` or `"..."` from a raw value, leaving the inner
/// text verbatim. Bare tokens, numbers, macros, and concatenations are kept
/// as-is (the serializer re-wraps them in braces, which is idempotent).
fn canonical_value(raw: &str) -> String {
    let t = raw.trim();
    if let Some(inner) = single_braced_inner(t) {
        inner.to_string()
    } else if let Some(inner) = single_quoted_inner(t) {
        inner.to_string()
    } else {
        t.to_string()
    }
}

/// `Some(inner)` iff `t` is exactly one balanced `{...}` group.
fn single_braced_inner(t: &str) -> Option<&str> {
    let b = t.as_bytes();
    if b.len() < 2 || b[0] != b'{' || b[b.len() - 1] != b'}' {
        return None;
    }
    let mut depth = 0i32;
    for (idx, &c) in b.iter().enumerate() {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 && idx != b.len() - 1 {
                    return None; // first group closes before the end
                }
            }
            _ => {}
        }
    }
    (depth == 0).then(|| &t[1..t.len() - 1])
}

/// `Some(inner)` iff `t` is exactly one `"..."` literal (no top-level `"`
/// inside, braces balanced).
fn single_quoted_inner(t: &str) -> Option<&str> {
    let b = t.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len() - 1] != b'"' {
        return None;
    }
    let mut brace = 0i32;
    for &c in &b[1..b.len() - 1] {
        match c {
            b'{' => brace += 1,
            b'}' if brace > 0 => brace -= 1,
            b'"' if brace == 0 => return None,
            _ => {}
        }
    }
    Some(&t[1..t.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_entry(src: &str) -> BibEntry {
        let items = parse(src);
        assert_eq!(items.len(), 1, "expected exactly one item: {items:?}");
        match &items[0] {
            BibItem::Entry(e) => e.clone(),
            other => panic!("expected entry, got {other:?}"),
        }
    }

    #[test]
    fn basic_entry() {
        let e = one_entry("@Article{key, Title = {Hello}, Year = 2025}");
        assert_eq!(e.entry_type, "article");
        assert_eq!(e.citekey, "key");
        assert_eq!(e.get("title"), Some("Hello"));
        assert_eq!(e.get("year"), Some("2025"));
    }

    #[test]
    fn quoted_and_braced_values_strip_one_level() {
        let e = one_entry(r#"@misc{k, a = "world", b = {nested {x} y}}"#);
        assert_eq!(e.get("a"), Some("world"));
        assert_eq!(e.get("b"), Some("nested {x} y"));
    }

    #[test]
    fn comma_inside_braces_is_not_a_separator() {
        let e = one_entry("@misc{k, author = {Doe, John}, year = {2020}}");
        assert_eq!(e.get("author"), Some("Doe, John"));
        assert_eq!(e.get("year"), Some("2020"));
    }

    #[test]
    fn brace_inside_quotes_does_not_end_value() {
        let e = one_entry(r#"@misc{k, t = "a}b", y = {2020}}"#);
        assert_eq!(e.get("t"), Some("a}b"));
        assert_eq!(e.get("y"), Some("2020"));
    }

    #[test]
    fn paren_delimited_entry() {
        let e = one_entry("@misc(k, title = {Hi}, year = {1999})");
        assert_eq!(e.citekey, "k");
        assert_eq!(e.get("title"), Some("Hi"));
        assert_eq!(e.get("year"), Some("1999"));
    }

    #[test]
    fn no_fields() {
        let e = one_entry("@misc{loneKey}");
        assert_eq!(e.citekey, "loneKey");
        assert!(e.fields.is_empty());
    }

    #[test]
    fn string_preamble_comment_are_verbatim() {
        let items = parse("@string{acl = {ACL}}\n@preamble{\"x\"}\n@comment{whatever {nested}}");
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], BibItem::Verbatim(v) if v == "@string{acl = {ACL}}"));
        assert!(matches!(&items[1], BibItem::Verbatim(v) if v == "@preamble{\"x\"}"));
        assert!(matches!(&items[2], BibItem::Verbatim(v) if v == "@comment{whatever {nested}}"));
    }

    #[test]
    fn free_text_between_entries_is_kept() {
        let items = parse("% a note\n@misc{k}");
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], BibItem::Verbatim(v) if v == "% a note"));
        assert!(matches!(&items[1], BibItem::Entry(e) if e.citekey == "k"));
    }

    #[test]
    fn field_order_is_preserved() {
        let e = one_entry("@misc{k, zebra = {1}, apple = {2}, mango = {3}}");
        let keys: Vec<_> = e.fields.keys().cloned().collect();
        assert_eq!(keys, vec!["zebra", "apple", "mango"]);
    }

    #[test]
    fn unterminated_entry_does_not_panic() {
        // The brace never closes, so the value is unbalanced and kept verbatim
        // (we don't guess where it should end). The key thing is no panic.
        let e = one_entry("@misc{k, title = {Hello");
        assert_eq!(e.citekey, "k");
        assert_eq!(e.get("title"), Some("{Hello"));
    }

    #[test]
    fn empty_input() {
        assert!(parse("").is_empty());
        assert!(parse("   \n  \t ").is_empty());
    }

    #[test]
    fn concatenation_kept_raw() {
        let e = one_entry(r#"@misc{k, m = jan # " 2020"}"#);
        assert_eq!(e.get("m"), Some(r#"jan # " 2020""#));
    }
}
