use std::borrow::Cow;
use std::ops::Range;

use crate::item::BibItem;
use niutero_core::BibEntry;

/// Parse `.bib` source into an ordered item stream. Tolerant: malformed input
/// is captured verbatim rather than dropped, and never panics. A leading UTF-8
/// BOM is stripped and CRLF / lone CR are normalized to LF first, so a file
/// authored on Windows (or with a BOM) round-trips to clean LF output instead
/// of churning or leaving a stray verbatim block.
pub fn parse(src: &str) -> Vec<BibItem> {
    let normalized = normalize_input(src);
    Parser {
        src: normalized.as_ref(),
        b: normalized.as_bytes(),
        i: 0,
    }
    .run()
}

/// The 1-based, inclusive line numbers `(start, end)` of the entry with cite key
/// `citekey` within `src`, or `None` if there is no such entry. Lines are
/// counted by `\n`, exactly as `git` numbers them, so the result can be handed
/// straight to `git log -L<start>,<end>:<file>`. Verbatim blocks
/// (`@string`/`@preamble`/`@comment` and free text) are skipped.
///
/// The span is recovered from the *actual* source text (not a re-serialization),
/// so it is correct even for a hand-formatted `.bib`. Line numbers count `\n`
/// the way git does (a CRLF counts once, a lone CR not at all), so passing a
/// committed blob's content yields numbers that match `git log -L` byte-for-byte.
pub fn entry_line_span(src: &str, citekey: &str) -> Option<(usize, usize)> {
    let normalized = normalize_input(src);
    let s = normalized.as_ref();
    let range = Parser {
        src: s,
        b: s.as_bytes(),
        i: 0,
    }
    .run_spanned()
    .into_iter()
    .find_map(|(it, r)| match it {
        BibItem::Entry(e) if e.citekey == citekey => Some(r),
        _ => None,
    })?;
    // Count lines exactly as git numbers them: by physical `\n` only.
    // `normalize_input` folds lone CRs into `\n` (right for canonical output, but
    // wrong here — git does *not* treat a lone CR as a line break), so we count
    // against a byte-aligned text that keeps lone CRs as `\r`. It strips the same
    // BOM and folds only `\r\n`; the only remaining difference from `s` is that
    // each lone CR stays one `\r` byte instead of one `\n` byte, so every offset
    // in `range` still indexes the same position. `range` brackets the entry from
    // its `@` to just past the closing delimiter — neither endpoint is a `\n`.
    let counted = src
        .strip_prefix('\u{feff}')
        .unwrap_or(src)
        .replace("\r\n", "\n");
    debug_assert_eq!(
        counted.len(),
        s.len(),
        "line-counting text must align with parse text"
    );
    let bytes = counted.as_bytes();
    let line_of = |end: usize| bytes[..end].iter().filter(|&&b| b == b'\n').count() + 1;
    Some((line_of(range.start), line_of(range.end)))
}

