//! Settings (design spec §9). Sub-navigation: Library · Workflow · AI assistant ·
//! PDF attachments · Appearance · Keymap · Sync & sharing · Integrations.
//!
//! Wired to the engine where the engine supports it: **Appearance → Theme** and
//! **Accent** are live; **Sync → Git remote** points `origin` via
//! `engine::connect`; **AI assistant** persists to the machine-local registry
//! (`engine::set_ai_config`) and its **Test** runs a live `engine::ai_test`. The
//! library name / key-pattern, the workflow toggles, and the **PDF attachments**
//! config (HF repo / token, auto-fetch) are preview-only — the engine has no
//! config-setter for them yet — and the font pickers are visual. Honest about
//! which controls do something (a "not persisted" note marks the preview pages).

use eframe::egui::{self, Color32, RichText};
use niutero_engine::AiConfig;

use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};
use crate::widgets;

/// Accent swatches (Appearance). Index 0 is the theme's own green (no override).
pub const ACCENTS: [(u8, u8, u8); 5] = [
    (0x1F, 0x8A, 0x5B),
    (0x2A, 0x6F, 0xDB),
    (0xD9, 0x77, 0x57),
    (0x7C, 0x5C, 0xD9),
    (0xB9, 0x1C, 0x1C),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsView {
    Library,
    Workflow,
    Ai,
    Pdf,
    Appearance,
    Keymap,
    Sync,
    Integrations,
}

/// AI provider options (Settings → AI assistant → Provider). Only Anthropic is
/// wired; the rest are listed for the roadmap but can't be selected — the
/// engine refuses to call out with another provider configured, so the GUI
/// must not let one be picked.
const PROVIDERS: [&str; 4] = [
    "Anthropic (Claude)",
    "OpenAI",
    "OpenAI-compatible (local)",
    "Google (Gemini)",
];

/// Map a stored provider value back to its [`PROVIDERS`] index. Empty,
/// `anthropic` (the canonical stored value), or the legacy display label all
/// resolve to 0; unrecognized values fall back to 0 too.
pub fn provider_index(provider: &str) -> usize {
    let p = provider.trim();
    if p.is_empty() || p.eq_ignore_ascii_case("anthropic") {
        return 0;
    }
    PROVIDERS.iter().position(|x| *x == p).unwrap_or(0)
}

/// View-local Settings state. The text/toggle values seed from the vault once
/// (`seeded`), then are edited locally; only the wired controls emit actions.
pub struct SettingsState {
    pub view: SettingsView,
    pub search: String,
    pub name: String,
    pub pattern: String,
    pub remote: String,
    pub enrich: bool,
    pub commit: bool,
    pub dupes: usize,
    pub density: usize,
    pub seeded: bool,
    /// AI config is machine-local (not vault-bound), so it seeds on its own flag.
    pub ai_seeded: bool,
    // AI assistant (persisted via engine::set_ai_config).
    pub ai_enabled: bool,
    pub ai_provider: usize,
    pub ai_key: String,
    pub ai_model: String,
    pub ai_base: String,
    /// The AI config as last persisted — the dirty check for the every-frame
    /// flush, so an edit can't be silently dropped by navigating away before
    /// a `lost_focus` fires.
    pub ai_saved: AiConfig,
    /// Whether an AI text field has focus *this frame* (typing in progress —
    /// don't flush mid-keystroke).
    pub ai_field_focused: bool,
    // PDF attachments (persisted: repo + auto-fetch per vault via
    // engine::update_pdf_prefs, token per machine via engine::set_hf_token).
    pub pdf_repo: String,
    pub pdf_auto: bool,
    /// Token input buffer — drained into `set_hf_token` on flush; the stored
    /// token itself is never read back into the UI.
    pub pdf_token_buf: String,
    /// Whether a machine token exists (display only).
    pub pdf_token_set: bool,
    /// (repo, auto_fetch) as last persisted — the dirty check.
    pub pdf_saved: (String, bool),
    /// A PDF text field has focus this frame — don't flush mid-keystroke.
    pub pdf_field_focused: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        SettingsState {
            view: SettingsView::Library,
            search: String::new(),
            name: String::new(),
            pattern: String::new(),
            remote: String::new(),
            enrich: true,
            commit: true,
            dupes: 0,
            density: 0,
            seeded: false,
            ai_seeded: false,
            ai_enabled: false,
            ai_provider: 0,
            ai_key: String::new(),
            // Single-sourced from the engine so CLI and GUI defaults can't
            // drift (enabling AI here must not silently switch the model).
            ai_model: niutero_engine::DEFAULT_MODEL.into(),
            ai_base: String::new(),
            ai_saved: AiConfig::default(),
            ai_field_focused: false,
            pdf_repo: String::new(),
            pdf_auto: false,
            pdf_token_buf: String::new(),
            pdf_token_set: false,
            pdf_saved: (String::new(), false),
            pdf_field_focused: false,
        }
    }
}

