# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A ground-up rewrite of **niutero**, a lightweight, LaTeX-oriented citation manager
(Rust). The repo is greenfield — `plan.md` is the authoritative spec for scope,
milestones, and the CLI command surface. Read it first. This file captures the
durable rules and architecture; `plan.md` carries the milestone detail.

## Non-negotiable rules

1. **Written from scratch — never read, port, or copy code from the old project at
   `../niutero`.** That project is a *product* reference only (it tells you what
   features exist, the data model, and the invariants), never an implementation
   source. This is an explicit user instruction; honor it strictly.
2. **The CLI is the complete interface.** Every capability is a `niutero-cli`
   subcommand, with `--json` output and tests, *before* anything else consumes it.
   The GUI (`niutero-gui`, Phase 2 — now built, egui) is a thin client over this
   same surface, able to do nothing the CLI cannot. New features still land
   engine → CLI → GUI, in that order.
3. **`.bib` is the source of truth and stays niutero-agnostic.** Private data (tags,
   notes, saved views) lives in the hidden `.niutero/` sidecar, **never** in
   `references.bib`. A collaborator who doesn't use niutero must get a clean `.bib`.
4. **Deterministic, byte-stable serialization.** For an unchanged entry,
   `parse → serialize` must be byte-identical (stable field order, preserved
   `@string`/`@preamble`/`@comment`). This is the project's #1 technical risk — it is
   what keeps git diffs/merges clean — so it is locked first (M1) and guarded by
   round-trip property tests against a large real `.bib` fixture.

## Architecture

A Cargo workspace that keeps domain logic out of any UI. Crates (built in
milestone order — see `plan.md`):

```
niutero-core    domain model (BibEntry + validate(), Library, filter) — no IO/UI
niutero-bib     tolerant .bib parser + deterministic serializer   <- the foundation
niutero-vault   vault IO: .niutero/ sidecar, atomic writes (temp + rename)
niutero-sync    git sync by shelling out to system git
niutero-norm    offline, propose-only normalization
niutero-online  curl shell-out: DOI / Anthropic / HuggingFace
niutero-engine  operations layer: init/open/list/show/add/edit/rm + owned EntryView DTO
niutero-cli     thin arg-parse + output shell over niutero-engine (binary: niutero-cli)
niutero-gui     egui thin client over niutero-engine (binary: niutero)
```

`niutero-engine` is the reusable surface that makes "CLI is the complete interface"
real: every capability is an engine function over an open `Vault`, the CLI only
parses args and formats output, and the Phase-2 GUI calls the engine directly
(not shell out). Add new capabilities to the engine first, then expose a thin CLI
command. Entries from untrusted input must pass `BibEntry::validate()` before being
written (the serializer assumes valid, brace-balanced values).

Data model: **a library is a folder** ("vault"). The folder holds `references.bib`
(portable, tool-agnostic) plus a `.niutero/` sidecar (`config.toml`, `meta.json`
keyed by citekey, `views.toml`). A machine-local registry tracks recent vaults and
personal prefs and is **not** synced with the vault. Organization is tags + named
saved filter views — there is no "collection" object.

Optional features (git sync, normalization, online enrich, import-by-DOI, LaTeX
tex-scan, LLM, PDF, browser connector) layer on later and must stay off the base
path: the core must work fully offline with all of them disabled.

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
