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
    /// Persist any unsaved AI-settings edit (the typed-key-then-navigate-away
    /// case). Called when leaving the Settings tool, switching libraries, and
    /// on app exit; a no-op when the state is clean or unseeded.
    pub(super) fn flush_ai_settings(&mut self) {
        if let Some(cfg) = settings::take_ai_dirty(&mut self.settings) {
            if let Err(e) = engine::set_ai_config(cfg) {
                self.set_toast(format!("Couldn't save AI settings: {e}"));
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
        let mut actions = Vec::new();
        settings::settings(ctx, theme, &mut self.settings, dark, accent, &mut actions);
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
            self.settings.pattern = lib
                .vault
                .config
                .citekey_pattern
                .clone()
                .unwrap_or_else(|| "{auth}{year}{title.1}{Title.2}".into());
            self.settings.seeded = true;
        }
    }

    fn apply_settings_action(&mut self, action: SettingsAction, ctx: &egui::Context) {
        match action {
            SettingsAction::SetTheme(dark) => self.dark = dark,
            SettingsAction::SetAccent(i) => self.accent_idx = i,
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
            SettingsAction::SetAi(cfg) => {
                if let Err(e) = engine::set_ai_config(cfg) {
                    self.toast = Some(format!("Couldn't save AI settings: {e}"));
                }
            }
            SettingsAction::TestAi => self.start_ai_test(ctx),
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
