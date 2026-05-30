//! Library — Reader view (design spec §4·B): a tag-first sidebar, a card list,
//! and a dominant, reading-first pane (a centered 720px column with a PDF-page
//! preview and the abstract in serif). Both side panels collapse for a
//! full-width read.
//!
//! Shares Classic's entry-formatting, tag, and edit helpers via `super::`; only
//! the card list and the reading pane are bespoke to this layout.

use eframe::egui::{self, Color32, RichText};
use niutero_engine::EntryView;

use super::{LibAction, LibState};
use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};

/// Render the Reader view. Queued engine actions go into `actions`.
pub fn reader(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    if st.selected.is_none() {
        st.selected = entries.first().map(|e| e.citekey.clone());
    }

    // Responsive: the reading pane is the priority, so collapse the list first,
    // then the tags, when the window can't fit all three comfortably.
    let avail = ctx.content_rect().width() - 60.0; // minus the rail
    let hide_tags = st.hide_tags || avail < 1080.0;
    let hide_list = st.hide_list || avail < 740.0;

    if !hide_tags {
        egui::SidePanel::left("niu-reader-tags")
            .exact_width(210.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 10,
                        top: 14,
                        bottom: 12,
                    }),
            )
            .show(ctx, |ui| tags_sidebar(ui, theme, entries, st));
    }

    if !hide_list {
        egui::SidePanel::left("niu-reader-list")
            .exact_width(340.0)
            .resizable(false)
            .frame(egui::Frame::default().fill(theme.bg))
            .show(ctx, |ui| card_list(ui, theme, entries, st));
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            let sel = st
                .selected
                .as_ref()
                .and_then(|k| entries.iter().find(|e| &e.citekey == k));
            match sel {
                Some(e) => reading_pane(ui, theme, e, st, actions),
                None => {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No entry selected").color(theme.muted));
                    });
                }
            }
        });
}

// --------------------------------------------------------------- tags sidebar

fn tags_sidebar(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // "All Entries" — clears the tag + reading-status filters.
            let all_on = st.active_tag.is_none() && st.reading_filter.is_none();
            if nav_row(
                ui,
                theme,
                Some(Glyph::Library),
                "All Entries",
                all_on,
                entries.len(),
            ) {
                st.active_tag = None;
                st.reading_filter = None;
            }
            ui.add_space(10.0);

            for (label, tags) in super::tag_groups(entries) {
                section_label(ui, theme, &label);
                for (full, value, count, color) in tags {
                    let on = st.active_tag.as_deref() == Some(full.as_str());
                    if tag_row(ui, theme, &value, color, on, count) {
                        st.active_tag = if on { None } else { Some(full.clone()) };
                        st.reading_filter = None;
                    }
                }
                ui.add_space(8.0);
            }

            // Reading status — display + filter (Reader-only section, spec §4·B).
            section_label(ui, theme, "Reading status");
            for (label, key) in [
                ("Unread", "unread"),
                ("Reading", "reading"),
                ("Read", "done"),
            ] {
                let count = entries.iter().filter(|e| e.status == key).count();
                let on = st.reading_filter.as_deref() == Some(key);
                let dot = super::status_color(theme, key);
                if tag_row(ui, theme, label, dot, on, count) {
                    st.reading_filter = if on { None } else { Some(key.to_string()) };
                    st.active_tag = None;
                }
            }
        });
}

fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(2.0);
}

/// A 34px nav row with an optional leading icon (used for "All Entries").
fn nav_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Glyph>,
    label: &str,
    on: bool,
    count: usize,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if on { theme.sel } else { theme.surface_2 },
        );
    }
    let mut x = rect.left() + 10.0;
    if let Some(g) = icon {
        icons::paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(x + 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            g,
            theme.accent,
        );
        x += 25.0;
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if on { theme.text } else { theme.text_2 },
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    resp.clicked()
}

/// A 30px tag/status row: colored dot · value · count. Returns whether clicked.
fn tag_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &str,
    dot: Color32,
    on: bool,
    count: usize,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if on { theme.sel } else { theme.surface_2 },
        );
    }
    let dc = rect.left_center() + egui::vec2(14.0, 0.0);
    ui.painter().rect_filled(
        egui::Rect::from_center_size(dc, egui::vec2(8.0, 8.0)),
        egui::CornerRadius::same(3),
        dot,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(26.0, 0.0),
        egui::Align2::LEFT_CENTER,
        value,
        egui::FontId::proportional(13.0),
        if on { theme.text } else { theme.text_2 },
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    resp.clicked()
}

// ----------------------------------------------------------------- card list