/// An engine/app-touching request from Settings.
pub enum SettingsAction {
    SetTheme(bool),
    SetAccent(usize),
    SetGitRemote(String),
    /// Persist the AI assistant config (machine-local registry).
    SetAi(AiConfig),
    /// Run a live connection test against the configured provider.
    TestAi,
    /// Persist the open vault's PDF prefs (HF repo + auto-fetch).
    SetPdfPrefs {
        repo: String,
        auto_fetch: bool,
    },
    /// Store (or clear, with an empty string) the machine-local HF token.
    SetHfToken(String),
    /// Create the vault's HF dataset repo (online, off-thread).
    CreatePdfRepo,
    Toast(String),
}

/// Build an [`AiConfig`] from the current Settings state. Index 0 stores the
/// canonical `anthropic` (what the engine's provider guard accepts), never the
/// display label.
fn current_ai(st: &SettingsState) -> AiConfig {
    AiConfig {
        enabled: st.ai_enabled,
        provider: if st.ai_provider == 0 {
            "anthropic".to_string()
        } else {
            PROVIDERS
                .get(st.ai_provider)
                .copied()
                .unwrap_or("")
                .to_string()
        },
        api_key: st.ai_key.clone(),
        model: st.ai_model.trim().to_string(),
        base_url: st.ai_base.trim().to_string(),
    }
}

/// The unsaved AI config, if the state has drifted from what was last
/// persisted — and mark it saved (callers must actually persist the returned
/// config). `None` while unseeded or clean.
pub fn take_ai_dirty(st: &mut SettingsState) -> Option<AiConfig> {
    if !st.ai_seeded {
        return None;
    }
    let cur = current_ai(st);
    if cur != st.ai_saved {
        st.ai_saved = cur.clone();
        Some(cur)
    } else {
        None
    }
}

/// After seeding from the persisted config, mark the current state clean so
/// the dirty-flush doesn't immediately re-save what was just loaded.
pub fn mark_ai_clean(st: &mut SettingsState) {
    st.ai_saved = current_ai(st);
}

/// The unsaved PDF prefs (repo, auto_fetch), if drifted from what was last
/// persisted — and mark them saved. `None` while no vault is seeded or clean.
pub fn take_pdf_dirty(st: &mut SettingsState) -> Option<(String, bool)> {
    if !st.seeded {
        return None; // prefs are per-vault; nothing to save without one
    }
    let cur = (st.pdf_repo.trim().to_string(), st.pdf_auto);
    if cur != st.pdf_saved {
        st.pdf_saved = cur.clone();
        Some(cur)
    } else {
        None
    }
}

/// After seeding the PDF prefs, mark them clean.
pub fn mark_pdf_clean(st: &mut SettingsState) {
    st.pdf_saved = (st.pdf_repo.trim().to_string(), st.pdf_auto);
}

