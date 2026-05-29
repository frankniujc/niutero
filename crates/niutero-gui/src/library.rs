//! Library — Classic 3-pane view (design spec §4·A): a tag-first sidebar, the
//! item list, and a lock-guarded editable detail panel.
//!
//! The view renders from the loaded `Vec<EntryView>` and mutates cheap UI state
//! (`LibState`) in place. Anything that touches the engine (edits, status/stars,
//! tags, clipboard, links) is returned as a [`LibAction`] for the app to apply
//! after rendering — this keeps the immutable library borrow and the mutable
//! engine call from overlapping.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText};
use niutero_engine::{EntryView, Status};

use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};

/// Cheap, view-local UI state (no engine data).
pub struct LibState {
    pub selected: Option<String>,
    pub active_tag: Option<String>,
    pub search: String,
    /// Detail panel lock — editable only when `false`. Locked by default.
    pub locked: bool,
    pub hide_tags: bool,
    pub hide_detail: bool,
    /// Edit buffers for the selected entry's text fields, valid while unlocked.
    buffers: BTreeMap<String, String>,
    buffers_for: Option<String>,
    /// Inline "add tag" input state.
    adding_tag: bool,
    new_tag: String,
}

impl LibState {
    /// Invalidate the detail edit buffers so they reload from the (possibly
    /// just-edited) entry on the next frame.
    pub fn refresh(&mut self) {
        self.buffers_for = None;
    }
}

impl Default for LibState {
    fn default() -> Self {
        LibState {
            selected: None,
            active_tag: None,
            search: String::new(),
            locked: true,
            hide_tags: false,
            hide_detail: false,
            buffers: BTreeMap::new(),
            buffers_for: None,
            adding_tag: false,
            new_tag: String::new(),
        }
    }
}

/// An engine-touching action the view requests; the app applies it post-render.
pub enum LibAction {
    /// `engine::edit` the selected entry: set `field` to `value` (empty unsets).
    Edit(String, String),
    SetStatus(Status),
    SetStars(Option<u8>),
    AddTag(String),
    RemoveTag(String),
    OpenUrl(String),
    Cite,
    Bibtex,
}

/// Render the Classic view. Returns nothing; queued engine actions go into
/// `actions`. `entries` is already filtered? No — we filter here from the full set.
pub fn classic(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // Default selection: first entry.
    if st.selected.is_none() {
        st.selected = entries.first().map(|e| e.citekey.clone());
    }

    // ---- Tags sidebar (left) -------------------------------------------------
    if !st.hide_tags {
        egui::SidePanel::left("niu-tags")
            .exact_width(232.0)
            .resizable(false)
            .frame(panel_frame(theme).inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 14,
                bottom: 12,
            }))
            .show_inside(ui, |ui| tags_sidebar(ui, theme, entries, st));
    }

    // ---- Detail panel (right) -----------------------------------------------
    if !st.hide_detail {
        egui::SidePanel::right("niu-detail")
            .exact_width(392.0)
            .resizable(false)
            .frame(panel_frame(theme).inner_margin(egui::Margin::same(20)))
            .show_inside(ui, |ui| {
                let sel = st
                    .selected
                    .as_ref()
                    .and_then(|k| entries.iter().find(|e| &e.citekey == k));
                match sel {
                    Some(e) => detail_panel(ui, theme, e, st, actions),
                    None => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("No entry selected").color(theme.muted));
                        });
                    }
                }
            });
    }

    // ---- Item list (center) --------------------------------------------------
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show_inside(ui, |ui| item_list(ui, theme, entries, st));
}

// ----------------------------------------------------------------- sidebar

fn tags_sidebar(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    // "All Entries" clears the tag filter.
    let all_on = st.active_tag.is_none();
    if row_button(ui, theme, "All Entries", all_on, entries.len()).clicked() {
        st.active_tag = None;
    }
    ui.add_space(10.0);

    for (label, tags) in tag_groups(entries) {
        ui.label(
            RichText::new(label.to_uppercase())
                .size(11.0)
                .strong()
                .color(theme.muted),
        );
        ui.add_space(2.0);
        for (full, value, count, color) in tags {
            let on = st.active_tag.as_deref() == Some(full.as_str());
            let (rect, resp) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
            if on || resp.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(8),
                    if on { theme.sel } else { theme.surface_2 },
                );
            }
            let dot = rect.left_center() + egui::vec2(14.0, 0.0);
            ui.painter().rect_filled(
                egui::Rect::from_center_size(dot, egui::vec2(8.0, 8.0)),
                egui::CornerRadius::same(3),
                color,
            );
            ui.painter().text(
                rect.left_center() + egui::vec2(26.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &value,
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
            if resp.clicked() {
                st.active_tag = if on { None } else { Some(full.clone()) };
            }
        }
        ui.add_space(8.0);
    }
}

