//! Tags tool body and appliers: tag rename/merge/delete (with confirmation),
//! the tag-wizard modal step loop, and the wizard apply paths (organize /
//! auto-tag / import).

use eframe::egui;
use niutero_engine::{self as engine, EntryView};

use crate::dialog::{self, Dialog};
use crate::tags::{self, TagAction};
use crate::theme::Theme;
use crate::widgets::plural_y;

use super::{tool_placeholder, NiuteroApp};

impl NiuteroApp {
    // ----------------------------------------------------------- Tags tool

    pub(super) fn body_tags(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.library.is_none() {
            tool_placeholder(ctx, theme, "Tags", "Open a library to manage its tags.");
            return;
        }
        let mut actions = Vec::new();
        let lib = self.library.as_ref().unwrap();
        let (entries, gen) = (&lib.entries, lib.gen);
        tags::tags_tab(ctx, theme, entries, gen, &mut self.tags, &mut actions);
        for a in actions {
            match a {
                // Destructive vocabulary ops get the same confirmation an
                // entry delete does: one click would otherwise irreversibly
                // rewrite every carrying entry's sidecar.
                TagAction::Delete(name) => {
                    let n = self.tag_entry_count(&name);
                    self.dialog = Some(Dialog::confirm_tag_op(
                        format!("Delete tag '{name}'?"),
                        format!(
                            "This removes it from {n} entr{}. It cannot be undone.",
                            plural_y(n)
                        ),
                        "Delete".into(),
                        dialog::TagOp::Delete(name),
                    ));
                }
                TagAction::Merge { from, into } => {
                    let n = self.tag_entry_count(&from);
                    self.dialog = Some(Dialog::confirm_tag_op(
                        format!("Merge '{from}' into '{into}'?"),
                        format!(
                            "'{from}' disappears from {n} entr{}; they keep '{into}'. \
                             It cannot be undone.",
                            plural_y(n)
                        ),
                        "Merge".into(),
                        dialog::TagOp::Merge { from, into },
                    ));
                }
                other => self.apply_tag_action(other, ctx),
            }
        }
    }

    /// How many entries currently carry `tag` (for confirm-dialog copy).
    fn tag_entry_count(&self, tag: &str) -> usize {
        self.library
            .as_ref()
            .map(|l| {
                l.entries
                    .iter()
                    .filter(|e| e.tags.iter().any(|t| t == tag))
                    .count()
            })
            .unwrap_or(0)
    }

    pub(super) fn apply_tag_action(&mut self, action: TagAction, _ctx: &egui::Context) {
        match action {
            // Rename and Merge are one engine op (rename onto a new or existing
            // name); afterwards we follow the tag to its new name.
            TagAction::Rename { from, to } | TagAction::Merge { from, into: to } => {
                // Did the target already exist? Then this is really a *merge* —
                // surface that honestly rather than calling it a rename.
                let merged_into = self
                    .library
                    .as_ref()
                    .is_some_and(|l| l.entries.iter().any(|e| e.tags.iter().any(|t| t == &to)));
                let res = self
                    .library
                    .as_mut()
                    .map(|lib| engine::rename_tag(&mut lib.vault, &from, &to));
                match res {
                    Some(Ok(n)) => {
                        if let Some(lib) = self.library.as_mut() {
                            lib.reload();
                        }
                        self.lib.refresh();
                        if self.tags.selected.as_deref() == Some(from.as_str()) {
                            self.tags.selected = Some(to.clone());
                        }
                        // The Library's live tag filter must follow the rename
                        // too, or its views silently empty with no sidebar row
                        // left to un-toggle.
                        if self.lib.active_tag.as_deref() == Some(from.as_str()) {
                            self.lib.active_tag = Some(to.clone());
                        }
                        // Carry the session-local color across the rename (don't
                        // clobber the destination's own color on a merge).
                        self.tags.migrate_color(&from, &to);
                        self.after_mutation();
                        self.set_toast(if merged_into {
                            format!("Merged '{from}' into '{to}' ({n} entr{})", plural_y(n))
                        } else {
                            format!("Renamed '{from}' → '{to}' ({n} entr{})", plural_y(n))
                        });
                    }
                    Some(Err(e)) => self.set_toast(e),
                    None => {}
                }
            }
            TagAction::Delete(name) => {
                let res = self
                    .library
                    .as_mut()
                    .map(|lib| engine::delete_tag(&mut lib.vault, &name));
                match res {
                    Some(Ok(n)) => {
                        if let Some(lib) = self.library.as_mut() {
                            lib.reload();
                        }
                        self.lib.refresh();
                        if self.tags.selected.as_deref() == Some(name.as_str()) {
                            self.tags.selected = None;
                        }
                        // A filter on the deleted tag would show an
                        // unexplained empty library — clear it.
                        if self.lib.active_tag.as_deref() == Some(name.as_str()) {
                            self.lib.active_tag = None;
                        }
                        self.after_mutation();
                        self.set_toast(format!("Deleted '{name}' from {n} entr{}", plural_y(n)));
                    }
                    Some(Err(e)) => self.set_toast(e),
                    None => {}
                }
            }
            TagAction::Jump(key) => self.jump_to_entry(key),
            TagAction::Wizard(kind) => {
                // A new wizard must not inherit a previous wizard's in-flight
                // job (its results would land in the wrong review step).
                self.cancel_wizard_ai_job();
                self.tag_wizard = Some(tags::Wizard::new(kind));
            }
        }
    }

