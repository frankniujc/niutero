# Handoff — niutero_2

A working handoff for whoever (human or Claude) picks this up next. For the
*spec* read `plan.md`; for the durable rules/architecture read `CLAUDE.md`.
This file is the "where are we right now" layer on top of those.

## What this is

A ground-up, **CLI-first** rewrite of niutero, a lightweight LaTeX-oriented
citation manager in Rust. Phase 1 = a complete, tested CLI for all base +
optional functionality — **done**. Phase 2 (a GUI as a thin client over
`niutero-engine`) was originally descoped, then un-descoped: **`niutero-gui`
exists** (egui) and calls the engine directly — it does nothing the CLI cannot.

Hard rule: **written from scratch.** The old project at `../niutero` is a
*product* reference only (features, data model, invariants) — never an
implementation source. Exception the user authorized: `../bib_fixer/fix_bib.py`
is the explicit spec that W2's offline normalizer was ported from.

## Current state (2026-06-10)

- **Phase 1 is COMPLETE** — base scope (M1–M5) plus every optional feature, all
  built, tested offline, reviewed, and committed. `niutero-engine` is the
  operations layer; the CLI is a thin shell over it. (External features' live
  network paths are code-complete but unverified in this offline sandbox — see
  "Remaining work".)
- **Phase 2 GUI EXISTS** — `niutero-gui` (egui, custom titlebar/theme), a thin
  client calling `niutero-engine` directly (never shelling out). Tool rail:
  **Library** (Classic / Reader views — Board temporarily removed, see the
  2026-06-10 (later) section; tags sidebar, full detail panel
  with edit/tags/status/stars/cite/BibTeX/delete), **Tags** tool (vocabulary
  table, rename/merge/delete with confirm dialogs, Import + Organize + Auto-tag
  wizards), **Normalize** (offline preview/apply, health report, re-key),
  **AI** (chat grounded in the library, live when configured), **Settings**
  (every page persists to its proper home — vault config vs machine registry;
  still visual-only: font pickers, density, the search box, Keymap/Integrations
  stubs). The G1–G5 tab surface landed 2026-05-30; see
  `gui-button-audit.md` for the real-vs-mock map.
- **Phase 1 wave history**, in order:
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

- **Git**: don't trust this file for branch/push state — check `git status` and
  `git log origin/main..main` yourself (this doc has gone stale on commit counts
  before). The user decides when to push.
  Remote: `git@github.com:frankniujc/niutero.git` (repo renamed from
  `niutero_2` on 2026-06-10; the local folder is still `niutero_2/`).
- **Tests**: full `cargo test --workspace` was green at W6 (**269 tests**) and
  has grown since; fmt + clippy (`-D warnings`) clean at each landing. Re-run
  the full gate before the next commit.
  Norm has an optional whole-library idempotence test: run with
  `NIUTERO_BIB_FIXTURE=/path/to/library.bib cargo test -p niutero-norm`.

## 2026-06-10 landing — AI/tags surface, security hardening, GUI reliability

One large landing (committed as one release). What changed, by crate:

**niutero-cli** (`crates/niutero-cli/src/main.rs`)
- New `tags <vault> list|rename|merge|delete [--json]` — whole-library tag
  vocabulary ops, sidecar-only.
- New `ai` group: `ai config [--enable] [--provider] [--key|--key-stdin]
  [--model] [--base-url] [--json]` (`--key-stdin` keeps the key out of argv;
  short keys never echoed; config RMW now inside the registry lock via
  `engine::update_ai_config`), `ai test [--json]`, `ai ask <vault> Q [--json]`
  (terminal output sanitized of control chars), and **`ai organize <vault>
  [--instructions S | --plan FILE] [--apply] [--json]`** — closes the
  invariant-3 gap for the GUI's Organize wizard. Plan-only prints the model's
  merge plan; `--json` output round-trips as `--plan` input (that path is fully
  OFFLINE); `--apply` runs the merges via `engine::apply_tag_merges` with
  per-merge results. New-tag suggestions are always advisory.
