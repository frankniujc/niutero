//! Floating overlays (design spec §7 popup, §8 task toast): the AI sparkle FAB +
//! compact chat popup (bottom-right) and the background-task progress toast
//! (bottom-left). Both float above the tool body via `egui::Area`.
//!
//! These are pure chrome over app state; user intent comes back as
//! [`OverlayMsg`]s the app applies (toggle popup, open the full AI tab, jump to
//! Review, dismiss the task, …). The popup chat is the same honest preview as
//! the AI tab — citation chips jump to real entries.

use eframe::egui::{self, Color32, RichText};
use niutero_engine::EntryView;

use crate::ai;
use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};
use crate::widgets;

/// A simulated background task (e.g. Online enrich). Progress is derived from
/// elapsed time so the toast animates without a real worker thread.
pub struct TaskState {
    pub label: String,
    pub done_label: String,
    pub total: usize,
    /// `ctx.input().time` when the task started, and how long the demo runs.
    pub start: f64,
    pub duration: f64,
}

impl TaskState {
    pub fn progress(&self, now: f64) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (((now - self.start) / self.duration) as f32).clamp(0.0, 1.0)
        }
    }
}

/// What the user asked of an overlay; the app applies these post-render.
pub enum OverlayMsg {
    ToggleAi,
    OpenAiTab,
    CloseAi,
    OpenEntry(String),
    Review,
    DismissTask,
    Toast(String),
}

/// Draw the overlays. `ai_open` is the popup's open state; `task` the running
/// background task (if any); `popup_input` the popup composer buffer.
pub fn overlays(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    ai_open: bool,
    task: Option<&TaskState>,
    popup_input: &mut String,
    msgs: &mut Vec<OverlayMsg>,
) {
    // AI popup panel (above the FAB), when open.
    if ai_open {
        egui::Area::new("niu-ai-popup".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-22.0, -104.0))
            .show(ctx, |ui| popup_panel(ui, theme, entries, popup_input, msgs));
    }

    // FAB (bottom-right).
    egui::Area::new("niu-fab".into())
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-22.0, -44.0))
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(50.0, 50.0), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(16), theme.accent);
            let g = if ai_open {
                Glyph::Close
            } else {
                Glyph::Sparkle
            };
            icons::paint_at(ui, rect.shrink(13.0), g, Color32::WHITE);
            if resp.on_hover_text("AI Assistant").clicked() {
                msgs.push(OverlayMsg::ToggleAi);
            }
        });

    // Background task toast (bottom-left).
    if let Some(t) = task {
        egui::Area::new("niu-task".into())
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(22.0, -44.0))
            .show(ctx, |ui| task_toast(ui, theme, t, msgs));
    }
}

// ------------------------------------------------------------------- popup

fn popup_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    input: &mut String,
    msgs: &mut Vec<OverlayMsg>,
) {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(16.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if theme.dark { 120 } else { 40 }),
        })
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(360.0, 460.0));
            ui.set_max_size(egui::vec2(360.0, 460.0));
            ui.vertical(|ui| {
                // header
                egui::Frame::default()
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 16,
                        top: 12,
                        bottom: 12,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (b, _) = ui
                                .allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                            ui.painter().rect_filled(
                                b,
                                egui::CornerRadius::same(7),
                                theme.accent_tint,
                            );
                            icons::paint_at(ui, b.shrink(5.0), Glyph::Sparkle, theme.accent);
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("Assistant")
                                    .size(14.0)
                                    .strong()
                                    .color(theme.text),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if icbtn(ui, theme, Glyph::Close).clicked() {
                                        msgs.push(OverlayMsg::CloseAi);
                                    }
                                    if icbtn(ui, theme, Glyph::Expand)
                                        .on_hover_text("Open full tab")
                                        .clicked()
                                    {
                                        msgs.push(OverlayMsg::OpenAiTab);
                                    }
                                },
                            );
                        });
                    });
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1.0, theme.border_2),
                );

                // chat (scroll), fills the middle
                let cited = ai::grounding_entries(entries);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(330.0)
                    .show(ui, |ui| {
                        egui::Frame::default()
                            .inner_margin(egui::Margin::same(16))
                            .show(ui, |ui| {
                                ai::user_bubble(
                                    ui,
                                    theme,
                                    "Any SAE unlearning papers I haven't tagged yet?",
                                );
                                ui.add_space(16.0);
                                ai::assistant_row(ui, theme, |ui| {
                                    if cited.is_empty() {
                                        ui.label(
                                            RichText::new("Nothing in the library yet.")
                                                .size(13.5)
                                                .color(theme.text),
                                        );
                                        return;
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "{} match but could use ",
                                            cited.len()
                                        ))
                                        .size(13.5)
                                        .color(theme.text),
                                    );
                                    ui.label(
                                        RichText::new("wf:to-cite")
                                            .size(13.5)
                                            .strong()
                                            .color(theme.accent),
                                    );
                                    ui.add_space(6.0);
                                    for e in &cited {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("•").color(theme.muted));
                                            if ai::cite_chip(ui, theme, &ai::cite_label(e))
                                                .clicked()
                                            {
                                                msgs.push(OverlayMsg::OpenEntry(e.citekey.clone()));
                                            }
                                        });
                                    }
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("Want me to tag them?")
                                            .size(13.5)
                                            .color(theme.text),
                                    );
                                    ui.add_space(11.0);
                                    ui.horizontal_wrapped(|ui| {
                                        if widgets::button(
                                            ui,
                                            theme,
                                            Some(Glyph::Tag),
                                            "Tag both",
                                            false,
                                            28.0,
                                        )
                                        .clicked()
                                        {
                                            msgs.push(OverlayMsg::Toast(
                                                "Bulk-tag needs a connected model (preview)".into(),
                                            ));
                                        }
                                        if widgets::button(
                                            ui,
                                            theme,
                                            None,
                                            "Show in list",
                                            false,
                                            28.0,
                                        )
                                        .clicked()
                                        {
                                            msgs.push(OverlayMsg::OpenAiTab);
                                        }
                                    });
                                });
                            });
                    });

                // footer input
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1.0, theme.border_2),
                );
                egui::Frame::default()
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        egui::Frame::default()
                            .fill(theme.surface_2)
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin {
                                left: 13,
                                right: 6,
                                top: 6,
                                bottom: 6,
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(input)
                                            .hint_text("Ask across your library…")
                                            .desired_width(f32::INFINITY)
                                            .frame(false),
                                    );
                                    let (r, resp) = ui.allocate_exact_size(
                                        egui::vec2(32.0, 32.0),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        r,
                                        egui::CornerRadius::same(9),
                                        theme.accent,
                                    );
                                    icons::paint_at(ui, r.shrink(9.0), Glyph::Send, Color32::WHITE);
                                    if resp.clicked() && !input.trim().is_empty() {
                                        msgs.push(OverlayMsg::Toast(
                                            "No model connected yet (preview)".into(),
                                        ));
                                        input.clear();
                                    }
                                });
                            });
                    });
            });
        });
}

