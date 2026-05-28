//! niutero-cli — the complete interface to niutero. The GUI (Phase 2) is a
//! thin client over this same surface; every capability is a subcommand here
//! first, with `--json` for machine consumers.
//!
//! Exit codes: 0 = ok, 1 = error (bad usage / IO / not found). clap itself
//! exits 2 on argument-parse errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use niutero_bib::{entries, parse, BibItem};
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

/// Split a `NAME=VALUE` argument on the first `=`.
fn split_field(s: &str) -> Result<(&str, &str), String> {
    s.split_once('=')
        .filter(|(n, _)| !n.is_empty())
        .ok_or_else(|| format!("field must be NAME=VALUE: '{s}'"))
}

/// Parse BibTeX source into its entries, erroring if there are none.
fn parse_entries(src: &str) -> Result<Vec<BibEntry>, String> {
    let parsed = parse(src);
    let es: Vec<BibEntry> = entries(&parsed).cloned().collect();
    if es.is_empty() {
        Err("no BibTeX entries found in input".into())
    } else {
        Ok(es)
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

fn cmd_add(
    vault: &Path,
    bibtex: Option<String>,
    from: Option<PathBuf>,
    type_: Option<String>,
    key: Option<String>,
    field: Vec<String>,
) -> Result<(), String> {
    let v = open(vault)?;
    let mut items = v
        .read_items()
        .map_err(|e| format!("read references.bib: {e}"))?;

    let new_entries: Vec<BibEntry> = match (bibtex, from) {
        (Some(_), Some(_)) => return Err("use either --bibtex or --from, not both".into()),
        (Some(src), None) => {
            if type_.is_some() || key.is_some() {
                return Err("--bibtex cannot be combined with --type/--key".into());
            }
            parse_entries(&src)?
        }
        (None, Some(path)) => {
            if type_.is_some() || key.is_some() {
                return Err("--from cannot be combined with --type/--key".into());
            }
            let src = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            parse_entries(&src)?
        }
        (None, None) => {
            let (Some(t), Some(k)) = (type_, key) else {
                return Err("specify --bibtex, --from, or both --type and --key".into());
            };
            let mut e = BibEntry::new(t, k);
            for f in &field {
                let (name, value) = split_field(f)?;
                e.set(name, value);
            }
            vec![e]
        }
    };

    // Reject duplicates against existing entries and within the batch.
    let mut seen: std::collections::HashSet<String> =
        entries(&items).map(|e| e.citekey.clone()).collect();
    for e in &new_entries {
        if !seen.insert(e.citekey.clone()) {
            return Err(format!(
                "cite key '{}' already exists (use `edit` to change it)",
                e.citekey
            ));
        }
    }

    let keys: Vec<String> = new_entries.iter().map(|e| e.citekey.clone()).collect();
    for e in new_entries {
        items.push(BibItem::Entry(e));
    }
    v.write_items(&items)
        .map_err(|e| format!("write references.bib: {e}"))?;
    println!("Added {}: {}", keys.len(), keys.join(", "));
    Ok(())
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
    let v = open(vault)?;
    let mut items = v
        .read_items()
        .map_err(|e| format!("read references.bib: {e}"))?;
    let idx = items
        .iter()
        .position(|it| matches!(it, BibItem::Entry(e) if e.citekey == citekey))
        .ok_or_else(|| format!("no entry with cite key '{citekey}'"))?;
    if let BibItem::Entry(e) = &mut items[idx] {
        if let Some(t) = type_ {
            e.entry_type = t.to_ascii_lowercase();
        }
        for f in &field {
            let (name, value) = split_field(f)?;
            e.set(name, value);
        }
        for name in &unset {
            e.remove(name);
        }
    }
    v.write_items(&items)
        .map_err(|e| format!("write references.bib: {e}"))?;
    println!("Updated {citekey}");
    Ok(())
}

fn cmd_rm(vault: &Path, citekey: &str) -> Result<(), String> {
    let mut v = open(vault)?;
    let mut items = v
        .read_items()
        .map_err(|e| format!("read references.bib: {e}"))?;
    let idx = items
        .iter()
        .position(|it| matches!(it, BibItem::Entry(e) if e.citekey == citekey))
        .ok_or_else(|| format!("no entry with cite key '{citekey}'"))?;
    items.remove(idx);
    v.write_items(&items)
        .map_err(|e| format!("write references.bib: {e}"))?;
    // Clean up the sidecar entry, but only touch disk if there was one.
    if v.meta.remove(citekey).is_some() {
        v.save_sidecar()
            .map_err(|e| format!("update sidecar: {e}"))?;
    }
    println!("Removed {citekey}");
    Ok(())
}
