# import — `niutero import`, the browser connector

**Covers:** merging an external `.bib` (or a DOI fetch, or a browser capture)
into the library: duplicate policy, the post-import hook pipeline, and the
connector's resolve → dedupe → clean flow.

## Bulk import

`cmd_import` → `engine::import(v, file, policy)` / `engine::import_doi` →
`merge_incoming` under the vault lock. `DupPolicy`: `skip` (records the
existing key in `ImportReport.skipped_keys`), `overwrite` in place, or
`rename` to a fresh `key-2`-style key (`unique_key`). All-or-nothing: any
invalid entry aborts before the single write; existing entries and verbatim
blocks are preserved.

**Hooks** — after every import, one shared engine pipeline runs:
`run_import_hooks(v, &report.touched_keys(), force_normalize)` = enrich →
normalize → PDF fetch, over **every touched key** (added + renamed +
overwritten — an overwrite is re-cleaned exactly like a fresh add). All
opt-in (`enrich_on_import` / `normalize_on_import` / `auto_fetch_pdf`) and
best-effort: failures become warnings, never a failed import. `engine::add`
(paste-BibTeX, the GUI New-Entry form) runs `auto_normalize` too — every entry
surface honors `normalize_on_import`.

## The browser connector pipeline

`POST /import {identifier?, metadata?, tags?}` → `connector_import`
(`niutero-engine/src/connector.rs`):

1. **Resolve** (`resolve_entries`, network, no lock): an OpenReview id fetches
   the venue's canonical BibTeX (OpenReview-ONLY — never doi.org); a DOI /
   arXiv id resolves via doi.org content negotiation; otherwise
   `build_entry_from_metadata` uses the page's scraped tags. **Fallback rule:**
   when an identifier fails to resolve (a 5xx, a venue without BibTeX, an
   unreadable OpenReview id) but the page metadata is usable, the capture
   still lands from the metadata, and the outcome carries
   `fallback: <reason>` so the popup says "(saved from page metadata)". The
   identifier that failed to resolve is still the paper's identity, so it is
   attached to the fallback entry (`attach_identifier`: a DOI as `doi`, an
   arXiv id as `eprint` + `archiveprefix`) — `enrich` and the arXiv pass can
   finish the job later. A venue-typed page whose venue tag was blank builds a
   `@misc` (never a hollow `@inproceedings` without a booktitle).
2. **Re-key** — `rekey_to_base_pattern`: the library's base cite-key pattern,
   no suffix, so a re-capture of the same source renders the same key.
3. **Content-identity triage** (under the lock): a base-key collision is only
   a *duplicate* when `dedup::same_work` says so (DOI ∥ URL ∥ normalized-title
   equality). A **different paper** with a colliding key gets a letter suffix
   (`next_free_key`) and is added — never silently dropped. A same-work
   collision under `rename` is suffixed here too, so connector adds use the
   letter style (bulk import keeps `-2`).
4. **Merge** — `merge_incoming` under the library's `on_dup` policy.
5. **Tags** — applied to touched ∪ `skipped_keys`: a skipped duplicate still
   gets the popup's tags (that capture was the user's tagging gesture);
   `tags_updated` is reported.
6. **Hooks** — `run_import_hooks(…, force_normalize = true)`: a capture is
   ALWAYS normalized, independent of `normalize_on_import`.
7. **Refresh** — keep-updated exports + auto-commit when the `.bib` changed;
   the host callback `on_import(&ImportOutcome)` lets the GUI toast accurately
   and reload only when something changed.

The response (`outcome_body`) carries `citekey`, a **display** `title` (brace
protection stripped; the stored entry keeps `{{…}}`), `added` / `overwritten` /
`skipped` / `renamed`, `tags_updated`, and `fallback` when set — all
`undefined`-tolerated by the popup for cross-version compatibility.

**Server shape**: the accept loop only accepts; every connection is served on
its own thread (`serve_loop`), so a capture mid-fetch blocks neither the next
capture nor `ServerHandle::stop` (which joins only the accept loop — toggling
the connector off in the GUI never waits for a curl timeout). Concurrent
captures serialize on the vault lock, which `engine::lock_vault` now waits up
to ~2 s for instead of failing at once — a GUI edit landing at the same instant
as a capture is a short pause, not a lost capture. PDF auto-fetch understands
OpenReview forum/pdf urls (`fetchable_pdf_url` → `openreview.net/pdf?id=`).

## Output & exit codes (CLI import)

`Imported: N added[, N overwritten][, N renamed][, N skipped]` + hook lines on
stderr. `--json` emits the `ImportReport` (with `skipped_keys` when non-empty).
Exit `0` ok · `1` error.

## Tests

Engine `connector.rs` inline: security predicates (`Origin: null` refused,
oversized headers 413), identifier parsing, the OpenReview-never-doi
invariant, `different_paper_with_colliding_key_is_added_with_letter_suffix`,
`rename_policy_reports_renamed_with_letter_suffix`,
`skip_recapture_still_applies_tags`,
`resolve_failure_with_usable_metadata_falls_back`,
`metadata_fallback_keeps_the_failed_identifier_on_the_entry`,
`venue_typed_page_without_a_venue_becomes_misc`,
`requests_are_served_concurrently`,
`connector_always_normalizes_even_with_the_toggle_off`,
`connector_overwrite_recapture_still_normalizes_and_retags`.
`niutero-core/src/dedup.rs`: the `same_work` suite.
CLI `tests/exchange.rs`: policy matrix + `import_overwrite_runs_hooks_on_
overwritten_keys`; `tests/mutate.rs::add_normalizes_when_normalize_on_import_
is_on`. Engine: `run_import_hooks_covers_touched_not_just_new`.

## Deferred / gotchas

- No JS test harness for `extension/` (scrape/popup verified via the live
  capture matrix).
- The two suffix styles are deliberate: bulk import `key-2` (pinned by
  `exchange.rs::import_rename_keeps_both`), connector `keya` (matches `add`'s
  collision style).