/// Render the Settings tool. `dark`/`accent_idx` reflect the live appearance
/// state; engine/app requests come back in `actions`.
pub fn settings(
    ctx: &egui::Context,
    theme: &Theme,
    st: &mut SettingsState,
    dark: bool,
    accent_idx: usize,
    actions: &mut Vec<SettingsAction>,
) {
    egui::SidePanel::left("niu-settings-nav")
        .exact_width(224.0)
        .resizable(false)
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin::symmetric(12, 18)),
        )
        .show(ctx, |ui| {
            // search header (visual)
            egui::Frame::default()
                .fill(theme.surface_2)
                .corner_radius(9.0)
                .inner_margin(egui::Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::show(ui, Glyph::Search, 15.0, theme.muted);
                        ui.add(
                            egui::TextEdit::singleline(&mut st.search)
                                .hint_text("Search settings…")
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                    });
                });
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            for (icon, label, v) in [
                (Glyph::Library, "Library", SettingsView::Library),
                (Glyph::Refresh, "Workflow", SettingsView::Workflow),
                (Glyph::Ai, "AI assistant", SettingsView::Ai),
                (Glyph::Attach, "PDF attachments", SettingsView::Pdf),
                (Glyph::Sun, "Appearance", SettingsView::Appearance),
                (Glyph::More, "Keymap", SettingsView::Keymap),
                (Glyph::Sync, "Sync & sharing", SettingsView::Sync),
                (Glyph::Link, "Integrations", SettingsView::Integrations),
            ] {
                if widgets::subnav_item(ui, theme, icon, label, None, st.view == v) {
                    st.view = v;
                }
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::default()
                        .inner_margin(egui::Margin {
                            left: 44,
                            right: 44,
                            top: 30,
                            bottom: 48,
                        })
                        .show(ui, |ui| {
                            widgets::centered_column(ui, 820.0, |ui| match st.view {
                                SettingsView::Library => lib_view(ui, theme, st),
                                SettingsView::Workflow => workflow_view(ui, theme, st),
                                SettingsView::Ai => ai_view(ui, theme, st, actions),
                                SettingsView::Pdf => pdf_view(ui, theme, st, actions),
                                SettingsView::Appearance => {
                                    appearance_view(ui, theme, st, dark, accent_idx, actions)
                                }
                                SettingsView::Sync => sync_view(ui, theme, st, actions),
                                SettingsView::Keymap => {
                                    stub(ui, theme, "Keymap", "Keyboard shortcuts — coming soon.")
                                }
                                SettingsView::Integrations => integrations_view(ui, theme),
                            });
                        });
                });
        });

    // Navigate-away flush: if the AI page isn't rendered this frame (so its
    // `lost_focus` saves can never fire again) — or it is but no field has
    // focus — persist any drift. Without this, clicking another settings page
    // mid-edit silently dropped the typed API key.
    if st.view != SettingsView::Ai {
        st.ai_field_focused = false;
    }
    if !st.ai_field_focused {
        if let Some(cfg) = take_ai_dirty(st) {
            actions.push(SettingsAction::SetAi(cfg));
        }
    }
    // Same discipline for the PDF page: repo/auto-fetch drift flushes once no
    // field has focus, and a typed token is drained into a save rather than
    // silently dropped on navigation.
    if st.view != SettingsView::Pdf {
        st.pdf_field_focused = false;
    }
    if !st.pdf_field_focused {
        if let Some((repo, auto_fetch)) = take_pdf_dirty(st) {
            actions.push(SettingsAction::SetPdfPrefs { repo, auto_fetch });
        }
        let tok = st.pdf_token_buf.trim().to_string();
        if !tok.is_empty() {
            st.pdf_token_buf.clear();
            st.pdf_token_set = true;
            actions.push(SettingsAction::SetHfToken(tok));
        }
    }
}

// --------------------------------------------------------------------- views

