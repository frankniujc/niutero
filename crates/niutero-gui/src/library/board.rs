//! Library — Board view (design spec §4·C): a kanban by reading status
//! (To Read / Reading / Read). Cards show venue, title, creator, tags and a
//! star-rating dot row; clicking a card opens a slide-in detail drawer (the
//! same lock + fields + Cite/BibTeX/link footer as Classic, reusing
//! [`super::detail_fields`] + [`super::detail_footer`]).
//!
//! Moving a card between columns is done by changing its reading status in the
//! drawer — the board reflows on the next reload. (The design's drag-between-
//! columns is not part of its control inventory.)

use eframe::egui::{self, Color32, RichText};
use niutero_engine::{EntryView, Status};

use super::{LibAction, LibState};
use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};

/// The three board columns: (status key, label, dot color selector).
const COLUMNS: [(&str, &str); 3] = [
    ("unread", "To Read"),
    ("reading", "Reading"),
    ("done", "Read"),
];

/// Fixed board-card geometry, so columns can be row-virtualized.
const CARD_H: f32 = 126.0;
const CARD_GAP: f32 = 11.0;

/// The reading [`Status`] a column's key maps to (for "add into this column").
fn status_of(key: &str) -> Status {
    match key {
        "reading" => Status::Reading,
        "done" => Status::Done,
        _ => Status::Unread,
    }
}

/// Render the Board view. Queued engine actions go into `actions`.
pub fn board(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // Drawer (right) is shown first so egui sizes the board area correctly.
    if st.drawer_open {
        let sel = st
            .selected
            .clone()
            .and_then(|k| entries.iter().find(|e| e.citekey == k));
        match sel {
            Some(e) => {
                egui::SidePanel::right("niu-board-drawer")
                    .exact_width(400.0)
                    .resizable(false)
                    .frame(egui::Frame::default().fill(theme.surface))
                    .show(ctx, |ui| drawer(ui, theme, e, st, actions));
            }
            None => st.drawer_open = false,
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            header_bar(ui, theme, entries, st, actions);
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.min_rect().bottom(),
                egui::Stroke::new(1.0, theme.border),
            );
            if st.board_grid {
                columns(ui, theme, entries, st, actions);
            } else {
                list_view(ui, theme, entries, st, actions);
            }
        });
}

// ----------------------------------------------------------------- header bar

fn header_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 22,
            right: 22,
            top: 13,
            bottom: 13,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Library")
                        .size(17.0)
                        .strong()
                        .color(theme.text),
                );
                // count badge
                egui::Frame::default()
                    .fill(theme.surface_2)
                    .corner_radius(20.0)
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(entries.len().to_string())
                                .size(12.0)
                                .strong()
                                .color(theme.muted),
                        );
                    });
                ui.add_space(8.0);
                // search box (max ~300)
                egui::Frame::default()
                    .fill(theme.surface_2)
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(300.0);
                        ui.horizontal(|ui| {
                            icons::show(ui, Glyph::Search, 16.0, theme.muted);
                            ui.add(
                                egui::TextEdit::singleline(&mut st.search)
                                    .hint_text("Search & filter")
                                    .desired_width(220.0)
                                    .frame(false),
                            );
                        });
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Add (primary) → the shared new-entry dialog.
                    if pri_btn(ui, theme, Glyph::Plus, "Add")
                        .on_hover_text("New entry")
                        .clicked()
                    {
                        actions.push(LibAction::NewEntry(None));
                    }
                    ui.add_space(6.0);
                    // Layout toggle: kanban grid ↔ single-column list.
                    let grid = st.board_grid;
                    egui::Frame::default()
                        .fill(theme.surface_2)
                        .corner_radius(9.0)
                        .inner_margin(3)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if seg_icon(ui, theme, Glyph::Rows, !grid)
                                    .on_hover_text("List view")
                                    .clicked()
                                {
                                    st.board_grid = false;
                                }
                                if seg_icon(ui, theme, Glyph::Grid, grid)
                                    .on_hover_text("Board view")
                                    .clicked()
                                {
                                    st.board_grid = true;
                                }
                            });
                        });
                });
            });
        });
}

// -------------------------------------------------------------------- columns

fn columns(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // Search scan cached; the per-column status split is a cheap live filter.
    super::ensure_search_cache(st, entries);
    let cards: Vec<Vec<&EntryView>> = COLUMNS
        .iter()
        .map(|(key, _)| {
            st.search_cache
                .iter()
                .map(|&i| &entries[i])
                .filter(|e| &e.status == key)
                .collect()
        })
        .collect();

    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(22, 18))
        .show(ui, |ui| {
            ui.columns(3, |cols| {
                for (i, (key, label)) in COLUMNS.iter().enumerate() {
                    column(&mut cols[i], theme, key, label, &cards[i], st, actions);
                }
            });
        });
}

