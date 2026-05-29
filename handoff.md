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

- **Phase 1 (M1–M5) is complete and was reviewed** (8-agent review, fixes
  landed, `niutero-engine` extracted as the operations layer).
- **Post-Phase-1 backlog in progress**, routed as waves. Done so far:
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

- **Git**: `main` is **2 commits ahead of `origin/main`** (W3a + W3b unpushed).
  Working tree clean. Remote: `git@github.com:frankniujc/niutero_2.git`.
- **Tests**: full `cargo test --workspace` green at W3b (170 tests); fmt + clippy
  (`-D warnings`) clean. Re-run the full gate before the next commit.

## Remaining work (tracked as tasks #43–#48)

Routing decision in force: **do the non-external-dependency waves first
(W2–W4)**; external-service crates are deferred (see below).

- ~~**W3b — per-entry `history` command** (#43)~~ — **DONE** (see above).
- **W3c — 3-way entry-level merge resolver** (#44): `niutero-core::merge`
  (base/ours/theirs → merged + conflicts, entry- and field-level), `niutero-sync`
  reads `:1:/:2:/:3:` stages during a conflict and finalizes-or-aborts, engine
  attempts a structured merge on pull conflict and auto-commits if clean. Cover
  with a proptest. This is the biggest single remaining piece.
- **W4 — medium batch**: keep-updated auto-export (#45), file locking via fs2
  (#46), duplicate detection & merge (#47), normalize profiles + sync-strategy
  config (#48).

Each wave: tests + `cargo fmt --all --check` + `cargo clippy --workspace
--all-targets -- -D warnings` + `cargo test --workspace` all green, then commit
(and push when convenient).

## Deferred — do NOT build now

- **Phase 2 GUI** (descoped entirely).
- **External-service crates**: DOI import/fetch, online enrich, browser
  connector, LLM, PDF management; cross-platform CI. These must stay off the
  base path — the core works fully offline with all of them disabled.
- See `memory/project_niutero_deferred.md` for the comprehensive backlog.

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