/// One tag in the tree: (full `ns:value`, display value, count, dot color).
type TagEntry = (String, String, usize, Color32);
/// A namespace group: (display label, its tags).
type TagGroup = (String, Vec<TagEntry>);

/// Group the library's distinct tags by namespace (`topics:` → "Topics", …).
fn tag_groups(entries: &[EntryView]) -> Vec<TagGroup> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in entries {
        for t in &e.tags {
            *counts.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let mut groups: BTreeMap<String, Vec<TagEntry>> = BTreeMap::new();
    for (full, count) in counts {
        let (ns, value) = match full.split_once(':') {
            Some((ns, v)) => (ns.to_string(), v.to_string()),
            None => (String::new(), full.clone()),
        };
        let color = tag_color(&full);
        groups
            .entry(ns)
            .or_default()
            .push((full, value, count, color));
    }
    groups
        .into_iter()
        .map(|(ns, tags)| (ns_label(&ns), tags))
        .collect()
}

fn ns_label(ns: &str) -> String {
    match ns {
        "topics" => "Topics".into(),
        "wf" => "Workflow".into(),
        "" => "Tags".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

/// A stable per-tag color from the design's semantic palette.
fn tag_color(name: &str) -> Color32 {
    const PALETTE: [(u8, u8, u8); 6] = [
        (0x1F, 0x8A, 0x5B), // accent green
        (0x2A, 0x6F, 0xDB), // blue
        (0xB6, 0x79, 0x2B), // amber
        (0x8A, 0x5B, 0xD9), // purple
        (0xC2, 0x53, 0x6B), // rose
        (0x2F, 0x8E, 0x8A), // teal
    ];
    let h = name
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let (r, g, b) = PALETTE[(h as usize) % PALETTE.len()];
    Color32::from_rgb(r, g, b)
}

fn row_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    on: bool,
    count: usize,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if on { theme.sel } else { theme.surface_2 },
        );
    }
    ui.painter().text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        if on { theme.text } else { theme.text_2 },
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(10.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    resp
}

// -------------------------------------------------------------------- list

fn item_list(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    // Toolbar: collapse-left · search · collapse-right.
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if icon_btn(ui, theme, Glyph::PanelLeft, st.hide_tags).clicked() {
                    st.hide_tags = !st.hide_tags;
                }
                // New-entry / add-by-identifier: rendered now, wired in a later
                // wave (each needs a small input dialog).
                let _ = icon_btn(ui, theme, Glyph::Plus, false).on_hover_text("New entry");
                let _ = icon_btn(ui, theme, Glyph::Link, false).on_hover_text("Add by DOI / arXiv");
                // search box fills the middle
                egui::Frame::default()
                    .fill(theme.surface_2)
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 44.0);
                        ui.horizontal(|ui| {
                            icons::show(ui, Glyph::Search, 16.0, theme.muted);
                            ui.add(
                                egui::TextEdit::singleline(&mut st.search)
                                    .hint_text("Search all fields & tags")
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            );
                        });
                    });
                if icon_btn(ui, theme, Glyph::PanelRight, st.hide_detail).clicked() {
                    st.hide_detail = !st.hide_detail;
                }
            });
        });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.min_rect().bottom(),
        egui::Stroke::new(1.0, theme.border),
    );

    let search = st.search.to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut shown = 0usize;
            for e in entries {
                if !matches_filter(e, &st.active_tag, &search) {
                    continue;
                }
                shown += 1;
                if list_row(ui, theme, e, st.selected.as_deref() == Some(&e.citekey)).clicked() {
                    st.selected = Some(e.citekey.clone());
                    st.buffers_for = None; // re-load edit buffers for the new pick
                }
            }
            if shown == 0 {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No matching entries").color(theme.muted));
                });
            }
        });
}

