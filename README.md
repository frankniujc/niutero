# niutero

A lightweight, LaTeX-oriented **citation manager** with a CLI-first design. A
library is just a folder: a portable `references.bib` (the source of truth) plus
a hidden `.niutero/` sidecar for private data (tags, notes, reading status,
saved views, config). Hand the `.bib` to any tool — a collaborator who doesn't
use niutero still gets a clean, tool-agnostic bibliography.

> Status: Phase 1 (the CLI) is **complete**. Phase 2, `niutero-gui` (egui), is
> **built and evolving** — a thin client over the same engine. See `plan.md` /
> `handoff.md`.

## What it does

- **Deterministic, byte-stable `.bib`** — `parse → serialize` is byte-identical
  for an unchanged entry (stable field order; `@string`/`@preamble`/`@comment`
  preserved), so git diffs and merges stay clean.
- **Organize** without touching the `.bib`: tags (incl. `topics:`/`wf:`
  namespaces), notes, reading `status` (unread/reading/done), `stars` (0–5), and
  saved filter views — all in `.niutero/`.
- **Find**: `list`/`export` with a filter query (`tag:foo status:reading stars:>=4`,
  plus free-text), and `tex-scan` to report used / missing / unused cite keys.
- **Normalize** (offline, propose-only): drop noise fields, protect title caps,
  clip long author lists, and a default **AI/ML venue-canonicalization** ruleset
  that collapses every messy spelling of a conference/journal to one canonical
  name. Configurable via `.niutero/norm.toml` (with named `--profile`s).
- **Citation keys**: a `{auth}{year}{title.N}` pattern auto-keys new entries and
  re-keys the whole library on demand (with collision suffixes).
- **Git sync**: `connect` + `sync` (commit → pull → push) with a structured
  3-way merge that auto-resolves entry/field-level conflicts in `references.bib`.
- **Maintain**: per-entry git `history`, an offline `analyze` health report, and
  duplicate detection + merge (`dedupe`).
- **Capture from the browser**: `connector` runs a token-gated loopback server,
  and a Manifest V3 Chrome extension (`extension/`) grabs the citation on the
  page you're viewing — DOI pages resolve to canonical BibTeX server-side, others
  build from the page's `citation_*` meta tags. Talks only to `127.0.0.1`.

## Quick start

```sh
cargo run -p niutero-cli -- init mylib
cargo run -p niutero-cli -- add mylib --type article \
  --field "author=Vaswani, Ashish" --field "year=2017" \
  --field "title=Attention Is All You Need"            # cite key auto-generated
cargo run -p niutero-cli -- list mylib --query "tag:nlp status:reading"
cargo run -p niutero-cli -- normalize mylib            # preview; --write to apply
cargo run -p niutero-cli -- connect mylib git@github.com:you/mylib.git
cargo run -p niutero-cli -- sync mylib
```

Every subcommand takes the vault folder as its first argument and supports
`--json` (exceptions: `connector`, `cite`, `init`, `connect`, `forget`, `sync`).
Exit codes: `0` ok · `1` error · `2` actionable (a CI gate — e.g. `tex-scan`
undefined refs, `normalize --check`, an unresolvable `sync` conflict).
The CLI binary is named `niutero-cli`; the GUI binary is `niutero`.

## Vault layout

```text
mylib/
├── references.bib       # portable source of truth — never carries private data
└── .niutero/
    ├── config.toml      # library name, schema version, citekey pattern
    ├── meta.json        # per-citekey tags / notes / status / stars
    ├── norm.toml         # normalization config (+ profiles)
    └── views.toml       # named saved filter views
```

## Build & test

```sh
cargo build --workspace
cargo test  --workspace
cargo fmt   --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture

A Cargo workspace that keeps domain logic out of any UI; every capability is an
`niutero-engine` function over an open vault, and the CLI is a thin shell over it
(the GUI drives the exact same code).

```text
niutero-core    domain model: BibEntry + validate(), filter, citekey, merge, dedup, texscan — no IO
niutero-bib     tolerant .bib parser + deterministic serializer (the foundation)
niutero-vault   vault IO: .niutero/ sidecar, atomic writes, exclusive lock
niutero-sync    git sync by shelling out to system git (no libgit2, no credentials)
niutero-norm    offline, propose-only normalization
niutero-online  online helpers via system curl: DOI import/enrich, Anthropic LLM, HuggingFace PDFs
niutero-engine  operations layer — every capability lives here
niutero-cli     thin clap arg-parse + output shell (binary: niutero-cli)
niutero-gui     egui thin client over niutero-engine (binary: niutero)
```

See `CLAUDE.md` for the durable rules and `plan.md` for the full spec.

## License

MIT OR Apache-2.0.