// -------------------------------------------------------------- task toast

fn task_toast(ui: &mut egui::Ui, theme: &Theme, t: &TaskState, msgs: &mut Vec<OverlayMsg>) {
    let now = ui.ctx().input(|i| i.time);
    let pct = t.progress(now);
    let done = pct >= 1.0;
    let count = (pct * t.total as f32).round() as usize;
    if !done {
        ui.ctx().request_repaint(); // keep the bar animating
    }

    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(14.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if theme.dark { 120 } else { 40 }),
        })
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            ui.set_max_width(320.0);
            // header
            ui.horizontal(|ui| {
                let (b, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                let (bg, fg, glyph) = if done {
                    (theme.accent_tint, theme.accent, Glyph::CheckCircle)
                } else {
                    (theme.surface_2, theme.text_2, Glyph::Sync)
                };
                ui.painter().rect_filled(b, egui::CornerRadius::same(8), bg);
                icons::paint_at(ui, b.shrink(6.0), glyph, fg);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    let title = if done { &t.done_label } else { &t.label };
                    ui.label(RichText::new(title).size(13.5).strong().color(theme.text));
                    let sub = if done {
                        format!("All {} entries processed.", t.total)
                    } else {
                        format!("{count} / {} entries", t.total)
                    };
                    ui.label(RichText::new(sub).size(11.5).color(theme.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if done {
                        if icbtn(ui, theme, Glyph::Close).clicked() {
                            msgs.push(OverlayMsg::DismissTask);
                        }
                    } else {
                        ui.label(
                            RichText::new(format!("{}%", (pct * 100.0).round() as i32))
                                .font(theme::mono(12.5))
                                .color(theme.text_2),
                        );
                    }
                });
            });
            ui.add_space(10.0);
            // progress bar
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(bar, egui::CornerRadius::same(3), theme.surface_2);
            let fill =
                egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * pct, bar.height()));
            ui.painter()
                .rect_filled(fill, egui::CornerRadius::same(3), theme.accent);
            ui.add_space(9.0);
            // actions
            if done {
                ui.horizontal(|ui| {
                    if widgets::button(ui, theme, None, "Review changes", true, 28.0).clicked() {
                        msgs.push(OverlayMsg::Review);
                    }
                    if widgets::button(ui, theme, None, "Dismiss", false, 28.0).clicked() {
                        msgs.push(OverlayMsg::DismissTask);
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{count} / {}", t.total))
                            .size(11.5)
                            .color(theme.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Run in background")
                                        .size(11.5)
                                        .color(theme.muted),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            msgs.push(OverlayMsg::DismissTask);
                        }
                    });
                });
            }
        });
}

/// A 28×28 transparent icon button (popup/toast chrome).
fn icbtn(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme.surface_2);
    }
    icons::paint_at(ui, rect.shrink(7.0), glyph, theme.muted);
    resp
}
