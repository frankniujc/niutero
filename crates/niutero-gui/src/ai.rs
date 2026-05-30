//! AI Assistant — the full chat tab (design spec §7).
//!
//! There is no LLM wired in Phase 1 (the engine exposes no model op, and the CLI
//! is the complete interface), so this is an honest *preview*: the thread is a
//! demo conversation grounded in the **real** library — citation chips jump to
//! the actual entries, and "Copy citations" really copies `engine::cite` output.
//! Composing a message appends it with a clearly-labelled placeholder reply.
//!
//! Engine-touching requests come back as [`AiAction`]s the app applies.

use eframe::egui::{self, Color32, RichText};
use niutero_engine::EntryView;

use crate::icons::{self, Glyph};
use crate::theme::Theme;
use crate::widgets;

/// View-local chat state (just the composer text + any messages the user sent
/// this session — the leading demo turn is rendered from the library).
#[derive(Default)]
pub struct AiState {
    pub input: String,
    pub turns: Vec<Turn>,
}

pub struct Turn {
    pub user: bool,
    pub text: String,
}

/// An engine-touching request from the AI tab/popup.
pub enum AiAction {
    /// Jump to the entry in the Library (Classic).
    OpenEntry(String),
    /// Copy formatted citations for these cite keys.
    CopyCitations(Vec<String>),
    /// A preview-only notice (actions with no real backend yet).
    Toast(String),
}

const SUGGEST: [&str; 3] = [
    "Find gaps in my SAE coverage",
    "Draft a related-work paragraph on unlearning",
    "Which papers should I read next?",
];

/// Render the AI Assistant tab. `entries` grounds the demo answer + citations.
pub fn ai_tab(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut AiState,
    actions: &mut Vec<AiAction>,
) {
    // Context bar (top).
    egui::TopBottomPanel::top("niu-ai-ctx")
        .frame(
            egui::Frame::default()
                .fill(theme.bg)
                .inner_margin(egui::Margin::symmetric(28, 13)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("AI Assistant")
                        .size(17.0)
                        .strong()
                        .color(theme.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, theme, Some(Glyph::Plus), "New chat", false, 30.0)
                        .clicked()
                    {
                        st.turns.clear();
                    }
                    ui.menu_button(
                        RichText::new("Scope: My Library  ▾")
                            .size(13.0)
                            .color(theme.text),
                        |ui| {
                            let _ = ui.button("My Library");
                            let _ = ui.button("Current view");
                            let _ = ui.button("Selected entries");
                        },
                    );
                });
            });
        });

    // Composer (bottom).
    egui::TopBottomPanel::bottom("niu-ai-composer")
        .frame(
            egui::Frame::default()
                .fill(theme.bg)
                .inner_margin(egui::Margin {
                    left: 28,
                    right: 28,
                    top: 14,
                    bottom: 18,
                }),
        )
        .show(ctx, |ui| {
            widgets::centered_column(ui, 760.0, |ui| composer(ui, theme, entries.len(), st));
        });

    // Thread (fills the rest).
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    widgets::centered_column(ui, 760.0, |ui| {
                        egui::Frame::default()
                            .inner_margin(egui::Margin {
                                left: 0,
                                right: 0,
                                top: 28,
                                bottom: 8,
                            })
                            .show(ui, |ui| thread(ui, theme, entries, st, actions));
                    });
                });
        });
}

