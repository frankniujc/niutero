# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A ground-up rewrite of **niutero**, a lightweight, LaTeX-oriented citation manager
(Rust). Phase 1 (the complete, tested CLI) is done; Phase 2 (`niutero-gui`, egui)
is built and evolving. This file captures the durable rules and architecture.

Where the docs live:

- `plan.md` — the authoritative spec for scope, milestones, and the CLI command
  surface. Read it first.
- `design/` — per-feature walkthroughs (`overview.md`, then one doc per area)
  tracing the real data flow `CLI → engine → crate → disk`, naming actual
  functions and the tests that cover them. **They are kept in sync with the code:
  when you change a feature, update its design doc and touch the tests it names.**
- `handoff.md` — the "where are we right now" layer: current state, wave history,
  remaining work. Update it at the end of a work session.
- `gui-button-audit.md` — the real-vs-mock map of the GUI surface.
- `extension/README.md` — the browser connector: security model, load-unpacked
  steps, the port contract.

## Non-negotiable rules

1. **Written from scratch — never read, port, or copy code from the old project at
   `../niutero`.** That project is a *product* reference only (it tells you what
   features exist, the data model, and the invariants), never an implementation
   source. This is an explicit user instruction; honor it strictly. The single
   user-authorized exception: `../bib_fixer/fix_bib.py` served as the explicit
   spec for `niutero-norm`'s offline rules.
2. **The CLI is the complete interface.** Every capability is a `niutero-cli`
   subcommand, with `--json` output and tests, *before* anything else consumes it.
   The GUI is a thin client over this same surface, able to do nothing the CLI
   cannot. New features still land engine → CLI → GUI, in that order.
3. **`.bib` is the source of truth and stays niutero-agnostic.** Private data (tags,
   notes, status, stars, saved views) lives in the hidden `.niutero/` sidecar,
   **never** in `references.bib`. A collaborator who doesn't use niutero must get
   a clean `.bib`.
4. **Deterministic, byte-stable serialization.** For an unchanged entry,
   `parse → serialize` must be byte-identical (stable field order, preserved
   `@string`/`@preamble`/`@comment`). This is the project's #1 technical risk — it is
   what keeps git diffs/merges clean — so it was locked first (M1) and is guarded
   by round-trip property tests against a large real `.bib` fixture.

## Architecture

A Cargo workspace that keeps domain logic out of any UI. Dependency direction
(arrows point at the dependency):

```
niutero-cli / niutero-gui ──► niutero-engine ──► niutero-vault ──► niutero-bib ──► niutero-core
                                    │
                                    ├──► niutero-sync    git sync by shelling out to system git (no libgit2)
                                    ├──► niutero-norm    offline, propose-only normalization (norm.toml + profiles)
                                    └──► niutero-online  system-curl shell-out: DOI/arXiv/OpenReview, Anthropic LLM, HF PDFs
```

- `niutero-core` — domain model, no IO/UI: `BibEntry` + `validate()`, `Library`,
  filter queries, citekey generation, entry merge + dedup, tex-scan.
- `niutero-bib` — tolerant `.bib` parser + deterministic serializer (the foundation).
- `niutero-vault` — vault IO: `.niutero/` sidecar, atomic writes (temp + rename),
  exclusive lock, machine-local registry.
- `niutero-engine` — the operations layer: **every capability is a function here**
  over an open `Vault`, returning owned DTOs (e.g. `EntryView`). Also hosts the
  connector server, sync ops, tags ops, AI, PDF ops.
- `niutero-cli` — thin clap arg-parse + output shell (binary: `niutero-cli`).
- `niutero-gui` — egui thin client calling the engine directly, never shelling
  out (binary: `niutero`).

`niutero-engine` is what makes "CLI is the complete interface" real: add new
capabilities to the engine first, then expose a thin CLI command, then the GUI.
Entries from untrusted input must pass `BibEntry::validate()` before being
written (the serializer assumes valid, brace-balanced values).

Data model: **a library is a folder** ("vault"). The folder holds `references.bib`
(portable, tool-agnostic) plus a `.niutero/` sidecar (`config.toml`, `meta.json`
keyed by citekey, `views.toml`, optional `norm.toml`), all git-synced with the
library. A machine-local registry tracks recent vaults and personal prefs and is
**not** synced. Organization is tags + named saved filter views — there is no
"collection" object.

Optional features (git sync, normalization, online enrich, import-by-DOI, LaTeX
tex-scan, LLM, PDF, browser connector) layer on later and must stay off the base
path: the core must work fully offline with all of them disabled.

### Browser connector

The connector's client — a Manifest V3 extension (Chrome + Firefox) — lives in
`extension/` (plain JS, not a workspace crate). It is a thin client too: it only
POSTs `{ identifier, metadata }` to the loopback `connector` endpoints, so it can
do nothing the connector can't. Resolution happens server-side (OpenReview venue
BibTeX, doi.org for DOI/arXiv, scraped meta tags as fallback); every capture is
re-keyed and normalized on import; a failed resolution falls back to the page's
scraped metadata instead of losing the capture. Security is Zotero-style
(loopback bind + Host checks; `POST /import` additionally requires a present
extension Origin — `Origin: null` is refused; no token). The port (`23510`) is
referenced in three places that must stay in sync: Rust
`connector::DEFAULT_PORT` (the CLI defaults from the same constant),
`manifest.json` (`host_permissions`), and `popup.js` (`NIUTERO`).

## Commands

All `niutero-cli` subcommands take the vault folder as the first positional arg
and support `--json` — except `connector` (a blocking server), `cite` (a single
literal line), `init`/`connect`/`forget` (trivial confirmations), and `sync`
(status goes to exit codes). Exit codes are `0` ok / `1` error / `2` actionable
(CI gate).

```sh
cargo build --workspace
cargo test  --workspace
cargo test  -p niutero-bib                       # one crate
cargo test  -p niutero-bib roundtrip::byte_stable # one test (name filter)

cargo run   -p niutero-cli -- <subcommand> <vault> [options]   # binary: niutero-cli (the GUI binary is niutero)

cargo fmt    --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Logging is via `RUST_LOG` (e.g. `RUST_LOG=niutero=debug cargo run -p niutero-cli -- ...`).

## Platform note

Developed on a Windows-on-ARM machine **without Visual Studio**, so a non-standard
linker setup (llvm-mingw + xwin) is required to build. The resulting
`.cargo/config.toml` is machine-specific and gitignored — the product itself stays
portable; this is only the dev-machine toolchain workaround.
`rust-toolchain.toml` pins the toolchain to 1.89 to match that setup.

Gotcha: a running `niutero.exe` (GUI) locks its own binary, so a rebuild can leave
the **old** exe in place. If a GUI/connector fix "didn't work", close the GUI and
rebuild before debugging further.