fn lib_view(ui: &mut egui::Ui, theme: &Theme, st: &mut SettingsState) {
    section_title(ui, theme, "Library");
    row(
        ui,
        theme,
        "Library name",
        "A label for this library.",
        false,
        |ui| {
            widgets::text_input(ui, theme, &mut st.name, false);
        },
    );
    row(
        ui,
        theme,
        "Citation key pattern",
        "Tokens take an optional .N index; casing follows the token. Imports get this key; Re-key \
         applies it to existing entries.",
        true,
        |ui| {
            ui.vertical(|ui| {
                widgets::text_input(ui, theme, &mut st.pattern, true);
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for tok in [
                        "{auth}",
                        "{year}",
                        "{title}",
                        "{title.N}",
                        "{Title.N}",
                        "{title-content-word}",
                    ] {
                        token_chip(ui, theme, tok);
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Example:").size(12.0).color(theme.muted));
                    ui.label(
                        RichText::new("vaswani2017attentionIsAll")
                            .font(theme::mono(12.0))
                            .color(theme.accent),
                    );
                });
            });
        },
    );
    not_persisted_note(ui, theme);
}

fn workflow_view(ui: &mut egui::Ui, theme: &Theme, st: &mut SettingsState) {
    section_title(ui, theme, "Workflow");
    row(
        ui,
        theme,
        "Enrich on import",
        "When the browser connector captures an entry, look up a published version automatically.",
        false,
        |ui| {
            if widgets::toggle(ui, theme, st.enrich) {
                st.enrich = !st.enrich;
            }
        },
    );
    row(
        ui,
        theme,
        "Auto-commit changes",
        "Commit to git after each batch of edits so the library has a full history.",
        false,
        |ui| {
            if widgets::toggle(ui, theme, st.commit) {
                st.commit = !st.commit;
            }
        },
    );
    row(
        ui,
        theme,
        "On duplicate capture",
        "What to do when a captured paper already exists in the library.",
        true,
        |ui| {
            if let Some(i) = widgets::segmented(
                ui,
                theme,
                &[("Ask", None), ("Merge", None), ("Skip", None)],
                st.dupes,
                false,
            ) {
                st.dupes = i;
            }
        },
    );
    not_persisted_note(ui, theme);
}

#[allow(clippy::too_many_arguments)]
fn appearance_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut SettingsState,
    dark: bool,
    accent_idx: usize,
    actions: &mut Vec<SettingsAction>,
) {
    section_title(ui, theme, "Appearance");
    row(
        ui,
        theme,
        "Theme",
        "Light or dark. Changes apply instantly.",
        false,
        |ui| {
            let sel = usize::from(dark);
            if let Some(i) = widgets::segmented(
                ui,
                theme,
                &[("Light", Some(Glyph::Sun)), ("Dark", Some(Glyph::Moon))],
                sel,
                false,
            ) {
                actions.push(SettingsAction::SetTheme(i == 1));
            }
        },
    );
    row(
        ui,
        theme,
        "Accent color",
        "Used for selection, links, and primary actions.",
        false,
        |ui| {
            ui.horizontal(|ui| {
                for (i, (r, g, b)) in ACCENTS.iter().enumerate() {
                    let clicked = widgets::swatch(
                        ui,
                        Color32::from_rgb(*r, *g, *b),
                        i == accent_idx,
                        28.0,
                        3.0,
                    );
                    ui.add_space(9.0);
                    if clicked {
                        actions.push(SettingsAction::SetAccent(i));
                    }
                }
            });
        },
    );
    row(
        ui,
        theme,
        "Density",
        "How tightly list rows are packed.",
        false,
        |ui| {
            if let Some(i) = widgets::segmented(
                ui,
                theme,
                &[("Comfortable", None), ("Compact", None)],
                st.density,
                false,
            ) {
                st.density = i;
            }
        },
    );
    row(
        ui,
        theme,
        "Interface font",
        "The sans-serif used throughout the UI.",
        false,
        |ui| dropdown(ui, theme, "Hanken Grotesk"),
    );
    row(
        ui,
        theme,
        "Reading font",
        "Serif used for paper titles and abstracts.",
        true,
        |ui| dropdown(ui, theme, "Newsreader"),
    );
}