fn card_list(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    // Header: search box + a status line ("N items · sorted by date added").
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 10,
        })
        .show(ui, |ui| {
            egui::Frame::default()
                .fill(theme.surface_2)
                .corner_radius(9.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::show(ui, Glyph::Search, 16.0, theme.muted);
                        ui.add(
                            egui::TextEdit::singleline(&mut st.search)
                                .hint_text("Search the library")
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                    });
                });
        });

    let q = st.search.to_lowercase();
    let shown: Vec<&EntryView> = entries.iter().filter(|e| reader_match(e, st, &q)).collect();

    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 0,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} items · sorted by date added", shown.len()))
                        .size(12.5)
                        .color(theme.muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ =
                        super::icon_btn(ui, theme, Glyph::Filter, false).on_hover_text("Filter");
                });
            });
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::default()
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 0,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    if shown.is_empty() {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("No matching entries").color(theme.muted));
                        });
                    }
                    for e in shown {
                        let sel = st.selected.as_deref() == Some(&e.citekey);
                        if item_card(ui, theme, e, sel) {
                            st.selected = Some(e.citekey.clone());
                            st.buffers_for = None; // reload edit buffers for the new pick
                        }
                    }
                });
        });
}

fn reader_match(e: &EntryView, st: &LibState, q: &str) -> bool {
    st.active_tag.as_ref().is_none_or(|t| e.tags.contains(t))
        && st.reading_filter.as_ref().is_none_or(|s| &e.status == s)
        && super::matches_search(e, q)
}

/// A paper card: type badge · status · 2-line serif title · creator.
fn item_card(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, selected: bool) -> bool {
    let resp = egui::Frame::default()
        .fill(if selected {
            theme.accent_tint
        } else {
            theme.surface
        })
        .stroke(egui::Stroke::new(
            1.0,
            if selected { theme.accent } else { theme.border },
        ))
        .corner_radius(11.0)
        .inner_margin(egui::Margin {
            left: 13,
            right: 13,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // type badge (left)
                let (g, gc) = super::type_glyph(theme, &e.entry_type);
                icons::show(ui, g, 13.0, gc);
                let venue = venue_short(e);
                let year = e.fields.get("year").map(String::as_str).unwrap_or("");
                ui.label(
                    RichText::new(format!("{venue} {year}").trim().to_uppercase())
                        .size(10.5)
                        .strong()
                        .color(gc),
                );
                // status (right)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sc = super::status_color(theme, &e.status);
                    ui.label(
                        RichText::new(status_label(&e.status))
                            .size(10.5)
                            .strong()
                            .color(sc),
                    );
                    let dot = ui
                        .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                        .0;
                    ui.painter().circle_filled(dot.center(), 3.0, sc);
                });
            });
            ui.add_space(5.0);
            let title = crate::tex::display(
                e.fields
                    .get("title")
                    .map(String::as_str)
                    .unwrap_or("(untitled)"),
            );
            ui.label(
                RichText::new(super::ellipsize(&title, 110))
                    .font(theme::serif(16.0))
                    .color(theme.text),
            );
            ui.add_space(5.0);
            ui.label(
                RichText::new(super::creator_of(e))
                    .size(12.5)
                    .color(theme.text_2),
            );
        });
    let r = ui.interact(
        resp.response.rect,
        ui.id().with(("reader-card", &e.citekey)),
        egui::Sense::click(),
    );
    ui.add_space(8.0);
    r.clicked()
}

// ---------------------------------------------------------------- reading pane

fn reading_pane(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    super::ensure_buffers(st, e);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Center a 720px reading column with the spec's 34/44/48 padding.
            // (Centering is done by `centered_column` via add_space, not by an
            // i8 inner_margin — a wide window's pad would overflow i8.)
            crate::widgets::centered_column(ui, 720.0, |ui| {
                egui::Frame::default()
                    .inner_margin(egui::Margin {
                        left: 44,
                        right: 44,
                        top: 34,
                        bottom: 48,
                    })
                    .show(ui, |ui| {
                        reading_column(ui, theme, e, st, actions);
                    });
            });
        });
}