/// Strip a leading UTF-8 BOM and normalize line endings to `\n`. Borrows when
/// there is nothing to change (the common case).
fn normalize_input(src: &str) -> Cow<'_, str> {
    let stripped = src.strip_prefix('\u{feff}').unwrap_or(src);
    if stripped.contains('\r') {
        Cow::Owned(stripped.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(stripped)
    }
}

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn run(self) -> Vec<BibItem> {
        self.run_spanned().into_iter().map(|(it, _)| it).collect()
    }

    /// Like [`run`](Self::run), but pairs each item with its byte range in the
    /// (normalized) source. Used to recover an entry's line span; `run`
    /// delegates here so the two can never disagree on item boundaries.
    fn run_spanned(mut self) -> Vec<(BibItem, Range<usize>)> {
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
                    items.push((BibItem::Verbatim(txt.to_string()), start..self.i));
                }
            }
            if self.i >= n {
                break;
            }
            self.item(&mut items);
        }
        items
    }

    /// Precondition: `self.b[self.i] == b'@'`. Pushes the parsed item paired
    /// with its `[start, end)` byte range (`@` through the closing delimiter).
    fn item(&mut self, items: &mut Vec<(BibItem, Range<usize>)>) {
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
                items.push((BibItem::Verbatim(txt.to_string()), at..self.i));
            }
            return;
        }

        match typ.as_str() {
            "string" | "preamble" | "comment" => {
                let end = self.scan_block(self.i);
                let raw = self.src[at..end].trim();
                items.push((BibItem::Verbatim(raw.to_string()), at..end));
                self.i = end;
            }
            _ => {
                let e = self.entry(typ);
                items.push((BibItem::Entry(e), at..self.i));
            }
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
        assert_eq!(e.entry_type(), "article");
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

    #[test]
    fn line_span_of_a_canonical_entry() {
        // Two entries separated by a blank line, exactly as the serializer emits.
        let src = "@misc{a,\n  title = {A}\n}\n\n@article{b,\n  title = {B},\n  year = {2020}\n}\n";
        assert_eq!(entry_line_span(src, "a"), Some((1, 3)));
        assert_eq!(entry_line_span(src, "b"), Some((5, 8)));
        assert_eq!(entry_line_span(src, "missing"), None);
    }

    #[test]
    fn line_span_skips_leading_verbatim() {
        // A @string macro and a comment precede the entry; only the entry counts.
        let src = "@string{acl = {ACL}}\n\n% a note\n@misc{k,\n  title = {Hi}\n}\n";
        // @misc starts on line 4 (`@string` line 1, blank line 2, comment line 3).
        assert_eq!(entry_line_span(src, "k"), Some((4, 6)));
        assert_eq!(entry_line_span(src, "acl"), None); // @string is not an entry
    }

    #[test]
    fn line_span_of_a_no_field_entry() {
        let src = "@misc{lone\n}\n";
        assert_eq!(entry_line_span(src, "lone"), Some((1, 2)));
    }

    #[test]
    fn line_span_handles_values_with_braces_and_commas() {
        // A field value spanning the line with a brace group and a comma must not
        // confuse where the entry ends.
        let src =
            "@misc{k,\n  author = {Doe, John and {Foo} Bar},\n  year = {2020}\n}\n\n@misc{after}\n";
        assert_eq!(entry_line_span(src, "k"), Some((1, 4)));
        assert_eq!(entry_line_span(src, "after"), Some((6, 6)));
    }

    #[test]
    fn line_span_is_invariant_under_crlf_and_bom() {
        let lf = "@misc{a,\n  t = {A}\n}\n\n@misc{b}\n";
        let crlf_bom = "\u{feff}@misc{a,\r\n  t = {A}\r\n}\r\n\r\n@misc{b}\r\n";
        assert_eq!(entry_line_span(lf, "b"), entry_line_span(crlf_bom, "b"));
        assert_eq!(entry_line_span(crlf_bom, "a"), Some((1, 3)));
    }

    #[test]
    fn line_span_counts_lone_cr_the_way_git_does() {
        // git numbers lines by `\n` only — a lone CR (old-Mac, or a stray CR in a
        // committed blob) is NOT a line break. Entry `a` uses lone CRs internally;
        // to git the file is 5 newline-delimited lines and `b` begins on line 4.
        // The span must match that, not an over-count that would feed `git log -L`
        // an out-of-range line number.
        let blob = "@misc{a,\r  t = {A}\r}\n\n@misc{b,\n  t = {B}\n}\n";
        // Lines (by \n): 1=`@misc{a,\r  t = {A}\r}`, 2=``, 3=`@misc{b,`, 4=`  t = {B}`, 5=`}`.
        assert_eq!(entry_line_span(blob, "a"), Some((1, 1)));
        assert_eq!(entry_line_span(blob, "b"), Some((3, 5)));
    }

    #[test]
    fn bom_and_crlf_are_normalized() {
        let lf = "@misc{k,\n  title = {Hi}\n}\n";
        let crlf_bom = "\u{feff}@misc{k,\r\n  title = {Hi}\r\n}\r\n";
        assert_eq!(parse(crlf_bom), parse(lf));
        let out = crate::to_bibtex(&parse(crlf_bom));
        assert!(!out.contains('\r'), "no CR in canonical output");
        assert!(!out.contains('\u{feff}'), "no BOM in canonical output");
    }
}
