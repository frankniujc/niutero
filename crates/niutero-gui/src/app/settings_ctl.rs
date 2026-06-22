//! Settings + AI tool bodies, the floating overlays (AI popup / task toast),
//! and their action appliers.

use std::sync::atomic::Ordering;

use eframe::egui;
use log::info;
use niutero_engine::{self as engine, EntryView};

use crate::ai::{self, AiAction};
use crate::overlays::{self, OverlayMsg};
use crate::settings::{self, SettingsAction};
use crate::theme::Theme;

use super::jobs::AiJobKind;
use super::{LibView, NiuteroApp, Tool};

impl NiuteroApp {
    /// Persist every unsaved Settings edit (AI config, PDF prefs, a typed HF
    /// token, the library name/pattern) — the typed-then-navigate-away case.
    /// Called when leaving the Settings tool, switching libraries (BEFORE the
    /// old library is dropped, since the vault-config groups are per-vault),
    /// and on app exit. No-op when clean. The same `persist_*` helpers back
    /// the SettingsAction appliers, so each persistence chain exists once.
    pub(super) fn flush_settings(&mut self) {
        if let Some((prev, cur)) = settings::take_ai_dirty(&mut self.settings) {
            self.persist_ai(prev, cur);
        }
        if let Some((repo, auto_fetch)) = settings::take_pdf_dirty(&mut self.settings) {
            self.persist_pdf(repo, auto_fetch);
        }
        let tok = self.settings.pdf_token_buf.trim().to_string();
        if !tok.is_empty() {
            self.settings.pdf_token_buf.clear();
            self.persist_token(tok, true);
        }
        if let Some((name, pattern)) = settings::take_lib_dirty(&mut self.settings) {
            self.persist_lib(name, pattern);
        }
    }

    /// Write only the CHANGED AI fields under the registry lock, so a
    /// concurrent `niutero-cli ai config` edit to another field survives a
    /// GUI save. On failure: toast + reseed the page from the engine, so the
    /// UI shows what is actually stored rather than pretending the rejected
    /// value was saved.
    fn persist_ai(&mut self, prev: engine::AiConfig, cur: engine::AiConfig) {
        let r = engine::update_ai_config(|c| {
            if cur.enabled != prev.enabled {
                c.enabled = cur.enabled;
            }
            if cur.provider != prev.provider {
                c.provider = cur.provider.clone();
            }
            if cur.api_key != prev.api_key {
                c.api_key = cur.api_key.clone();
            }
            if cur.model != prev.model {
                c.model = cur.model.clone();
            }
            if cur.base_url != prev.base_url {
                c.base_url = cur.base_url.clone();
            }
        });
        if let Err(e) = r {
            self.set_toast(format!("Couldn't save AI settings: {e}"));
            self.settings.ai_seeded = false; // reseed from the engine's truth
        }
    }