/// One 56px list row: type glyph · serif title + tag hashes · creator · year · PDF.
fn list_row(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, selected: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme.sel);
        ui.painter().vline(
            rect.left() + 1.5,
            rect.y_range(),
            egui::Stroke::new(3.0, theme.sel_line),
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme.surface_2);
    }
    // type glyph
    let (glyph, gcolor) = type_glyph(theme, &e.entry_type);
    let gr = egui::Rect::from_center_size(
        rect.left_center() + egui::vec2(26.0, 0.0),
        egui::vec2(20.0, 20.0),
    );
    icons::paint(ui.painter(), gr, glyph, gcolor);

    let x = rect.left() + 48.0;
    let title = e
        .fields
        .get("title")
        .map(String::as_str)
        .unwrap_or("(untitled)");
    ui.painter().text(
        egui::pos2(x, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(title, 64),
        theme::serif(15.5),
        theme.text,
    );
    let creator = creator_of(e);
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    let tagline = e
        .tags
        .iter()
        .map(|t| format!("#{}", t.rsplit(':').next().unwrap_or(t)))
        .collect::<Vec<_>>()
        .join("  ");
    let sub = if tagline.is_empty() {
        format!("{creator}   ·   {year}")
    } else {
        format!("{creator}   ·   {year}   ·   {tagline}")
    };
    ui.painter().text(
        egui::pos2(x, rect.center().y + 11.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(&sub, 76),
        egui::FontId::proportional(12.0),
        theme.muted,
    );
    // PDF clip on the right
    if has_pdf(e) {
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(20.0, 0.0),
                egui::vec2(16.0, 16.0),
            ),
            Glyph::Attach,
            theme.faint,
        );
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.border_2),
    );
    resp
}

// ------------------------------------------------------------------ detail

fn detail_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // (Re)load edit buffers when the selection or lock changed.
    if st.buffers_for.as_deref() != Some(e.citekey.as_str()) {
        st.buffers.clear();
        for f in [
            "title",
            "author",
            "booktitle",
            "journal",
            "year",
            "doi",
            "abstract",
        ] {
            st.buffers
                .insert(f.to_string(), e.fields.get(f).cloned().unwrap_or_default());
        }
        st.buffers_for = Some(e.citekey.clone());
    }
    let locked = st.locked;

    // header: type badge + lock toggle
    ui.horizontal(|ui| {
        let (_, gcolor) = type_glyph(theme, &e.entry_type);
        ui.label(
            RichText::new(type_label(&e.entry_type))
                .size(11.0)
                .strong()
                .color(gcolor),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let g = if locked { Glyph::Lock } else { Glyph::Unlock };
            let col = if locked { theme.muted } else { theme.accent };
            if icon_btn_colored(ui, theme, g, col, !locked)
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
        });
    });
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Title (serif).
            edit_text(ui, theme, st, actions, "title", locked, true);
            ui.add_space(10.0);

            // Authors (compact line locked; editable raw line when unlocked — the
            // Zotero per-author rows are a planned refinement).
            meta_label(ui, theme, "Authors");
            if locked {
                ui.label(
                    RichText::new(authors_line(e))
                        .size(13.5)
                        .color(theme.text_2),
                );
            } else {
                edit_field_raw(ui, theme, st, actions, "author", false);
            }
            ui.add_space(8.0);

            let pub_field = if e.fields.contains_key("journal") {
                "journal"
            } else {
                "booktitle"
            };
            meta_row(ui, theme, st, actions, "Publication", pub_field, locked);
            meta_row(ui, theme, st, actions, "Year", "year", locked);
            meta_row(ui, theme, st, actions, "DOI", "doi", locked);

            // Cite key (read-only — re-keying is a Normalize action).
            meta_label(ui, theme, "Citation key");
            ui.label(
                RichText::new(&e.citekey)
                    .font(theme::mono(12.5))
                    .color(theme.text_2),
            );
            ui.add_space(10.0);

            // Status + stars.
            meta_label(ui, theme, "Reading");
            status_stars(ui, theme, e, actions);
            ui.add_space(10.0);

            // Tags.
            meta_label(ui, theme, "Tags");
            tags_editor(ui, theme, e, locked, st, actions);
            ui.add_space(12.0);

            // Abstract (serif).
            meta_label(ui, theme, "Abstract");
            if locked {
                ui.label(
                    RichText::new(e.fields.get("abstract").map(String::as_str).unwrap_or("—"))
                        .font(theme::serif(13.5))
                        .color(theme.text_2),
                );
            } else {
                let buf = st.buffers.get_mut("abstract").unwrap();
                let r = ui.add(
                    egui::TextEdit::multiline(buf)
                        .desired_width(f32::INFINITY)
                        .desired_rows(5),
                );
                if r.lost_focus() {
                    actions.push(LibAction::Edit(
                        "abstract".into(),
                        st.buffers["abstract"].clone(),
                    ));
                }
            }
            ui.add_space(16.0);

            // Footer: Cite + link (row 1), BibTeX (row 2) — two narrow rows (spec §4·A).
            ui.horizontal(|ui| {
                if primary_btn(ui, theme, "Cite").clicked() {
                    actions.push(LibAction::Cite);
                }
                if let Some(url) = e.fields.get("url").filter(|u| !u.is_empty()) {
                    if icon_btn(ui, theme, Glyph::Link, false)
                        .on_hover_text("Open source")
                        .clicked()
                    {
                        actions.push(LibAction::OpenUrl(url.clone()));
                    }
                }
            });
            ui.add_space(6.0);
            if ghost_btn(ui, theme, "BibTeX", 176.0).clicked() {
                actions.push(LibAction::Bibtex);
            }
        });
}

