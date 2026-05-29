# Niutero — UI Design Specification

A desktop reference manager — a git-backed BibTeX library organizer, in the
spirit of Zotero but reimagined around an **Obsidian-style tag-first workflow**
and an automated **library normalization** engine. Clean, calm, lots of
whitespace; light + dark; green accent.

> This document describes the design intent, structure, visual system, and the
> function of **every control** in the prototype. The prototype lives in
> `Niutero.html`, assembled from the modules in `app/`.

---

## 1. Product concept

Niutero manages a researcher's `.bib` file as a **version-controlled, normalized,
searchable** library:

- **Git-backed** — the library is a `.bib` committed to a git remote; the status
  bar shows the branch and a "modified" flag.
- **Browser connector** — a local capture extension (status bar shows
  `127.0.0.1:23510`) pushes papers from the browser into the library.
- **Tag-first** — there are no Zotero-style collections. Organization is done
  with two-level namespaced tags (Obsidian style).
- **Normalize** — a first-class tool that keeps a large `.bib` (1,292 entries in
  the mock) consistent: canonical venues, title casing, arXiv→published
  promotion, citation-key regeneration.
- **AI assistant** — chat grounded in the user's own library, with inline
  citations and "librarian" actions (tagging, related-work drafting).

### Tools (left rail tabs)

| Tool | Purpose |
|------|---------|
| **Library** | Browse, search, read, and edit entries. Three layouts (see §4). |
| **Normalize** | Analyze and clean the `.bib`: plan, health report, diff review, ruleset, re-key. |
| **AI Assistant** | Chat across the library; cite and act on entries. |
| **Settings** | Library config, workflow, appearance, sync, integrations. |

---

## 2. Visual system

### Typography
- **UI / sans:** Hanken Grotesk (400–700). All chrome, labels, controls.
- **Reading / serif:** Newsreader. Paper **titles** and **abstracts** — gives a
  scholarly, literary feel and separates "content" from "chrome".
- **Mono:** JetBrains Mono. Citation keys, DOIs, identifiers, git/branch text.

### Color tokens (CSS custom properties on `.niu`)
Defined in `app/shared.jsx`. Light and dark are the same token names with
different values (`.niu[data-theme="dark"]`).

| Token | Role |
|-------|------|
| `--bg` | App background |
| `--surface` / `--surface-2` | Panels / insets, hover fills |
| `--text` / `--text-2` / `--muted` / `--faint` | Text hierarchy (4 steps) |
| `--border` / `--border-2` | Hairlines (strong / subtle) |
| `--accent` | Green — selection, links, primary actions (`#1F8A5B` light, `#3BB178` dark) |
| `--accent-tint` / `--accent-tint-2` | Accent washes for chips, active rails |
| `--sel` / `--sel-line` | List selection fill + 3px inset marker |
| `--shadow` / `--shadow-lg` | Elevation for cards / popovers |

**Status/semantic colors** (literal hex, theme-independent): amber `#B6792B`
(reading / modified / warnings), rose `#C2536B` (removed in diffs), plus per-type
and per-tag colors.

### Shape & spacing
- Radii: 6–8px (chips, buttons), 9–14px (cards, popovers), squircle logo tile.
- Buttons: 32px tall (`.niu-btn`), 32px icon buttons (`.niu-icbtn`).
- Density: "comfortable" — list rows 56px, generous padding.

### Logo
A stylistic serif **N** (Newsreader) reversed white on an accent **squircle
tile** (`NiuMark` in `shared.jsx`). The glyph is nudged down ~7% to optically
center (caps reserve descender space). Appears in the rail and titlebar; scales
down to a favicon.

---

## 3. App shell (`WindowChrome`, `ToolRail`, `StatusBar`)