    /// Persist the vault's PDF repo + auto-fetch (synced config). A config
    /// write dirties the repo, so the auto-commit hook runs like the CLI's.
    fn persist_pdf(&mut self, repo: String, auto_fetch: bool) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        let r = engine::set_pdf_repo(&mut lib.vault, &repo).and_then(|()| {
            engine::set_workflow(&mut lib.vault, None, None, None, Some(auto_fetch), None)
                .map(|_| ())
        });
        match r {
            Ok(()) => self.after_mutation(),
            Err(e) => {
                self.set_toast(format!("Couldn't save PDF settings: {e}"));
                self.settings.seeded = false; // reseed the vault-bound groups
            }
        }
    }

    /// Persist the library name + citekey pattern (synced config).
    fn persist_lib(&mut self, name: String, pattern: String) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        match engine::set_library_meta(&mut lib.vault, Some(&name), Some(&pattern)) {
            Ok(()) => self.after_mutation(),
            Err(e) => {
                self.set_toast(format!("Couldn't save library settings: {e}"));
                self.settings.seeded = false;
            }
        }
    }

    /// Persist the library's workflow toggles (synced config).
    fn persist_workflow(&mut self, enrich: bool, commit: bool, on_dup: String, normalize: bool) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        match engine::set_workflow(
            &mut lib.vault,
            Some(enrich),
            Some(commit),
            Some(&on_dup),
            None,
            Some(normalize),
        ) {
            Ok(_) => self.after_mutation(),
            Err(e) => {
                self.set_toast(format!("Couldn't save workflow settings: {e}"));
                self.settings.seeded = false;
            }
        }
    }

    /// Store/clear the machine HF token; `pdf_token_set` reflects the actual
    /// engine outcome, never an optimistic guess. `quiet` skips the success
    /// toast (the navigate-away flush shouldn't pop one).
    fn persist_token(&mut self, tok: String, quiet: bool) {
        let clearing = tok.trim().is_empty();
        match engine::set_hf_token(&tok) {
            Ok(()) => {
                self.settings.pdf_token_set = !clearing;
                if !quiet {
                    self.set_toast(if clearing {
                        "HF token cleared"
                    } else {
                        "HF token saved (machine-local)"
                    });
                }
            }
            Err(e) => {
                self.settings.pdf_token_set = engine::hf_token_set().unwrap_or(false);
                self.set_toast(format!("Couldn't save the HF token: {e}"));
            }
        }
    }

    // ------------------------------------------------------------- AI tool

    pub(super) fn body_ai(&mut self, ctx: &egui::Context, theme: &Theme) {
        let entries: &[EntryView] = self
            .library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or(&[]);
        let mut actions = Vec::new();
        ai::ai_tab(ctx, theme, entries, &mut self.ai, &mut actions);
        for a in actions {
            self.apply_ai_action(a, ctx);
        }
    }

    fn apply_ai_action(&mut self, action: AiAction, ctx: &egui::Context) {
        match action {
            AiAction::Ask(q) => {
                // The tab's composer keeps its text on refusal (start_ai_ask
                // only clears it once the job starts).
                let _ = self.start_ai_ask(q, ctx);
            }
            AiAction::NewChat => {
                // The thread was cleared in the view; drop any in-flight ask
                // so its stale answer can't arrive as an orphan turn.
                if self
                    .ai_job
                    .as_ref()
                    .is_some_and(|j| j.kind == AiJobKind::Ask)
                {
                    self.cancel_ai_job();
                }
            }
            AiAction::OpenEntry(key) => self.jump_to_entry(key),
            AiAction::CopyCitations(keys) => {
                let Some(lib) = self.library.as_ref() else {
                    return;
                };
                let mut out = String::new();
                let mut n = 0usize;
                for k in &keys {
                    if let Ok(c) = engine::cite(&lib.vault, k) {
                        out.push_str(&c);
                        out.push('\n');
                        n += 1;
                    }
                }
                ctx.copy_text(out);
                self.set_toast(format!(
                    "Copied {n} citation{}",
                    if n == 1 { "" } else { "s" }
                ));
            }
        }
    }

    /// Switch to the Library (Classic) and select `key`.
    pub(super) fn jump_to_entry(&mut self, key: String) {
        self.tool = Tool::Library;
        self.lib_view = LibView::Classic;
        self.lib.selected = Some(key);
        self.lib.refresh();
    }

    // -------------------------------------------------------- Settings tool

    pub(super) fn body_settings(&mut self, ctx: &egui::Context, theme: &Theme) {
        self.ensure_settings_seed();
        let dark = self.dark;
        let accent = self.accent_idx;
        let first_last = self.author_first_last;
        let mut actions = Vec::new();
        settings::settings(
            ctx,
            theme,
            &mut self.settings,
            dark,
            accent,
            first_last,
            &mut actions,
        );
        for a in actions {
            self.apply_settings_action(a, ctx);
        }
    }

    /// Seed the editable Settings fields. AI config is machine-local and seeds on
    /// its own flag; the library name / key-pattern seed from the open vault.
    fn ensure_settings_seed(&mut self) {
        if !self.settings.ai_seeded {
            if let Ok(cfg) = engine::ai_config() {
                self.settings.ai_enabled = cfg.enabled;
                self.settings.ai_provider = settings::provider_index(&cfg.provider);
                self.settings.ai_key = cfg.api_key;
                if !cfg.model.trim().is_empty() {
                    self.settings.ai_model = cfg.model;
                }
                self.settings.ai_base = cfg.base_url;
            }
            self.settings.ai_seeded = true;
            // What was just loaded is by definition clean — without this the
            // dirty-flush would pointlessly re-save the config straight away.
            settings::mark_ai_clean(&mut self.settings);
        }
        if self.settings.seeded {
            return;
        }
        if let Some(lib) = self.library.as_ref() {
            self.settings.name = lib.vault.config.name.clone();
            // The pattern field shows exactly what's stored; empty = default.
            self.settings.pattern = lib.vault.config.citekey_pattern.clone().unwrap_or_default();
            // Workflow toggles seed from the library's own config.
            let w = &lib.vault.config.workflow;
            self.settings.enrich = w.enrich_on_import;
            self.settings.commit = w.auto_commit;
            self.settings.normalize = w.normalize_on_import;
            self.settings.dupes = settings::dup_index(w.on_dup.as_deref());
            // The git remote is read straight from the repo — an
            // already-connected vault shows it without re-entry.
            self.settings.remote = engine::remote_url(&lib.vault).unwrap_or_default();
            // PDF: repo/auto-fetch live in the vault config (legacy registry
            // values still honored by the resolution fns).
            self.settings.pdf_repo = engine::pdf_repo(&lib.vault)
                .ok()
                .flatten()
                .unwrap_or_default();
            self.settings.pdf_auto = engine::pdf_auto_fetch_enabled(&lib.vault);
            self.settings.pdf_token_set = engine::hf_token_set().unwrap_or(false);
            self.settings.seeded = true;
            settings::mark_pdf_clean(&mut self.settings);
            settings::mark_lib_clean(&mut self.settings);
        }
    }

    fn apply_settings_action(&mut self, action: SettingsAction, ctx: &egui::Context) {
        match action {
            SettingsAction::SetTheme(dark) => {
                self.dark = dark;
                self.persist_ui_prefs();
            }
            SettingsAction::SetAccent(i) => {
                self.accent_idx = i;
                self.persist_ui_prefs();
            }
            SettingsAction::SetAuthorStyle(first_last) => {
                self.author_first_last = first_last;
                self.persist_ui_prefs();
            }
            SettingsAction::SetGitRemote(url) => {
                let r = self
                    .library
                    .as_ref()
                    .map(|lib| engine::connect(&lib.vault, &url));
                match r {
                    Some(Ok(())) => {
                        info!("set git remote → {url}");
                        self.toast = Some("Set git remote".into());
                    }
                    Some(Err(e)) => self.toast = Some(format!("Remote failed: {e}")),
                    None => {}
                }
            }
            SettingsAction::SetAi { prev, cur } => self.persist_ai(prev, cur),
            SettingsAction::TestAi => self.start_ai_test(ctx),
            SettingsAction::SetPdfPrefs { repo, auto_fetch } => self.persist_pdf(repo, auto_fetch),
            SettingsAction::SetLibraryMeta { name, pattern } => self.persist_lib(name, pattern),
            SettingsAction::SetWorkflow {
                enrich_on_import,
                auto_commit,
                on_dup,
                normalize_on_import,
            } => self.persist_workflow(enrich_on_import, auto_commit, on_dup, normalize_on_import),
            SettingsAction::SetHfToken(tok) => self.persist_token(tok, false),
            SettingsAction::CreatePdfRepo => self.start_create_pdf_repo(ctx),
            SettingsAction::SetConnectorEnabled(on) => {
                // The actual start/stop happens next frame in `sync_connector`;
                // here we just flip the flag and persist it (machine-local).
                self.connector.enabled = on;
                self.connector.error = None; // a fresh attempt clears a prior failure
                self.persist_ui_prefs();
            }
            SettingsAction::Toast(m) => self.toast = Some(m),
        }
    }

    // --------------------------------------------------------- overlays

    pub(super) fn overlays(&mut self, ctx: &egui::Context, theme: &Theme) {
        let entries: &[EntryView] = self
            .library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or(&[]);
        let mut msgs = Vec::new();
        overlays::overlays(
            ctx,
            theme,
            entries,
            self.ai_popup_open,
            self.task.as_ref(),
            &mut self.ai_popup_input,
            &self.ai,
            &mut msgs,
        );
        for m in msgs {
            self.apply_overlay_msg(m, ctx);
        }
    }

    fn apply_overlay_msg(&mut self, msg: OverlayMsg, ctx: &egui::Context) {
        match msg {
            OverlayMsg::ToggleAi => self.ai_popup_open = !self.ai_popup_open,
            OverlayMsg::CloseAi => self.ai_popup_open = false,
            OverlayMsg::OpenAiTab => {
                self.ai_popup_open = false;
                self.tool = Tool::Ai;
            }
            OverlayMsg::OpenEntry(key) => {
                self.ai_popup_open = false;
                self.jump_to_entry(key);
            }
            // Keep the popup open so the answer streams in where it was asked.
            // The composer buffer is cleared only when the job actually starts
            // — a busy/no-library refusal must not eat the typed question.
            OverlayMsg::Ask(q) => {
                if self.start_ai_ask(q, ctx) {
                    self.ai_popup_input.clear();
                }
            }
            OverlayMsg::DismissTask => self.task = None,
            OverlayMsg::CancelTask => {
                // Signal the worker to stop; keep `bg` so it isn't double-started,
                // and hide the toast now (the worker clears `bg` when it exits).
                if let Some(bg) = self.bg.as_ref() {
                    bg.cancel.store(true, Ordering::Relaxed);
                }
                self.task = None;
            }
        }
    }
}
