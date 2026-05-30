//! Shared chrome widgets used by the tool tabs (Normalize, AI, Settings): the
//! design's `SubNav`, `Segmented`, toggle switch, `.niu-btn` buttons, and the
//! `nzCard` surface. Ported faithfully from `app/shared.jsx` (spec §2).
//!
//! These are the bits the Library views don't need (the Library has its own
//! list/detail widgets), kept here so the three tabs share one implementation.

use eframe::egui::{self, Color32, RichText};

use crate::icons::{self, Glyph};
use crate::theme::Theme;

/// A vertical `SubNav` item (38px): optional icon · label · optional count
/// badge. Active = accent-tint fill + accent text. Returns whether clicked.
pub fn subnav_item(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Glyph,
    label: &str,
    badge: Option<usize>,
    active: bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
    if active || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(9),
            if active {
                theme.accent_tint
            } else {
                theme.surface_2
            },
        );
    }
    let fg = if active { theme.accent } else { theme.text_2 };
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0 + 8.5, rect.center().y),
            egui::vec2(17.0, 17.0),
        ),
        icon,
        fg,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 12.0 + 27.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        fg,
    );
    if let Some(n) = badge {
        ui.painter().text(
            rect.right_center() - egui::vec2(12.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            egui::FontId::proportional(11.0),
            fg,
        );
    }
    resp.clicked()
}

/// A horizontal segmented control. `options` are (label, optional leading icon);
/// returns the newly-clicked index, or `None`. `sm` uses the 26px height.
pub fn segmented(
    ui: &mut egui::Ui,
    theme: &Theme,
    options: &[(&str, Option<Glyph>)],
    selected: usize,
    sm: bool,
) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::default()
        .fill(theme.surface_2)
        .corner_radius(9.0)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, (label, icon)) in options.iter().enumerate() {
                    if seg_option(ui, theme, label, *icon, i == selected, sm) {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}

fn seg_option(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    icon: Option<Glyph>,
    on: bool,
    sm: bool,
) -> bool {
    let h = if sm { 26.0 } else { 30.0 };
    let pad = 12.0;
    let icon_w = if icon.is_some() { 21.0 } else { 0.0 };
    let text_w = label.len() as f32 * 7.2 + 4.0;
    let w = pad * 2.0 + icon_w + text_w;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), theme.surface);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), theme.surface_2);
    }
    let fg = if on { theme.accent } else { theme.text_2 };
    let mut x = rect.left() + pad;
    if let Some(g) = icon {
        icons::paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(x + 7.5, rect.center().y),
                egui::vec2(15.0, 15.0),
            ),
            g,
            fg,
        );
        x += icon_w;
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        fg,
    );
    resp.clicked()
}

/// A 42×25 toggle switch. Returns whether it was clicked (caller flips state).
pub fn toggle(ui: &mut egui::Ui, theme: &Theme, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(42.0, 25.0), egui::Sense::click());
    let track = if on { theme.accent } else { theme.faint };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), track);
    let knob_x = if on {
        rect.right() - 12.5
    } else {
        rect.left() + 12.5
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 9.5, Color32::WHITE);
    resp.clicked()
}

/// The shared `.niu-btn` / `.niu-btn.pri` button: optional leading icon + label.
/// `height` defaults to 32 when 0. Returns the `Response`.
pub fn button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Glyph>,
    label: &str,
    primary: bool,
    height: f32,
) -> egui::Response {
    let h = if height <= 0.0 { 32.0 } else { height };
    let fg = if primary { Color32::WHITE } else { theme.text };
    let fill = if primary { theme.accent } else { theme.surface };
    let resp = egui::Frame::default()
        .fill(fill)
        .stroke(egui::Stroke::new(
            1.0,
            if primary { theme.accent } else { theme.border },
        ))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(
            13,
            ((h - 16.0) * 0.5).round() as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                if let Some(g) = icon {
                    icons::show(ui, g, 16.0, fg);
                }
                ui.label(RichText::new(label).size(13.0).strong().color(fg));
            });
        });
    resp.response.interact(egui::Sense::click())
}

/// The shared `nzCard` surface frame (surface fill, hairline border, 14px radius,
/// soft shadow). Use as `card(theme).show(ui, |ui| { … })`.
pub fn card(theme: &Theme) -> egui::Frame {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(14.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 10,
            spread: 0,
            color: Color32::from_black_alpha(if theme.dark { 60 } else { 14 }),
        })
}

/// A card head row: leading icon + bold title (+ optional right-aligned note).
pub fn card_head(ui: &mut egui::Ui, theme: &Theme, icon: Glyph, title: &str, note: Option<&str>) {
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(20, 15))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, icon, 17.0, theme.accent);
                ui.add_space(2.0);
                ui.label(RichText::new(title).size(13.5).strong().color(theme.text));
                if let Some(n) = note {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(n).size(12.0).strong().color(theme.muted));
                    });
                }
            });
        });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.min_rect().bottom(),
        egui::Stroke::new(1.0, theme.border_2),
    );
}

/// A tab header: big serif-less title (h1) + optional subtitle, with a
/// right-aligned metadata line ("{lib} · {n} entries").
pub fn tab_header(ui: &mut egui::Ui, theme: &Theme, title: &str, subtitle: &str, meta: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(26.0).strong().color(theme.text));
            if !subtitle.is_empty() {
                ui.add_space(2.0);
                ui.label(RichText::new(subtitle).size(13.5).color(theme.muted));
            }
        });
        if !meta.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                ui.label(RichText::new(meta).size(13.0).color(theme.text_2));
            });
        }
    });
    ui.add_space(20.0);
}

/// Center a fixed-width content column (the tabs use a 880px reading column).
pub fn centered_column<R>(
    ui: &mut egui::Ui,
    width: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let col = width.min(ui.available_width());
    let pad = ((ui.available_width() - col) * 0.5).max(0.0);
    let mut out = None;
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_max_width(col);
            out = Some(add(ui));
        });
    });
    out.unwrap()
}
