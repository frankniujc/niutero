//! niutero-cli — the complete interface to niutero. The GUI (Phase 2) is a
//! thin client over this same surface; every capability is a subcommand here
//! first, with `--json` for machine consumers.
//!
//! Exit codes: 0 = ok, 1 = error (bad usage / IO / not found). clap itself
//! exits 2 on argument-parse errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use niutero_bib::entries;
use niutero_core::{filter, BibEntry};
use niutero_vault::Vault;
use serde::Serialize;

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
    }
}

/// Stable JSON shape for an entry (also consumed by the future GUI). All five
/// fields are always present so the schema is predictable.
#[derive(Serialize)]
struct EntryOut<'a> {
    citekey: &'a str,
    #[serde(rename = "type")]
    entry_type: &'a str,
    fields: &'a indexmap::IndexMap<String, String>,
    tags: &'a [String],
    note: &'a str,
}

fn entry_out<'a>(e: &'a BibEntry, v: &'a Vault) -> EntryOut<'a> {
    let (tags, note): (&[String], &str) = match v.meta.get(&e.citekey) {
        Some(m) => (&m.tags, &m.note),
        None => (&[], ""),
    };
    EntryOut {
        citekey: &e.citekey,
        entry_type: &e.entry_type,
        fields: &e.fields,
        tags,
        note,
    }
}

fn cmd_init(path: &Path) -> Result<(), String> {
    let v = Vault::init(path).map_err(|e| format!("init {}: {e}", path.display()))?;
    println!(
        "Initialized vault '{}' at {}",
        v.config.name,
        v.root.display()
    );
    Ok(())
}

fn open(vault: &Path) -> Result<Vault, String> {
    Vault::open(vault).map_err(|e| format!("open {}: {e}", vault.display()))
}

fn cmd_list(
    vault: &Path,
    query: Option<String>,
    view: Option<String>,
    json: bool,
) -> Result<(), String> {
    let v = open(vault)?;
    let items = v
        .read_items()
        .map_err(|e| format!("read references.bib: {e}"))?;
    let q = resolve_query(&v, query, view)?;
    let matched: Vec<&BibEntry> = entries(&items)
        .filter(|e| {
            let tags = v
                .meta
                .get(&e.citekey)
                .map(|m| m.tags.as_slice())
                .unwrap_or(&[]);
            filter::entry_matches(&q, e, tags)
        })
        .collect();

    if json {
        let out: Vec<EntryOut> = matched.iter().map(|e| entry_out(e, &v)).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
    } else {
        for e in &matched {
            let title = e.get("title").unwrap_or("");
            println!("{:<28} {:<14} {title}", e.citekey, e.entry_type);
        }
        println!("{} entr(ies).", matched.len());
    }
    Ok(())
}

fn resolve_query(v: &Vault, query: Option<String>, view: Option<String>) -> Result<String, String> {
    match (query, view) {
        (Some(_), Some(_)) => Err("use either --query or --view, not both".into()),
        (Some(q), None) => Ok(q),
        (None, Some(name)) => v
            .views
            .views
            .iter()
            .find(|w| w.name == name)
            .map(|w| w.query.clone())
            .ok_or_else(|| format!("no saved view named '{name}'")),
        (None, None) => Ok(String::new()),
    }
}

fn cmd_show(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = open(vault)?;
    let items = v
        .read_items()
        .map_err(|e| format!("read references.bib: {e}"))?;
    let e = entries(&items)
        .find(|e| e.citekey == citekey)
        .ok_or_else(|| format!("no entry with cite key '{citekey}'"))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entry_out(e, &v)).map_err(|e| e.to_string())?
        );
    } else {
        println!("@{}{{{}}}", e.entry_type, e.citekey);
        let width = e.fields.keys().map(String::len).max().unwrap_or(0);
        for (k, val) in &e.fields {
            println!("  {k:<width$} = {val}");
        }
        if let Some(m) = v.meta.get(citekey) {
            if !m.tags.is_empty() {
                println!("tags: {}", m.tags.join(", "));
            }
            if !m.note.is_empty() {
                println!("note: {}", m.note);
            }
        }
    }
    Ok(())
}