fn thread(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut AiState,
    actions: &mut Vec<AiAction>,
) {
    let cited = grounding_entries(entries);

    // Demo turn — a question grounded in the real library.
    user_bubble(
        ui,
        theme,
        "Which papers in my library use sparse autoencoders for unlearning, and do they agree on \
         whether it works?",
    );
    ui.add_space(20.0);
    assistant_row(ui, theme, |ui| {
        if cited.is_empty() {
            ui.label(
                RichText::new("Your library is empty — add some entries and ask again.")
                    .size(14.5)
                    .color(theme.text),
            );
            return;
        }
        ui.label(
            RichText::new(format!(
                "{} entr{} in your library touch SAE-based unlearning:",
                cited.len(),
                if cited.len() == 1 { "y" } else { "ies" }
            ))
            .size(14.5)
            .color(theme.text),
        );
        ui.add_space(8.0);
        for e in &cited {
            ui.horizontal(|ui| {
                ui.label(RichText::new("•").color(theme.muted));
                if cite_chip(ui, theme, &cite_label(e)).clicked() {
                    actions.push(AiAction::OpenEntry(e.citekey.clone()));
                }
                let title =
                    crate::tex::display(e.fields.get("title").map(String::as_str).unwrap_or(""));
                ui.label(
                    RichText::new(crate::library::ellipsize(&title, 52))
                        .size(13.5)
                        .color(theme.text_2),
                );
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "They diverge on efficacy — some report clean forgetting, others collateral \
                 damage to retained capabilities.  (Preview: not a real model answer.)",
            )
            .size(14.5)
            .color(theme.text),
        );
        ui.add_space(11.0);
        // answer actions
        ui.horizontal_wrapped(|ui| {
            if widgets::button(
                ui,
                theme,
                Some(Glyph::Quote),
                "Draft related-work ¶",
                false,
                30.0,
            )
            .clicked()
            {
                actions.push(AiAction::Toast(
                    "Drafting needs a connected model (preview)".into(),
                ));
            }
            let tag_lbl = format!("Tag these {} “unlearning”", cited.len());
            if widgets::button(ui, theme, Some(Glyph::Tag), &tag_lbl, false, 30.0).clicked() {
                actions.push(AiAction::Toast(
                    "Bulk-tag needs a connected model (preview)".into(),
                ));
            }
            if widgets::button(ui, theme, Some(Glyph::Copy), "Copy citations", false, 30.0)
                .clicked()
            {
                actions.push(AiAction::CopyCitations(
                    cited.iter().map(|e| e.citekey.clone()).collect(),
                ));
            }
        });
    });

    // Any messages the user composed this session.
    for t in &st.turns {
        ui.add_space(20.0);
        if t.user {
            user_bubble(ui, theme, &t.text);
        } else {
            assistant_row(ui, theme, |ui| {
                ui.label(RichText::new(&t.text).size(14.5).color(theme.text));
            });
        }
    }
}

fn composer(ui: &mut egui::Ui, theme: &Theme, count: usize, st: &mut AiState) {
    // Suggestion chips.
    ui.horizontal_wrapped(|ui| {
        for s in SUGGEST {
            if pill_chip(ui, theme, s) {
                st.input = s.to_string();
            }
        }
    });
    ui.add_space(11.0);

    // Input + send.
    let mut send = false;
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(14.0)
        .inner_margin(egui::Margin {
            left: 16,
            right: 10,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut st.input)
                        .hint_text("Ask across your library…")
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .frame(false),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::click());
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(11), theme.accent);
                    icons::paint_at(ui, rect.shrink(10.0), Glyph::Send, Color32::WHITE);
                    if resp.clicked() {
                        send = true;
                    }
                });
            });
        });
    if send {
        send_message(st, count);
    }
    ui.add_space(9.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(format!(
                "Answers are grounded in your {count} entries · responses can be wrong, verify \
                 citations"
            ))
            .size(11.5)
            .color(theme.faint),
        );
    });
}

fn send_message(st: &mut AiState, count: usize) {
    let text = st.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    st.turns.push(Turn { user: true, text });
    st.turns.push(Turn {
        user: false,
        text: format!(
            "No model is connected yet — this is a preview. With one wired I'd search your \
             {count} entries and answer with citations.",
        ),
    });
    st.input.clear();
}

// ----------------------------------------------------------- shared chat bits

/// A right-aligned user message bubble (accent fill, white text).
pub(crate) fn user_bubble(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.set_max_width(ui.available_width());
        let w = (ui.available_width() * 0.78).max(120.0);
        egui::Frame::default()
            .fill(theme.accent)
            .corner_radius(egui::CornerRadius {
                nw: 16,
                ne: 16,
                sw: 4,
                se: 16,
            })
            .inner_margin(egui::Margin::symmetric(16, 11))
            .show(ui, |ui| {
                ui.set_max_width(w);
                ui.label(RichText::new(text).size(14.5).color(Color32::WHITE));
            });
    });
}

/// An assistant row: a sparkle avatar bubble + the message content.
pub(crate) fn assistant_row(ui: &mut egui::Ui, theme: &Theme, content: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        let (av, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(av, egui::CornerRadius::same(9), theme.accent_tint);
        icons::paint_at(ui, av.shrink(7.0), Glyph::Sparkle, theme.accent);
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            content(ui);
        });
    });
}

/// An inline citation chip ("Creator et al. YEAR"). Returns whether clicked.
pub(crate) fn cite_chip(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let resp = egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(12.0).strong().color(theme.accent));
        });
    resp.response.interact(egui::Sense::click())
}

/// A pill suggestion chip. Returns whether clicked.
fn pill_chip(ui: &mut egui::Ui, theme: &Theme, label: &str) -> bool {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(20.0)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(12.5).color(theme.text_2));
        })
        .response
        .interact(egui::Sense::click())
        .clicked()
}

// ------------------------------------------------------------------ grounding

/// Entries the demo answer cites: those tagged for unlearning, else the first
/// few — so the citation chips always point at real entries.
pub(crate) fn grounding_entries(entries: &[EntryView]) -> Vec<&EntryView> {
    let mut v: Vec<&EntryView> = entries
        .iter()
        .filter(|e| e.tags.iter().any(|t| t.contains("unlearn")))
        .collect();
    if v.is_empty() {
        v = entries.iter().take(3).collect();
    }
    v.truncate(3);
    v
}

/// "Lastname et al. YEAR" (or "Lastname YEAR" for a single author).
pub(crate) fn cite_label(e: &EntryView) -> String {
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    let authors: Vec<&str> = e
        .fields
        .get("author")
        .map(|a| a.split(" and ").map(str::trim).collect())
        .unwrap_or_default();
    let last = authors
        .first()
        .map(|a| a.split(',').next().unwrap_or(a).trim())
        .unwrap_or("Anon");
    let last = crate::tex::display(last);
    if authors.len() > 1 {
        format!("{last} et al. {year}").trim().to_string()
    } else {
        format!("{last} {year}").trim().to_string()
    }
}
