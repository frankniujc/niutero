//! niutero-cli — a thin front-end over `niutero-engine`. It parses arguments
//! into engine requests and formats results; every operation lives in the
//! engine, so the future GUI drives the exact same code.
//!
//! Exit codes: 0 = ok; 1 = error (bad usage / IO / not found); 2 = actionable
//! (a CI gate — `tex-scan` undefined refs, `normalize --check` would-change,
//! `sync` conflict). clap also exits 2 on argument-parse errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use niutero_engine::{self as engine, AddSource, DupPolicy, Filter};

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
        /// Entry type (--key optional; auto-generated from the pattern if omitted).
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
    /// Add/remove tags on an entry (no flags = show current tags).
    Tag {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Tag to add (repeatable).
        #[arg(long = "add", value_name = "TAG")]
        add: Vec<String>,
        /// Tag to remove (repeatable).
        #[arg(long = "remove", value_name = "TAG")]
        remove: Vec<String>,
    },
    /// Set, clear, or show an entry's note.
    Note {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Set the note to this text.
        #[arg(long)]
        set: Option<String>,
        /// Clear the note.
        #[arg(long)]
        clear: bool,
    },
    /// Manage saved filter views.
    View {
        /// Vault folder.
        vault: PathBuf,
        #[command(subcommand)]
        action: ViewAction,
    },
    /// Import entries from a .bib file (merge with a duplicate-key policy).
    Import {
        /// Vault folder.
        vault: PathBuf,
        /// The .bib file to import.
        file: PathBuf,
        /// What to do when a cite key already exists.
        #[arg(long = "on-dup", value_enum, default_value_t = OnDup::Skip)]
        on_dup: OnDup,
    },
    /// Export the (optionally filtered) library to a standalone .bib file.
    Export {
        /// Vault folder.
        vault: PathBuf,
        /// Output file path.
        #[arg(long)]
        out: PathBuf,
        /// Filter query: free text and `tag:foo` terms, all ANDed.
        #[arg(long)]
        query: Option<String>,
        /// Use a saved view's query (mutually exclusive with --query).
        #[arg(long)]
        view: Option<String>,
    },
    /// Scan .tex/.aux files and report used / missing / unused cite keys.
    TexScan {
        /// Vault folder.
        vault: PathBuf,
        /// One or more .tex/.aux files to scan.
        #[arg(required = true)]
        tex: Vec<PathBuf>,
        /// Write a pruned .bib of only the cited (used) entries here.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Connect the vault to a git remote for syncing.
    Connect {
        /// Vault folder.
        vault: PathBuf,
        /// Remote URL (e.g. git@github.com:user/repo.git).
        url: String,
    },
    /// Commit local changes, pull, and push (needs `connect` first).
    Sync {
        /// Vault folder.
        vault: PathBuf,
        /// Commit message (default: "niutero: sync").
        #[arg(long)]
        message: Option<String>,
    },
    /// Normalize entries offline (propose-only): review, then --write.
    Normalize {
        /// Vault folder.
        vault: PathBuf,
        /// Apply the changes (default is a dry-run preview).
        #[arg(long)]
        write: bool,
        /// CI gate: exit 2 if anything would change (does not write).
        #[arg(long)]
        check: bool,
        /// Emit JSON (the per-entry field-level diffs) instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print `\cite{key}` for an entry (to paste into LaTeX).
    Cite {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
    },
    /// Show the git history of one entry (the commits that changed it).
    History {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Regenerate cite keys from the library's pattern (preview, then --write).
    Rekey {
        /// Vault folder.
        vault: PathBuf,
        /// Apply the changes (default is a preview).
        #[arg(long)]
        write: bool,
        /// Override the library's citation-key pattern for this run.
        #[arg(long)]
        pattern: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show or set an entry's reading status (unread / reading / done).
    Status {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Set the status (omit to show the current one).
        #[arg(long, value_enum)]
        set: Option<StatusArg>,
    },
    /// Show or set an entry's star rating (0–5; 0 clears it).
    Stars {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Set the rating (omit to show the current one).
        #[arg(long)]
        set: Option<u8>,
    },
    /// Scan the library for offline hygiene issues (a health report).
    Analyze {
        /// Vault folder.
        vault: PathBuf,
        /// Emit JSON (full per-check entry lists) instead of a summary.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum OnDup {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Clone, Copy, ValueEnum)]
enum StatusArg {
    Unread,
    Reading,
    Done,
}

impl From<StatusArg> for engine::Status {
    fn from(s: StatusArg) -> Self {
        match s {
            StatusArg::Unread => engine::Status::Unread,
            StatusArg::Reading => engine::Status::Reading,
            StatusArg::Done => engine::Status::Done,
        }
    }
}

#[derive(Subcommand)]
enum ViewAction {
    /// List saved views.
    List {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Add a saved view.
    Add {
        /// View name.
        name: String,
        /// The view's filter query.
        #[arg(long)]
        query: String,
    },
    /// Remove a saved view by name.
    Rm {
        /// View name.
        name: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Adapt a `()`-returning handler to a success exit code.
fn ok(_: ()) -> ExitCode {
    ExitCode::SUCCESS
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.cmd {
        Cmd::Init { path } => cmd_init(&path).map(ok),
        Cmd::List {
            vault,
            query,
            view,
            json,
        } => cmd_list(&vault, query, view, json).map(ok),
        Cmd::Show {
            vault,
            citekey,
            json,
        } => cmd_show(&vault, &citekey, json).map(ok),
        Cmd::Add {
            vault,
            bibtex,
            from,
            type_,
            key,
            field,
        } => cmd_add(&vault, bibtex, from, type_, key, field).map(ok),
        Cmd::Edit {
            vault,
            citekey,
            field,
            unset,
            type_,
        } => cmd_edit(&vault, &citekey, field, unset, type_).map(ok),
        Cmd::Rm { vault, citekey } => cmd_rm(&vault, &citekey).map(ok),
        Cmd::Tag {
            vault,
            citekey,
            add,
            remove,
        } => cmd_tag(&vault, &citekey, add, remove).map(ok),
        Cmd::Note {
            vault,
            citekey,
            set,
            clear,
        } => cmd_note(&vault, &citekey, set, clear).map(ok),
        Cmd::View { vault, action } => cmd_view(&vault, action).map(ok),
        Cmd::Import {
            vault,
            file,
            on_dup,
        } => cmd_import(&vault, &file, on_dup).map(ok),
        Cmd::Export {
            vault,
            out,
            query,
            view,
        } => cmd_export(&vault, &out, query, view).map(ok),
        Cmd::TexScan {
            vault,
            tex,
            out,
            json,
        } => cmd_tex_scan(&vault, &tex, out, json),
        Cmd::Connect { vault, url } => cmd_connect(&vault, &url).map(ok),
        Cmd::Sync { vault, message } => cmd_sync(&vault, message),
        Cmd::Normalize {
            vault,
            write,
            check,
            json,
        } => cmd_normalize(&vault, write, check, json),
        Cmd::Cite { vault, citekey } => cmd_cite(&vault, &citekey).map(ok),
        Cmd::History {
            vault,
            citekey,
            json,
        } => cmd_history(&vault, &citekey, json).map(ok),
        Cmd::Rekey {
            vault,
            write,
            pattern,
            json,
        } => cmd_rekey(&vault, write, pattern, json).map(ok),
        Cmd::Status {
            vault,
            citekey,
            set,
        } => cmd_status(&vault, &citekey, set).map(ok),
        Cmd::Stars {
            vault,
            citekey,
            set,
        } => cmd_stars(&vault, &citekey, set).map(ok),
        Cmd::Analyze { vault, json } => cmd_analyze(&vault, json).map(ok),
    }
}

/// Translate `--query` / `--view` into a [`Filter`], rejecting both at once.
fn filter_from(query: Option<String>, view: Option<String>) -> Result<Filter, String> {
    match (query, view) {
        (Some(_), Some(_)) => Err("use either --query or --view, not both".into()),
        (Some(q), None) => Ok(Filter::Query(q)),
        (None, Some(name)) => Ok(Filter::View(name)),
        (None, None) => Ok(Filter::All),
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
    let views = engine::list(&v, filter_from(query, view)?)?;
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
        if view.status != "unread" {
            println!("status: {}", view.status);
        }
        if let Some(n) = view.stars {
            println!("stars: {n}");
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
        (None, None) => match type_ {
            // --key is optional: without it, the engine derives the key from the
            // library's citation-key pattern.
            Some(t) => Ok(AddSource::Fields {
                type_: t,
                key: key.unwrap_or_default(),
                fields: field,
            }),
            None if key.is_some() => Err("--key requires --type".into()),
            None => Err("specify --bibtex, --from, or --type (with optional --key)".into()),
        },
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

fn cmd_tag(
    vault: &Path,
    citekey: &str,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<(), String> {
    let tags = if add.is_empty() && remove.is_empty() {
        engine::current_tags(&engine::open(vault)?, citekey)?
    } else {
        let mut v = engine::open(vault)?;
        engine::set_tags(&mut v, citekey, &add, &remove)?
    };
    if tags.is_empty() {
        println!("(no tags)");
    } else {
        println!("tags: {}", tags.join(", "));
    }
    Ok(())
}

fn cmd_note(vault: &Path, citekey: &str, set: Option<String>, clear: bool) -> Result<(), String> {
    if set.is_some() && clear {
        return Err("use either --set or --clear, not both".into());
    }
    if let Some(text) = set {
        let mut v = engine::open(vault)?;
        engine::set_note(&mut v, citekey, Some(text))?;
        println!("Set note for {citekey}");
    } else if clear {
        let mut v = engine::open(vault)?;
        engine::set_note(&mut v, citekey, None)?;
        println!("Cleared note for {citekey}");
    } else {
        let note = engine::current_note(&engine::open(vault)?, citekey)?;
        if note.is_empty() {
            println!("(no note)");
        } else {
            println!("{note}");
        }
    }
    Ok(())
}

fn cmd_view(vault: &Path, action: ViewAction) -> Result<(), String> {
    match action {
        ViewAction::List { json } => {
            let v = engine::open(vault)?;
            let views = engine::views(&v);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(views).map_err(|e| e.to_string())?
                );
            } else if views.is_empty() {
                println!("(no saved views)");
            } else {
                for w in views {
                    println!("{}: {}", w.name, w.query);
                }
            }
        }
        ViewAction::Add { name, query } => {
            let mut v = engine::open(vault)?;
            engine::add_view(&mut v, name.clone(), query)?;
            println!("Added view '{name}'");
        }
        ViewAction::Rm { name } => {
            let mut v = engine::open(vault)?;
            engine::remove_view(&mut v, &name)?;
            println!("Removed view '{name}'");
        }
    }
    Ok(())
}

fn cmd_import(vault: &Path, file: &Path, on_dup: OnDup) -> Result<(), String> {
    let v = engine::open(vault)?;
    let policy = match on_dup {
        OnDup::Skip => DupPolicy::Skip,
        OnDup::Overwrite => DupPolicy::Overwrite,
        OnDup::Rename => DupPolicy::Rename,
    };
    let r = engine::import(&v, file, policy)?;
    let mut parts = vec![format!("{} added", r.added)];
    if r.overwritten > 0 {
        parts.push(format!("{} overwritten", r.overwritten));
    }
    if !r.renamed.is_empty() {
        parts.push(format!("{} renamed", r.renamed.len()));
    }
    if r.skipped > 0 {
        parts.push(format!("{} skipped", r.skipped));
    }
    println!("Imported: {}", parts.join(", "));
    for (old, new) in &r.renamed {
        println!("  {old} -> {new}");
    }
    Ok(())
}

fn cmd_export(
    vault: &Path,
    out: &Path,
    query: Option<String>,
    view: Option<String>,
) -> Result<(), String> {
    let v = engine::open(vault)?;
    let n = engine::export(&v, filter_from(query, view)?, out)?;
    println!("Exported {n} entr(ies) to {}", out.display());
    Ok(())
}

fn cmd_tex_scan(
    vault: &Path,
    tex: &[PathBuf],
    out: Option<PathBuf>,
    json: bool,
) -> Result<ExitCode, String> {
    let v = engine::open(vault)?;
    let report = engine::tex_scan(&v, tex)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "used {}, missing {}, unused {}{}",
            report.used.len(),
            report.missing.len(),
            report.unused.len(),
            if report.cite_all {
                "  (\\nocite{*})"
            } else {
                ""
            }
        );
        if !report.missing.is_empty() {
            println!("missing (cited, not in library):");
            for k in &report.missing {
                println!("  {k}");
            }
        }
        if !report.unused.is_empty() {
            println!("unused (in library, never cited):");
            for k in &report.unused {
                println!("  {k}");
            }
        }
    }
    if let Some(out) = out {
        let n = engine::export_keys(&v, &report.used, &out)?;
        println!("Wrote {n} cited entr(ies) to {}", out.display());
    }
    // CI gate: undefined references are actionable.
    Ok(if report.missing.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn cmd_connect(vault: &Path, url: &str) -> Result<(), String> {
    let v = engine::open(vault)?;
    engine::connect(&v, url)?;
    println!("Connected {} to {url}", v.root.display());
    Ok(())
}

fn cmd_sync(vault: &Path, message: Option<String>) -> Result<ExitCode, String> {
    let v = engine::open(vault)?;
    match engine::sync(&v, message)? {
        engine::SyncStatus::Synced { committed, merged } => {
            let what = if merged {
                "auto-merged remote changes"
            } else if committed {
                "committed local changes"
            } else {
                "nothing to commit"
            };
            println!("Synced ({what})");
            Ok(ExitCode::SUCCESS)
        }
        engine::SyncStatus::Conflict => {
            eprintln!(
                "sync hit a merge conflict it could not auto-resolve and was aborted; \
                 resolve it with git, then re-run"
            );
            Ok(ExitCode::from(2))
        }
    }
}

fn cmd_normalize(vault: &Path, write: bool, check: bool, json: bool) -> Result<ExitCode, String> {
    if write && check {
        return Err("use either --write or --check, not both".into());
    }
    let v = engine::open(vault)?;
    let changes = if write {
        engine::normalize_apply(&v)?
    } else {
        engine::normalize_preview(&v)?
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&changes).map_err(|e| e.to_string())?
        );
        // --check is still a CI gate even in JSON mode.
        return Ok(if check && !changes.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        });
    }
    if changes.is_empty() {
        println!("Already normalized: nothing to change.");
        return Ok(ExitCode::SUCCESS);
    }
    let verb = if write { "changed" } else { "would change" };
    println!("{} entr(ies) {verb}:", changes.len());
    for c in &changes {
        println!("  {}", c.citekey);
        for n in &c.notes {
            println!("    - {n}");
        }
    }
    if check {
        return Ok(ExitCode::from(2));
    }
    if !write {
        println!("(dry run — re-run with --write to apply)");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_cite(vault: &Path, citekey: &str) -> Result<(), String> {
    let v = engine::open(vault)?;
    println!("{}", engine::cite(&v, citekey)?);
    Ok(())
}

fn cmd_history(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = engine::open(vault)?;
    let commits = engine::history(&v, citekey)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&commits).map_err(|e| e.to_string())?
        );
    } else if commits.is_empty() {
        println!("(no history for {citekey})");
    } else {
        for c in &commits {
            // Abbreviated hash and the date (not the time) for a scannable log.
            let short = c.hash.get(..9).unwrap_or(&c.hash);
            let day = c.date.get(..10).unwrap_or(&c.date);
            println!("{short}  {day}  {}", c.subject);
        }
    }
    Ok(())
}

fn cmd_rekey(vault: &Path, write: bool, pattern: Option<String>, json: bool) -> Result<(), String> {
    let changes = if write {
        let mut v = engine::open(vault)?;
        engine::rekey_apply(&mut v, pattern.as_deref())?
    } else {
        let v = engine::open(vault)?;
        engine::rekey_preview(&v, pattern.as_deref())?
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&changes).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if changes.is_empty() {
        println!("All cite keys already match the pattern.");
        return Ok(());
    }
    let verb = if write { "Re-keyed" } else { "Would re-key" };
    println!("{verb} {} entr(ies):", changes.len());
    for c in &changes {
        let flag = if c.disambiguated {
            "  (suffix added)"
        } else {
            ""
        };
        println!("  {} -> {}{flag}", c.citekey, c.new_key);
    }
    if !write {
        println!("(preview — re-run with --write to apply)");
    }
    Ok(())
}

fn cmd_status(vault: &Path, citekey: &str, set: Option<StatusArg>) -> Result<(), String> {
    if let Some(s) = set {
        let mut v = engine::open(vault)?;
        let status = engine::Status::from(s);
        engine::set_status(&mut v, citekey, status)?;
        println!("Set status of {citekey} to {}", status.as_str());
    } else {
        let view = engine::show(&engine::open(vault)?, citekey)?;
        println!("{}", view.status);
    }
    Ok(())
}

fn cmd_stars(vault: &Path, citekey: &str, set: Option<u8>) -> Result<(), String> {
    if let Some(n) = set {
        let mut v = engine::open(vault)?;
        engine::set_stars(&mut v, citekey, Some(n))?;
        if n == 0 {
            println!("Cleared rating of {citekey}");
        } else {
            println!("Set {citekey} to {n} star(s)");
        }
    } else {
        match engine::show(&engine::open(vault)?, citekey)?.stars {
            Some(n) => println!("{n}"),
            None => println!("(unrated)"),
        }
    }
    Ok(())
}

fn cmd_analyze(vault: &Path, json: bool) -> Result<(), String> {
    let v = engine::open(vault)?;
    let report = engine::analyze(&v)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!("{} entr(ies) scanned.", report.total);
    for c in &report.checks {
        let n = c.keys.len();
        let mark = if n == 0 { "ok" } else { "!!" };
        println!("  [{mark}] {:<22} {n:>4}   {}", c.label, c.hint);
    }
    Ok(())
}