    /// Render the open tag wizard (modal) and act on its outcome.
    pub(super) fn tag_wizard_step(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(mut wiz) = self.tag_wizard.take() else {
            return;
        };
        let entries: &[EntryView] = self
            .library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or(&[]);
        match tags::wizard_ui(ctx, theme, &mut wiz, entries) {
            tags::WizardOutcome::Keep => self.tag_wizard = Some(wiz),
            tags::WizardOutcome::Close => {
                // Closing a wizard abandons its in-flight model call too —
                // otherwise the result lands in whatever wizard opens next.
                self.cancel_wizard_ai_job();
            }
            tags::WizardOutcome::ScanImport { files } => {
                // Run the LaTeX cite-scan (we hold the vault), feed the result
                // back into the wizard, and keep it open at the review step.
                match self
                    .library
                    .as_ref()
                    .map(|l| engine::tex_scan(&l.vault, &files))
                {
                    Some(Ok(report)) => {
                        // `\nocite{*}` cites the whole library — `used` holds only
                        // explicit cites, so expand to every entry in that case.
                        let matched = if report.cite_all {
                            self.library
                                .as_ref()
                                .map(|l| l.entries.iter().map(|e| e.citekey.clone()).collect())
                                .unwrap_or(report.used)
                        } else {
                            report.used
                        };
                        wiz.set_scan(matched, report.missing);
                    }
                    Some(Err(e)) => self.toast = Some(format!("Scan failed: {e}")),
                    None => {}
                }
                self.tag_wizard = Some(wiz);
            }
            tags::WizardOutcome::ApplyImport { tag, keys } => {
                let summary = self.apply_import(tag, keys);
                wiz.set_applied(summary); // the Done step reports real counts
                self.tag_wizard = Some(wiz);
            }
            tags::WizardOutcome::RunOrganize { instructions } => {
                if !self.start_organize(instructions, ctx) {
                    wiz.fail();
                }
                self.tag_wizard = Some(wiz);
            }
            tags::WizardOutcome::ApplyOrganize { merges, new_tags } => {
                let summary = self.apply_organize(&merges, &new_tags);
                wiz.set_applied(summary);
                self.tag_wizard = Some(wiz);
            }
            tags::WizardOutcome::RunAutotag { keys } => {
                if !self.start_auto_tag(keys, ctx) {
                    wiz.fail();
                }
                self.tag_wizard = Some(wiz);
            }
            tags::WizardOutcome::ApplyAutotag { assignments } => {
                let summary = self.apply_autotag(assignments);
                wiz.set_applied(summary);
                self.tag_wizard = Some(wiz);
            }
        }
    }