- `connector` now prints a per-session token; `POST /capture` requires it
  (`Authorization: Bearer` or `X-Niutero-Token`); wildcard CORS removed; 512 KB
  body cap; socket timeouts.
- `tags rename/merge/delete` and `ai organize --apply` refresh keep-updated
  export targets (mutated_vault arms). `suggest-tags` help now says it needs
  LLM assist enabled (`ai config --enable true`), not just `$ANTHROPIC_API_KEY`.

**niutero-engine**
- `pub DEFAULT_MODEL = "claude-haiku-4-5"` (the old id had an invalid date
  suffix; the GUI now seeds from the same constant).
- `resolve_ai` hard-errors on an unwired provider (non-anthropic) or a
  non-empty `base_url`, so keys/data can't be silently misrouted; stored key
  trimmed; error messages tool-neutral (mention both the CLI command and
  Settings).
- New: `update_ai_config` (locked RMW), `apply_tag_merges` + `MergeApplied`,
  `set_tags_bulk` (one sidecar write for wizard-scale applies); `OrganizePlan`
  derives `Deserialize`. `ai_test` max_tokens 16→64 (thinking-model safe);
  `grounding_context` budget is now a hard pre-append check.

**niutero-online**
- Fixed the Windows-fatal curl bug: the `-K` config's `data = "@path"` line now
  uses forward slashes (curl unescapes backslashes in double-quoted config
  values, e.g. `\T`→TAB — every AI call failed on Windows).
- The curl config (the only thing holding the API key) now goes to curl via
  stdin (`-K -`) — the key never touches disk; the request body uses a
  random-named tempfile (0600 on Unix), auto-deleted.
- LLM call switched `-fsSL`→`-sSL` + new `extract_text` surfaces the API's own
  `error.message` and flags token-limit truncation. Keys with
  quotes/backslashes/control chars are rejected (config-injection guard).

**niutero-vault**
- `vaults.toml` (can hold the AI key) is written owner-only (0600) on Unix via
  a private atomic-write variant.

**niutero-gui**
- Settings → AI: config persists reliably (per-frame dirty flush + flush on
  tool/library switch and app exit — navigating away used to silently drop a
  typed key); provider dropdown only allows Anthropic (others visible but
  disabled "not wired yet"); Base URL field hidden unless a legacy value needs
  clearing; model field seeded from `engine::DEFAULT_MODEL`; the PDF page's
  HF-token input disabled ("coming soon" — it stored nothing).
- Tags tool: Delete and Merge go through a confirm dialog with the affected
  entry count; detail rename commits only on Enter (Esc/click-away reverts);
  the Import wizard's project field no longer pre-fills "sae-survey"; color
  section labeled "(this session only)"; tag-model cache keyed on an explicit
  library reload generation, not pointer identity.
- Wizards: Apply paths use one bulk engine call (`set_tags_bulk` /
  `apply_tag_merges`) instead of per-entry sidecar rewrites on the UI thread;
  Done steps report REAL applied/skipped/failed counts fed back from the app;
  done-step copy no longer claims tags were "applied to references.bib" (they
  live in the `.niutero` sidecar — references.bib untouched).
- AI jobs: stamped with kind + vault root and a cancel flag; switching
  libraries cancels jobs, closes wizards, resets the chat (no cross-library
  result delivery); closing a wizard cancels its in-flight model call; results
  only land in a matching wizard kind; a dead worker no longer strands the
  spinner; "New chat" cancels the in-flight ask; the AI popup keeps the typed
  question if the ask is refused; the AI tab's fake Scope menu is now an honest
  static label.
- Library: the active tag filter follows renames/merges and clears on delete
  (views no longer silently empty); repeated identical toasts re-arm the
  auto-dismiss timer.

**Refactors landing alongside** (same release, other agents):
`niutero-engine/src/lib.rs` split into submodules (`ai.rs` extracted; flat API
preserved via re-exports); `niutero-gui` `tags.rs` split into
`tags/{mod,wizards}.rs`; the `app.rs` impl split into an `app/` directory;
duplicated private widgets unified into `widgets.rs`.

