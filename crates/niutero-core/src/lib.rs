//! niutero-core — domain model. Pure logic; no IO, no UI.
//!
//! The `.bib` is the source of truth and stays niutero-agnostic: private data
//! (tags / notes / saved views) is NOT modeled here — it lives in the vault
//! sidecar (see `niutero-vault`). This crate only knows about bibliographic
//! entries and a library of them.

pub mod citekey;
mod entry;
pub mod filter;
mod library;
pub mod texscan;

pub use citekey::KeyPattern;
pub use entry::BibEntry;
pub use library::Library;
