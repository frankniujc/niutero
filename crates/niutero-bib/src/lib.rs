//! niutero-bib — a tolerant `.bib` parser and a deterministic serializer.
//!
//! # The contract (this is the project's #1 invariant)
//!
//! Serialization is **canonical and idempotent**. Writing an item stream
//! produces one fixed format:
//!
//! * entry types and field names are lowercased (BibTeX is case-insensitive
//!   there),
//! * two-space indent, one field per line, ` = ` separator, `{}` delimiters,
//! * field order is preserved exactly as parsed,
//! * no trailing comma after the last field,
//! * entries separated by one blank line, file ends with a newline.
//!
//! Field **values** are kept verbatim (the text inside the delimiters, inner
//! braces included). `@string` / `@preamble` / `@comment` blocks and any free
//! text between entries round-trip verbatim.
//!
//! The consequence: the *first* save of a non-canonical file may reformat it
//! (lowercasing, re-indenting, `"x"`/`2020`/`macro` → `{...}`), but every save
//! after that is **byte-identical** for an unchanged entry. That is what keeps
//! git diffs and merges clean. The property the tests pin down is therefore
//! idempotence: `serialize(parse(serialize(parse(x)))) == serialize(parse(x))`.

mod item;
mod parse;
mod serialize;

pub use item::BibItem;
pub use parse::{entry_line_span, parse};
pub use serialize::{to_bibtex, to_bibtex_entries};

use niutero_core::BibEntry;

/// Iterate over just the entries in an item stream, skipping verbatim blocks.
pub fn entries(items: &[BibItem]) -> impl Iterator<Item = &BibEntry> {
    items.iter().filter_map(|it| match it {
        BibItem::Entry(e) => Some(e),
        BibItem::Verbatim(_) => None,
    })
}