## 2026-06-10 (later) — Board view removed; PDF attachments built for real

- **Board view temporarily removed** (user request). `LibView::Board`, the
  titlebar switcher entry, `NIU_VIEW=board`, and `library/board.rs` are gone —
  restore the file from git history when it returns. The status/stars
  machinery it used stays live (Reader filter, detail panels, `status:`
  queries).
- **PDF attachments are now a real feature** (engine → CLI → GUI, per the
  architecture rule):
  - Engine `pdf_ops`: per-vault `PdfPrefs { repo, auto_fetch }` + per-machine
    `hf_token` in the registry (locked RMW; token never exposed back through
    the API); `create_pdf_repo` / `pdf_push` / `pdf_pull` over new
    `niutero-online` HF calls (create-repo, NDJSON commit upload, `resolve/`
    download — token via `-K -` stdin like the LLM path; download keeps
    `--fail` so an error page is never written as a "PDF"); pure
    `fetchable_pdf_url` (direct `.pdf` / arXiv abs → pdf; landing pages
    refused); `auto_fetch_pdfs` post-import hook (opt-in, best-effort, no
    partial files). `EntryView.has_pdf` is now a real on-disk check.
  - CLI: `pdf --push/--pull`, new `pdf-config` (see plan.md Post-M5);
    `import` runs the auto-fetch hook (stderr-only reporting);
    `ImportReport.added_keys`/`new_keys()` feed it.
  - GUI: context menu gains Attach / Fetch (only for fetchable urls) / Pull
    from HF; attach auto-pushes when HF is configured; Open PDF falls back to
    an HF pull; the Settings PDF page is fully real (repo + auto-fetch per
    vault, token per machine with the same navigate-away flush as the AI key,
    Create-repo runs live off-thread). The `has_pdf` clip/indicator no longer
    lies.
  - **HF calls are network-unverified** (built offline, same standing as the
    other externals): exercise create-repo → push → pull against a real repo
    + `hf_` token before trusting them. The create-repo payload sends the
    namespaced `name` — if the Hub rejects that form, split into
    `name`/`organization` (noted in `niutero-online::hf_create_dataset`).
  - Tests: +13 (online HF builders/base64/validation, engine prefs/gates/
    auto-fetch/`has_pdf`, CLI `pdf-config` round-trip + offline gates +
    opt-in auto-fetch via an RFC-2606 `.invalid` host).

## 2026-06-10 (third landing) — every setting persists to its proper home

User decisions: appearance → machine-local; workflow → the vault's own config.

- **Vault `config.toml` grew** (synced, collaborators share it): `pdf_repo`
  (moved out of the machine registry; legacy registry value still honored as
  a read fallback, cleared on the next set) and `[workflow]`
  (`enrich_on_import` / `auto_commit` / `on_dup` / `auto_fetch_pdf` — all
  default-off so the base path stays offline and nothing commits uninvited).
