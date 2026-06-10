//! Library + Normalize tool bodies and their appliers: the three Library
//! views, the modal dialogs (new entry / add-by-DOI / delete / import), entry
//! CRUD, and the offline normalize pipeline.

use eframe::egui::{self, Color32, RichText};
use log::{info, warn};
use niutero_engine::{self as engine, AddSource};

use crate::dialog::{self, Dialog, DialogOutcome};
use crate::library::{self, LibAction, LibState};
use crate::normalize::{self, NormAction, NormCache, NormView};
use crate::tags::TagAction;
use crate::theme::{self, Theme};

use super::{pick_folder, tool_placeholder, LibView, NiuteroApp, VaultPick};

impl NiuteroApp {
    pub(super) fn body_library(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.library.is_none() {
            let err = self.open_error.clone();
            let mut pick = None;
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(theme.bg))
                .show(ctx, |ui| empty_state(ui, theme, err.as_deref(), &mut pick));
            if let Some(p) = pick {
                self.apply_vault_pick(p);
            }
            return;
        }
        let mut actions = Vec::new();
        // Mirror the appearance pref into the view state (display-only).
        self.lib.author_first_last = self.author_first_last;
        let entries = &self.library.as_ref().unwrap().entries;
        match self.lib_view {
            LibView::Classic => library::classic(ctx, theme, entries, &mut self.lib, &mut actions),
            LibView::Reader => library::reader(ctx, theme, entries, &mut self.lib, &mut actions),
        }
        for a in actions {
            match a {
                // Dialog-opening actions don't need a selection, so they're
                // handled here rather than in `apply_lib_action`.
                LibAction::NewEntry(status) => self.dialog = Some(Dialog::new_entry(status)),
                LibAction::AddByDoi => self.dialog = Some(Dialog::add_by_doi()),
                LibAction::AttachPdf => self.attach_pdf_flow(ctx),
                LibAction::FetchPdf => {
                    if let Some(key) = self.lib.selected.clone() {
                        self.start_pdf_fetch(key, ctx);
                    }
                }
                LibAction::PullPdf => {
                    if let Some(key) = self.lib.selected.clone() {
                        if self.hf_ready() {
                            self.start_pdf_pull(key, ctx);
                        } else {
                            self.set_toast(
                                "Configure an HF repo + token in Settings → PDF attachments \
                                 first",
                            );
                        }
                    }
                }
                LibAction::Delete(key) => {
                    // Re-borrow `self.library` (not the `entries` binding) so the
                    // immutable borrow doesn't span the loop and clash with the
                    // `other` arm's `&mut self`.
                    let title = self
                        .library
                        .as_ref()
                        .and_then(|l| l.entries.iter().find(|e| e.citekey == key))
                        .and_then(|e| e.fields.get("title"))
                        .map(|t| crate::tex::display(t))
                        .unwrap_or_else(|| key.clone());
                    self.dialog = Some(Dialog::confirm_delete(key, title));
                }
                other => self.apply_lib_action(other, ctx),
            }
        }
    }

    /// Apply an engine-touching action from the Library view, then reload.
    fn apply_lib_action(&mut self, action: LibAction, ctx: &egui::Context) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        let Some(key) = self.lib.selected.clone() else {
            return;
        };
        // Set by the arms that write to the .bib, so we refresh the cached git
        // state only after a real mutation (read-only actions like Cite/OpenUrl
        // would otherwise spawn git subprocesses on every click).
        let mut dirtied = false;
        // Deferred past the `lib` borrow: starting the HF pull needs &mut self.
        let mut pull_pdf = false;
        match action {
            LibAction::Edit(field, value) => {
                let (set, unset): (Vec<String>, Vec<String>) = if value.trim().is_empty() {
                    (vec![], vec![field.clone()])
                } else {
                    (vec![format!("{field}={value}")], vec![])
                };
                match engine::edit(&lib.vault, &key, &set, &unset, None) {
                    Ok(()) => {
                        info!("edit {key}.{field}");
                        lib.reload();
                        self.lib.refresh();
                        dirtied = true;
                    }
                    Err(e) => self.toast = Some(format!("Edit failed: {e}")),
                }
            }
            LibAction::SetStatus(s) => {
                if let Err(e) = engine::set_status(&mut lib.vault, &key, s) {
                    self.toast = Some(format!("Status failed: {e}"));
                } else {
                    lib.reload();
                    dirtied = true;
                }
            }
            LibAction::SetStars(n) => {
                if let Err(e) = engine::set_stars(&mut lib.vault, &key, n) {
                    self.toast = Some(format!("Stars failed: {e}"));
                } else {
                    lib.reload();
                    dirtied = true;
                }
            }
            LibAction::AddTag(t) => {
                if let Err(e) = engine::set_tags(&mut lib.vault, &key, &[t], &[]) {
                    self.toast = Some(format!("Tag failed: {e}"));
                } else {
                    lib.reload();
                    dirtied = true;
                }
            }
            LibAction::RemoveTag(t) => {
                if let Err(e) = engine::set_tags(&mut lib.vault, &key, &[], &[t]) {
                    self.toast = Some(format!("Untag failed: {e}"));
                } else {
                    lib.reload();
                    dirtied = true;
                }
            }
            LibAction::OpenUrl(u) => ctx.open_url(egui::OpenUrl::new_tab(u)),
            LibAction::OpenPdf => {
                let p = engine::pdf_path(&lib.vault, &key);
                if p.exists() {
                    let s = p.to_string_lossy().replace('\\', "/");
                    let url = if s.starts_with('/') {
                        format!("file://{s}")
                    } else {
                        format!("file:///{s}")
                    };
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                } else {
                    // Stale view or never-cached file — try an HF pull after
                    // the `lib` borrow ends (the job needs `&mut self`).
                    pull_pdf = true;
                }
            }
            // Field-assign + gen bump (set_toast would re-borrow all of self
            // while `lib` is alive): copying twice in 2.5s repeats the exact
            // message, and the gen bump still re-arms the dismiss timer.
            LibAction::Cite => match engine::cite(&lib.vault, &key) {
                Ok(s) => {
                    ctx.copy_text(s);
                    self.toast = Some("Copied citation".into());
                    self.toast_gen += 1;
                }
                Err(e) => {
                    self.toast = Some(e);
                    self.toast_gen += 1;
                }
            },
            LibAction::Bibtex => match engine::entry_bibtex(&lib.vault, &key) {
                Ok(s) => {
                    ctx.copy_text(s);
                    self.toast = Some("Copied BibTeX".into());
                    self.toast_gen += 1;
                }
                Err(e) => {
                    self.toast = Some(e);
                    self.toast_gen += 1;
                }
            },
            // Handled in `body_library` (they open a dialog / spawn a job and
            // must not run inside this `lib` borrow).
            LibAction::NewEntry(_)
            | LibAction::AddByDoi
            | LibAction::Delete(_)
            | LibAction::AttachPdf
            | LibAction::FetchPdf
            | LibAction::PullPdf => {}
        }
        // Only a real mutation dirties the work tree: run the post-mutation
        // bookkeeping (opt-in auto-commit + git-status refresh) then — never
        // on read-only Cite/BibTeX/Open actions.
        if dirtied {
            self.after_mutation();
        }
        if pull_pdf {
            if self.hf_ready() {
                self.start_pdf_pull(key, ctx);
            } else {
                self.set_toast(
                    "No local PDF — configure an HF repo + token in Settings → PDF attachments \
                     to pull it",
                );
            }
        }
    }

    // ------------------------------------------------------------- Normalize

    pub(super) fn body_normalize(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.library.is_none() {
            tool_placeholder(
                ctx,
                theme,
                "Normalize",
                "Open a library to analyze and clean it.",
            );
            return;
        }
        self.ensure_norm();
        let Some(cache) = self.norm_cache.as_ref() else {
            tool_placeholder(ctx, theme, "Normalize", "Could not analyze the library.");
            return;
        };
        let entries = &self.library.as_ref().unwrap().entries;
        let mut actions = Vec::new();
        normalize::normalize(ctx, theme, entries, cache, &mut self.norm, &mut actions);
        for a in actions {
            self.apply_norm_action(a, ctx);
        }
    }

    /// Compute (once) the offline analysis the Normalize tool renders. Cheap
    /// for small libraries; cached until invalidated by an apply or a switch.
    fn ensure_norm(&mut self) {
        if self.norm_cache.is_some() {
            return;
        }
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        let report = match engine::analyze(&lib.vault) {
            Ok(r) => r,
            Err(e) => {
                warn!("analyze: {e}");
                self.toast = Some(format!("Analyze failed: {e}"));
                return;
            }
        };
        let diffs = engine::normalize_preview(&lib.vault, None).unwrap_or_default();
        let rekey = engine::rekey_preview(&lib.vault, None).unwrap_or_default();
        let pattern = lib
            .vault
            .config
            .citekey_pattern
            .clone()
            .unwrap_or_else(|| "{auth}{year}{title.1}{Title.2}".into());
        let total = report.total;
        self.norm_cache = Some(NormCache {
            report,
            diffs,
            rekey,
            pattern,
            total,
        });
    }

    fn apply_norm_action(&mut self, action: NormAction, ctx: &egui::Context) {
        match action {
            NormAction::RunOffline => self.norm.view = NormView::Review,
            NormAction::StartEnrich => self.start_enrich(ctx),
            NormAction::RefreshRekey => {
                let res = self
                    .library
                    .as_ref()
                    .map(|lib| engine::rekey_preview(&lib.vault, None));
                match res {
                    Some(Ok(rekey)) => {
                        let changed = rekey.len();
                        if let Some(c) = self.norm_cache.as_mut() {
                            c.rekey = rekey;
                        }
                        self.toast = Some(format!(
                            "{changed} key{} would change",
                            if changed == 1 { "" } else { "s" }
                        ));
                    }
                    Some(Err(e)) => self.toast = Some(format!("Re-key preview failed: {e}")),
                    None => {}
                }
            }
            NormAction::CopyPatch => {
                let patch = self
                    .norm_cache
                    .as_ref()
                    .map(|c| build_patch(&c.diffs))
                    .unwrap_or_default();
                ctx.copy_text(patch);
                self.toast = Some("Copied patch".into());
            }
            NormAction::ApplyAll => self.apply_all_norm(),
            NormAction::ApplyRekey => {
                let res = self
                    .library
                    .as_mut()
                    .map(|lib| engine::rekey_apply(&mut lib.vault, None));
                match res {
                    Some(Ok(changes)) => {
                        if let Some(lib) = self.library.as_mut() {
                            lib.reload();
                        }
                        // Cite keys changed → the Library selection is stale.
                        self.lib = LibState::default();
                        self.norm_cache = None;
                        self.after_mutation();
                        let n = changes.len();
                        self.toast = Some(format!(
                            "Re-keyed {n} entr{}",
                            if n == 1 { "y" } else { "ies" }
                        ));
                    }
                    Some(Err(e)) => self.toast = Some(format!("Re-key failed: {e}")),
                    None => {}
                }
            }
        }
    }

    /// Apply every staged change not rejected: a single atomic `normalize_apply`
    /// when nothing is rejected, else `edit` per accepted entry.
    fn apply_all_norm(&mut self) {
        let none_rejected = !self.norm.done.values().any(|v| !v);
        let mut applied = 0usize;
        let mut error: Option<String> = None;

        if none_rejected {
            if let Some(lib) = self.library.as_ref() {
                match engine::normalize_apply(&lib.vault, None) {
                    Ok(ch) => applied = ch.len(),
                    Err(e) => error = Some(e),
                }
            }
        } else {
            let to_apply: Vec<(String, Vec<String>, Vec<String>)> = self
                .norm_cache
                .as_ref()
                .map(|c| {
                    c.diffs
                        .iter()
                        .filter(|d| self.norm.done.get(&d.citekey) != Some(&false))
                        .map(|d| {
                            let (s, u) = norm_edit_args(d);
                            (d.citekey.clone(), s, u)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Some(lib) = self.library.as_ref() {
                for (k, s, u) in &to_apply {
                    match engine::edit(&lib.vault, k, s, u, None) {
                        Ok(()) => applied += 1,
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                }
            }
        }

        match error {
            Some(e) => self.toast = Some(format!("Apply failed: {e}")),
            None => {
                if let Some(lib) = self.library.as_mut() {
                    lib.reload();
                }
                self.lib.refresh();
                self.norm_cache = None;
                self.norm.done.clear();
                self.norm.view = NormView::Overview;
                self.after_mutation();
                self.toast = Some(format!(
                    "Applied {applied} change{}",
                    if applied == 1 { "" } else { "s" }
                ));
            }
        }
    }

    // -------------------------------------------------- dialogs & background

    /// Render the open modal dialog (if any) and act on its outcome.
    pub(super) fn dialog_step(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        match dialog::dialog_ui(ctx, theme, &mut dialog) {
            DialogOutcome::Keep => self.dialog = Some(dialog),
            DialogOutcome::Cancel => {}
            DialogOutcome::CreateEntry => {
                if let Dialog::NewEntry(f) = &dialog {
                    // On failure (e.g. a duplicate key) keep the dialog open so
                    // the user can fix the input.
                    if !self.create_entry(f) {
                        self.dialog = Some(dialog);
                    }
                }
            }
            DialogOutcome::FetchDoi => {
                let doi = match &dialog {
                    Dialog::AddByDoi(f) => f.doi.trim().to_string(),
                    _ => String::new(),
                };
                if doi.is_empty() {
                    self.toast = Some("Enter a DOI to fetch".into());
                    self.dialog = Some(dialog);
                } else if !self.start_doi_import(doi, ctx) {
                    // Busy / no library — keep the dialog so the typed DOI isn't lost.
                    self.dialog = Some(dialog);
                }
            }
            DialogOutcome::ImportFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("BibTeX", &["bib"])
                    .pick_file()
                {
                    self.import_bib_file(&path, ctx);
                } else {
                    self.dialog = Some(dialog); // picker cancelled — keep dialog
                }
            }
            DialogOutcome::Delete => {
                if let Dialog::ConfirmDelete { key, .. } = &dialog {
                    let key = key.clone();
                    self.delete_entry(&key);
                }
            }
            DialogOutcome::ApplyTagOp => {
                if let Dialog::ConfirmTagOp { op, .. } = dialog {
                    match op {
                        dialog::TagOp::Delete(name) => {
                            self.apply_tag_action(TagAction::Delete(name), ctx)
                        }
                        dialog::TagOp::Merge { from, into } => {
                            self.apply_tag_action(TagAction::Merge { from, into }, ctx)
                        }
                    }
                }
            }
        }
    }

    /// Pick a local PDF (rfd) and attach it to the selected entry; when an HF
    /// repo + token are configured the file is pushed there off-thread too.
    fn attach_pdf_flow(&mut self, ctx: &egui::Context) {
        let Some(key) = self.lib.selected.clone() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_title("Attach a PDF")
            .pick_file()
        else {
            return; // picker cancelled
        };
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        match engine::attach_pdf(&lib.vault, &key, &path) {
            Ok(_) => {
                lib.reload(); // has_pdf flips in the views
                self.lib.refresh();
                if self.hf_ready() {
                    self.start_pdf_push(key, ctx); // toasts when done
                } else {
                    self.set_toast("PDF attached");
                }
            }
            Err(e) => self.set_toast(format!("Attach failed: {e}")),
        }
    }

    /// Delete `key` from the library via `engine::rm`, then reload.
    fn delete_entry(&mut self, key: &str) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        match engine::rm(&mut lib.vault, key) {
            Ok(()) => {
                lib.reload();
                if self.lib.selected.as_deref() == Some(key) {
                    self.lib.selected = None;
                }
                self.lib.refresh();
                self.after_mutation();
                self.norm_cache = None;
                self.toast = Some("Deleted entry".into());
            }
            Err(e) => self.toast = Some(format!("Delete failed: {e}")),
        }
    }

    /// Create an entry from the dialog fields via `engine::add`. Returns whether
    /// it succeeded (so the caller can keep the dialog open on failure).
    fn create_entry(&mut self, f: &dialog::NewEntryForm) -> bool {
        let Some(lib) = self.library.as_mut() else {
            return true;
        };
        let mut fields = Vec::new();
        let mut push = |name: &str, val: &str| {
            let v = val.trim();
            if !v.is_empty() {
                fields.push(format!("{name}={v}"));
            }
        };
        push("title", &f.title);
        push("author", &f.author);
        push("year", &f.year);
        // The venue field maps to journal for articles, booktitle otherwise.
        let venue_field = if f.type_ == "article" {
            "journal"
        } else {
            "booktitle"
        };
        push(venue_field, &f.venue);
        push("doi", &f.doi);

        let src = AddSource::Fields {
            type_: f.type_.clone(),
            key: f.key.trim().to_string(),
            fields,
        };
        match engine::add(&lib.vault, src) {
            Ok(keys) => {
                let key = keys.into_iter().next();
                if let (Some(k), Some(status)) = (key.as_ref(), f.status) {
                    let _ = engine::set_status(&mut lib.vault, k, status);
                }
                lib.reload();
                self.lib.refresh();
                if let Some(k) = key {
                    self.lib.selected = Some(k);
                }
                self.after_mutation();
                self.norm_cache = None;
                self.toast = Some("Added entry".into());
                true
            }
            Err(e) => {
                self.toast = Some(format!("Add failed: {e}"));
                false
            }
        }
    }

    /// Import every entry from a local `.bib` file (offline). The duplicate
    /// policy is the library's configured default (`config --on-dup`), else
    /// rename — so a GUI import never silently drops entries.
    fn import_bib_file(&mut self, path: &std::path::Path, ctx: &egui::Context) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        let policy = engine::default_dup_policy(&lib.vault, engine::DupPolicy::Rename);
        match engine::import(&lib.vault, path, policy) {
            Ok(rep) => {
                lib.reload();
                self.lib.refresh();
                self.after_mutation();
                self.norm_cache = None;
                self.toast = Some(format!(
                    "Imported {} · renamed {} · skipped {} · overwritten {}",
                    rep.added,
                    rep.renamed.len(),
                    rep.skipped,
                    rep.overwritten,
                ));
                // Opt-in post-import hooks (PDF auto-fetch / enrich-on-import)
                // run off-thread; a no-op when both prefs are off.
                self.start_post_import(rep.new_keys(), ctx);
            }
            Err(e) => self.toast = Some(format!("Import failed: {e}")),
        }
    }
}

/// Translate a normalize change into `engine::edit` arguments: `FIELD=VALUE`
/// for each set, and field names to unset (a change to nothing).
fn norm_edit_args(d: &niutero_engine::NormChange) -> (Vec<String>, Vec<String>) {
    let mut set = Vec::new();
    let mut unset = Vec::new();
    for c in &d.diffs {
        match &c.to {
            Some(v) => set.push(format!("{}={}", c.field, v)),
            None => unset.push(c.field.clone()),
        }
    }
    (set, unset)
}

/// Render the staged normalization changes as a human-readable text patch.
fn build_patch(diffs: &[niutero_engine::NormChange]) -> String {
    // Flatten values to one line so an embedded newline (e.g. a multi-line
    // abstract) can't break the line-oriented patch format.
    fn oneline(s: &str) -> String {
        s.replace(['\n', '\r'], " ")
    }
    let mut out = String::new();
    for d in diffs {
        out.push_str(&format!("@ {}\n", d.citekey));
        for c in &d.diffs {
            match (&c.from, &c.to) {
                (Some(f), Some(t)) => out.push_str(&format!(
                    "  {}: - {}\n  {}: + {}\n",
                    c.field,
                    oneline(f),
                    c.field,
                    oneline(t)
                )),
                (None, Some(t)) => out.push_str(&format!("  {}: + {}\n", c.field, oneline(t))),
                (Some(f), None) => out.push_str(&format!("  {}: - {}\n", c.field, oneline(f))),
                (None, None) => {}
            }
        }
        out.push('\n');
    }
    out
}

fn empty_state(ui: &mut egui::Ui, theme: &Theme, err: Option<&str>, pick: &mut Option<VaultPick>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.32);
        ui.label(
            RichText::new("No library open")
                .font(theme::serif(22.0))
                .color(theme.text),
        );
        ui.add_space(4.0);
        if let Some(e) = err {
            ui.label(RichText::new(e).color(theme.rose));
        } else {
            ui.label(
                RichText::new("Open a folder as a library, or create a new one.")
                    .color(theme.muted),
            );
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            // center the two buttons
            let pad = (ui.available_width() - 300.0).max(0.0) * 0.5;
            ui.add_space(pad);
            let open = ui.add(
                egui::Button::new(
                    RichText::new("Open library…")
                        .size(13.0)
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(theme.accent)
                .corner_radius(8.0)
                .min_size(egui::vec2(140.0, 34.0)),
            );
            if open.clicked() {
                if let Some(p) = pick_folder("Open a library folder") {
                    *pick = Some(VaultPick::Open(p));
                }
            }
            ui.add_space(10.0);
            let new = ui.add(
                egui::Button::new(
                    RichText::new("New library…")
                        .size(13.0)
                        .strong()
                        .color(theme.text),
                )
                .fill(theme.surface)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .corner_radius(8.0)
                .min_size(egui::vec2(140.0, 34.0)),
            );
            if new.clicked() {
                if let Some(p) = pick_folder("Choose a folder for the new library") {
                    *pick = Some(VaultPick::New(p));
                }
            }
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new("(or launch with a path:  niutero <folder>)")
                .font(theme::mono(11.0))
                .color(theme.faint),
        );
    });
}