fn column(
    ui: &mut egui::Ui,
    theme: &Theme,
    key: &str,
    label: &str,
    cards: &[&EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // header: dot + label + count + "+"
    ui.horizontal(|ui| {
        let dot = ui
            .allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover())
            .0;
        ui.painter()
            .circle_filled(dot.center(), 4.5, super::status_color(theme, key));
        ui.label(RichText::new(label).size(13.5).strong().color(theme.text));
        ui.label(
            RichText::new(cards.len().to_string())
                .size(12.0)
                .color(theme.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if super::icon_btn(ui, theme, Glyph::Plus, false)
                .on_hover_text("Add paper here")
                .clicked()
            {
                actions.push(LibAction::NewEntry(Some(status_of(key))));
            }
        });
    });
    ui.add_space(8.0);

    // Virtualized column: only visible cards build per frame. The trailing
    // "+ Add paper" affordance is rendered as one extra virtual row.
    let row_h = CARD_H + CARD_GAP;
    let n = cards.len();
    egui::ScrollArea::vertical()
        .id_salt(("niu-board-col", key))
        .auto_shrink([false, false])
        .show_rows(ui, row_h, n + 1, |ui, range| {
            for i in range {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Sense::hover(),
                );
                if i == n {
                    if add_paper(ui, theme, rect) {
                        actions.push(LibAction::NewEntry(Some(status_of(key))));
                    }
                    continue;
                }
                let e = cards[i];
                let sel = st.selected.as_deref() == Some(&e.citekey) && st.drawer_open;
                let card_rect =
                    egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), CARD_H));
                let mut card_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(card_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                if card(&mut card_ui, theme, e, sel) {
                    st.selected = Some(e.citekey.clone());
                    st.drawer_open = true;
                    st.buffers_for = None;
                }
            }
        });
}

/// The Board's list layout (the Rows toggle): a single virtualized column of
/// every matching entry, full-width, same cards as the grid.
fn list_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    super::ensure_search_cache(st, entries);
    let shown: Vec<usize> = st.search_cache.clone();
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(22, 16))
        .show(ui, |ui| {
            if shown.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No matching entries").color(theme.muted));
                });
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pri_btn(ui, theme, Glyph::Plus, "Add")
                    .on_hover_text("New entry")
                    .clicked()
                {
                    actions.push(LibAction::NewEntry(None));
                }
            });
            ui.add_space(8.0);
            let row_h = CARD_H + CARD_GAP;
            egui::ScrollArea::vertical()
                .id_salt("niu-board-list")
                .auto_shrink([false, false])
                .show_rows(ui, row_h, shown.len(), |ui, range| {
                    let w = ui.available_width().min(720.0);
                    for i in range {
                        let e = &entries[shown[i]];
                        let sel = st.selected.as_deref() == Some(&e.citekey) && st.drawer_open;
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_h),
                            egui::Sense::hover(),
                        );
                        let card_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, CARD_H));
                        let mut card_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(card_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        if card(&mut card_ui, theme, e, sel) {
                            st.selected = Some(e.citekey.clone());
                            st.drawer_open = true;
                            st.buffers_for = None;
                        }
                    }
                });
        });
}

/// One board card. Returns whether it was clicked.
fn card(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, selected: bool) -> bool {
    let inner = egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { theme.accent } else { theme.border },
        ))
        .corner_radius(13.0)
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 13,
            bottom: 13,
        })
        .show(ui, |ui| {
            // header: venue badge + pdf indicator
            ui.horizontal(|ui| {
                let (g, gc) = super::type_glyph(theme, &e.entry_type);
                icons::show(ui, g, 13.0, gc);
                let venue = venue_short(e);
                let yy = e
                    .fields
                    .get("year")
                    .map(|y| {
                        y.chars()
                            .rev()
                            .take(2)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let label = if yy.is_empty() {
                    venue
                } else {
                    format!("{venue} '{yy}")
                };
                ui.label(
                    RichText::new(label.to_uppercase())
                        .size(10.5)
                        .strong()
                        .color(gc),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (pg, pc) = if super::has_pdf(e) {
                        (Glyph::Attach, theme.accent)
                    } else {
                        (Glyph::Doc, theme.faint)
                    };
                    icons::show(ui, pg, 15.0, pc);
                });
            });
            ui.add_space(7.0);
            // title (serif, single line so the card height is fixed)
            let title = crate::tex::display(
                e.fields
                    .get("title")
                    .map(String::as_str)
                    .unwrap_or("(untitled)"),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(super::ellipsize(&title, 72))
                        .font(theme::serif(16.5))
                        .color(theme.text),
                )
                .truncate(),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(super::creator_of(e))
                    .size(12.5)
                    .color(theme.text_2),
            );
            ui.add_space(9.0);
            // footer: first 3 tag chips + stars
            ui.horizontal_wrapped(|ui| {
                for t in e.tags.iter().take(3) {
                    tag_chip(ui, theme, t);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let stars = e.stars.unwrap_or(0).min(5);
                    for _ in 0..stars {
                        let d = ui
                            .allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover())
                            .0;
                        ui.painter().circle_filled(d.center(), 2.5, theme.accent);
                    }
                });
            });
        });

    let r = ui.interact(
        inner.response.rect,
        ui.id().with(("board-card", &e.citekey)),
        egui::Sense::click(),
    );
    r.clicked()
}

