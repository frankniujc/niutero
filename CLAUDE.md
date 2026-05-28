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
   There is no GUI yet — it is Phase 2 and will be a thin client over this same
   surface, able to do nothing the CLI cannot. If a future feature will need an
   operation, add the CLI command first.
3. **`.bib` is the source of truth and stays niutero-agnostic.** Private data (tags,
   notes, saved views) lives in the hidden `.niutero/` sidecar, **never** in
   `references.bib`. A collaborator who doesn't use niutero must get a clean `.bib`.
4. **Deterministic, byte-stable serialization.** For an unchanged entry,
   `parse → serialize` must be byte-identical (stable field order, preserved
   `@string`/`@preamble`/`@comment`). This is the project's #1 technical risk — it is
   what keeps git diffs/merges clean — so it is locked first (M1) and guarded by
   round-trip property tests against a large real `.bib` fixture.

## Architecture

A Cargo workspace that keeps domain logic out of any UI. Planned crates (built in
milestone order — see `plan.md`):

```
niutero-core    domain model (BibEntry, Library, config/meta/view types, filter) — no IO/UI
niutero-bib     tolerant .bib parser + deterministic serializer   <- the foundation
niutero-vault   vault IO: .niutero/ sidecar + machine-local registry
niutero-cli     the complete interface — a thin wrapper over the crates above
```

Data model: **a library is a folder** ("vault"). The folder holds `references.bib`
(portable, tool-agnostic) plus a `.niutero/` sidecar (`config.toml`, `meta.json`
keyed by citekey, `views.toml`). A machine-local registry tracks recent vaults and
personal prefs and is **not** synced with the vault. Organization is tags + named
saved filter views — there is no "collection" object.

Optional features (git sync, normalization, online enrich, import-by-DOI, LaTeX
tex-scan, LLM, PDF, browser connector) layer on later and must stay off the base
path: the core must work fully offline with all of them disabled.

## Commands

The workspace is scaffolded in M1; these are the standing conventions once crates
exist. All `niutero-cli` subcommands take the vault folder as the first positional
arg and support `--json`; exit codes are `0` ok / `1` error / `2` actionable (CI gate).

```sh
cargo build --workspace
cargo test  --workspace
cargo test  -p niutero-bib                       # one crate
cargo test  -p niutero-bib roundtrip::byte_stable # one test (name filter)

cargo run   -p niutero-cli -- <subcommand> <vault> [options]   # binary: niutero

cargo fmt    --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Logging is via `RUST_LOG` (e.g. `RUST_LOG=niutero=debug cargo run -p niutero-cli -- ...`).

## Platform note

Developed on a Windows-on-ARM machine **without Visual Studio**, so a non-standard
linker setup (llvm-mingw + xwin) is required to build. The resulting
`.cargo/config.toml` is machine-specific and gitignored — the product itself stays
portable; this is only the dev-machine toolchain workaround.