fn status_stars(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, actions: &mut Vec<LibAction>) {
    ui.horizontal(|ui| {
        for (label, s) in [
            ("Unread", Status::Unread),
            ("Reading", Status::Reading),
            ("Done", Status::Done),
        ] {
            let on = e.status == s.as_str();
            let txt = RichText::new(label).size(12.0).color(if on {
                Color32::WHITE
            } else {
                theme.text_2
            });
            let fill = if on {
                status_color(theme, &e.status)
            } else {
                theme.surface_2
            };
            if ui
                .add(egui::Button::new(txt).fill(fill).corner_radius(7.0))
                .clicked()
            {
                actions.push(LibAction::SetStatus(s));
            }
        }
        ui.add_space(8.0);
        // stars 1..5
        let cur = e.stars.unwrap_or(0);
        for n in 1..=5u8 {
            let col = if n <= cur { theme.amber } else { theme.faint };
            if icon_btn_colored(ui, theme, Glyph::Star, col, false).clicked() {
                actions.push(LibAction::SetStars(if cur == n { None } else { Some(n) }));
            }
        }
    });
}

fn tags_editor(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    locked: bool,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    ui.horizontal_wrapped(|ui| {
        for t in &e.tags {
            let value = t.rsplit(':').next().unwrap_or(t);
            egui::Frame::default()
                .fill(theme.surface_2)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.painter(); // ensure a paint surface
                        let dot = ui
                            .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                            .0;
                        ui.painter()
                            .rect_filled(dot, egui::CornerRadius::same(2), tag_color(t));
                        ui.label(RichText::new(value).size(11.0).color(theme.text));
                        if !locked
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("×").size(11.0).color(theme.muted),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                        {
                            actions.push(LibAction::RemoveTag(t.clone()));
                        }
                    });
                });
        }
        if !locked {
            if st.adding_tag {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.new_tag)
                        .hint_text("ns:value")
                        .desired_width(90.0),
                );
                if r.lost_focus() {
                    let t = st.new_tag.trim().to_string();
                    if !t.is_empty() {
                        actions.push(LibAction::AddTag(t));
                    }
                    st.new_tag.clear();
                    st.adding_tag = false;
                }
            } else if ui
                .add(
                    egui::Button::new(RichText::new("+ add").size(11.0).color(theme.muted))
                        .frame(false),
                )
                .clicked()
            {
                st.adding_tag = true;
            }
        }
    });
}

// ----------------------------------------------------------- detail helpers

fn meta_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(theme.muted));
    ui.add_space(2.0);
}

/// A labelled meta field (Publication/Year/DOI), editable when unlocked.
fn meta_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    label: &str,
    field: &str,
    locked: bool,
) {
    meta_label(ui, theme, label);
    if locked {
        let v = st.buffers.get(field).cloned().unwrap_or_default();
        let shown = if v.is_empty() { "—".to_string() } else { v };
        let font = if field == "doi" {
            theme::mono(12.5)
        } else {
            egui::FontId::proportional(13.5)
        };
        ui.label(RichText::new(shown).font(font).color(theme.text_2));
    } else {
        edit_field_raw(ui, theme, st, actions, field, field == "doi");
    }
    ui.add_space(8.0);
}