/// The dashed "+ Add paper" affordance, painted into the top of `row_rect`.
/// Returns whether it was clicked.
fn add_paper(ui: &mut egui::Ui, theme: &Theme, row_rect: egui::Rect) -> bool {
    let rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(row_rect.width(), 40.0));
    let resp = ui
        .interact(rect, ui.id().with("board-add-paper"), egui::Sense::click())
        .on_hover_text("Add paper here");
    let stroke = egui::Stroke::new(
        1.0,
        if resp.hovered() {
            theme.faint
        } else {
            theme.border
        },
    );
    // dashed-ish border (egui has no dash; a thin solid hairline reads close enough)
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(11),
        stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+  Add paper",
        egui::FontId::proportional(12.5),
        theme.muted,
    );
    resp.clicked()
}

// --------------------------------------------------------------------- drawer

fn drawer(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    super::ensure_buffers(st, e);
    let mut close = false;

    // Pinned header: type pill · lock · close (border-bottom from the panel).
    egui::TopBottomPanel::top("niu-drawer-header")
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin::symmetric(16, 12)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                super::type_badge(ui, theme, e);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::icon_btn(ui, theme, Glyph::Close, false)
                        .on_hover_text("Close")
                        .clicked()
                    {
                        close = true;
                    }
                    let locked = st.locked;
                    if super::lock_toggle(ui, theme, locked).clicked() {
                        st.locked = !locked;
                        st.buffers_for = None;
                    }
                });
            });
        });

    // Pinned footer: Cite / link / BibTeX.
    egui::TopBottomPanel::bottom("niu-drawer-footer")
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 4,
                    bottom: 14,
                }),
        )
        .show_inside(ui, |ui| super::detail_footer(ui, theme, e, actions));

    // Scrollable fields (with the reading row, since the Board is status-centric).
    egui::CentralPanel::default()
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 14,
                    bottom: 8,
                }),
        )
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    super::detail_fields(ui, theme, e, st, actions, true)
                });
        });

    if close {
        st.drawer_open = false;
    }
}

// ------------------------------------------------------------- tiny widgets

fn tag_chip(ui: &mut egui::Ui, theme: &Theme, tag: &str) {
    let value = tag.rsplit(':').next().unwrap_or(tag);
    egui::Frame::default()
        .fill(theme.surface_2)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let dot = ui
                    .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                    .0;
                ui.painter()
                    .rect_filled(dot, egui::CornerRadius::same(2), super::tag_color(tag));
                ui.label(RichText::new(value).size(11.0).color(theme.text_2));
            });
        });
}

/// A primary pill button (icon + label).
fn pri_btn(ui: &mut egui::Ui, theme: &Theme, icon: Glyph, label: &str) -> egui::Response {
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

/// A 30×28 segmented icon cell (grid/rows layout toggle).
fn seg_icon(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), theme.surface);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), theme.surface_2);
    }
    let color = if on { theme.accent } else { theme.muted };
    icons::paint_at(ui, rect.shrink(7.0), glyph, color);
    resp
}

/// Short venue label (leading token of journal/booktitle, else type family).
fn venue_short(e: &EntryView) -> String {
    e.fields
        .get("journal")
        .or_else(|| e.fields.get("booktitle"))
        .map(|v| {
            let v = crate::tex::display(v);
            v.split([' ', ',']).next().unwrap_or(&v).to_string()
        })
        .unwrap_or_else(|| match e.entry_type.as_str() {
            "inproceedings" | "conference" => "CONF".into(),
            "article" => "JOURNAL".into(),
            _ => "PREPRINT".into(),
        })
}