- **The workflow toggles are real behaviors now**, engine-first:
  `auto_commit_if_enabled` (commit-only, stats-aware message, fires after
  every successful CLI mutation incl. sidecar-only ones, and after every GUI
  mutation via `after_mutation`); `auto_enrich` post-import hook (DOI fill);
  `default_dup_policy` (import's default when no `--on-dup`); PDF auto-fetch
  moved onto the same config. GUI imports honor all of them (post-import
  hooks run off-thread as a task toast).
- **New CLI `config <vault>`** (get/set name / pattern / the workflow trio,
  `--json`); `sync-config` shows the `origin` remote read straight from the
  repo; `import --on-dup` became optional (flag > config > skip);
  `pdf-config --repo/--auto-fetch` write the vault config now.
- **Engine**: `set_library_meta` (validated name; pattern stored as given —
  `KeyPattern::parse` is infallible by design), `set_workflow`, `pdf_repo`
  resolution (config → legacy), `set_pdf_repo`, `remote_url`, `ui_prefs` /
  `set_ui_prefs`, `auto_commit_if_enabled`, `auto_enrich`.
- **Registry** gained `[ui]` (dark + accent — personal, never synced). The
  GUI loads it at boot and persists on every theme/accent change, so the app
  reopens looking the way it was left.
- **GUI Settings**: Library and Workflow pages are fully real (same
  navigate-away flush as the AI key); the Sync page's Git remote field seeds
  from the repo; the PDF page reads/writes the vault config; the
  "not persisted yet" note is gone because nothing needs it anymore. Still
  visual-only: font pickers, density, settings search, Keymap/Integrations.
- Tests: 336 passing (+9): vault config/UiPrefs round-trips, engine setter
  validation + pdf-repo precedence + auto-commit/auto-enrich gating +
  remote_url, CLI `config` black-box (incl. config.toml contents, on-dup
  honored by import, auto-commit end-to-end, remote display).

## 2026-06-10 (fourth, small) — author UX + a rename

- Tags toolbar "Import project" → **"Tag from LaTeX"** (it never imported
  anything; it tags what a manuscript cites). Wizard title/done copy updated.
- Unlocked detail (Classic + Reader): the author field edits as **one row per
  author** (hint `Last, First`, per-row ✕, "+ Add author"), committed back as
  the BibTeX `A and B and C` field only when it actually changed.
- New Appearance pref **Author names**: `Lastname, First` (as stored — the
  default) or `First Lastname` (flips the comma form, display-only; corporate
  / comma-less names pass through). Lives in the registry `[ui]` as
  `author_first_last`; `set_ui_prefs` now takes the whole `UiPrefs`.

## 2026-06-10 (fifth) — pre-push sweep: hardening, logging, cleanup

A full pre-push review found **8 confirmed correctness issues — all fixed**
before pushing:

- **HIGH GUI bug**: a focus-loss edit could write entry A's field into entry B
  when the click that blurred the field also moved the selection.
  `LibAction::Edit` now carries its citekey, and unchanged buffers no longer
  commit.
- Disabling PDF auto-fetch now also clears the legacy registry toggle — it
  previously could never be turned off on a machine that still had the
  registry-era pref set.
- Connector captures run the same post-import hooks as every other import
  (PDF auto-fetch, enrich-on-import, auto-commit).
- GUI background jobs only auto-commit when a library-mutating job actually
  succeeded.
- GUI settings writes go through shared persist helpers: config-page saves now
  auto-commit like the CLI's, failed saves reseed the page instead of pretending
  success, and the AI config saves only changed fields under the registry lock
  (a concurrent CLI edit survives).
- `ai organize --apply` exits 1 when any merge failed; `enrich` / `pdf` /
  `suggest-tags` gained `--json`.
- Logging landed across engine/online/vault behind `RUST_LOG` (the CLI now
  initializes `env_logger`; secrets are never logged), plus assorted
  stale-doc / dead-code cleanup.

## 2026-06-22 — browser extension (connector client) built

The browser connector finally has its client: a **Manifest V3 Chrome extension
in `extension/`** (plain JS, not a workspace crate). It captures the citation on
the active tab and POSTs it to the local connector. It is a thin client — it
only talks to the loopback endpoints, so it can do nothing the connector can't.

- **Architecture decision: the extension talks ONLY to `127.0.0.1`.** DOIs
  resolve server-side: the extension extracts a DOI (publisher meta, JSON-LD,
  arXiv→DataCite) and POSTs the bare DOI to the **new `POST /capture/doi`**
  route, which calls the engine's tested `import_doi` (doi.org). Non-DOI pages
  build BibTeX from Highwire `citation_*` / Dublin Core meta tags locally and
  POST it to `/capture`. This keeps `host_permissions` to loopback only and
  reuses the engine's canonical-BibTeX path. Page access is `activeTab` +
  `chrome.scripting.executeScript` (no broad content script).
- **Engine change** (`crates/niutero-engine/src/connector.rs`): added
  `/capture/doi`; refactored the shared post-capture tail into `finish_capture`
  (runs the keep-updated refresh + opt-in hooks only when `added > 0`) and the
  401 into `unauthorized()`. 2 new offline-safe tests (401, empty-DOI 400) —
  connector suite now 9. The DOI happy-path needs the network, so it's covered
  by the existing `import_doi` path, not a connector unit test.
- **Extension layout**: `manifest.json`, `background.js` (service worker),
  `popup.*`, `options.*`, `lib/{extract,bibtex,connector,config,capture}.js`,
  generated `icons/*.png` (pure-stdlib `gen-icons.py`), `README.md`. The pure
  BibTeX builder has `node --test` coverage (`test/bibtex.test.mjs`, 7 tests).
- **Reviewed by a 5-dimension adversarial workflow; all 5 confirmed findings
  fixed**: (HIGH) authors were doubled when a page emitted both Highwire and
  Dublin Core author tags → pick one source by precedence + dedup; digit-leading
  cite keys → `ref`-prefix guard; JSON-LD `@graph`/array-`sameAs`/`identifier`
  DOIs now found; `hasMeta` now accepts conference/booktitle pages; SICI DOIs no
  longer truncated at `<`. Also removed the unused `localhost` host permission.
- **Install**: `chrome://extensions` → Developer mode → Load unpacked →
  `extension/`. Start `niutero-cli connector <vault>`, paste the printed token
  into the extension's Options. **Not yet verified in a live browser** (no Chrome
  load on this dev machine) — the logic is unit-tested and the connector route
  is integration-tested, but an end-to-end capture from a real page is untested.

