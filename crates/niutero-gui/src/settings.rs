//! Settings (design spec §9). Sub-navigation: Library · Workflow · Appearance ·
//! Keymap · Sync & sharing · Integrations.
//!
//! Wired to the engine where the engine supports it: **Appearance → Theme** and
//! **Accent** are live; **Sync → Push-after-commit** persists via
//! `engine::set_sync_prefs`; **Git remote** points `origin` via `engine::connect`.
//! The library name / key-pattern fields show the real values but are not yet
//! persisted (the engine has no config-setter), and the workflow toggles / font
//! pickers are visual — matching the design's "mock" markers. Honest about which
//! controls do something.

use eframe::egui::{self, Color32, RichText};

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
    Appearance,
    Keymap,
    Sync,
    Integrations,
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
    pub push: bool,
    pub seeded: bool,
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
            push: true,
            seeded: false,
        }
    }
}

/// An engine/app-touching request from Settings.
pub enum SettingsAction {
    SetTheme(bool),
    SetAccent(usize),
    SetGitRemote(String),
    SetPush(bool),
    Toast(String),
}

/// Render the Settings tool. `schema` is the read-only config schema version;
/// `dark`/`accent_idx` reflect the live appearance state.
pub fn settings(
    ctx: &egui::Context,
    theme: &Theme,
    st: &mut SettingsState,
    schema: u32,
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
                                SettingsView::Library => lib_view(ui, theme, st, schema),
                                SettingsView::Workflow => workflow_view(ui, theme, st),
                                SettingsView::Appearance => {
                                    appearance_view(ui, theme, st, dark, accent_idx, actions)
                                }
                                SettingsView::Sync => sync_view(ui, theme, st, actions),
                                SettingsView::Keymap => stub(
                                    ui,
                                    theme,
                                    "Keymap",
                                    "Keyboard shortcuts — coming soon.",
                                ),
                                SettingsView::Integrations => stub(
                                    ui,
                                    theme,
                                    "Integrations",
                                    "Zotero import, Obsidian, and Overleaf connectors — coming soon.",
                                ),
                            });
                        });
                });
        });
}

// --------------------------------------------------------------------- views

fn lib_view(ui: &mut egui::Ui, theme: &Theme, st: &mut SettingsState, schema: u32) {
    section_title(ui, theme, "Library");
    row(
        ui,
        theme,
        "Library name",
        "A label for this library.",
        false,
        |ui| {
            text_input(ui, theme, &mut st.name, false);
        },
    );
    row(
        ui,
        theme,
        "Default profile",
        "Profile applied to new entries when none is given.",
        false,
        |ui| dropdown(ui, theme, "None"),
    );
    row(
        ui,
        theme,
        "Citation key pattern",
        "Tokens take an optional .N index; casing follows the token. Imports get this key; Re-key \
         applies it to existing entries.",
        false,
        |ui| {
            ui.vertical(|ui| {
                text_input(ui, theme, &mut st.pattern, true);
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for tok in ["{auth}", "{year}", "{title}", "{title.N}", "{Title.N}"] {
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
    row(
        ui,
        theme,
        "Schema version",
        "The .niutero config format version. Read-only.",
        true,
        |ui| {
            ui.label(
                RichText::new(schema.to_string())
                    .font(theme::mono(14.0))
                    .color(theme.muted),
            );
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
                    if swatch(ui, Color32::from_rgb(*r, *g, *b), i == accent_idx, theme) {
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
            if text_input(ui, theme, &mut st.remote, true).lost_focus()
                && !st.remote.trim().is_empty()
            {
                actions.push(SettingsAction::SetGitRemote(st.remote.trim().to_string()));
            }
        },
    );
    row(
        ui,
        theme,
        "Push after commit",
        "Automatically push to the remote after each auto-commit.",
        false,
        |ui| {
            if widgets::toggle(ui, theme, st.push) {
                st.push = !st.push;
                actions.push(SettingsAction::SetPush(st.push));
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
                    ui.set_max_width(420.0);
                    ui.label(RichText::new(title).size(15.0).strong().color(theme.text));
                    ui.add_space(4.0);
                    ui.label(RichText::new(desc).size(13.0).color(theme.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.set_min_width(300.0);
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

/// A bordered text input (`niu-mono` when `mono`). Returns the edit `Response`.
fn text_input(ui: &mut egui::Ui, theme: &Theme, buf: &mut String, mono: bool) -> egui::Response {
    let mut resp = None;
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(9.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(280.0);
            let mut te = egui::TextEdit::singleline(buf)
                .desired_width(f32::INFINITY)
                .frame(false);
            if mono {
                te = te.font(theme::mono(12.5));
            }
            resp = Some(ui.add(te));
        });
    resp.unwrap()
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

/// A 28×28 accent swatch; a ring marks the selected one. Returns whether clicked.
fn swatch(ui: &mut egui::Ui, color: Color32, selected: bool, theme: &Theme) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), color);
    if selected {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            egui::CornerRadius::same(10),
            egui::Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
        let _ = theme;
    }
    ui.add_space(9.0);
    resp.clicked()
}

fn not_persisted_note(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(10.0);
    ui.label(
        RichText::new("These fields aren't persisted yet — they preview the planned controls.")
            .size(11.5)
            .color(theme.faint),
    );
}
