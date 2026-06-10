//! Shared chrome widgets used by the tool tabs (Normalize, AI, Settings): the
//! design's `SubNav`, `Segmented`, toggle switch, `.niu-btn` buttons, and the
//! `nzCard` surface. Ported faithfully from `app/shared.jsx` (spec §2).
//!
//! These are the bits the Library views don't need (the Library has its own
//! list/detail widgets), kept here so the three tabs share one implementation.

use eframe::egui::{self, Color32, RichText};

use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};

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
            ((h - 16.0) * 0.5).round().clamp(0.0, 60.0) as i8,
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

/// A virtualized, **variable-height** vertical list, rendered inside the current
/// (already-scrolling) `ui`. Only items whose row intersects the viewport are
/// built; the rest just reserve their cached height, so a list of thousands of
/// rich cards costs ~one screenful of widgets per frame.
///
/// Unlike `ScrollArea::show_rows` (which needs a uniform row height), this
/// measures each item the first time it's on-screen and caches the height in
/// `heights` (caller-owned, persisted across frames). `estimate` seeds rows that
/// haven't been measured yet; `gap` is the space added below each item.
pub fn virtual_list(
    ui: &mut egui::Ui,
    count: usize,
    gap: f32,
    estimate: f32,
    heights: &mut Vec<f32>,
    mut item: impl FnMut(&mut egui::Ui, usize),
) {
    if heights.len() != count {
        // The list changed shape — reseed (heights re-measure as rows display).
        heights.clear();
        heights.resize(count, estimate);
    }
    let width = ui.available_width();
    let clip = ui.clip_rect();
    let margin = 64.0; // build slightly beyond the viewport to avoid edge popping
    for (i, slot) in heights.iter_mut().enumerate() {
        let h = *slot;
        let top = ui.cursor().min.y;
        if top + h < clip.top() - margin || top > clip.bottom() + margin {
            // Off-screen: just reserve the row's (cached) height.
            ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
        } else {
            // On-screen: build it, and refine the cached height from the actual.
            let measured = ui.scope(|ui| item(ui, i)).response.rect.height();
            if (measured - h).abs() > 1.0 {
                *slot = measured;
                ui.ctx().request_repaint(); // next frame positions/culls accurately
            }
        }
        ui.add_space(gap);
    }
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

// ------------------------------------------------- shared tiny helpers
// One canonical copy each of the small helpers that had drifted into several
// modules (app titlebar, overlays, tags, settings, library views); the copies
// differed only in dimensions, which are parameters here.

/// A transparent square icon button: `size`×`size`, glyph inset by `inset` on
/// every side, soft surface tint on hover (titlebar / modal / popup chrome).
pub fn icbtn(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: Glyph,
    size: f32,
    inset: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme.surface_2);
    }
    icons::paint_at(ui, rect.shrink(inset), glyph, theme.muted);
    resp
}

/// A primary pill button (accent fill, white icon + label).
pub fn pri_btn(ui: &mut egui::Ui, theme: &Theme, icon: Glyph, label: &str) -> egui::Response {
    egui::Frame::default()
        .fill(theme.accent)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, icon, 16.0, Color32::WHITE);
                ui.label(
                    RichText::new(label)
                        .size(13.0)
                        .strong()
                        .color(Color32::WHITE),
                );
            });
        })
        .response
        .interact(egui::Sense::click())
}

/// A `size`×`size` color swatch (8px radius); when `on`, a same-color ring is
/// drawn `ring` outside the rect. Returns whether clicked.
pub fn swatch(ui: &mut egui::Ui, c: Color32, on: bool, size: f32, ring: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), c);
    if on {
        ui.painter().rect_stroke(
            rect.expand(ring),
            egui::CornerRadius::same(10),
            egui::Stroke::new(2.0, c),
            egui::StrokeKind::Outside,
        );
    }
    resp.clicked()
}

/// An uppercase 11px section label, with `above`/`below` spacing around it.
pub fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str, above: f32, below: f32) {
    ui.add_space(above);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(below);
}

/// "y" / "ies" suffix for an entry count ("entr{}").
pub fn plural_y(n: usize) -> &'static str {
    if n == 1 {
        "y"
    } else {
        "ies"
    }
}

/// A bordered single-line text input (`niu-mono` when `mono`). Returns the edit
/// `Response` so the caller can persist on `lost_focus()`.
pub fn text_input(
    ui: &mut egui::Ui,
    theme: &Theme,
    buf: &mut String,
    mono: bool,
) -> egui::Response {
    framed_input(ui, theme, buf, mono, false, "")
}

/// A masked text input for secrets (API keys / tokens) — [`text_input`] with
/// the mono font and `.password(true)`.
pub fn password_input(
    ui: &mut egui::Ui,
    theme: &Theme,
    buf: &mut String,
    hint: &str,
) -> egui::Response {
    framed_input(ui, theme, buf, true, true, hint)
}

fn framed_input(
    ui: &mut egui::Ui,
    theme: &Theme,
    buf: &mut String,
    mono: bool,
    password: bool,
    hint: &str,
) -> egui::Response {
    let mut resp = None;
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(9.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(280.0);
            let mut te = egui::TextEdit::singleline(buf)
                .password(password)
                .hint_text(hint)
                .desired_width(f32::INFINITY)
                .frame(false);
            if mono {
                te = te.font(theme::mono(12.5));
            }
            resp = Some(ui.add(te));
        });
    resp.unwrap()
}