fn sync_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut SettingsState,
    actions: &mut Vec<SettingsAction>,
) {
    section_title(ui, theme, "Sync & sharing");
    row(
        ui,
        theme,
        "Git remote",
        "The repository your .bib library is committed and pushed to.",
        false,
        |ui| {
            if widgets::text_input(ui, theme, &mut st.remote, true).lost_focus()
                && !st.remote.trim().is_empty()
            {
                actions.push(SettingsAction::SetGitRemote(st.remote.trim().to_string()));
            }
        },
    );
    row(
        ui,
        theme,
        "Browser connector",
        "Local port the capture extension talks to.",
        false,
        |ui| {
            ui.horizontal(|ui| {
                let dot = ui
                    .allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover())
                    .0;
                ui.painter().circle_filled(dot.center(), 4.0, theme.accent);
                ui.label(
                    RichText::new("127.0.0.1:23510")
                        .font(theme::mono(13.0))
                        .color(theme.text_2),
                );
            });
        },
    );
    row(
        ui,
        theme,
        "Share library",
        "Generate a read-only link to the current state of the library.",
        true,
        |ui| {
            if widgets::button(
                ui,
                theme,
                Some(Glyph::Link),
                "Create share link",
                false,
                32.0,
            )
            .clicked()
            {
                actions.push(SettingsAction::Toast(
                    "Share links are not implemented yet".into(),
                ));
            }
        },
    );
}

fn ai_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut SettingsState,
    actions: &mut Vec<SettingsAction>,
) {
    section_title(ui, theme, "AI assistant");
    lede(
        ui,
        theme,
        "LLM-assisted tagging, search, Q&A and analysis across your library. The model only ever \
         suggests — it never edits references.bib.",
    );
    // Any control change re-persists the whole config (machine-local registry).
    // Text edits also flush from `settings()` once their field loses focus or
    // stops rendering, so navigating away can't drop a typed key.
    let mut changed = false;
    row(
        ui,
        theme,
        "Enable LLM assist",
        "Off by default. Turn it on to unlock the AI Assistant tool, auto-tagging, and the \
         Organize / Auto-tag wizards.",
        false,
        |ui| {
            if widgets::toggle(ui, theme, st.ai_enabled) {
                st.ai_enabled = !st.ai_enabled;
                changed = true;
            }
        },
    );
    // Clicking Test in the same frame a field lost focus must save *before* the
    // test runs (the test reads the persisted config), so defer it past `changed`.
    let mut test = false;
    let enabled = st.ai_enabled;
    let mut focused = false;
    ui.add_enabled_ui(enabled, |ui| {
        subhead(ui, theme, "Connection");
        row(
            ui,
            theme,
            "Provider",
            "Only Anthropic is wired today — the others are listed for the roadmap and can't \
             be selected yet.",
            false,
            |ui| {
                if provider_select(ui, &mut st.ai_provider) {
                    changed = true;
                }
            },
        );
        row(
            ui,
            theme,
            "API key",
            "Stored machine-local in vaults.toml — never written to the library or committed to \
             git.",
            false,
            |ui| {
                let r = widgets::password_input(ui, theme, &mut st.ai_key, "sk-…");
                focused |= r.has_focus();
                if r.lost_focus() {
                    changed = true;
                }
            },
        );
        let model_desc = format!(
            "Which model to call (default: {}).",
            niutero_engine::DEFAULT_MODEL
        );
        row(ui, theme, "Model", &model_desc, false, |ui| {
            let r = widgets::text_input(ui, theme, &mut st.ai_model, true);
            focused |= r.has_focus();
            if r.lost_focus() {
                changed = true;
            }
        });
        // The Base URL is a stored-but-unhonored field: the engine refuses to
        // call out while one is set. Only surface it when a legacy value needs
        // clearing — never invite new input into a dead field.
        if !st.ai_base.trim().is_empty() {
            row(
                ui,
                theme,
                "Base URL",
                "Not honored yet — AI calls refuse to run while one is set. Clear it here.",
                false,
                |ui| {
                    let r = widgets::text_input(ui, theme, &mut st.ai_base, true);
                    focused |= r.has_focus();
                    if r.lost_focus() {
                        changed = true;
                    }
                },
            );
        }
        row(
            ui,
            theme,
            "Test connection",
            "Sends a tiny request and reports success or the error. Only title, authors, year, \
             venue, tags and abstract of in-scope entries are ever sent.",
            true,
            |ui| {
                if widgets::button(ui, theme, Some(Glyph::Sparkle), "Test", false, 32.0).clicked() {
                    test = true;
                }
            },
        );
    });
    st.ai_field_focused = focused;
    // Save first, then test against the just-saved config.
    if changed {
        if let Some(cfg) = take_ai_dirty(st) {
            actions.push(SettingsAction::SetAi(cfg));
        }
    }
    if test {
        actions.push(SettingsAction::TestAi);
    }
}

