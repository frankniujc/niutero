use niutero_core::BibEntry;

/// One top-level item in a `.bib` file, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BibItem {
    /// A structured bibliographic entry (`@article{...}`, etc.).
    Entry(BibEntry),
    /// Anything kept verbatim: `@string` / `@preamble` / `@comment` blocks and
    /// free text between entries. Re-emitted unchanged (only the surrounding
    /// blank lines are normalized).
    Verbatim(String),
}