    /// Apply the accepted Organize merges in one engine call
    /// (`apply_tag_merges` — same locking/merge semantics as the CLI's
    /// `ai organize --apply`). New-tag suggestions are advisory (a tag exists
    /// only on entries). Returns the real outcome for the Done step.
    fn apply_organize(
        &mut self,
        merges: &[(String, String)],
        new_tags: &[String],
    ) -> tags::ApplySummary {
        let Some(lib) = self.library.as_mut() else {
            return tags::ApplySummary::default();
        };
        let req: Vec<engine::TagMerge> = merges
            .iter()
            .map(|(from, into)| engine::TagMerge {
                from: from.clone(),
                into: into.clone(),
                reason: String::new(),
            })
            .collect();
        let results = engine::apply_tag_merges(&mut lib.vault, &req);
        let mut s = tags::ApplySummary::default();
        for r in &results {
            match (&r.error, r.changed) {
                (Some(e), _) => {
                    s.failed += 1;
                    if s.first_error.is_none() {
                        s.first_error = Some(e.clone());
                    }
                }
                // Ok(0): the `from` tag matched nothing (stale/hallucinated
                // plan line) — skipped, not "applied".
                (None, 0) => s.skipped += 1,
                (None, _) => s.applied += 1,
            }
        }
        if s.applied > 0 {
            // Follow the Library's live tag filter across applied merges.
            for r in results
                .iter()
                .filter(|r| r.error.is_none() && r.changed > 0)
            {
                if self.lib.active_tag.as_deref() == Some(r.from.as_str()) {
                    self.lib.active_tag = Some(r.into.clone());
                }
            }
            lib.reload();
            self.lib.refresh();
            self.after_mutation();
        }
        let mut msg = format!(
            "Merged {} tag{}",
            s.applied,
            if s.applied == 1 { "" } else { "s" }
        );
        if s.skipped > 0 {
            msg.push_str(&format!(" ({} skipped)", s.skipped));
        }
        if s.failed > 0 {
            match &s.first_error {
                Some(e) => msg.push_str(&format!(" ({} failed — {e})", s.failed)),
                None => msg.push_str(&format!(" ({} failed)", s.failed)),
            }
        }
        if !new_tags.is_empty() {
            msg.push_str(&format!(" · {} suggested", new_tags.len()));
        }
        self.set_toast(msg);
        s
    }

    /// Apply the accepted Auto-tag assignments in ONE sidecar write
    /// (`set_tags_bulk` — the per-entry loop rewrote and fsynced the whole
    /// sidecar once per entry, freezing the UI on large libraries). Adds only,
    /// never removes. Returns the real outcome for the Done step.
    fn apply_autotag(&mut self, assignments: Vec<(String, Vec<String>)>) -> tags::ApplySummary {
        let Some(lib) = self.library.as_mut() else {
            return tags::ApplySummary::default();
        };
        let adds: Vec<(String, Vec<String>)> = assignments
            .into_iter()
            .filter(|(_, t)| !t.is_empty())
            .collect();
        let tag_total: usize = adds.iter().map(|(_, t)| t.len()).sum();
        let mut s = tags::ApplySummary::default();
        match engine::set_tags_bulk(&mut lib.vault, &adds) {
            Ok((changed, unknown)) => {
                s.applied = changed;
                s.skipped = unknown.len();
                s.tags = tag_total;
            }
            Err(e) => {
                s.failed = adds.len();
                s.first_error = Some(e);
            }
        }
        if s.applied > 0 {
            lib.reload();
            self.lib.refresh();
            self.after_mutation();
        }
        match &s.first_error {
            Some(e) => self.set_toast(format!("Auto-tag failed: {e}")),
            None => {
                let mut msg = format!(
                    "Tagged {} entr{} with {} tag{}",
                    s.applied,
                    if s.applied == 1 { "y" } else { "ies" },
                    s.tags,
                    if s.tags == 1 { "" } else { "s" }
                );
                if s.skipped > 0 {
                    msg.push_str(&format!(" ({} not found)", s.skipped));
                }
                self.set_toast(msg);
            }
        }
        s
    }

    /// Tag every cited+matched entry from the Import wizard with `tag`
    /// (sidecar), in one bulk write. Returns the real outcome.
    fn apply_import(&mut self, tag: String, keys: Vec<String>) -> tags::ApplySummary {
        let Some(lib) = self.library.as_mut() else {
            return tags::ApplySummary::default();
        };
        let adds: Vec<(String, Vec<String>)> =
            keys.into_iter().map(|k| (k, vec![tag.clone()])).collect();
        let mut s = tags::ApplySummary::default();
        match engine::set_tags_bulk(&mut lib.vault, &adds) {
            Ok((changed, unknown)) => {
                s.applied = changed;
                s.skipped = unknown.len();
                s.tags = changed;
            }
            Err(e) => {
                s.failed = adds.len();
                s.first_error = Some(e);
            }
        }
        if s.applied > 0 {
            lib.reload();
            self.lib.refresh();
            self.after_mutation();
        }
        match &s.first_error {
            Some(e) => self.set_toast(format!("Tagging failed: {e}")),
            None => {
                let mut msg = format!("Tagged {} entr{} {tag}", s.applied, plural_y(s.applied));
                if s.skipped > 0 {
                    msg.push_str(&format!(" ({} not found)", s.skipped));
                }
                self.set_toast(msg);
            }
        }
        s
    }
}
