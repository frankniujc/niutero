# Handoff — niutero_2

A working handoff for whoever (human or Claude) picks this up next. For the
*spec* read `plan.md`; for the durable rules/architecture read `CLAUDE.md`.
This file is the "where are we right now" layer on top of those.

## What this is

A ground-up, **CLI-first** rewrite of niutero, a lightweight LaTeX-oriented
citation manager in Rust. Phase 1 = a complete, tested CLI for all base +
optional functionality. Phase 2 (a GUI as a thin client over `niutero-engine`)
is **not being built** — explicitly descoped by the user.

Hard rule: **written from scratch.** The old project at `../niutero` is a
*product* reference only (features, data model, invariants) — never an
implementation source. Exception the user authorized: `../bib_fixer/fix_bib.py`
is the explicit spec that W2's offline normalizer was ported from.

## Current state (2026-05-29)

- **Phase 1 is COMPLETE** — base scope (M1–M5) plus every optional feature, all
  built, tested offline, reviewed, and committed. `niutero-engine` is the
  operations layer; the CLI is a thin shell over it. (External features' live
  network paths are code-complete but unverified in this offline sandbox — see
  "Remaining work".)
- **Waves**, in order. All committed; all unpushed. Done:
  - **Wave 1** (`8dfd8ee`): small deferred fixes — CRLF/BOM input normalization,
    `open` hardening, entry-type encapsulation, init scaffolding, `cite`.
  - **W2** (`5851e4e`, pushed): **real offline normalization** — full port of
    `fix_bib`'s offline rules into `niutero-norm` (keep-field whitelist,
    doi→url + drop, author clipping, title `{{...}}` capital protection,
    conference-acronym tagging + bare-acronym expansion, ACL-anthology venue
    inference, booktitle volume-strip, content-word title-casing, whitespace
    tidy). All rules idempotent. New `NormConfig` schema + documented
    `norm.toml`.
  - **W3a** (`aa94551`, **NOT yet pushed**): byte-stable repo hygiene
    (`connect`/`sync` write `.gitattributes` `* text=auto eol=lf` + set
    `core.autocrlf=false` local) and **stats-aware commit messages**
    (`niutero: 3 added, 1 changed` derived from the entry-level diff vs HEAD).
  - **W3b** (**committed, NOT yet pushed**): **per-entry `history` command**.
    `niutero-bib::entry_line_span` recovers an entry's exact 1-based line range
    from arbitrary `.bib` text (parser now tracks byte spans via `run_spanned`;
    `run` delegates to it). `niutero-sync::log_lines` wraps
    `git log -L<s>,<e>:references.bib --no-patch --format=…` (crate stays
    dependency-free; plain `Commit` struct). Engine `history(v, citekey)` locates
    the span in the **committed HEAD blob** (not the working tree — `git log -L`
    numbers lines against HEAD) and maps to a `HistoryCommit` Serialize DTO. CLI
    `history <citekey> [--json]`. Reviewed by a 4-lens adversarial workflow;
    fixes landed (lone-CR line-count fix, HEAD-driven existence check so a
    locally-deleted entry's history still shows, reworded no-commit error).
  - **W-design** (**committed, NOT yet pushed**): four operational features
    distilled from the UI design handoff (`api.anthropic.com/.../Niutero.html`;
    the UI itself is **not** built — only the offline engine/CLI logic it implies):
    (1) **citation-key pattern + re-key** — `niutero-core::KeyPattern`
    (`{auth}{year}{title.N}{Title.N}{title-content-word.N}`, casing-by-token,
    shared title-word cursor) + `Config.citekey_pattern`; engine
    `rekey_preview`/`rekey_apply` (letter-suffix collisions, churn-minimizing,
    **migrates the meta.json sidecar** on rename, validates + rolls back on
    sidecar-write failure); `add` auto-keys when `--key` is omitted; CLI
    `rekey [--write] [--pattern] [--json]`. (2) **status + stars** sidecar fields
    (`EntryMeta`, default-pruned) + `set_status`/`set_stars` + `EntryView` +
    `status:`/`stars:>=N` filter terms; CLI `status`/`stars`. (3) **`analyze`** —
    offline health report (offline-changeable, odd titles, inconsistent venues,
    missing url/year). (4) **structured normalize diffs** (`NormChange.diffs`) +
    `normalize --json`. Reviewed by a 5-lens adversarial workflow (9 findings);
    fixes landed (rekey idempotence on disambiguated keys, preview validates like
    `--write`, sidecar-write rollback, `is_empty` folds defaults, `is_odd_title`
    spares acronyms, stale-meta comment, help text).
  - **W-norm** (**committed, NOT yet pushed**): **default AI/ML venue-canonicalization
    ruleset** for `niutero-norm`. New `canonicalize_venues` config (default on)
    collapses every ordinal/year/"Proceedings of the …" variant of a recognized
    conference/journal to one canonical `Full Name (ACRONYM)` string (unified
    `CONFERENCE_RULES` → `CANONICAL_VENUE` tables; ACL-anthology id authoritative;
    bare acronym + "Proc. of X" handled). Covers the major ML/NLP/CV/IR venues;
    non-AI venues left alone. Each canonical is a fixed point (guarded by a unit
    test over the whole table) so it stays idempotent. Tested against the real
    `~/Desktop/all.bib` (1153 entries, idempotent; 305→164 distinct booktitles)
    via the optional `NIUTERO_BIB_FIXTURE` env-var test, plus Google-Scholar-style
    messy fixtures. Reviewed by a 3-lens adversarial workflow (5 findings);
    fixes landed (NeurIPS D&B word-order, tightened SIGIR/KDD `acm .*` patterns so
    siblings aren't mislabeled, restored bare-NeurIPS end anchor, discriminating
    CVPR-vs-ICCV test).
  - **W3c** (**committed, NOT yet pushed**): **structured 3-way merge resolver**.
    `niutero-core::merge` (base/ours/theirs `&[BibEntry]` → merged + entry/field
    conflicts; proptest + a model-based proptest). `niutero-sync` no longer
    auto-aborts on conflict — `pull` leaves the merge in progress; new
    `conflicted_paths` / `merge_stage(1/2/3)` / `finalize_merge` / `abort_merge`.
    Engine `sync` calls `try_resolve_merge` on a pull conflict: auto-merges
    references.bib when safe (commits the merge → `Synced { merged: true }`),
    else aborts → `Conflict`. **Conservative** (after a 6-finding adversarial
    review that caught two HIGH silent-data-loss bugs): bails unless all three
    git stages exist (no whole-file modify/delete), both sides agree on the
    verbatim/`@string` blocks, and only references.bib conflicted.
  - **W4a** (`3c6563d`): **file locking** (#46). `Vault::lock()` (std 1.89
    `File::try_lock`, lock file in temp dir keyed by vault path) — every mutating
    engine op takes it; readers don't. Serializes concurrent processes.
  - **W4b** (`a7fcb53`): **duplicate detection & merge** (#47).
    `niutero-core::dedup::duplicate_groups` (surname+year+title signature);
    `analyze` "Likely duplicates" check; engine `dedupe_preview`/`dedupe_merge`
    (fold cluster into richest entry, union fields + sidecar); CLI `dedupe [--merge]`.
  - **W4c** (`origin`-unpushed): **normalize profiles** (half of #48).
    `[profiles.<name>]` in norm.toml, `NormConfig::resolve`, `normalize --profile`.
    Plus a top-level repo **README.md** (long-flagged gap).
  - **W5a–W5d** (committed, unpushed): **the five external-service features**, all
    built behind the offline base path. HTTP shells out to the system `curl` (the
    same pattern niutero-sync uses for `git`); `niutero-online` is the new crate.
    (a) **DOI import** — `import --doi` (doi.org content negotiation → BibTeX).
    (b) **online enrich** — `enrich <key>` fills missing fields from the entry's
    DOI. (c) **browser connector** — `connector` runs a tiny loopback HTTP server
    whose `POST /capture` adds BibTeX (the browser-extension endpoint). (d) **PDF
    management** — `pdf <key> [--attach|--fetch]`, binaries in git-ignored
    `pdfs/`, never in the .bib. (e) **LLM tag suggestions** — `suggest-tags <key>`
    via the Anthropic Messages API (key via a temp `curl -K` config, never argv).
    Pure logic (URL/prompt/parse/HTTP-spec building) is unit-tested; the actual
    network calls **could not be exercised in this offline sandbox** — they are
    code-complete but **network-unverified**.
  - **W6** (`5e29770`, unpushed): **machine-local registry + the features on it**,
    finishing Phase 1's base scope. `niutero-vault::registry` (`vaults.toml` at the
    platform config dir or `$NIUTERO_REGISTRY`; never synced). #45 **keep-updated
    auto-export** (`export-target add/rm/list`; targets re-exported after every
    change via a central CLI trigger; atomic mirror writes; refuses references.bib
    even by aliased spelling). #48 **sync-strategy config** (`sync-config`;
    machine-local pull/push toggles `sync()` honors). **recent**/**forget**
    commands. **`--json`** added to add/edit/rm/tag/note/import/export/status/stars.
    Reviewed by a read-only adversarial workflow (6 findings, all fixed): the
    references.bib-alias data-loss path (HIGH), an unlocked registry read-modify-save
    race (now a locked `with_registry_mut`), non-atomic mirror writes, connector
    captures not refreshing mirrors, a tex-scan `--json` stdout leak, and
    non-hermetic `sync_*` tests (isolated via a shared crate-level registry env lock).

- **Git**: `main` is **14 commits ahead of `origin/main`** (W3a · W3b · W-design ·
  W-norm · W3c · W4a · W4b · W4c · W5a · W5b · W5c · W5d · W6 — all unpushed; the
  user has not asked to push). Working tree clean.
  Remote: `git@github.com:frankniujc/niutero_2.git`.
- **Tests**: full `cargo test --workspace` green at W6 (**269 tests**); fmt +
  clippy (`-D warnings`) clean. Re-run the full gate before the next commit.
  Norm has an optional whole-library idempotence test: run with
  `NIUTERO_BIB_FIXTURE=/path/to/library.bib cargo test -p niutero-norm`.

## Remaining work — Phase 1 is COMPLETE

Every Phase-1 item (base scope + all optional features) is now built, tested
offline, and committed. Nothing buildable remains for Phase 1. What's left is
out of Phase-1 scope or unverifiable here:

- **External features are network-UNVERIFIED.** DOI import, enrich, connector,
  PDF fetch, and LLM tag-suggestions are code-complete with their pure logic
  tested, but no live HTTP/API call could run in this offline sandbox. Before
  trusting them, exercise each against the real services (a real DOI, an entry
  with a fillable DOI, the loopback `POST /capture`, a PDF url, and a request
  with `$ANTHROPIC_API_KEY` set) and add the network-path integration coverage.
- **Phase 2 GUI** — explicitly descoped. It will be a thin client over
  `niutero-engine` (every capability already has an engine fn + CLI command).
- **Cross-platform CI matrix** — not set up (developed on Windows-on-ARM only).

## Known limitations (acceptable, documented)

- The registry's `record_open` and the user-initiated pref mutators take a
  cross-process lock (`vaults.toml.lock`), so a confirmed pref change isn't lost
  to a concurrent writer. Registry *reads* are lock-free (the atomic rename means
  a reader sees the old or new file whole, never torn).
- Pre-W6 deferred path-resolution note still stands (`file_at_head` repo-root vs
  cwd-relative) — see the gotchas section; only bites a vault nested in a larger
  repo.

## Build & test

```sh
cargo build --workspace
cargo test  --workspace
cargo test  -p niutero-norm                 # one crate
cargo fmt   --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run   -p niutero-cli -- <subcommand> <vault> [--json]
```

**Platform gotcha**: developed on Windows-on-ARM **without Visual Studio**, so a
non-standard linker (llvm-mingw `lld-link.exe` + xwin libs) is required. The
resulting `.cargo/config.toml` is machine-specific and **gitignored** — the
product stays portable; this is only the dev-machine toolchain workaround.
`rust-toolchain` is pinned to 1.89.

## Architecture (crates, in milestone/build order)

```
niutero-core    domain model: BibEntry + validate(), Library, filter, texscan — no IO/UI
niutero-bib     tolerant .bib parser + deterministic serializer (the foundation)
niutero-vault   vault IO: .niutero/ sidecar, atomic writes (temp + fsync + rename)
niutero-sync    git sync by shelling out to system `git` (no libgit2, no creds)
niutero-norm    offline, propose-only normalization (W2 port of bib_fixer)
niutero-engine  operations layer — EVERY capability is a fn here over an open Vault
niutero-cli     thin clap arg-parse + output shell (binary: `niutero`)
```

`niutero-engine` is the reusable surface that makes "CLI is the complete
interface" real. **Add new capabilities to the engine first**, then expose a
thin CLI command.

## Invariants you must not break

1. `.bib` is the **source of truth** and stays **niutero-agnostic** — private
   data (tags/notes/views) lives only in `.niutero/`, never in `references.bib`.
2. **Byte-stable serialization**: for an unchanged entry, `parse → serialize` is
   byte-identical (stable field order; `@string`/`@preamble`/`@comment`
   preserved). Guarded by round-trip proptests. This is the #1 technical risk.
3. **Validate at the boundary**: untrusted entries pass `BibEntry::validate()`
   before being written (serializer assumes valid, brace-balanced values).
4. **Atomic writes**: temp + fsync + rename.
5. Exit codes: `0` ok / `1` error / `2` actionable (CI gate — tex-scan undefined
   refs, `normalize --check`, sync conflict; clap usage errors also exit 2).

## Gotchas / lessons (save yourself the rediscovery)

- **`fields` stores INNER field values** (parser strips one outer brace level),
  *unlike* `fix_bib` which keeps `{...}`. The normalizer operates on inner text.
- **Rust `regex` has no look-ahead/look-behind.** W2 dropped `fix_bib`'s ICCV
  `(?! and)` guard — CVPR is matched *first* in `CONFERENCE_RULES`, so a CVPR
  title never reaches the ICCV rule. Keep rule order load-bearing.
- **`niutero_bib::parse()` is infallible** — returns `Vec<BibItem>` directly,
  not a `Result`.
- **`cargo build --workspace` does NOT compile test code**; a private-field or
  test-only break only shows under `clippy --all-targets` or `cargo test`. Run
  the full gate, not just build.
- **PowerShell + git commit**: `@'...'@` here-strings got mangled when chained on
  one line; multi-paragraph messages went through reliably as repeated `-m`
  single-quoted flags. The `Bash` tool's HEREDOC form also works.
- **Worktree/parallel agents were unavailable** this project (harness cached
  "not a git repository" from before `git init`), so work has been done
  sequentially in the main session.
- **`git log -L` numbers lines against HEAD, not the working tree** — verified
  empirically. So `history` reads the entry's span from `git show HEAD:…`, never
  the on-disk file. And git counts lines by `\n` only: a lone CR is NOT a line
  break, so `entry_line_span` must count `\n` on text where lone CRs are *kept*
  (its parse text folds them to `\n`, which is correct for canonical output but
  would over-count lines here).
- **Known deferred (W3b review, low severity, intentionally not fixed):**
  (a) `file_at_head` uses a **repo-root-relative** path (`HEAD:references.bib`)
  while `log_lines` uses a **cwd-relative** `-L:references.bib`; they agree only
  when repo root == vault root (always true for `connect`-managed vaults, but a
  vault manually nested in a larger repo would mis-report). Root cause predates
  W3b (also affects `auto_commit_message`); fixing needs repo-wide path
  resolution (`git rev-parse --show-prefix`). (b) The "not a git repository"
  tests rely on `is_repo` returning false for the OS temp dir; if `TMPDIR` ever
  sits inside a git work tree the upward walk escapes it (pre-existing pattern,
  env-specific). Consider `GIT_CEILING_DIRECTORIES` if it ever bites CI.

## Pointers

- `plan.md` — authoritative scope/milestones/CLI surface.
- `CLAUDE.md` — durable rules + architecture.
- `design/` — step-by-step walkthroughs (overview/bib/vault/init scaffolded;
  more to fill in).
- `memory/` — cross-session memory: `project_niutero_rebuild.md`,
  `project_niutero_deferred.md`, `reference_niutero_original.md`.
- `../bib_fixer/fix_bib.py` — the offline-normalization spec (W2 source).
