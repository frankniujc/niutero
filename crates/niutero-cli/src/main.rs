//! niutero-cli — a thin front-end over `niutero-engine`. It parses arguments
//! into engine requests and formats results; every operation lives in the
//! engine, so the future GUI drives the exact same code.
//!
//! Exit codes: 0 = ok, 1 = error (bad usage / IO / not found). clap itself
//! exits 2 on argument-parse errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use niutero_engine::{self as engine, AddSource, Filter};

#[derive(Parser)]
#[command(
    name = "niutero",
    version,
    about = "Lightweight, LaTeX-oriented citation manager (CLI)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Initialize a folder as a niutero vault.
    Init {
        /// Folder to initialize (created if it does not exist).
        path: PathBuf,
    },
    /// List entries, optionally filtered by a query or a saved view.
    List {
        /// Vault folder.
        vault: PathBuf,
        /// Filter query: free text and `tag:foo` terms, all ANDed.
        #[arg(long)]
        query: Option<String>,
        /// Use a saved view's query (mutually exclusive with --query).
        #[arg(long)]
        view: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show one entry by cite key.
    Show {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key to show.
        citekey: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Add a new entry: from raw BibTeX, a file, or --type/--key/--field.
    Add {
        /// Vault folder.
        vault: PathBuf,
        /// Raw BibTeX to add (one or more entries).
        #[arg(long)]
        bibtex: Option<String>,
        /// Read BibTeX from a file.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Entry type (requires --key).
        #[arg(long = "type")]
        type_: Option<String>,
        /// Cite key (requires --type).
        #[arg(long)]
        key: Option<String>,
        /// Field as NAME=VALUE (repeatable; flag-built entry).
        #[arg(long = "field", value_name = "NAME=VALUE")]
        field: Vec<String>,
    },
    /// Edit an existing entry's fields or type.
    Edit {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key to edit.
        citekey: String,
        /// Set a field as NAME=VALUE (repeatable).
        #[arg(long = "field", value_name = "NAME=VALUE")]
        field: Vec<String>,
        /// Remove a field by name (repeatable).
        #[arg(long = "unset", value_name = "NAME")]
        unset: Vec<String>,
        /// Change the entry type.
        #[arg(long = "type")]
        type_: Option<String>,
    },
    /// Remove an entry (and its sidecar metadata).
    Rm {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key to remove.
        citekey: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Init { path } => cmd_init(&path),
        Cmd::List {
            vault,
            query,
            view,
            json,
        } => cmd_list(&vault, query, view, json),
        Cmd::Show {
            vault,
            citekey,
            json,
        } => cmd_show(&vault, &citekey, json),
        Cmd::Add {
            vault,
            bibtex,
            from,
            type_,
            key,
            field,
        } => cmd_add(&vault, bibtex, from, type_, key, field),
        Cmd::Edit {
            vault,
            citekey,
            field,
            unset,
            type_,
        } => cmd_edit(&vault, &citekey, field, unset, type_),
        Cmd::Rm { vault, citekey } => cmd_rm(&vault, &citekey),
    }
}

fn cmd_init(path: &Path) -> Result<(), String> {
    let v = engine::init(path)?;
    println!(
        "Initialized vault '{}' at {}",
        v.config.name,
        v.root.display()
    );
    Ok(())
}

fn cmd_list(
    vault: &Path,
    query: Option<String>,
    view: Option<String>,
    json: bool,
) -> Result<(), String> {
    let v = engine::open(vault)?;
    let filter = match (query, view) {
        (Some(_), Some(_)) => return Err("use either --query or --view, not both".into()),
        (Some(q), None) => Filter::Query(q),
        (None, Some(name)) => Filter::View(name),
        (None, None) => Filter::All,
    };
    let views = engine::list(&v, filter)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&views).map_err(|e| e.to_string())?
        );
    } else {
        for view in &views {
            let title = view.fields.get("title").map(String::as_str).unwrap_or("");
            println!("{:<28} {:<14} {title}", view.citekey, view.entry_type);
        }
        println!("{} entr(ies).", views.len());
    }
    Ok(())
}

fn cmd_show(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = engine::open(vault)?;
    let view = engine::show(&v, citekey)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?
        );
    } else {
        println!("@{}{{{}}}", view.entry_type, view.citekey);
        let width = view.fields.keys().map(String::len).max().unwrap_or(0);
        for (k, val) in &view.fields {
            println!("  {k:<width$} = {val}");
        }
        if !view.tags.is_empty() {
            println!("tags: {}", view.tags.join(", "));
        }
        if !view.note.is_empty() {
            println!("note: {}", view.note);
        }
    }
    Ok(())
}

fn cmd_add(
    vault: &Path,
    bibtex: Option<String>,
    from: Option<PathBuf>,
    type_: Option<String>,
    key: Option<String>,
    field: Vec<String>,
) -> Result<(), String> {
    let v = engine::open(vault)?;
    let source = add_source(bibtex, from, type_, key, field)?;
    let keys = engine::add(&v, source)?;
    println!("Added {}: {}", keys.len(), keys.join(", "));
    Ok(())
}

/// Translate the add flags into an [`AddSource`], rejecting bad combinations.
/// Flag semantics are the CLI's concern; the engine just takes a clean source.
fn add_source(
    bibtex: Option<String>,
    from: Option<PathBuf>,
    type_: Option<String>,
    key: Option<String>,
    field: Vec<String>,
) -> Result<AddSource, String> {
    match (bibtex, from) {
        (Some(_), Some(_)) => Err("use either --bibtex or --from, not both".into()),
        (Some(src), None) => {
            if type_.is_some() || key.is_some() {
                return Err("--bibtex cannot be combined with --type/--key".into());
            }
            Ok(AddSource::Bibtex(src))
        }
        (None, Some(path)) => {
            if type_.is_some() || key.is_some() {
                return Err("--from cannot be combined with --type/--key".into());
            }
            Ok(AddSource::File(path))
        }
        (None, None) => {
            let (Some(t), Some(k)) = (type_, key) else {
                return Err("specify --bibtex, --from, or both --type and --key".into());
            };
            Ok(AddSource::Fields {
                type_: t,
                key: k,
                fields: field,
            })
        }
    }
}

fn cmd_edit(
    vault: &Path,
    citekey: &str,
    field: Vec<String>,
    unset: Vec<String>,
    type_: Option<String>,
) -> Result<(), String> {
    if field.is_empty() && unset.is_empty() && type_.is_none() {
        return Err("specify at least one of --field, --unset, or --type".into());
    }
    let v = engine::open(vault)?;
    engine::edit(&v, citekey, &field, &unset, type_)?;
    println!("Updated {citekey}");
    Ok(())
}

fn cmd_rm(vault: &Path, citekey: &str) -> Result<(), String> {
    let mut v = engine::open(vault)?;
    engine::rm(&mut v, citekey)?;
    println!("Removed {citekey}");
    Ok(())
}
