# GUI Button Audit — what's real, what's a placeholder

Audited the full interactive surface of `niutero-gui` (every button, clickable
icon, toggle, menu, clickable row) and cross-referenced each placeholder against
the `niutero-engine` public API to see whether the backing capability already
exists.

**Out of scope (per request):** the **Settings** tab and the **AI / LLM
assistant** tab. AI-adjacent controls elsewhere (the floating AI popup, "Auto-tag
with AI") are listed but flagged, not specced.

**Legend for engine support**
- ✅ **Engine ready** — the capability exists in `niutero-engine`; the button
  just needs to be wired (+ usually a small dialog/flow).
- ⚠️ **Partial** — an adjacent engine function exists but not exactly what the
  button implies, or a product decision is needed.
- ❌ **No engine** — no backing function; engine work required first.
- 🎨 **GUI-only** — purely presentational; no engine needed.

---

## 1. Library — Classic view

### Toolbar (above the list)
| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| **+** (New item) | **Placeholder** — response discarded (`let _ = icon_btn(...)`, `mod.rs:479`) | Add-entry dialog (type + fields, or paste BibTeX) | ✅ `add(AddSource::Fields{..})` / `add(AddSource::Bibtex(..))` |
| **🔗** (Add by ID/DOI) | **Placeholder** — discarded (`mod.rs:480`) | DOI input → fetch & insert; also "import .bib file" | ✅ `import_doi(doi, policy)`, `import(file, policy)` (online/offline) |
| Search box | **Real** — live filter (`mod.rs:495`) | — | — |
| ◧ / ◨ collapse panels | **Real** (cosmetic toggle of `hide_tags`/`hide_detail`) | — | 🎨 |

### Column header sort (TITLE · CREATOR · YEAR)
- **Real** — cycles asc→desc→off (`mod.rs:678`, `SortState::click`). ✅ done.

### List rows
- **Real** — row click selects (`mod.rs:571`). Type glyph & PDF clip are display-only (correct).

### Detail panel (shared by Classic + Board drawer)
| Control | Current behavior | Engine |
|---|---|---|
| Lock / Unlock | **Real** — toggles edit mode (`mod.rs:858`) | 🎨 |
| Title / Author / Publication / Year / DOI / Abstract edits | **Real** — queue `LibAction::Edit` on focus-loss → `engine::edit` | ✅ wired |
| Add tag / remove tag (× on chip) | **Real** — `LibAction::AddTag`/`RemoveTag` → `engine::set_tags` | ✅ wired |
| **Cite** | **Real** — `engine::cite` → clipboard | ✅ wired |
| **BibTeX** | **Real** — `engine::entry_bibtex` → clipboard | ✅ wired |
| Open link | **Real** — `LibAction::OpenUrl` | ✅ wired |
| Status (Unread/Reading/Done) + Stars | **Real**, but only rendered in the **Board drawer** (Classic detail intentionally omits the Reading row) → `engine::set_status` / `set_stars` | ✅ wired |

> Detail panel is essentially fully wired — the gaps are all in the toolbars and the tag sidebar.

---

## 2. Library — Tags sidebar (Classic + Reader)

| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| **All Entries** row | **Real** — clears tag filter | — | — |
| Tag rows / namespace group collapse | **Real** (filter / cosmetic collapse) | — | 🎨 |
| **✦** Auto-tag (TAGS header) | **Placeholder** — discarded (`mod.rs:323`) | Suggest tags for selected/all entries | ✅ `suggest_tags(citekey)` — *offline heuristic, not blocked by the LLM tab* |
| **+** New tag (TAGS header) | **Placeholder** — discarded (`mod.rs:322`) | Product decision: tags are created by tagging an entry; there's no standalone "tag" object | ⚠️ via `set_tags` (no dedicated "create tag") |

---

## 3. Library — Reader view

| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| Reading-status filter (Unread/Reading/Read) | **Real** — sets `reading_filter` | — | — |
| Card click | **Real** — selects | — | — |
| Search | **Real** | — | — |
| **Filter** icon (card-list header) | **Placeholder** — discarded (`reader.rs:277`) | Either a local facet filter (GUI) or surface saved views | ⚠️ `views` / `add_view` / `remove_view` exist if it means saved views; otherwise 🎨 |
| ⋯ menu (copy key / cite / BibTeX) | **Real** | — | ✅ wired |
| Star (favorite) | **Real** — `set_stars` | — | ✅ wired |
| Open PDF / Cite / Copy BibTeX / Source | **Real** | — | ✅ wired |
| Lock / hide tags / hide list | **Real** (cosmetic) | — | 🎨 |

---

## 4. Library — Board view

| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| Card click | **Real** — selects + opens drawer | — | — |
| Search | **Real** | — | — |
| Drawer header (close, lock) + fields + footer (Cite/Link/BibTeX) + status/stars | **Real** — reuses shared detail | — | ✅ wired |
| **Add** (header) | **Placeholder** — "coming soon" hover (`board.rs:116`) | Add-entry dialog (shared with Classic **+**) | ✅ `add` |
| **+** Add paper (each column header) | **Placeholder** — "coming soon" (`board.rs:184`) | Add entry **pre-set to that column's status** | ✅ `add` + `set_status` |
| **+ Add paper** dashed box (column bottom) | **Placeholder** — "coming soon" (`board.rs:332`) | Same as above | ✅ `add` + `set_status` |
| Layout toggle (Rows / Grid) | **Placeholder/cosmetic** — Rows "coming soon"; Grid is decorative (`board.rs:126`) | Actual list/grid layout switch, or remove | 🎨 GUI-only |

> **Drag-and-drop between columns** (the natural kanban gesture) isn't implemented; dragging a card to another column would be `set_status`. ✅ engine ready.

---

## 5. Normalize tab

| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| Overview: Offline cleanup → **Run** | **Real** — navigates to Review (`NormAction::RunOffline`) | — | ✅ (apply happens in Review) |
| Overview: Health row **Fix** / **View** | **Real** — navigate / highlight | — | — |
| Overview: Online enrich → **Run** | **Real navigation, but the task is SIMULATED** — a fake 6s timer toast, no network (`app.rs:861`) | Replace the fake timer with real per-entry enrich (loop over filter) + a real background task | ✅ `enrich(citekey)` (online; per-entry — bulk = loop) |
| Review: **Apply all** | **Real** — `engine::normalize_apply` (or per-entry `edit` when some rejected) | — | ✅ wired |
| Review: Accept / Reject / Reject all / Copy as patch | **Real** — staging + clipboard | — | ✅ wired |
| Review: **Back to overview** | **Real** — nav | — | — |
| Ruleset: 6 rule **toggles** | **Placeholder** — flips in-memory `st.rules[i]` only; **not persisted, runs ignore it** (`normalize.rs:614`, doc note) | Read/write the vault's normalize ruleset (`.niutero/norm.toml` profile) and pass it to preview/apply | ❌ **no engine fn** to read/write the ruleset profile (preview/apply only take a *profile name*) |
| Re-key: **Apply re-key** | **Real** — `engine::rekey_apply` | — | ✅ wired |
| Re-key: **Preview all {n}** | **Placeholder** — click discarded (`normalize.rs:740`) | Run & show the full preview | ✅ `rekey_preview` (likely already computed for the cache — trivial wire) |

> Also engine-ready but **not surfaced** in Normalize: **dedupe** (`dedupe_preview` / `dedupe_merge`). The analyze/health view reports issues but offers no "merge duplicates" action.

---

## 6. App shell — Tool rail, titlebar, status bar, overlays

| Button | Current behavior | Needs | Engine |
|---|---|---|---|
| Titlebar min / max / close | **Real** — viewport commands | — | 🎨 |
| Theme toggle (Sun/Moon) | **Real** | — | 🎨 |
| Library switcher menu (recent / open / new / **× unbind**) | **Real** — `engine::open`/`init`/`forget_vault` | — | ✅ wired |
| View switcher (Classic/Reader/Board) | **Real** | — | 🎨 |
| Rail: Library / Normalize / AI / Settings | **Real** — tab nav | — | — |
| Rail: **Sync** (bottom) | **Placeholder** — discarded, "commit & push — later wave" (`app.rs:683`) | Commit + push, with progress + conflict feedback | ✅ `sync(message)` (also `sync_prefs`, `connect`) |
| Status bar (path, branch, dirty, count) | **Real**, read-only | — | ✅ (`git_status`) |
| AI FAB / popup close / expand / citation chip / "Show in list" / task "Review changes"/"Dismiss" | **Real** (nav/state) | — | — |
| AI popup **Send** / **Tag both** | **Placeholder** — "no model connected" / "needs a connected model" toasts | *(AI tab — out of scope)* | — |

---

## 7. Engine capabilities that exist but have NO button yet

These are wired-up-able wins — the engine function is there, nothing in the GUI calls it:

| Capability | Engine fn | Natural home |
|---|---|---|
| Attach a PDF to an entry | ✅ `attach_pdf(citekey, src)` | Detail panel (only **Open** PDF is wired today) |
| Fetch a PDF online | ✅ `fetch_pdf(citekey)` | Detail panel |
| Per-entry online enrich | ✅ `enrich(citekey)` | Detail panel ⋯ menu |
| Notes (private sidecar) | ✅ `current_note` / `set_note` | Detail panel — **no notes field exists in the UI** |
| Delete an entry | ✅ `rm(citekey)` | Detail panel / row context menu — **no delete button anywhere** |
| Merge duplicates | ✅ `dedupe_preview` / `dedupe_merge` | Normalize |
| Saved views (filters) | ✅ `views` / `add_view` / `remove_view` | Tags sidebar / Reader filter |
| Export (whole / by keys / by filter) | ✅ `export` / `export_keys` | A menu or Settings — **no export UI** |
| Keep-updated export targets | ✅ `export_targets` / `_add` / `_remove` / `refresh_exports` | Settings (out of scope) |
| LaTeX `\cite` scan | ✅ `tex_scan(tex_files)` | A "used in my paper" feature — no UI |
| Entry history (git) | ✅ `history(citekey)` | Detail panel — no history view |
| Capture (browser/paste BibTeX) | ✅ `capture(bibtex)` | Add-entry flow |

---

## Summary

**Headline:** the **detail panel and the data-editing path are fully real** —
edits, tags, status, stars, cite, BibTeX, open-link/PDF all hit the engine. The
fake buttons cluster in three places: **creating/importing entries**, **the
Normalize ruleset**, and **sync**.

### A. Placeholders that are *engine-ready* — pure wiring (do these first)
1. **New entry** (+ in Classic toolbar, Add in Board header, + per Board column) → `add` (+ `set_status` for the column) — **one shared add-entry dialog covers 4 buttons.**
2. **Add by DOI / import .bib** (🔗 in Classic toolbar) → `import_doi` / `import`.
3. **Re-key "Preview all"** → `rekey_preview` — trivial.
4. **Rail Sync** → `sync` — wire + progress/conflict UI.
5. **Auto-tag (✦)** → `suggest_tags` — offline, *not* blocked by the AI tab.
6. **Online enrich "Run"** → replace the simulated timer with a real `enrich` loop.
7. **Board card drag-between-columns** → `set_status` (new gesture, engine ready).

### B. Placeholders that need *new engine work* first
1. **Normalize ruleset toggles** → need an engine fn to read/write the vault's
   normalize profile (`.niutero/norm.toml`); today preview/apply only accept a
   profile *name*. ❌

### C. Placeholders that are *GUI-only* (no engine; build or drop)
1. **Board layout toggle (Rows/Grid)** — decide whether to implement the grid
   layout or remove the control.
2. **Reader "Filter" icon** — local facet filter (GUI), *unless* it should open
   saved views (then it's engine-ready via `views`).
3. **TAGS header "+ New tag"** — needs a product decision (tags have no standalone
   object; they're created by tagging an entry).

### D. Engine features with *no button at all* (coverage gaps)
**Delete entry** (`rm`), **Notes** (`set_note`), **Attach/Fetch PDF**
(`attach_pdf`/`fetch_pdf`), **per-entry Enrich**, **Dedupe**
(`dedupe_*`), **Saved views**, **Export** (`export`/`export_keys`), **tex-scan**,
**entry history**. All have engine support; none are surfaced. The most
conspicuous omissions are **Delete** and **Notes** in the detail panel.

*(Settings tab and AI/LLM assistant tab excluded by request.)*