fn reading_column(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    let locked = st.locked;

    // ---- header bar: panel toggles · badges · lock/star/more
    ui.horizontal(|ui| {
        if super::icon_btn(ui, theme, Glyph::PanelLeft, st.hide_tags)
            .on_hover_text(if st.hide_tags {
                "Show tags panel"
            } else {
                "Hide tags panel"
            })
            .clicked()
        {
            st.hide_tags = !st.hide_tags;
        }
        if super::icon_btn(ui, theme, Glyph::Rows, st.hide_list)
            .on_hover_text(if st.hide_list {
                "Show list"
            } else {
                "Hide list"
            })
            .clicked()
        {
            st.hide_list = !st.hide_list;
        }
        // divider
        let (dr, _) = ui.allocate_exact_size(egui::vec2(9.0, 20.0), egui::Sense::hover());
        ui.painter().vline(
            dr.center().x,
            (dr.center().y - 9.0)..=(dr.center().y + 9.0),
            egui::Stroke::new(1.0, theme.border),
        );
        // type badge (pill)
        super::type_badge(ui, theme, e);
        // status indicator
        let sc = super::status_color(theme, &e.status);
        let dot = ui
            .allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover())
            .0;
        ui.painter().circle_filled(dot.center(), 3.5, sc);
        ui.label(
            RichText::new(status_label(&e.status))
                .size(12.0)
                .strong()
                .color(sc),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // more (overflow) — copy actions
            ui.menu_button(RichText::new("⋯").size(15.0).color(theme.muted), |ui| {
                if ui.button("Copy citation key").clicked() {
                    ui.ctx().copy_text(e.citekey.clone());
                    ui.close();
                }
                if ui.button("Copy citation").clicked() {
                    actions.push(LibAction::Cite);
                    ui.close();
                }
                if ui.button("Copy BibTeX").clicked() {
                    actions.push(LibAction::Bibtex);
                    ui.close();
                }
            });
            // star (favorite) — toggles a 5-star rating on/off
            let starred = e.stars.unwrap_or(0) > 0;
            let (sg, scol) = if starred {
                (Glyph::StarFilled, theme.amber)
            } else {
                (Glyph::Star, theme.muted)
            };
            if super::icon_btn_colored(ui, theme, sg, scol, false)
                .on_hover_text(if starred { "Unfavorite" } else { "Favorite" })
                .clicked()
            {
                actions.push(LibAction::SetStars(if starred { None } else { Some(5) }));
            }
            // lock
            let lg = if locked { Glyph::Lock } else { Glyph::Unlock };
            let lc = if locked { theme.muted } else { theme.accent };
            if super::icon_btn_colored(ui, theme, lg, lc, !locked)
                .on_hover_text(if locked {
                    "Locked — click to edit"
                } else {
                    "Editing — click to lock"
                })
                .clicked()
            {
                st.locked = !st.locked;
                st.buffers_for = None;
            }
            ui.label(
                RichText::new(if locked { "Locked" } else { "Editing" })
                    .size(11.0)
                    .strong()
                    .color(theme.muted),
            );
        });
    });
    ui.add_space(14.0);

    // ---- title (serif, large; editable when unlocked)
    if locked {
        let title = e
            .fields
            .get("title")
            .map(String::as_str)
            .unwrap_or("(untitled)");
        crate::tex::runs_label(ui, title, theme::serif(33.0), theme.text);
    } else {
        let buf = st.buffers.entry("title".into()).or_default();
        let r = ui.add(
            egui::TextEdit::multiline(buf)
                .font(theme::serif(24.0))
                .desired_width(f32::INFINITY)
                .desired_rows(2),
        );
        if r.lost_focus() {
            actions.push(LibAction::Edit("title".into(), st.buffers["title"].clone()));
        }
    }
    ui.add_space(8.0);

    // ---- authors (compact line; raw edit when unlocked)
    if locked {
        ui.label(
            RichText::new(super::authors_line(e))
                .size(15.0)
                .color(theme.text_2),
        );
    } else {
        super::edit_field_raw(ui, theme, st, actions, "author", false);
    }
    ui.add_space(4.0);

    // ---- venue · year
    let venue = crate::tex::display(
        e.fields
            .get("journal")
            .or_else(|| e.fields.get("booktitle"))
            .map(String::as_str)
            .unwrap_or(""),
    );
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    let vy = [venue.as_str(), year]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  ·  ");
    if !vy.is_empty() {
        ui.label(RichText::new(vy).size(13.5).color(theme.muted));
    }
    ui.add_space(18.0);

    // ---- action buttons
    ui.horizontal(|ui| {
        let has_pdf = super::has_pdf(e);
        if action_btn(ui, theme, Some(Glyph::Book), "Open PDF", true, has_pdf).clicked() && has_pdf
        {
            actions.push(LibAction::OpenPdf);
        }
        if action_btn(ui, theme, Some(Glyph::Quote), "Cite", false, true).clicked() {
            actions.push(LibAction::Cite);
        }
        if action_btn(ui, theme, None, "Copy BibTeX", false, true).clicked() {
            actions.push(LibAction::Bibtex);
        }
        if let Some(url) = e.fields.get("url").filter(|u| !u.is_empty()) {
            if action_btn(ui, theme, Some(Glyph::Link), "Source", false, true).clicked() {
                actions.push(LibAction::OpenUrl(url.clone()));
            }
        }
    });
    ui.add_space(22.0);

    // ---- PDF preview + abstract
    ui.horizontal_top(|ui| {
        let (thumb, _) = ui.allocate_exact_size(egui::vec2(168.0, 218.0), egui::Sense::hover());
        pdf_thumb(ui, theme, thumb);
        ui.add_space(22.0);
        ui.vertical(|ui| {
            super::meta_label(ui, theme, "Abstract");
            if locked {
                let abs = e.fields.get("abstract").map(String::as_str).unwrap_or("—");
                crate::tex::runs_label(ui, abs, theme::serif(15.0), theme.text);
            } else {
                let buf = st.buffers.entry("abstract".into()).or_default();
                let r = ui.add(
                    egui::TextEdit::multiline(buf)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                );
                if r.lost_focus() {
                    actions.push(LibAction::Edit(
                        "abstract".into(),
                        st.buffers["abstract"].clone(),
                    ));
                }
            }
        });
    });
    ui.add_space(22.0);
    ui.separator();
    ui.add_space(14.0);

    // ---- metadata grid (2 columns)
    ui.columns(2, |cols| {
        meta_cell(&mut cols[0], theme, "Citation key", &e.citekey, true);
        let doi = e.fields.get("doi").map(String::as_str).unwrap_or("—");
        meta_cell(&mut cols[1], theme, "DOI", doi, true);
    });
    ui.add_space(12.0);
    ui.columns(2, |cols| {
        let added = e.added.as_deref().unwrap_or("—");
        meta_cell(&mut cols[0], theme, "Added", added, false);
        meta_cell(
            &mut cols[1],
            theme,
            "Type",
            super::type_label(&e.entry_type),
            false,
        );
    });
    ui.add_space(24.0);

    // ---- tags
    super::meta_label(ui, theme, "Tags");
    super::tags_editor(ui, theme, e, locked, st, actions);
}

