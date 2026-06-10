//! AI Assistant — the chat tab (design spec §7), wired to the real model.
//!
//! Composing a question sends it to `engine::ask` off-thread (grounded in the
//! library); the answer streams back into the thread. The model is told to cite
//! cite keys in `[brackets]`, so we surface those as clickable chips that jump
//! to the real entries, plus a "Copy citations" action. Requires LLM assist to
//! be enabled (Settings → AI assistant) with a key.
//!
//! Engine-touching requests come back as [`AiAction`]s the app applies; the
//! actual model call runs on the app's off-thread AI worker.

use eframe::egui::{self, Color32, RichText};
use niutero_engine::EntryView;

use crate::icons::{self, Glyph};
use crate::theme::Theme;
use crate::widgets;

/// View-local chat state: the composer text, the conversation turns, and whether
/// an answer is currently in flight.
#[derive(Default)]
pub struct AiState {
    pub input: String,
    pub turns: Vec<Turn>,
    /// True while a question is awaiting the model's answer.
    pub pending: bool,
}

pub struct Turn {
    pub user: bool,
    pub text: String,
}

/// An engine-touching request from the AI tab/popup.
pub enum AiAction {
    /// Ask the model a question (grounded in the library), off-thread.
    Ask(String),
    /// Jump to the entry in the Library (Classic).
    OpenEntry(String),
    /// Copy formatted citations for these cite keys.
    CopyCitations(Vec<String>),
    /// The thread was cleared — drop any in-flight ask so its stale answer
    /// can't land in the fresh chat.
    NewChat,
}

const SUGGEST: [&str; 3] = [
    "Which papers should I read next?",
    "Summarize what my library says about evaluation",
    "What topics am I missing?",
];

/// Render the AI Assistant tab. `entries` grounds the answers + citation chips.
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
                        // A fresh thread: also drop the spinner and tell the
                        // app to invalidate any in-flight ask, or its stale
                        // answer would land as an orphan turn in the new chat.
                        st.turns.clear();
                        st.pending = false;
                        actions.push(AiAction::NewChat);
                    }
                    // Scope is always the whole library today — shown as a
                    // plain fact, not a menu of options that do nothing.
                    ui.label(
                        RichText::new("Scope: My Library")
                            .size(13.0)
                            .color(theme.muted),
                    )
                    .on_hover_text("Narrower scopes (current view, selection) are coming later.");
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
            widgets::centered_column(ui, 760.0, |ui| {
                composer(ui, theme, entries.len(), st, actions)
            });
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
    if st.turns.is_empty() && !st.pending {
        assistant_row(ui, theme, |ui| {
            ui.label(
                RichText::new(format!(
                    "Ask anything about your {} entries — I answer from your library and cite the \
                     papers I draw on.",
                    entries.len()
                ))
                .size(14.5)
                .color(theme.text),
            );
        });
        return;
    }

    for t in &st.turns {
        if t.user {
            user_bubble(ui, theme, &t.text);
        } else {
            assistant_row(ui, theme, |ui| {
                ui.label(RichText::new(&t.text).size(14.5).color(theme.text));
                // Surface the cite keys the model referenced as jump chips.
                let cited = cited_in(&t.text, entries);
                if !cited.is_empty() {
                    ui.add_space(9.0);
                    ui.horizontal_wrapped(|ui| {
                        for e in &cited {
                            if cite_chip(ui, theme, &cite_label(e)).clicked() {
                                actions.push(AiAction::OpenEntry(e.citekey.clone()));
                            }
                        }
                        if widgets::button(
                            ui,
                            theme,
                            Some(Glyph::Copy),
                            "Copy citations",
                            false,
                            28.0,
                        )
                        .clicked()
                        {
                            actions.push(AiAction::CopyCitations(
                                cited.iter().map(|e| e.citekey.clone()).collect(),
                            ));
                        }
                    });
                }
            });
        }
        ui.add_space(20.0);
    }

    if st.pending {
        assistant_row(ui, theme, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0).color(theme.accent));
                ui.label(RichText::new("Thinking…").size(14.0).color(theme.muted));
            });
        });
    }
}

/// The entries the model cited, by scanning the answer for `[citekey]` markers
/// that match real entries (de-duplicated, in order of appearance). Shared with
/// the popup chat (`overlays`).
pub(crate) fn cited_in<'a>(answer: &str, entries: &'a [EntryView]) -> Vec<&'a EntryView> {
    let mut out: Vec<&EntryView> = Vec::new();
    for e in entries {
        if answer.contains(&format!("[{}]", e.citekey))
            && !out.iter().any(|x| x.citekey == e.citekey)
        {
            out.push(e);
        }
    }
    out
}

fn composer(
    ui: &mut egui::Ui,
    theme: &Theme,
    count: usize,
    st: &mut AiState,
    actions: &mut Vec<AiAction>,
) {
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
        let q = st.input.trim().to_string();
        if !q.is_empty() && !st.pending {
            // The app records the turn, clears the input, and runs the model.
            actions.push(AiAction::Ask(q));
        }
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
