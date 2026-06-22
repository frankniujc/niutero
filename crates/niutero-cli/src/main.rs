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
    name = "niutero-cli",
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Remove an entry (and its sidecar metadata).
    Rm {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key to remove.
        citekey: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Manage the tag vocabulary across the whole library (sidecar only):
    /// list / rename / merge / delete.
    Tags {
        /// Vault folder.
        vault: PathBuf,
        #[command(subcommand)]
        action: TagsAction,
    },
    /// LLM assistant: configure, test the connection, ask a question, or
    /// organize the tag vocabulary.
    Ai {
        #[command(subcommand)]
        action: AiAction,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Manage saved filter views.
    View {
        /// Vault folder.
        vault: PathBuf,
        #[command(subcommand)]
        action: ViewAction,
    },
    /// Import entries from a .bib file, or online by --doi (merge w/ dup policy).
    Import {
        /// Vault folder.
        vault: PathBuf,
        /// The .bib file to import (omit when using --doi).
        file: Option<PathBuf>,
        /// Online: fetch this DOI's BibTeX from doi.org instead of a file.
        #[arg(long)]
        doi: Option<String>,
        /// What to do when a cite key already exists. Omitted: the library's
        /// configured default (`config --on-dup …`), else skip.
        #[arg(long = "on-dup", value_enum)]
        on_dup: Option<OnDup>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Commit message (default: an auto-generated entry-level summary,
        /// e.g. "niutero: 2 added, 1 changed").
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
        /// Use a named `[profiles.<name>]` from norm.toml instead of the base config.
        #[arg(long)]
        profile: Option<String>,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
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
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Scan the library for offline hygiene issues (a health report).
    Analyze {
        /// Vault folder.
        vault: PathBuf,
        /// Emit JSON (full per-check entry lists) instead of a summary.
        #[arg(long)]
        json: bool,
    },
    /// Find likely-duplicate entries; `--merge` folds each cluster into one.
    Dedupe {
        /// Vault folder.
        vault: PathBuf,
        /// Merge each cluster into its richest entry (default just lists them).
        #[arg(long)]
        merge: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Online: fill an entry's missing fields from its DOI (needs network).
    Enrich {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key to enrich.
        citekey: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Run the browser-connector server (loopback) until stopped (Ctrl-C).
    /// Captures require the printed session token (`Authorization: Bearer …`
    /// or `X-Niutero-Token: …`), so a random web page can't inject entries.
    Connector {
        /// Vault folder.
        vault: PathBuf,
        /// Loopback port to listen on.
        #[arg(long, default_value_t = 23510)]
        port: u16,
        /// Session token captures must present (default: generate and print
        /// a fresh one).
        #[arg(long)]
        token: Option<String>,
    },
    /// Manage an entry's attached PDF: show its path, --attach a file, --fetch
    /// it from the entry's url, or --push/--pull it to/from the vault's HF
    /// dataset repo (see `pdf-config`). Binaries live in `pdfs/` (git-ignored),
    /// never in the .bib.
    Pdf {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Copy this PDF file in and attach it to the entry.
        #[arg(long)]
        attach: Option<PathBuf>,
        /// Online: download the PDF from the entry's url.
        #[arg(long)]
        fetch: bool,
        /// Online (HF): upload the local PDF to the dataset repo.
        #[arg(long)]
        push: bool,
        /// Online (HF): download the PDF from the dataset repo.
        #[arg(long)]
        pull: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show or set the vault's PDF-attachment config. The HF dataset repo and
    /// auto-fetch-on-import are LIBRARY properties (saved in the vault's
    /// synced .niutero/config.toml — collaborators share them); only the HF
    /// account token is machine-local (vaults.toml, never synced).
    PdfConfig {
        /// Vault folder.
        vault: PathBuf,
        /// HF dataset repo as user/repo (an empty string clears it).
        #[arg(long)]
        repo: Option<String>,
        /// After imports, auto-fetch PDFs whose url is a direct .pdf or an
        /// arXiv abs page (off by default — keeps imports fully offline).
        #[arg(long)]
        auto_fetch: Option<bool>,
        /// HF access token. Note: argv is visible in the process list and
        /// shell history — prefer --token-stdin. An empty string clears it.
        #[arg(long)]
        token: Option<String>,
        /// Read the HF token from stdin (first line) instead of argv.
        #[arg(long, conflicts_with = "token")]
        token_stdin: bool,
        /// Online (HF): create the dataset repo (private; safe to re-run).
        #[arg(long)]
        create_repo: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Online (LLM): suggest tags for an entry. Needs LLM assist enabled
    /// (`ai config --enable true`; key from the config or $ANTHROPIC_API_KEY).
    /// Prints suggestions to review — apply with `tag --add`.
    SuggestTags {
        /// Vault folder.
        vault: PathBuf,
        /// Cite key.
        citekey: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the vaults this machine has opened, most-recent first (machine-local
    /// registry — never synced with any vault).
    Recent {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Drop a vault from this machine's recent list (leaves the vault untouched).
    Forget {
        /// Vault folder to forget.
        vault: PathBuf,
    },
    /// Manage keep-updated export targets: external `.bib` files this machine
    /// re-writes on every change (machine-local; e.g. an Overleaf checkout).
    ExportTarget {
        /// Vault folder.
        vault: PathBuf,
        #[command(subcommand)]
        action: ExportTargetAction,
    },
    /// Show or set the library's own config (`.niutero/config.toml`, synced
    /// with the library): name, citation-key pattern, and the shared workflow
    /// toggles. With no flags, prints the current config.
    Config {
        /// Vault folder.
        vault: PathBuf,
        /// Rename the library.
        #[arg(long)]
        name: Option<String>,
        /// Citation-key pattern (e.g. "{auth}{year}{Title.2}"); "" clears
        /// back to the built-in default.
        #[arg(long)]
        pattern: Option<String>,
        /// After imports, fill new entries' missing fields from their DOIs
        /// (online; off by default so imports stay offline).
        #[arg(long)]
        enrich_on_import: Option<bool>,
        /// After every library mutation, git-commit the vault (no push; off
        /// by default).
        #[arg(long)]
        auto_commit: Option<bool>,
        /// Default duplicate policy for imports when --on-dup isn't given:
        /// skip / overwrite / rename; "" clears it.
        #[arg(long)]
        on_dup: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show or set this machine's sync strategy for a vault (pull/push toggles;
    /// machine-local, so each machine can sync differently).
    SyncConfig {
        /// Vault folder.
        vault: PathBuf,
        /// Pull (and merge) from origin before pushing.
        #[arg(long)]
        pull: Option<bool>,
        /// Push to origin after committing/merging.
        #[arg(long)]
        push: Option<bool>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ExportTargetAction {
    /// List registered export targets.
    List {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Register (or update) a target `.bib`, optionally filtered by a query.
    /// Exports immediately and re-exports on every later change.
    Add {
        /// Path of the `.bib` to keep updated.
        out: PathBuf,
        /// Filter query (free text + `tag:`/`status:`/`stars:`); omit for all.
        #[arg(long)]
        query: Option<String>,
    },
    /// Unregister a target (the external file is left on disk).
    Rm {
        /// Path of the registered `.bib`.
        out: PathBuf,
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
enum AiAction {
    /// Show the machine-local AI config, or update the given fields.
    Config {
        /// Turn LLM assist on/off.
        #[arg(long)]
        enable: Option<bool>,
        /// Provider label (only `anthropic` is wired today).
        #[arg(long)]
        provider: Option<String>,
        /// API key (stored machine-local in vaults.toml). Note: argv is
        /// visible in the process list and shell history — prefer --key-stdin
        /// on shared machines.
        #[arg(long)]
        key: Option<String>,
        /// Read the API key from stdin (first line) instead of argv, so it
        /// never shows in the process list or shell history.
        #[arg(long, conflicts_with = "key")]
        key_stdin: bool,
        /// Model id.
        #[arg(long)]
        model: Option<String>,
        /// API base URL override (not honored yet — calls refuse to run while
        /// one is set, so requests can't be misrouted).
        #[arg(long)]
        base_url: Option<String>,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Test the connection with a tiny request.
    Test {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Ask a question grounded in a library.
    Ask {
        /// Vault folder.
        vault: PathBuf,
        /// The question.
        question: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Tidy the tag vocabulary: get a merge plan from the model (or load one
    /// with --plan, fully offline) and optionally --apply its merges. New-tag
    /// suggestions are always advisory — a tag exists only on entries.
    Organize {
        /// Vault folder.
        vault: PathBuf,
        /// Extra steering for the model (e.g. "only merge topics:*").
        #[arg(long, conflicts_with = "plan")]
        instructions: Option<String>,
        /// Apply a saved plan file instead of asking the model. The file is
        /// the JSON that `--json` emits — this path is fully offline.
        #[arg(long)]
        plan: Option<PathBuf>,
        /// Apply the plan's merges (default: print the plan to review).
        #[arg(long)]
        apply: bool,
        /// Emit JSON instead of text. The output is valid --plan input.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum TagsAction {
    /// List every tag with its entry count.
    List {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Rename a tag everywhere (merges if the target already exists).
    Rename {
        /// The existing tag (e.g. `topics:interp`).
        from: String,
        /// The new name (e.g. `topics:mech-interp`).
        to: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Merge one tag into another (alias of `rename`).
    Merge {
        /// The tag to fold away.
        from: String,
        /// The tag to keep.
        into: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Delete a tag from every entry that carries it.
    Delete {
        /// The tag to remove (e.g. `wf:to-cite`).
        name: String,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
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
    // clap's derived `Command` tree for this many subcommands is built as one
    // giant stack expression, which overflows Windows' default 1 MiB main
    // stack in unoptimized builds — run the real main on a roomier thread.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(real_main)
        .expect("spawn main thread")
        .join()
        .expect("main thread panicked")
}

fn real_main() -> ExitCode {
    // Route the `log` facade (engine/online/sync log through it too) to
    // stderr; silent unless RUST_LOG is set (e.g. RUST_LOG=niutero=debug).
    env_logger::init();
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

/// Open a vault and record it in the machine-local recent-vaults registry
/// (best-effort — recording never fails an open). Every command opens through
/// here so the "recent libraries" list stays current.
fn open_vault(vault: &Path) -> Result<engine::Vault, String> {
    let v = engine::open(vault)?;
    engine::record_open(&v.root);
    Ok(v)
}

/// Print a JSON value as pretty stdout (the machine-readable `--json` contract).
fn emit(value: serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    // A command that changes the library's exportable state triggers a refresh
    // of any keep-updated export targets afterwards (#45). Captured before the
    // match moves `cli.cmd`.
    let exportable_change = mutated_vault(&cli.cmd);
    // Any vault mutation (exportable or sidecar-only) is an auto-commit
    // candidate when the library opted in (`config --auto-commit true`).
    let commit_target = exportable_change
        .clone()
        .or_else(|| sidecar_mutation(&cli.cmd));
    let result = dispatch(cli.cmd);
    if result.is_ok() {
        if let Some(vault) = &exportable_change {
            refresh_keep_updated(vault);
        }
        if let Some(vault) = &commit_target {
            auto_commit_note(vault);
        }
    }
    result
}

/// Best-effort post-mutation auto-commit (stderr only — it must never change
/// the command's exit code or its `--json` stdout).
fn auto_commit_note(vault: &Path) {
    let Ok(v) = engine::open(vault) else { return };
    match engine::auto_commit_if_enabled(&v) {
        Ok(Some(msg)) => eprintln!("  ✓ auto-committed: {msg}"),
        Ok(None) => {}
        Err(e) => eprintln!("warning: auto-commit failed: {e}"),
    }
}

/// Commands that mutate only the sidecar / vault config (no exportable-state
/// change, so no keep-updated refresh) but still dirty the repo for
/// auto-commit purposes.
fn sidecar_mutation(cmd: &Cmd) -> Option<PathBuf> {
    match cmd {
        Cmd::Note {
            vault, set, clear, ..
        } if set.is_some() || *clear => Some(vault.clone()),
        Cmd::View { vault, action } if !matches!(action, ViewAction::List { .. }) => {
            Some(vault.clone())
        }
        Cmd::Config {
            vault,
            name,
            pattern,
            enrich_on_import,
            auto_commit,
            on_dup,
            ..
        } if name.is_some()
            || pattern.is_some()
            || enrich_on_import.is_some()
            || auto_commit.is_some()
            || on_dup.is_some() =>
        {
            Some(vault.clone())
        }
        Cmd::PdfConfig {
            vault,
            repo,
            auto_fetch,
            ..
        } if repo.is_some() || auto_fetch.is_some() => Some(vault.clone()),
        _ => None,
    }
}

/// The vault whose exportable state a command changes, or `None` for read-only
/// / registry-only commands. Drives the post-mutation keep-updated export.
fn mutated_vault(cmd: &Cmd) -> Option<PathBuf> {
    match cmd {
        Cmd::Add { vault, .. }
        | Cmd::Edit { vault, .. }
        | Cmd::Rm { vault, .. }
        | Cmd::Import { vault, .. }
        | Cmd::Enrich { vault, .. }
        | Cmd::Sync { vault, .. } => Some(vault.clone()),
        // Tag/status/stars change facets that `tag:`/`status:`/`stars:` filtered
        // targets select on, so they count as exportable changes too.
        Cmd::Tag {
            vault, add, remove, ..
        } if !add.is_empty() || !remove.is_empty() => Some(vault.clone()),
        // Vocabulary-wide tag mutations move the same facets, library-wide.
        Cmd::Tags { vault, action } if !matches!(action, TagsAction::List { .. }) => {
            Some(vault.clone())
        }
        Cmd::Ai {
            action: AiAction::Organize {
                vault, apply: true, ..
            },
        } => Some(vault.clone()),
        Cmd::Status {
            vault,
            set: Some(_),
            ..
        } => Some(vault.clone()),
        Cmd::Stars {
            vault,
            set: Some(_),
            ..
        } => Some(vault.clone()),
        Cmd::Rekey {
            vault, write: true, ..
        } => Some(vault.clone()),
        Cmd::Dedupe {
            vault, merge: true, ..
        } => Some(vault.clone()),
        Cmd::Normalize {
            vault, write: true, ..
        } => Some(vault.clone()),
        _ => None,
    }
}

/// Re-export every keep-updated target after a successful mutation. Best-effort:
/// the primary operation already succeeded, so a failed mirror is a warning, not
/// an error (it must not change the command's exit code). All notes go to
/// **stderr** so they never corrupt a command's `--json` stdout for a machine
/// consumer.
///
/// NOTE: these `eprintln!`s (and the auto-commit / auto-fetch / auto-enrich
/// notes) are deliberate USER-FACING product output, not logging — tests
/// assert their exact text, and they must show without RUST_LOG. Don't
/// migrate them to the `log` facade.
fn refresh_keep_updated(vault: &Path) {
    let Ok(v) = engine::open(vault) else { return };
    match engine::refresh_exports(&v) {
        Ok(outcomes) => {
            for o in outcomes {
                match o.error {
                    None => eprintln!("  ↻ {} entr(ies) → {}", o.count, o.out.display()),
                    Some(e) => eprintln!(
                        "warning: keep-updated export to {} failed: {e}",
                        o.out.display()
                    ),
                }
            }
        }
        Err(e) => eprintln!("warning: keep-updated export skipped: {e}"),
    }
}

fn dispatch(cmd: Cmd) -> Result<ExitCode, String> {
    match cmd {
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
            json,
        } => cmd_add(&vault, bibtex, from, type_, key, field, json).map(ok),
        Cmd::Edit {
            vault,
            citekey,
            field,
            unset,
            type_,
            json,
        } => cmd_edit(&vault, &citekey, field, unset, type_, json).map(ok),
        Cmd::Rm {
            vault,
            citekey,
            json,
        } => cmd_rm(&vault, &citekey, json).map(ok),
        Cmd::Tag {
            vault,
            citekey,
            add,
            remove,
            json,
        } => cmd_tag(&vault, &citekey, add, remove, json).map(ok),
        Cmd::Tags { vault, action } => cmd_tags(&vault, action).map(ok),
        Cmd::Ai { action } => cmd_ai(action),
        Cmd::Note {
            vault,
            citekey,
            set,
            clear,
            json,
        } => cmd_note(&vault, &citekey, set, clear, json).map(ok),
        Cmd::View { vault, action } => cmd_view(&vault, action).map(ok),
        Cmd::Import {
            vault,
            file,
            doi,
            on_dup,
            json,
        } => cmd_import(&vault, file, doi, on_dup, json).map(ok),
        Cmd::Export {
            vault,
            out,
            query,
            view,
            json,
        } => cmd_export(&vault, &out, query, view, json).map(ok),
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
            profile,
        } => cmd_normalize(&vault, write, check, json, profile),
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
            json,
        } => cmd_status(&vault, &citekey, set, json).map(ok),
        Cmd::Stars {
            vault,
            citekey,
            set,
            json,
        } => cmd_stars(&vault, &citekey, set, json).map(ok),
        Cmd::Analyze { vault, json } => cmd_analyze(&vault, json).map(ok),
        Cmd::Dedupe { vault, merge, json } => cmd_dedupe(&vault, merge, json).map(ok),
        Cmd::Enrich {
            vault,
            citekey,
            json,
        } => cmd_enrich(&vault, &citekey, json).map(ok),
        Cmd::Connector { vault, port, token } => cmd_connector(&vault, port, token).map(ok),
        Cmd::Pdf {
            vault,
            citekey,
            attach,
            fetch,
            push,
            pull,
            json,
        } => cmd_pdf(&vault, &citekey, attach, fetch, push, pull, json).map(ok),
        Cmd::PdfConfig {
            vault,
            repo,
            auto_fetch,
            token,
            token_stdin,
            create_repo,
            json,
        } => cmd_pdf_config(
            &vault,
            repo,
            auto_fetch,
            token,
            token_stdin,
            create_repo,
            json,
        )
        .map(ok),
        Cmd::SuggestTags {
            vault,
            citekey,
            json,
        } => cmd_suggest_tags(&vault, &citekey, json).map(ok),
        Cmd::Recent { json } => cmd_recent(json).map(ok),
        Cmd::Forget { vault } => cmd_forget(&vault).map(ok),
        Cmd::ExportTarget { vault, action } => cmd_export_target(&vault, action).map(ok),
        Cmd::Config {
            vault,
            name,
            pattern,
            enrich_on_import,
            auto_commit,
            on_dup,
            json,
        } => cmd_config(
            &vault,
            name,
            pattern,
            enrich_on_import,
            auto_commit,
            on_dup,
            json,
        )
        .map(ok),
        Cmd::SyncConfig {
            vault,
            pull,
            push,
            json,
        } => cmd_sync_config(&vault, pull, push, json).map(ok),
    }
}

fn cmd_config(
    vault: &Path,
    name: Option<String>,
    pattern: Option<String>,
    enrich_on_import: Option<bool>,
    auto_commit: Option<bool>,
    on_dup: Option<String>,
    json: bool,
) -> Result<(), String> {
    let mut v = open_vault(vault)?;
    if name.is_some() || pattern.is_some() {
        engine::set_library_meta(&mut v, name.as_deref(), pattern.as_deref())?;
    }
    if enrich_on_import.is_some() || auto_commit.is_some() || on_dup.is_some() {
        engine::set_workflow(
            &mut v,
            enrich_on_import,
            auto_commit,
            on_dup.as_deref(),
            None,
        )?;
    }
    let c = &v.config;
    if json {
        emit(serde_json::json!({
            "name": c.name,
            "citekey_pattern": c.citekey_pattern,
            "pdf_repo": c.pdf_repo,
            "workflow": {
                "enrich_on_import": c.workflow.enrich_on_import,
                "auto_commit": c.workflow.auto_commit,
                "on_dup": c.workflow.on_dup,
                "auto_fetch_pdf": c.workflow.auto_fetch_pdf,
            },
        }))?;
    } else {
        println!("name:            {}", c.name);
        println!(
            "citekey pattern: {}",
            c.citekey_pattern.as_deref().unwrap_or("(default)")
        );
        println!(
            "pdf repo:        {}",
            c.pdf_repo.as_deref().unwrap_or("(unset)")
        );
        println!("workflow:");
        println!("  enrich on import: {}", c.workflow.enrich_on_import);
        println!("  auto-commit:      {}", c.workflow.auto_commit);
        println!(
            "  on duplicate:     {}",
            c.workflow.on_dup.as_deref().unwrap_or("(tool default)")
        );
        println!("  auto-fetch pdf:   {}", c.workflow.auto_fetch_pdf);
    }
    Ok(())
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
    engine::record_open(&v.root);
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
    let v = open_vault(vault)?;
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
    let v = open_vault(vault)?;
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
    json: bool,
) -> Result<(), String> {
    let v = open_vault(vault)?;
    let source = add_source(bibtex, from, type_, key, field)?;
    let keys = engine::add(&v, source)?;
    if json {
        emit(serde_json::json!({ "added": keys }))?;
    } else {
        println!("Added {}: {}", keys.len(), keys.join(", "));
    }
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
    json: bool,
) -> Result<(), String> {
    if field.is_empty() && unset.is_empty() && type_.is_none() {
        return Err("specify at least one of --field, --unset, or --type".into());
    }
    let v = open_vault(vault)?;
    engine::edit(&v, citekey, &field, &unset, type_)?;
    if json {
        emit(serde_json::json!({ "updated": citekey }))?;
    } else {
        println!("Updated {citekey}");
    }
    Ok(())
}

fn cmd_rm(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let mut v = open_vault(vault)?;
    engine::rm(&mut v, citekey)?;
    if json {
        emit(serde_json::json!({ "removed": citekey }))?;
    } else {
        println!("Removed {citekey}");
    }
    Ok(())
}

fn cmd_tag(
    vault: &Path,
    citekey: &str,
    add: Vec<String>,
    remove: Vec<String>,
    json: bool,
) -> Result<(), String> {
    let tags = if add.is_empty() && remove.is_empty() {
        engine::current_tags(&open_vault(vault)?, citekey)?
    } else {
        let mut v = open_vault(vault)?;
        engine::set_tags(&mut v, citekey, &add, &remove)?
    };
    if json {
        emit(serde_json::json!({ "tags": tags }))?;
    } else if tags.is_empty() {
        println!("(no tags)");
    } else {
        println!("tags: {}", tags.join(", "));
    }
    Ok(())
}

fn cmd_tags(vault: &Path, action: TagsAction) -> Result<(), String> {
    match action {
        TagsAction::List { json } => {
            let v = open_vault(vault)?;
            let tags = engine::list_tags(&v);
            if json {
                let arr: Vec<_> = tags
                    .iter()
                    .map(|(name, count)| serde_json::json!({ "tag": name, "count": count }))
                    .collect();
                emit(serde_json::json!({ "tags": arr }))?;
            } else if tags.is_empty() {
                println!("(no tags)");
            } else {
                for (name, count) in tags {
                    println!("{count:>4}  {name}");
                }
            }
        }
        TagsAction::Rename { from, to, json } => {
            let mut v = open_vault(vault)?;
            let n = engine::rename_tag(&mut v, &from, &to)?;
            if json {
                emit(serde_json::json!({ "from": from, "to": to, "changed": n }))?;
            } else {
                println!("Renamed '{from}' → '{to}' on {n} entr{}", plural(n));
            }
        }
        TagsAction::Merge { from, into, json } => {
            let mut v = open_vault(vault)?;
            let n = engine::rename_tag(&mut v, &from, &into)?;
            if json {
                emit(serde_json::json!({ "from": from, "into": into, "changed": n }))?;
            } else {
                println!("Merged '{from}' into '{into}' on {n} entr{}", plural(n));
            }
        }
        TagsAction::Delete { name, json } => {
            let mut v = open_vault(vault)?;
            let n = engine::delete_tag(&mut v, &name)?;
            if json {
                emit(serde_json::json!({ "tag": name, "changed": n }))?;
            } else {
                println!("Deleted '{name}' from {n} entr{}", plural(n));
            }
        }
    }
    Ok(())
}

/// "y" / "ies" suffix for an entry count.
fn plural(n: usize) -> &'static str {
    if n == 1 {
        "y"
    } else {
        "ies"
    }
}

fn cmd_ai(action: AiAction) -> Result<ExitCode, String> {
    // Stays 0 unless an arm reports a partial failure (organize --apply with
    // failed merges) — the 0-ok / 1-error contract scripts rely on.
    let mut exit = ExitCode::SUCCESS;
    match action {
        AiAction::Config {
            enable,
            provider,
            key,
            key_stdin,
            model,
            base_url,
            json,
        } => {
            let key = if key_stdin {
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| format!("read key from stdin: {e}"))?;
                let k = line.trim().to_string();
                if k.is_empty() {
                    return Err("no key arrived on stdin".into());
                }
                Some(k)
            } else {
                key
            };
            let changed = enable.is_some()
                || provider.is_some()
                || key.is_some()
                || model.is_some()
                || base_url.is_some();
            // The whole read-modify-write runs inside the registry lock, so a
            // concurrent updater (GUI save, second CLI) can't be clobbered.
            let cfg = if changed {
                engine::update_ai_config(|cfg| {
                    if let Some(e) = enable {
                        cfg.enabled = e;
                    }
                    if let Some(p) = provider {
                        cfg.provider = p;
                    }
                    if let Some(k) = key {
                        cfg.api_key = k;
                    }
                    if let Some(m) = model {
                        cfg.model = m;
                    }
                    if let Some(b) = base_url {
                        cfg.base_url = b;
                    }
                })?
            } else {
                engine::ai_config()?
            };
            if json {
                emit(serde_json::json!({
                    "enabled": cfg.enabled,
                    "provider": cfg.provider,
                    "model": cfg.model,
                    "base_url": cfg.base_url,
                    "api_key_set": !cfg.api_key.is_empty(),
                }))?;
            } else {
                println!("enabled:  {}", cfg.enabled);
                println!(
                    "provider: {}",
                    if cfg.provider.is_empty() {
                        "anthropic (default)"
                    } else {
                        &cfg.provider
                    }
                );
                println!(
                    "model:    {}",
                    if cfg.model.is_empty() {
                        "(default)"
                    } else {
                        &cfg.model
                    }
                );
                println!(
                    "api key:  {}",
                    if cfg.api_key.is_empty() {
                        "(unset — falls back to $ANTHROPIC_API_KEY)".to_string()
                    } else {
                        mask_key(&cfg.api_key)
                    }
                );
                if !cfg.base_url.is_empty() {
                    println!("base url: {}", cfg.base_url);
                }
            }
        }
        AiAction::Test { json } => {
            let msg = engine::ai_test()?;
            if json {
                emit(serde_json::json!({ "ok": true, "message": msg }))?;
            } else {
                println!("{}", sanitize_terminal(&msg));
            }
        }
        AiAction::Ask {
            vault,
            question,
            json,
        } => {
            let v = open_vault(&vault)?;
            let answer = engine::ask(&v, &question)?;
            if json {
                emit(serde_json::json!({ "answer": answer }))?;
            } else {
                println!("{}", sanitize_terminal(&answer));
            }
        }
        AiAction::Organize {
            vault,
            instructions,
            plan,
            apply,
            json,
        } => {
            let mut v = open_vault(&vault)?;
            let plan_obj: engine::OrganizePlan = match plan {
                Some(path) => {
                    let text = std::fs::read_to_string(&path)
                        .map_err(|e| format!("read plan {}: {e}", path.display()))?;
                    serde_json::from_str(&text)
                        .map_err(|e| format!("parse plan {}: {e}", path.display()))?
                }
                None => engine::organize_tags(&v, instructions.as_deref().unwrap_or(""))?,
            };
            let applied = if apply {
                Some(engine::apply_tag_merges(&mut v, &plan_obj.merges))
            } else {
                None
            };
            if json {
                // The merges/new_tags keys make this output valid --plan input
                // (extra keys are ignored on the way back in).
                let mut out = serde_json::json!({
                    "merges": plan_obj.merges,
                    "new_tags": plan_obj.new_tags,
                });
                if let Some(results) = &applied {
                    out["applied"] = serde_json::to_value(results).map_err(|e| e.to_string())?;
                    out["changed_total"] =
                        serde_json::json!(results.iter().map(|r| r.changed).sum::<usize>());
                }
                emit(out)?;
            } else {
                if plan_obj.merges.is_empty() && plan_obj.new_tags.is_empty() {
                    println!("Nothing to tidy — the plan is empty.");
                }
                for m in &plan_obj.merges {
                    let why = if m.reason.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", sanitize_terminal(&m.reason))
                    };
                    println!("merge '{}' → '{}'{why}", m.from, m.into);
                }
                for t in &plan_obj.new_tags {
                    let why = if t.reason.is_empty() {
                        String::new()
                    } else {
                        format!("  ({})", sanitize_terminal(&t.reason))
                    };
                    println!("new tag suggestion: '{}'{why}  — advisory only", t.name);
                }
                match &applied {
                    None => {
                        if !plan_obj.merges.is_empty() {
                            println!("(plan only — re-run with --apply to merge)");
                        }
                    }
                    Some(results) => {
                        let (mut ok_n, mut skipped, mut failed) = (0usize, 0usize, 0usize);
                        for r in results {
                            match (&r.error, r.changed) {
                                (Some(e), _) => {
                                    failed += 1;
                                    println!("  ✗ '{}' → '{}': {e}", r.from, r.into);
                                }
                                (None, 0) => {
                                    skipped += 1;
                                    println!(
                                        "  – '{}' → '{}': nothing tagged '{}' (skipped)",
                                        r.from, r.into, r.from
                                    );
                                }
                                (None, n) => {
                                    ok_n += 1;
                                    println!(
                                        "  ✓ '{}' → '{}': {n} entr{}",
                                        r.from,
                                        r.into,
                                        plural(n)
                                    );
                                }
                            }
                        }
                        println!(
                            "Applied {ok_n} of {} merge(s) ({skipped} skipped, {failed} failed).",
                            results.len()
                        );
                    }
                }
            }
            // A merge that errored means the plan wasn't fully applied — a
            // script must see that without parsing stdout.
            if applied
                .as_ref()
                .is_some_and(|rs| rs.iter().any(|r| r.error.is_some()))
            {
                exit = ExitCode::from(1);
            }
        }
    }
    Ok(exit)
}

/// Show only a hint of a secret — never the whole thing, even when it's short.
fn mask_key(k: &str) -> String {
    let n = k.chars().count();
    if n < 10 {
        return format!("(set — {n} chars)");
    }
    let head: String = k.chars().take(6).collect();
    format!("{head}… ({n} chars)")
}

/// Strip C0/C1 control characters (keeping newline and tab) so model-controlled
/// text can't smuggle terminal escape sequences into the user's terminal.
fn sanitize_terminal(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

fn cmd_note(
    vault: &Path,
    citekey: &str,
    set: Option<String>,
    clear: bool,
    json: bool,
) -> Result<(), String> {
    if set.is_some() && clear {
        return Err("use either --set or --clear, not both".into());
    }
    // (action label for text output, resulting note for JSON output)
    let (action, note): (&str, Option<String>) = if let Some(text) = set {
        let mut v = open_vault(vault)?;
        engine::set_note(&mut v, citekey, Some(text.clone()))?;
        ("set", Some(text))
    } else if clear {
        let mut v = open_vault(vault)?;
        engine::set_note(&mut v, citekey, None)?;
        ("cleared", None)
    } else {
        let n = engine::current_note(&open_vault(vault)?, citekey)?;
        ("show", (!n.is_empty()).then_some(n))
    };
    if json {
        emit(serde_json::json!({ "note": note }))?;
    } else {
        match action {
            "set" => println!("Set note for {citekey}"),
            "cleared" => println!("Cleared note for {citekey}"),
            _ => match &note {
                Some(n) => println!("{n}"),
                None => println!("(no note)"),
            },
        }
    }
    Ok(())
}

fn cmd_view(vault: &Path, action: ViewAction) -> Result<(), String> {
    match action {
        ViewAction::List { json } => {
            let v = open_vault(vault)?;
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
            let mut v = open_vault(vault)?;
            engine::add_view(&mut v, name.clone(), query)?;
            println!("Added view '{name}'");
        }
        ViewAction::Rm { name } => {
            let mut v = open_vault(vault)?;
            engine::remove_view(&mut v, &name)?;
            println!("Removed view '{name}'");
        }
    }
    Ok(())
}

fn cmd_import(
    vault: &Path,
    file: Option<PathBuf>,
    doi: Option<String>,
    on_dup: Option<OnDup>,
    json: bool,
) -> Result<(), String> {
    let v = open_vault(vault)?;
    // Explicit flag wins; otherwise the library's configured default; else skip.
    let policy = match on_dup {
        Some(OnDup::Skip) => DupPolicy::Skip,
        Some(OnDup::Overwrite) => DupPolicy::Overwrite,
        Some(OnDup::Rename) => DupPolicy::Rename,
        None => engine::default_dup_policy(&v, DupPolicy::Skip),
    };
    let r = match (file, doi) {
        (Some(_), Some(_)) => return Err("use either a .bib file or --doi, not both".into()),
        (Some(f), None) => engine::import(&v, &f, policy)?,
        (None, Some(d)) => engine::import_doi(&v, &d, policy)?,
        (None, None) => return Err("specify a .bib file or --doi".into()),
    };
    // Post-import hooks (both opt-in and no-ops without their pref, so the
    // base import path stays offline). Best-effort, on stderr — they must
    // never corrupt the --json stdout or fail the import.
    match engine::auto_fetch_pdfs(&v, &r.new_keys()) {
        Ok((fetched, attempted)) if attempted > 0 => {
            eprintln!("  ⤓ auto-fetched {fetched}/{attempted} PDF(s)");
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: PDF auto-fetch skipped: {e}"),
    }
    match engine::auto_enrich(&v, &r.new_keys()) {
        Ok((filled, attempted)) if attempted > 0 => {
            eprintln!("  ✚ auto-enriched {filled}/{attempted} entr(ies)");
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: auto-enrich skipped: {e}"),
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
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

fn cmd_enrich(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = open_vault(vault)?;
    let filled = engine::enrich(&v, citekey)?;
    if json {
        emit(serde_json::json!({ "citekey": citekey, "filled": filled }))?;
    } else if filled.is_empty() {
        println!("{citekey}: already complete (nothing to fill).");
    } else {
        println!("{citekey}: filled {}", filled.join(", "));
    }
    Ok(())
}

fn cmd_connector(vault: &Path, port: u16, token: Option<String>) -> Result<(), String> {
    let v = open_vault(vault)?;
    let token = token.unwrap_or_else(engine::connector_token);
    println!("Browser connector listening on http://127.0.0.1:{port}  (Ctrl-C to stop)");
    println!("  POST BibTeX to /capture, or a bare DOI to /capture/doi");
    println!("session token: {token}");
    println!(
        "  captures must send it as 'Authorization: Bearer {token}' or 'X-Niutero-Token: {token}'"
    );
    println!(
        "  paste the token into the niutero browser extension (extension/) to capture by click"
    );
    engine::serve_connector(&v, port, &token)
}

#[allow(clippy::too_many_arguments)]
fn cmd_pdf(
    vault: &Path,
    citekey: &str,
    attach: Option<PathBuf>,
    fetch: bool,
    push: bool,
    pull: bool,
    json: bool,
) -> Result<(), String> {
    let picked = [attach.is_some(), fetch, push, pull]
        .iter()
        .filter(|b| **b)
        .count();
    if picked > 1 {
        return Err("use only one of --attach / --fetch / --push / --pull".into());
    }
    let v = open_vault(vault)?;
    // (action label, local path) — one JSON shape for every mode.
    let (action, path) = if let Some(src) = attach {
        let dest = engine::attach_pdf(&v, citekey, &src)?;
        if !json {
            println!("Attached {} -> {}", src.display(), dest.display());
        }
        ("attached", dest)
    } else if fetch {
        let dest = engine::fetch_pdf(&v, citekey)?;
        if !json {
            println!("Downloaded PDF -> {}", dest.display());
        }
        ("fetched", dest)
    } else if push {
        let remote = engine::pdf_push(&v, citekey)?;
        let local = engine::pdf_path(&v, citekey);
        if !json {
            println!("Pushed {} -> {remote}", local.display());
        }
        ("pushed", local)
    } else if pull {
        let dest = engine::pdf_pull(&v, citekey)?;
        if !json {
            println!("Pulled PDF -> {}", dest.display());
        }
        ("pulled", dest)
    } else {
        let path = engine::pdf_path(&v, citekey);
        if !json {
            let status = if path.exists() {
                "present"
            } else {
                "not attached"
            };
            println!("{} ({status})", path.display());
        }
        ("status", path)
    };
    if json {
        emit(serde_json::json!({
            "citekey": citekey,
            "action": action,
            "path": path,
            "present": path.exists(),
        }))?;
    }
    Ok(())
}

fn cmd_pdf_config(
    vault: &Path,
    repo: Option<String>,
    auto_fetch: Option<bool>,
    token: Option<String>,
    token_stdin: bool,
    create_repo: bool,
    json: bool,
) -> Result<(), String> {
    let mut v = open_vault(vault)?;
    let token = if token_stdin {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| format!("read token from stdin: {e}"))?;
        let t = line.trim().to_string();
        if t.is_empty() {
            return Err("no token arrived on stdin".into());
        }
        Some(t)
    } else {
        token
    };
    if let Some(t) = token {
        engine::set_hf_token(&t)?;
    }
    // Repo + auto-fetch are LIBRARY properties (.niutero/config.toml, synced):
    // configure once and every machine/collaborator reads them from the vault.
    if let Some(r) = repo {
        engine::set_pdf_repo(&mut v, &r)?;
    }
    if let Some(a) = auto_fetch {
        engine::set_workflow(&mut v, None, None, None, Some(a))?;
    }
    // Create after the config lands, so `--repo u/r --create-repo` works in one go.
    let created = if create_repo {
        Some(engine::create_pdf_repo(&v)?)
    } else {
        None
    };
    let repo_now = engine::pdf_repo(&v)?;
    let auto_now = engine::pdf_auto_fetch_enabled(&v);
    let token_set = engine::hf_token_set()?;
    if json {
        // The token itself never appears in any output — only whether one is set.
        let mut out = serde_json::json!({
            "repo": repo_now.clone().unwrap_or_default(),
            "auto_fetch": auto_now,
            "token_set": token_set,
        });
        if let Some(c) = &created {
            out["created"] = serde_json::json!(c);
        }
        emit(out)?;
    } else {
        println!("repo:       {}", repo_now.as_deref().unwrap_or("(unset)"));
        println!("auto-fetch: {auto_now}");
        println!(
            "hf token:   {}",
            if token_set {
                "set (machine-local)"
            } else {
                "(unset)"
            }
        );
        if let Some(c) = created {
            println!("{c}");
        }
    }
    Ok(())
}

fn cmd_suggest_tags(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = open_vault(vault)?;
    let tags = engine::suggest_tags(&v, citekey)?;
    if json {
        emit(serde_json::json!({ "citekey": citekey, "tags": tags }))?;
    } else if tags.is_empty() {
        println!("(no suggestions)");
    } else {
        println!("Suggested tags for {citekey}:");
        for t in &tags {
            println!("  {t}");
        }
        println!("Apply with: niutero-cli tag <vault> {citekey} --add <tag>");
    }
    Ok(())
}

fn cmd_export(
    vault: &Path,
    out: &Path,
    query: Option<String>,
    view: Option<String>,
    json: bool,
) -> Result<(), String> {
    let v = open_vault(vault)?;
    let n = engine::export(&v, filter_from(query, view)?, out)?;
    if json {
        emit(serde_json::json!({ "exported": n, "out": out.display().to_string() }))?;
    } else {
        println!("Exported {n} entr(ies) to {}", out.display());
    }
    Ok(())
}

fn cmd_tex_scan(
    vault: &Path,
    tex: &[PathBuf],
    out: Option<PathBuf>,
    json: bool,
) -> Result<ExitCode, String> {
    let v = open_vault(vault)?;
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
        // In --json mode the report JSON is the stdout payload; this notice goes
        // to stderr so it doesn't corrupt the stream for a machine consumer.
        if json {
            eprintln!("Wrote {n} cited entr(ies) to {}", out.display());
        } else {
            println!("Wrote {n} cited entr(ies) to {}", out.display());
        }
    }
    // CI gate: undefined references are actionable.
    Ok(if report.missing.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn cmd_connect(vault: &Path, url: &str) -> Result<(), String> {
    let v = open_vault(vault)?;
    engine::connect(&v, url)?;
    println!("Connected {} to {url}", v.root.display());
    Ok(())
}

fn cmd_sync(vault: &Path, message: Option<String>) -> Result<ExitCode, String> {
    let v = open_vault(vault)?;
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

fn cmd_normalize(
    vault: &Path,
    write: bool,
    check: bool,
    json: bool,
    profile: Option<String>,
) -> Result<ExitCode, String> {
    if write && check {
        return Err("use either --write or --check, not both".into());
    }
    let v = open_vault(vault)?;
    let changes = if write {
        engine::normalize_apply(&v, profile.as_deref())?
    } else {
        engine::normalize_preview(&v, profile.as_deref())?
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
    let v = open_vault(vault)?;
    println!("{}", engine::cite(&v, citekey)?);
    Ok(())
}

fn cmd_history(vault: &Path, citekey: &str, json: bool) -> Result<(), String> {
    let v = open_vault(vault)?;
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
        let mut v = open_vault(vault)?;
        engine::rekey_apply(&mut v, pattern.as_deref())?
    } else {
        let v = open_vault(vault)?;
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

fn cmd_status(
    vault: &Path,
    citekey: &str,
    set: Option<StatusArg>,
    json: bool,
) -> Result<(), String> {
    let status = if let Some(s) = set {
        let mut v = open_vault(vault)?;
        let status = engine::Status::from(s);
        engine::set_status(&mut v, citekey, status)?;
        status.as_str().to_string()
    } else {
        engine::show(&open_vault(vault)?, citekey)?.status
    };
    if json {
        emit(serde_json::json!({ "status": status }))?;
    } else if set.is_some() {
        println!("Set status of {citekey} to {status}");
    } else {
        println!("{status}");
    }
    Ok(())
}

fn cmd_stars(vault: &Path, citekey: &str, set: Option<u8>, json: bool) -> Result<(), String> {
    // Resulting rating: 0 (or cleared) is represented as `None`.
    let stars: Option<u8> = if let Some(n) = set {
        let mut v = open_vault(vault)?;
        engine::set_stars(&mut v, citekey, Some(n))?;
        (n != 0).then_some(n)
    } else {
        engine::show(&open_vault(vault)?, citekey)?.stars
    };
    if json {
        emit(serde_json::json!({ "stars": stars }))?;
    } else if let Some(n) = set {
        if n == 0 {
            println!("Cleared rating of {citekey}");
        } else {
            println!("Set {citekey} to {n} star(s)");
        }
    } else {
        match stars {
            Some(n) => println!("{n}"),
            None => println!("(unrated)"),
        }
    }
    Ok(())
}

fn cmd_analyze(vault: &Path, json: bool) -> Result<(), String> {
    let v = open_vault(vault)?;
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

fn cmd_dedupe(vault: &Path, merge: bool, json: bool) -> Result<(), String> {
    if merge {
        let mut v = open_vault(vault)?;
        let merges = engine::dedupe_merge(&mut v)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&merges).map_err(|e| e.to_string())?
            );
        } else if merges.is_empty() {
            println!("No duplicates found.");
        } else {
            println!("Merged {} cluster(s):", merges.len());
            for m in &merges {
                println!("  kept {} (dropped {})", m.kept, m.dropped.join(", "));
            }
        }
    } else {
        let v = open_vault(vault)?;
        let groups = engine::dedupe_preview(&v)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&groups).map_err(|e| e.to_string())?
            );
        } else if groups.is_empty() {
            println!("No duplicates found.");
        } else {
            println!("{} duplicate cluster(s):", groups.len());
            for g in &groups {
                println!("  {}", g.citekeys.join(", "));
            }
            println!("(re-run with --merge to fold each cluster into its first entry)");
        }
    }
    Ok(())
}

fn cmd_recent(json: bool) -> Result<(), String> {
    let recent = engine::recent_vaults()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recent).map_err(|e| e.to_string())?
        );
    } else if recent.is_empty() {
        println!("(no recent vaults)");
    } else {
        for r in &recent {
            println!("{}", r.path.display());
        }
    }
    Ok(())
}

fn cmd_forget(vault: &Path) -> Result<(), String> {
    if engine::forget_vault(vault)? {
        println!("Forgot {}", vault.display());
    } else {
        println!("{} was not in the recent list", vault.display());
    }
    Ok(())
}

fn cmd_export_target(vault: &Path, action: ExportTargetAction) -> Result<(), String> {
    let v = open_vault(vault)?;
    match action {
        ExportTargetAction::List { json } => {
            let targets = engine::export_targets(&v)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&targets).map_err(|e| e.to_string())?
                );
            } else if targets.is_empty() {
                println!("(no keep-updated export targets)");
            } else {
                for t in &targets {
                    match &t.query {
                        Some(q) => println!("{}  [filter: {q}]", t.out.display()),
                        None => println!("{}", t.out.display()),
                    }
                }
            }
        }
        ExportTargetAction::Add { out, query } => {
            engine::export_target_add(&v, &out, query.clone())?;
            match query {
                Some(q) => println!("Keeping {} updated (filter: {q})", out.display()),
                None => println!("Keeping {} updated", out.display()),
            }
        }
        ExportTargetAction::Rm { out } => {
            if engine::export_target_remove(&v, &out)? {
                println!("Stopped keeping {} updated", out.display());
            } else {
                println!("{} was not a registered target", out.display());
            }
        }
    }
    Ok(())
}

fn cmd_sync_config(
    vault: &Path,
    pull: Option<bool>,
    push: Option<bool>,
    json: bool,
) -> Result<(), String> {
    let v = open_vault(vault)?;
    let prefs = if pull.is_some() || push.is_some() {
        engine::set_sync_prefs(&v, pull, push)?
    } else {
        engine::sync_prefs(&v)?
    };
    // The remote is read straight from the vault's git repo (set via
    // `connect`), so an already-configured vault shows it without re-entry.
    let remote = engine::remote_url(&v);
    if json {
        emit(serde_json::json!({
            "pull": prefs.pull,
            "push": prefs.push,
            "remote": remote,
        }))?;
    } else {
        println!("sync strategy: pull={}, push={}", prefs.pull, prefs.push);
        println!(
            "remote:        {}",
            remote.as_deref().unwrap_or("(none — run `connect`)")
        );
    }
    Ok(())
}