fn pdf_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut SettingsState,
    actions: &mut Vec<SettingsAction>,
) {
    section_title(ui, theme, "PDF attachments");
    if !st.seeded {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Open a library to configure its PDF storage.")
                .size(14.0)
                .color(theme.muted),
        );
        return;
    }
    lede(
        ui,
        theme,
        "Store each entry’s PDF in your own HuggingFace dataset repo. Repos are always created \
         private — you are responsible for any copyrighted content you upload.",
    );
    let mut focused = false;
    subhead(ui, theme, "Storage");
    row(
        ui,
        theme,
        "HuggingFace dataset repo",
        "As user/repo, saved for this library. Each PDF lands at pdfs/<citekey>.pdf on the \
         remote and is cached under ${vault}/pdfs/ locally.",
        false,
        |ui| {
            let r = widgets::text_input(ui, theme, &mut st.pdf_repo, true);
            focused |= r.has_focus();
        },
    );
    row(
        ui,
        theme,
        "HuggingFace token",
        "Stored machine-local in vaults.toml — never in the library or git, and never shown \
         again once saved. Saves when the field loses focus.",
        false,
        |ui| {
            ui.horizontal(|ui| {
                if st.pdf_token_set
                    && widgets::button(ui, theme, None, "Clear", false, 30.0).clicked()
                {
                    st.pdf_token_set = false;
                    st.pdf_token_buf.clear();
                    actions.push(SettingsAction::SetHfToken(String::new()));
                }
                let hint = if st.pdf_token_set {
                    "set — type to replace"
                } else {
                    "hf_…"
                };
                let r = widgets::password_input(ui, theme, &mut st.pdf_token_buf, hint);
                focused |= r.has_focus();
            });
        },
    );
    subhead(ui, theme, "Auto-fetch");
    row(
        ui,
        theme,
        "Auto-fetch PDF on import",
        "Off by default, so imports stay fully offline. When on, fetches an imported entry's \
         url if it is a direct PDF or an arXiv abstract page — publisher landing pages are \
         skipped.",
        false,
        |ui| {
            if widgets::toggle(ui, theme, st.pdf_auto) {
                st.pdf_auto = !st.pdf_auto;
            }
        },
    );
    subhead(ui, theme, "Maintenance");
    row(
        ui,
        theme,
        "Create dataset repo",
        "Creates the repo as a private dataset. Safe to re-run if it already exists.",
        true,
        |ui| {
            if widgets::button(ui, theme, Some(Glyph::Plus), "Create", false, 32.0).clicked() {
                // Persist a just-typed repo first, so "type repo → Create"
                // works in one click (actions apply in order).
                if let Some((repo, auto_fetch)) = take_pdf_dirty(st) {
                    actions.push(SettingsAction::SetPdfPrefs { repo, auto_fetch });
                }
                actions.push(SettingsAction::CreatePdfRepo);
            }
        },
    );
    st.pdf_field_focused = focused;
    ui.add_space(10.0);
    ui.label(
        RichText::new(
            "Repo and auto-fetch are saved per library; the token is per machine. Nothing here \
             is ever written into the library or git.",
        )
        .size(11.5)
        .color(theme.faint),
    );
}