/// The title field: serif, large; editable when unlocked.
fn edit_text(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    field: &str,
    locked: bool,
    _serif: bool,
) {
    if locked {
        let v = st.buffers.get(field).cloned().unwrap_or_default();
        ui.label(RichText::new(v).font(theme::serif(19.0)).color(theme.text));
    } else {
        edit_field_raw(ui, theme, st, actions, field, false);
    }
}

/// A single-line editable buffer that emits an `Edit` action on commit.
fn edit_field_raw(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    field: &str,
    mono: bool,
) {
    let buf = st.buffers.entry(field.to_string()).or_default();
    let mut te = egui::TextEdit::singleline(buf).desired_width(f32::INFINITY);
    if mono {
        te = te.font(theme::mono(12.5));
    }
    let r = ui.add(te);
    if r.lost_focus() {
        actions.push(LibAction::Edit(
            field.to_string(),
            st.buffers[field].clone(),
        ));
    }
    let _ = theme;
}

// ------------------------------------------------------------- tiny widgets

fn panel_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
}

fn icon_btn(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, on: bool) -> egui::Response {
    icon_btn_colored(
        ui,
        theme,
        glyph,
        if on { theme.accent } else { theme.muted },
        on,
    )
}

fn icon_btn_colored(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: Glyph,
    color: Color32,
    on: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if on {
                theme.accent_tint
            } else {
                theme.surface_2
            },
        );
    }
    icons::paint(ui.painter(), rect.shrink(8.0), glyph, color);
    resp
}

fn primary_btn(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(13.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(theme.accent)
        .corner_radius(8.0)
        .min_size(egui::vec2(120.0, 32.0)),
    )
}

fn ghost_btn(ui: &mut egui::Ui, theme: &Theme, label: &str, w: f32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).strong().color(theme.text))
            .fill(theme.surface)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .corner_radius(8.0)
            .min_size(egui::vec2(w, 32.0)),
    )
}

// -------------------------------------------------------------- entry helpers

fn matches_filter(e: &EntryView, active_tag: &Option<String>, search_lower: &str) -> bool {
    active_tag.as_ref().is_none_or(|t| e.tags.contains(t)) && matches_search(e, search_lower)
}

fn matches_search(e: &EntryView, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    if e.citekey.to_lowercase().contains(q) {
        return true;
    }
    if e.tags.iter().any(|t| t.to_lowercase().contains(q)) {
        return true;
    }
    e.fields.values().any(|v| v.to_lowercase().contains(q))
}

fn creator_of(e: &EntryView) -> String {
    let authors = authors_vec(e);
    match authors.len() {
        0 => "—".into(),
        1 => last_name(&authors[0]),
        _ => format!("{} et al.", last_name(&authors[0])),
    }
}

fn authors_vec(e: &EntryView) -> Vec<String> {
    e.fields
        .get("author")
        .map(|a| {
            a.split(" and ")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn authors_line(e: &EntryView) -> String {
    let v = authors_vec(e);
    if v.is_empty() {
        "—".into()
    } else {
        v.join("  ·  ")
    }
}

fn last_name(author: &str) -> String {
    author
        .split(',')
        .next()
        .unwrap_or(author)
        .trim()
        .to_string()
}

fn has_pdf(e: &EntryView) -> bool {
    // The engine attaches PDFs under pdfs/<key>.pdf; here we use the presence of
    // a url as a proxy in the mock (real PDF detection lands with the Reader wave).
    e.fields.get("url").is_some_and(|u| !u.is_empty())
}

fn type_label(t: &str) -> &'static str {
    match t {
        "inproceedings" | "conference" => "Conference Paper",
        "article" => "Journal Article",
        "misc" => "Preprint",
        _ => "Document",
    }
}

fn type_glyph(theme: &Theme, t: &str) -> (Glyph, Color32) {
    match t {
        "inproceedings" | "conference" => (Glyph::Book, theme.accent),
        "article" => (Glyph::Doc, theme.blue),
        "misc" => (Glyph::Doc, theme.amber),
        _ => (Glyph::Doc, theme.muted),
    }
}

fn status_color(theme: &Theme, status: &str) -> Color32 {
    match status {
        "done" => theme.accent,
        "reading" => theme.amber,
        _ => theme.muted,
    }
}

fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