### Follow-ups (next work, in rough priority)

- **Live AI smoke** + live verification of the DOI / enrich / connector / PDF
  network paths — now including the HF create-repo / push / pull trio and a
  **real-browser end-to-end test of the extension** — still manually-verified-only.
- **Normalize ruleset engine API** — the GUI's ruleset toggles are still
  display-only; the engine has no ruleset read/write.
- **Vault-config setters** — library name / citekey pattern (Settings fields
  are visual-only today).
- **Tag-color persistence** — engine + CLI first (colors are session-local).
- **Multi-provider AI** (only Anthropic is wired; others refuse to run).
- **CI / packaging** — no `.github` yet (developed on Windows-on-ARM only).

## Remaining work

Phase 1 is complete; Phase 2's GUI tab surface (G1–G5) is built. What's left:

- **External features are network-UNVERIFIED.** DOI import, enrich, connector,
  PDF fetch, and the LLM features are code-complete with their pure logic
  tested, but no live HTTP/API call could run in this offline sandbox. Before
  trusting them, exercise each against the real services (a real DOI, an entry
  with a fillable DOI, the loopback `POST /capture` with the session token, a
  PDF url, and a configured `ai test`/`ai ask`) and add network-path coverage.
- **GUI polish** — see the follow-ups list above; also still GUI-less engine
  features: notes, history, dedupe-merge, saved views, export/export-targets,
  per-entry enrich. Other known-mock GUI bits: workflow toggles + fonts/density
  visual-only, keymap/integrations stubs. (PDF attach/fetch/pull and the PDF
  settings page are real as of 2026-06-10; the Board view is temporarily
  removed rather than half-mock.)
- **Cross-platform CI matrix** — not set up.

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
niutero-online  online helpers (DOI fetch, enrich, LLM via system `curl`)
niutero-engine  operations layer — EVERY capability is a fn here over an open Vault
niutero-cli     thin clap arg-parse + output shell (binary: `niutero`)
niutero-gui     Phase-2 egui thin client — calls niutero-engine directly
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
- **`curl -K` unescapes backslashes in double-quoted config values** — a
  Windows path in `data = "@C:\Temp\body.json"` turns `\T` into a TAB and the
  request never leaves the machine (this silently broke every AI call on
  Windows). Forward-slash any path that goes into a curl config (`curl`
  accepts `/` on Windows). Discovered fixing the AI calls; the config now goes
  to curl via stdin (`-K -`) anyway so the key never touches disk.
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