/// The Integrations placeholder — a centered empty state (design's reset).
fn integrations_view(ui: &mut egui::Ui, theme: &Theme) {
    section_title(ui, theme, "Integrations");
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        let (b, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(b, egui::CornerRadius::same(14), theme.surface_2);
        icons::paint_at(ui, b.shrink(12.0), Glyph::Link, theme.faint);
        ui.add_space(16.0);
        ui.label(
            RichText::new("No integrations yet")
                .size(15.0)
                .strong()
                .color(theme.text_2),
        );
        ui.add_space(5.0);
        ui.label(
            RichText::new("Connectors for external services will live here.")
                .size(13.0)
                .color(theme.muted),
        );
    });
}

/// A lede paragraph (design `stLede`): wraps at ~560px under the section title.
fn lede(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(2.0);
    ui.scope(|ui| {
        ui.set_max_width(560.0);
        ui.label(RichText::new(text).size(14.0).color(theme.text_2));
    });
    ui.add_space(8.0);
}

/// A subheading that groups rows within a page (design `stSub`).
fn subhead(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    ui.add_space(20.0);
    ui.label(
        RichText::new(label.to_uppercase())
            .size(11.0)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(2.0);
}

/// The provider dropdown (egui `ComboBox`). Only Anthropic is selectable —
/// the others are visible but disabled until the engine actually speaks their
/// protocol, so a third-party key can never be silently sent to Anthropic.
/// Returns whether the selection changed.
fn provider_select(ui: &mut egui::Ui, idx: &mut usize) -> bool {
    let before = *idx;
    egui::ComboBox::from_id_salt("niu-ai-provider")
        .width(300.0)
        .selected_text(PROVIDERS[(*idx).min(PROVIDERS.len() - 1)])
        .show_ui(ui, |ui| {
            for (i, p) in PROVIDERS.iter().enumerate() {
                if i == 0 {
                    ui.selectable_value(idx, i, *p);
                } else {
                    ui.add_enabled_ui(false, |ui| {
                        let mut dummy = *idx;
                        ui.selectable_value(&mut dummy, i, format!("{p} — not wired yet"));
                    });
                }
            }
        });
    *idx != before
}

fn stub(ui: &mut egui::Ui, theme: &Theme, title: &str, text: &str) {
    section_title(ui, theme, title);
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(14.0).color(theme.muted));
}

// ------------------------------------------------------------- row + widgets

fn section_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(RichText::new(title).size(26.0).strong().color(theme.text));
    ui.add_space(8.0);
}

/// A settings row: left label/description, right control. Hairline below unless
/// `last`.
fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    desc: &str,
    last: bool,
    control: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(0, 20))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(440.0); // design: label column maxWidth 440
                    ui.label(RichText::new(title).size(15.0).strong().color(theme.text));
                    ui.add_space(4.0);
                    ui.label(RichText::new(desc).size(13.0).color(theme.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.set_min_width(320.0); // design: control column 320
                    control(ui);
                });
            });
        });
    if !last {
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1.0, theme.border_2),
        );
    }
}

/// A read-only dropdown affordance (mock): value + chevron.
fn dropdown(ui: &mut egui::Ui, theme: &Theme, value: &str) {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(9.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(200.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(value).size(13.5).color(theme.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    icons::show(ui, Glyph::ChevronDown, 16.0, theme.muted);
                });
            });
        });
}

fn token_chip(ui: &mut egui::Ui, theme: &Theme, tok: &str) {
    egui::Frame::default()
        .fill(theme.surface_2)
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(tok)
                    .font(theme::mono(11.0))
                    .color(theme.text_2),
            );
        });
}

fn not_persisted_note(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(10.0);
    ui.label(
        RichText::new("These fields aren't persisted yet — they preview the planned controls.")
            .size(11.5)
            .color(theme.faint),
    );
}