```
┌───────────────────────────────────────────────── titlebar ─────────────┐
│ ● ● ●   N Niutero — BibVault     [Classic·Reader·Board]        ☾ theme  │
├──────┬──────────────────────────────────────────────────────────────────┤
│ rail │                                                                    │
│  N   │                       active tool body                            │
│ Lib  │                                                                    │
│ Nrm  │                                                                    │
│ AI   │                                                                    │
│ Set  │                                                                    │
│  ↻   │                                                                    │
├──────┴──────────────────────────────────────────────────────────────────┤
│ ● connector · 127.0.0.1:23510            ⎇ main · ● modified · 1,292 ent. │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Titlebar:** traffic-light dots (decorative), logo + "Niutero — {library}",
  a centered **view switcher** (Library only), and a **theme toggle**.
- **Tool rail:** logo, four tool buttons (active = accent tint + 3px marker),
  and a **Sync** button pinned to the bottom.
- **Status bar:** read-only — connector address (left), git branch + modified
  flag + entry count (right).

---

## 4. Library — three layouts

The titlebar switcher flips between three takes on the library. All share the
tag-first sidebar and the lock-guarded editable detail.

### A · Classic (`LibraryViewA`) — default
Four columns: **tags sidebar · item list · details panel**.
- **Sidebar:** "All Entries" (clear filter) → two-level **TagTree**
  (Topics / Workflow). A ✦ opens the AI auto-tag popover; "+" makes a new tag.
  When a tag is active a filter chip appears with an "✕" to clear.
- **List:** toolbar with **collapse-left**, new-item, add-by-identifier, search,
  **collapse-right**; sortable column header; 56px rows (type glyph, serif
  title, tag hashes, creator, year, PDF clip).
- **Details:** type badge + **lock toggle**; editable title, **authors**,
  publication, year, DOI, citation key, abstract, tags; footer **Cite + link**
  (row 1) and **BibTeX** (row 2).

### B · Reader (`LibraryViewB`)
Tags sidebar · card list · **dominant reading pane** (centered 720px column with
a PDF-page preview). Reader header carries **collapse-left**, **collapse-list**,
lock, star, and overflow. Both side panels collapse for a full-width read.

### C · Board (`LibraryViewC`)
A **kanban by reading status** (To Read / Reading / Read). Cards show venue,
title, creator, tags, and a star-rating dot row. Clicking a card opens a
**slide-in detail drawer** (same lock + fields + Cite/BibTeX/link footer).

---

## 5. Editing model — lock + inline fields

The detail panel is **directly editable but locked by default**.

- **`LockBtn`** (top-right of every detail panel/drawer) toggles
  locked ↔ editing. Label reads "Locked" / "Editing".
- **`EditField`** renders metadata as plain text when locked; when unlocked it
  becomes `contentEditable` with a subtle inset affordance (accent ring on focus).
- **`AuthorsField`** — because papers can have many authors, authors get
  special treatment (Zotero style):
  - *Locked:* one compact line, names joined with `·`.
  - *Editing:* one **row per author**, each labelled "Author" with separate
    **Last , First** fields, a hover **−** to remove, and a **+ Add author** row.

---

## 6. Normalize — the cleanup engine (`NormalizeTab`)

Sub-navigation: **Overview · Review changes · Ruleset · Re-key**.

- **Overview** — a *Recommended plan* (1. Offline cleanup → runs local rule
  passes; 2. Online enrich → fetches published versions as a **background task**)
  and a *Health* report: one row per issue class (inconsistent venues, missing
  URL/year, duplicates, arXiv mislabeled, odd titles) with a count and
  **View / Fix** actions.
- **Review changes** — the staging step after a cleanup run. A success banner
  explains nothing is written until applied; each entry shows a **field-level
  diff** (old struck through in rose → new in green) with per-entry
  **Accept/Reject** plus **Apply all / Reject all / Copy as patch**.
- **Ruleset** — the rules that drive cleanup (venue canonicalization, title
  casing, arXiv→published, required fields, duplicate detection, author format),
  each with an enable/disable toggle. Stored in `.niutero/rules.toml`.
- **Re-key** — regenerate citation keys from the library pattern
  (`{auth}{year}{title.1}{Title.2}`), with a before→after table and collision
  handling (`+a` suffix).

---

## 7. AI Assistant (`AITab`) + popup (`AIPopup`)

- **Full tab:** a chat thread grounded in the library. Answers embed **citation
  chips** (click to jump to the entry) and offer **librarian actions** (draft
  related-work, bulk-tag, copy citations). Context bar lets you set the **scope**
  and start a **new chat**; composer has suggestion chips and a send button.
- **Popup:** a floating **sparkle FAB** (bottom-right of every screen) expands
  into a compact chat with the same grounding, plus an **"open full tab"**
  affordance.

---

## 8. Background tasks (`TaskToast`)

Long-running jobs (e.g. Online enrich) surface as a **progress toast**
(bottom-left): spinner + label + live `n / total` count + percent bar. It can be
sent to run in the background; on completion it flips to a success state with
**Review changes** (jumps to Normalize) and **Dismiss**.

---

## 9. Settings (`SettingsTab`)

Sub-navigation: **Library · Workflow · Appearance · Keymap · Sync & sharing ·
Integrations**.

- **Library:** library name, default profile, **citation-key pattern** (token
  mini-language with `.N` indices and casing rules), read-only schema version.
- **Workflow:** enrich-on-import, auto-commit to git, duplicate-capture policy.
- **Appearance:** theme (live), accent swatches, density, UI + reading fonts.
- **Sync & sharing:** git remote, push-after-commit, connector status, share link.
- **Keymap / Integrations:** placeholders (Zotero/Obsidian/Overleaf — planned).

---

## 10. File map

| File | Responsibility |
|------|----------------|
| `Niutero.html` | Entry point; `FullApp` shell (tab + view + task state), mounts overlays, design canvas. |
| `app/data.js` | Mock library: entries, two-level tags (`topics:` / `wf:`), tag groups. |
| `app/shared.jsx` | Tokens, theming, icons, `WindowChrome`, `ToolRail`, `StatusBar`, `TagTree`, `EditField`, `AuthorsField`, `LockBtn`, `Segmented`, `SubNav`, `NiuMark`. |
| `app/DirectionA.jsx` | Classic library view + AI auto-tag popover. |
| `app/DirectionB.jsx` | Reader library view. |
| `app/DirectionC.jsx` | Board (kanban) library view + drawer. |
| `app/NormalizeTab.jsx` | Normalize tool (overview, review, ruleset, re-key). |
| `app/AITab.jsx` | AI Assistant full tab. |
| `app/SettingsTab.jsx` | Settings tool. |
| `app/Overlays.jsx` | AI popup FAB + background task toast. |

Each component file opens with a **BUTTON REFERENCE** comment block enumerating
every control and what it does — keep those in sync when adding controls.

---

## 11. Conventions for contributors

- **Colors** come from tokens, never hard-coded (except the few semantic
  status hexes). New colors should be derived in `oklch` from the accent.
- **Spacing** uses flex/grid + `gap`; avoid bare margins between siblings.
- **Editable fields** must go through `EditField` so the lock model holds.
- **New buttons** must get a `title` and an entry in the file's BUTTON REFERENCE.
- **Cross-file sharing:** components are attached to `window` (Babel scripts
  don't share scope) — export new shared pieces at the bottom of `shared.jsx`.

---

## 12. Interactable inventory (by page)

Every control the user can touch, grouped by screen. Type legend: **btn** =
button · **icon-btn** = icon button · **toggle** = on/off switch · **seg** =
segmented control · **text** = text input · **textarea** = multi-line input ·
**check** = checkbox · **swatch** = color swatch · **dropdown** = select (mock) ·
**edit** = inline contentEditable field · **row** = clickable list/card row ·
**chip** = clickable tag/suggestion.

### 12.0 Global chrome (all pages)

| Type | Element | Action |
|------|---------|--------|
| icon-btn | Theme toggle (titlebar) | Light ↔ dark |
| seg | View switcher (titlebar, Library only) | Classic / Reader / Board |
| icon-btn | Rail · Library | Go to Library tool |
| icon-btn | Rail · Normalize | Go to Normalize tool |
| icon-btn | Rail · AI Assistant | Go to AI tool |
| icon-btn | Rail · Settings | Go to Settings tool |
| icon-btn | Rail · Sync (bottom) | Commit & push library (mock) |
| icon-btn | AI FAB (bottom-right) | Open/close AI popup |
| — | 3 traffic-light dots | Decorative |
| — | Status bar | Read-only (connector, git, count) |

> A separate top-right **Light/Dark** pill exists outside the window (canvas
> harness only, not part of the app chrome).

### 12.1 Library — Classic (`LibraryViewA`)

**Tags sidebar**
| Type | Element | Action |
|------|---------|--------|
| row | "All Entries" | Clear the tag filter |
| icon-btn | ✦ Auto-tag with AI | Open AI auto-tag popover |
| icon-btn | + New tag | Create a tag (mock) |
| row | Topics / Workflow group header | Collapse/expand that namespace |
| row | Each tag (e.g. `mech-interp`) | Filter list by tag; click active to clear |
| icon-btn | Filter chip "✕" | Clear active tag filter (shown only when filtering) |

**List toolbar**
| Type | Element | Action |
|------|---------|--------|
| icon-btn | Collapse-left (panel) | Hide/show tags sidebar |
| icon-btn | + New item | Create a blank entry (mock) |
| icon-btn | Add by identifier (link) | Add via DOI / arXiv (mock) |
| text | Search box | Search all fields & tags |
| icon-btn | Collapse-right (panel) | Hide/show details panel |
| btn | "Creator ▾" column header | Sort by creator (mock) |
| row | Item row | Select entry → load in details |

**AI auto-tag popover** (`TagAIPopover`)
| Type | Element | Action |
|------|---------|--------|
| check | "Auto-apply on new imports" | Toggle option |
| btn | "Not now" | Close popover |
| btn | "Suggest tags" (primary) | Run scan → show suggestions |
| chip | Suggestion chips (result) | Suggested tag + count |
| btn | "Review each" | Review suggestions individually (mock) |
| btn | "Apply all" (primary) | Apply all suggestions (mock) |
| icon-btn | Close ✕ | Close popover |

**Details panel**
| Type | Element | Action |
|------|---------|--------|
| icon-btn | Lock toggle | Locked ↔ editing |
| edit | Title | Edit when unlocked |
| edit | Author rows (Last / First) | Edit when unlocked |
| icon-btn | "−" per author | Remove author (editing) |
| btn | "+ Add author" | Append author (editing) |
| edit | Publication, Year, DOI | Edit when unlocked |
| edit | Abstract | Edit when unlocked |
| btn | "Cite" (primary) | Copy formatted citation (mock) |
| icon-btn | Open link | Open source URL (mock) |
| btn | "BibTeX" | Copy raw BibTeX (mock) |

### 12.2 Library — Reader (`LibraryViewB`)

| Type | Element | Action |
|------|---------|--------|
| row | "All Entries" | (sidebar) library root |
| icon-btn | + New tag | Create a tag (mock) |
| row | Tag rows / group headers | Filter / collapse |
| row | Reading-status rows | Unread / Reading / Read (display+filter) |
| text | Search box (list) | Search |
| icon-btn | Filter | Filter list (mock) |
| row | Paper cards | Open paper in reader |
| icon-btn | Collapse-left | Hide/show tags sidebar |
| icon-btn | Collapse-list (rows) | Hide/show card list |
| icon-btn | Lock toggle | Locked ↔ editing |
| icon-btn | Star | Favourite (mock) |
| icon-btn | More (⋯) | Overflow menu (mock) |
| edit | Title / authors / venue / abstract / key / DOI | Edit when unlocked |
| btn | "Open PDF" (primary) | Open the PDF (mock) |
| btn | "Cite" / "Copy BibTeX" / "Source" | Cite / copy / open URL |
| btn | "+ add" (tags) | Attach a tag (mock) |

### 12.3 Library — Board (`LibraryViewC`)

| Type | Element | Action |
|------|---------|--------|
| text | Search box | Search & filter board |
| seg | Grid / rows toggle | Board vs list layout |
| btn | "Add" (primary) | Add a new entry |
| icon-btn | Column "+" | Add paper to that column (mock) |
| btn | "+ Add paper" (dashed) | Add paper to column (mock) |
| row | Cards | Open detail drawer |
| icon-btn | Lock toggle (drawer) | Locked ↔ editing |
| icon-btn | Close ✕ (drawer) | Close drawer |
| edit | Drawer fields | Edit when unlocked |
| btn | "Cite" / "BibTeX" | Cite / copy |
| icon-btn | Open link | Open URL |

### 12.4 Normalize (`NormalizeTab`)

| Type | Element | Action |
|------|---------|--------|
| row | SubNav: Overview / Review / Ruleset / Re-key | Switch sub-view |
| btn | "Run" — Offline cleanup | Run rules → jump to Review |
| btn | "Run" — Online enrich | Start background enrich task |
| row | Health issue rows | Select issue class |
| btn | "View" (health) | Inspect that issue class |
| btn | "Fix" (health, primary) | Jump to Review for that issue |
| btn | "Back to overview" | Return to Overview |
| btn | "Apply all" (primary) | Accept all staged changes |
| btn | "Reject all" | Discard all staged changes |
| btn | "Copy as patch" | Copy diff as patch (mock) |
| btn | Per-entry "Accept" / "Reject" | Accept/reject that entry's diff |
| toggle | Per-rule switch (Ruleset) | Enable/disable a rule |
| btn | "Preview all 1,292" (Re-key) | Preview new keys (mock) |
| btn | "Apply re-key" (primary) | Rewrite citation keys |

### 12.5 AI Assistant (`AITab`)

| Type | Element | Action |
|------|---------|--------|
| btn | "Scope: My Library ▾" | Choose AI reading scope (mock) |
| btn | "New chat" | Start a new chat |
| chip | Inline citation chips | Jump to entry |
| btn | "Draft related-work ¶" | Generate related-work (mock) |
| btn | "Tag these 3…" | Bulk-tag entries (mock) |
| btn | "Copy citations" | Copy citations (mock) |
| chip | Suggestion chips | Prefill a prompt |
| textarea | Composer input | Type a question |
| btn | Send (primary) | Submit prompt |

### 12.6 AI popup (`AIPopup`)

| Type | Element | Action |
|------|---------|--------|
| icon-btn | "Open full tab" (expand) | Switch to AI tab |
| icon-btn | Close ✕ | Close popup |
| btn | "Tag both" / "Show in list" | Answer actions (mock) |
| text | Popup input | Type a question |
| btn | Send (primary) | Submit prompt |

### 12.7 Background task toast (`TaskToast`)

| Type | Element | Action |
|------|---------|--------|
| btn | "Run in background" / close ✕ | Dismiss while running |
| btn | "Review changes" (primary) | On completion → Normalize |
| btn | "Dismiss" | On completion → close |

### 12.8 Settings (`SettingsTab`)

| Type | Element | Action |
|------|---------|--------|
| row | SubNav: Library / Workflow / Appearance / Keymap / Sync / Integrations | Switch section |
| text | Settings search | Filter settings (mock) |
| text | Library name | Rename library |
| dropdown | Default profile | Pick profile (mock) |
| text | Citation-key pattern | Edit key pattern |
| text | Git remote | Set remote URL |
| toggle | Enrich on import | Toggle |
| toggle | Auto-commit changes | Toggle |
| toggle | Push after commit | Toggle |
| seg | On duplicate capture | Ask / Merge / Skip |
| seg | Theme | Light / Dark (live) |
| swatch | Accent color | Pick accent (5 swatches) |
| seg | Density | Comfortable / Compact |
| dropdown | Interface font / Reading font | Pick font (mock) |
| btn | "Create share link" | Generate read-only link (mock) |

> "mock" marks controls that are visually complete but not yet wired to real
> behavior in the prototype.