// -------------------------------------------------------------- pane helpers

/// One reader action button (.niu-btn / .niu-btn.pri). `enabled=false` dims it.
fn action_btn(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Glyph>,
    label: &str,
    primary: bool,
    enabled: bool,
) -> egui::Response {
    let fill = if primary { theme.accent } else { theme.surface };
    let txt_col = if primary { Color32::WHITE } else { theme.text };
    let (col, tcol) = if enabled {
        (fill, txt_col)
    } else {
        (theme.surface_2, theme.faint)
    };
    egui::Frame::default()
        .fill(col)
        .stroke(egui::Stroke::new(
            1.0,
            if primary { theme.accent } else { theme.border },
        ))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(13, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(g) = icon {
                    icons::show(ui, g, 16.0, tcol);
                }
                ui.label(RichText::new(label).size(13.0).strong().color(tcol));
            });
        })
        .response
        .interact(egui::Sense::click())
}

/// A striped "PDF page" placeholder (the design's decorative thumbnail).
fn pdf_thumb(ui: &egui::Ui, theme: &Theme, rect: egui::Rect) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, egui::CornerRadius::same(8), theme.surface_2);
    p.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    // Diagonal hatch (135°), 9px pitch (spec's striped gradient), clipped.
    let step = 9.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        p.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x + rect.height(), rect.top()),
            ],
            egui::Stroke::new(1.0, theme.border_2),
        );
        x += step;
    }
    // mono filename chip at the bottom-center
    let chip = egui::Rect::from_center_size(
        rect.center_bottom() - egui::vec2(0.0, 16.0),
        egui::vec2(102.0, 18.0),
    );
    p.rect_filled(chip, egui::CornerRadius::same(5), theme.bg);
    p.text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        "pdf-page-1.png",
        theme::mono(10.0),
        theme.muted,
    );
}

/// A metadata grid cell: uppercase label + value (mono+accent, or plain).
fn meta_cell(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str, mono: bool) {
    super::meta_label(ui, theme, label);
    if mono {
        ui.label(
            RichText::new(value)
                .font(theme::mono(12.5))
                .color(theme.accent),
        );
    } else {
        ui.label(RichText::new(value).size(14.0).color(theme.text));
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "done" => "Read",
        "reading" => "Reading",
        _ => "Unread",
    }
}

/// Short venue label (the journal/booktitle's leading token, upper-cased by the
/// caller). Falls back to the entry type's family when no venue field exists.
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
