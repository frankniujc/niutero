//! Library — the three layouts of the Library tool (design spec §4): Classic
//! (this file, §4·A), Reader ([`reader`], §4·B), and Board ([`board`], §4·C).
//! All three share the tag-first sidebar and the lock-guarded editable detail;
//! Classic owns the shared helpers (entry formatting, the detail panel, the
//! small widgets), and the two sibling submodules reuse them via `super::`.
//!
//! Each view renders from the loaded `Vec<EntryView>` and mutates cheap UI state
//! (`LibState`) in place. Anything that touches the engine (edits, status/stars,
//! tags, clipboard, links) is returned as a [`LibAction`] for the app to apply
//! after rendering — this keeps the immutable library borrow and the mutable
//! engine call from overlapping.

use std::collections::{BTreeMap, HashSet};

use eframe::egui::{self, Color32, RichText};
use niutero_engine::{EntryView, Status};

use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};

mod board;
mod reader;
pub use board::board;
pub use reader::reader;

/// Cheap, view-local UI state (no engine data).
pub struct LibState {
    pub selected: Option<String>,
    pub active_tag: Option<String>,
    /// Reader: filter the card list by reading status (`unread`/`reading`/`done`).
    pub reading_filter: Option<String>,
    pub search: String,
    /// Detail panel lock — editable only when `false`. Locked by default.
    pub locked: bool,
    pub hide_tags: bool,
    pub hide_detail: bool,
    /// Reader: collapse the middle card list for a full-width read.
    pub hide_list: bool,
    /// Board: whether the slide-in detail drawer is open for `selected`.
    pub drawer_open: bool,
    /// Classic: sort the list by creator (CREATOR ▾ column header toggle).
    pub sort_by_creator: bool,
    /// Classic: tag-tree namespace groups the user has collapsed.
    collapsed_ns: HashSet<String>,
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
            reading_filter: None,
            search: String::new(),
            locked: true,
            hide_tags: false,
            hide_detail: false,
            hide_list: false,
            drawer_open: false,
            sort_by_creator: false,
            collapsed_ns: HashSet::new(),
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
    /// Open the entry's attached PDF (`pdfs/<key>.pdf`) in the OS viewer.
    OpenPdf,
    Cite,
    Bibtex,
}

/// Render the Classic view. Returns nothing; queued engine actions go into
/// `actions`. `entries` is already filtered? No — we filter here from the full set.
pub fn classic(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    // Default selection: first entry.
    if st.selected.is_none() {
        st.selected = entries.first().map(|e| e.citekey.clone());
    }

    // Responsive: on a genuinely narrow logical width, collapse panels so the
    // three columns never crowd out the list (the user can still toggle them).
    let avail = ctx.content_rect().width() - 60.0; // minus the rail
    let hide_tags = st.hide_tags || avail < 820.0;
    let hide_detail = st.hide_detail || avail < 560.0;

    // Tags sidebar (left). Top-level ctx panels so egui sizes the central list
    // correctly between the two side panels.
    if !hide_tags {
        egui::SidePanel::left("niu-tags")
            .exact_width(232.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 12,
                        top: 14,
                        bottom: 12,
                    }),
            )
            .show(ctx, |ui| tags_sidebar(ui, theme, entries, st));
    }

    // Detail panel (right).
    if !hide_detail {
        egui::SidePanel::right("niu-detail")
            .exact_width(392.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::same(20)),
            )
            .show(ctx, |ui| {
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

    // Item list (center).
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| item_list(ui, theme, entries, st));
}

// ----------------------------------------------------------------- sidebar

fn tags_sidebar(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    // "All Entries" (library icon) clears the tag filter.
    let all_on = st.active_tag.is_none();
    if all_entries_row(ui, theme, all_on, entries.len()) {
        st.active_tag = None;
    }
    ui.add_space(12.0);

    // TAGS section header with the AI auto-tag (✦) and new-tag (+) buttons.
    // Both open popovers in the design; rendered now, wired in a later wave.
    tags_header(ui, theme);

    for (label, tags) in tag_groups(entries) {
        let collapsed = st.collapsed_ns.contains(&label);
        if group_header(ui, theme, &label, tags.len(), collapsed) {
            if collapsed {
                st.collapsed_ns.remove(&label);
            } else {
                st.collapsed_ns.insert(label.clone());
            }
        }
        if collapsed {
            continue;
        }
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
            // indented under the group header
            let dot = rect.left_center() + egui::vec2(22.0, 0.0);
            ui.painter().rect_filled(
                egui::Rect::from_center_size(dot, egui::vec2(8.0, 8.0)),
                egui::CornerRadius::same(3),
                color,
            );
            ui.painter().text(
                rect.left_center() + egui::vec2(34.0, 0.0),
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
        ui.add_space(6.0);
    }
}

/// "All Entries" row with a leading library icon. Returns whether clicked.
fn all_entries_row(ui: &mut egui::Ui, theme: &Theme, on: bool, count: usize) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if on { theme.sel } else { theme.surface_2 },
        );
    }
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        ),
        Glyph::Library,
        theme.accent,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "All Entries",
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
    resp.clicked()
}

/// "TAGS" section header with ✦ (AI auto-tag) and + (new tag) buttons.
fn tags_header(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new("TAGS").size(11.0).strong().color(theme.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let _ = mini_icon_btn(ui, theme, Glyph::Plus).on_hover_text("New tag");
            let _ = mini_icon_btn(ui, theme, Glyph::Sparkle).on_hover_text("Auto-tag with AI");
        });
    });
    ui.add_space(2.0);
}

/// Collapsible namespace group header: chevron · LABEL · count. Returns clicked.
fn group_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    count: usize,
    collapsed: bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    let chevron = if collapsed {
        Glyph::ChevronRight
    } else {
        Glyph::ChevronDown
    };
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::vec2(12.0, 12.0),
        ),
        chevron,
        theme.muted,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label.to_uppercase(),
        egui::FontId::proportional(11.0),
        theme.muted,
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

/// A 22×22 transparent icon button (sidebar header controls).
fn mini_icon_btn(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), theme.surface_2);
    }
    icons::paint_at(ui, rect.shrink(4.0), glyph, theme.muted);
    resp
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

    // Sortable column header (TITLE · CREATOR ▾ · YEAR), aligned to the row
    // columns; clicking CREATOR toggles the sort.
    list_header(ui, theme, st);

    let search = st.search.to_lowercase();
    // Only the matching entries, optionally sorted, then row-virtualized: with a
    // large library (1,292 in the mock) egui would otherwise lay out a text
    // galley for every row each frame. `show_rows` runs the closure for just the
    // visible range (rows are a uniform 62px).
    let mut shown: Vec<&EntryView> = entries
        .iter()
        .filter(|e| matches_filter(e, &st.active_tag, &search))
        .collect();
    if st.sort_by_creator {
        // Key once (creator_of allocates) rather than in every comparison.
        let mut keyed: Vec<(String, &EntryView)> =
            shown.into_iter().map(|e| (creator_of(e), e)).collect();
        keyed.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        shown = keyed.into_iter().map(|(_, e)| e).collect();
    }
    if shown.is_empty() {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No matching entries").color(theme.muted));
                });
            });
        return;
    }
    // Rows are flush (each draws its own bottom hairline), so the pitch must be
    // exactly 62 — `show_rows` reads this spacing to size the virtual range.
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 62.0, shown.len(), |ui, range| {
            for i in range {
                let e = shown[i];
                if list_row(ui, theme, e, st.selected.as_deref() == Some(&e.citekey)).clicked() {
                    st.selected = Some(e.citekey.clone());
                    st.buffers_for = None; // re-load edit buffers for the new pick
                }
            }
        });
}

/// The list's column x-positions (shared by the header and rows), measured from
/// the right so the title column flexes. A fixed clip slot keeps columns aligned
/// whether or not a given row has a PDF.
struct ListCols {
    creator_right: f32,
    year_right: f32,
    title_right: f32,
}

fn list_cols(rect: egui::Rect) -> ListCols {
    let year_right = rect.right() - 14.0 - 26.0; // right pad + clip slot
    let creator_right = year_right - 46.0 - 14.0; // year width + gap
    let title_right = creator_right - 150.0 - 14.0; // creator width + gap
    ListCols {
        creator_right,
        year_right,
        title_right,
    }
}

/// Sortable column header row (spec §4·A).
fn list_header(ui: &mut egui::Ui, theme: &Theme, st: &mut LibState) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let cols = list_cols(rect);
    let cy = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, cy),
        egui::Align2::LEFT_CENTER,
        "TITLE",
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    // CREATOR ▾ — clickable to toggle sort (accent when active).
    let creator_col = if st.sort_by_creator {
        theme.accent
    } else {
        theme.muted
    };
    let creator_txt = "CREATOR  ▾";
    let cw = creator_txt.len() as f32 * 6.0;
    let chit = egui::Rect::from_min_max(
        egui::pos2(cols.creator_right - cw, rect.top()),
        egui::pos2(cols.creator_right + 4.0, rect.bottom()),
    );
    if ui
        .interact(chit, ui.id().with("sort-creator"), egui::Sense::click())
        .on_hover_text("Sort by creator")
        .clicked()
    {
        st.sort_by_creator = !st.sort_by_creator;
    }
    ui.painter().text(
        egui::pos2(cols.creator_right, cy),
        egui::Align2::RIGHT_CENTER,
        creator_txt,
        egui::FontId::proportional(11.0),
        creator_col,
    );
    ui.painter().text(
        egui::pos2(cols.year_right, cy),
        egui::Align2::RIGHT_CENTER,
        "YEAR",
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.border),
    );
}

/// One 62px list row (spec §4·A): type glyph · serif title with a tag-hash line
/// underneath · right-aligned CREATOR and YEAR columns · PDF clip.
fn list_row(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, selected: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 62.0), egui::Sense::click());
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
    let cols = list_cols(rect);
    let cy = rect.center().y;

    // type glyph
    let (glyph, gcolor) = type_glyph(theme, &e.entry_type);
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(egui::pos2(rect.left() + 26.0, cy), egui::vec2(20.0, 20.0)),
        glyph,
        gcolor,
    );

    let title_left = rect.left() + 48.0;
    let title_w = (cols.title_right - title_left).max(80.0);
    let title = crate::tex::display(
        e.fields
            .get("title")
            .map(String::as_str)
            .unwrap_or("(untitled)"),
    );
    let tagline = e
        .tags
        .iter()
        .map(|t| format!("#{}", t.rsplit(':').next().unwrap_or(t)))
        .collect::<Vec<_>>()
        .join("  ");
    let has_tags = !tagline.is_empty();

    // Title (and tag-hash line under it, when present), left block vertically
    // centered within the row.
    let title_y = if has_tags { cy - 9.0 } else { cy };
    ui.painter().text(
        egui::pos2(title_left, title_y),
        egui::Align2::LEFT_CENTER,
        ellipsize(&title, (title_w / 7.4) as usize),
        theme::serif(15.5),
        theme.text,
    );
    if has_tags {
        ui.painter().text(
            egui::pos2(title_left, cy + 11.0),
            egui::Align2::LEFT_CENTER,
            ellipsize(&tagline, (title_w / 6.6) as usize),
            egui::FontId::proportional(12.0),
            theme.muted,
        );
    }

    // CREATOR and YEAR columns (right-aligned), vertically centered.
    let creator = creator_of(e);
    ui.painter().text(
        egui::pos2(cols.creator_right, cy),
        egui::Align2::RIGHT_CENTER,
        ellipsize(&creator, 20),
        egui::FontId::proportional(13.0),
        theme.text_2,
    );
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    ui.painter().text(
        egui::pos2(cols.year_right, cy),
        egui::Align2::RIGHT_CENTER,
        year,
        egui::FontId::proportional(13.0),
        theme.text_2,
    );

    // PDF clip on the far right.
    if has_pdf(e) {
        icons::paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(rect.right() - 16.0, cy),
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

/// (Re)load the edit buffers for `e`'s text fields when the selection changed,
/// so the unlocked `EditField`s start from the entry's current values. Shared by
/// the Classic detail panel, the Reader pane, and the Board drawer.
pub(super) fn ensure_buffers(st: &mut LibState, e: &EntryView) {
    if st.buffers_for.as_deref() == Some(e.citekey.as_str()) {
        return;
    }
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

/// The accent-tint type pill (icon + uppercase label) used in the Reader pane
/// header and the Board drawer header.
pub(super) fn type_badge(ui: &mut egui::Ui, theme: &Theme, e: &EntryView) {
    let (g, _) = type_glyph(theme, &e.entry_type);
    egui::Frame::default()
        .fill(theme.accent_tint)
        .corner_radius(7.0)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, g, 14.0, theme.accent);
                ui.label(
                    RichText::new(type_label(&e.entry_type).to_uppercase())
                        .size(11.5)
                        .strong()
                        .color(theme.accent),
                );
            });
        });
}

pub(super) fn detail_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    ensure_buffers(st, e);
    let locked = st.locked;

    // header: type pill badge + "Locked/Editing" label + lock toggle
    ui.horizontal(|ui| {
        type_badge(ui, theme, e);
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
            ui.label(
                RichText::new(if locked { "Locked" } else { "Editing" })
                    .size(11.0)
                    .strong()
                    .color(theme.muted),
            );
        });
    });
    ui.add_space(8.0);
    // Classic detail omits the reading-status/stars row (it's in the Board).
    detail_body(ui, theme, e, st, actions, false);
}

/// The detail content below the header (title … Cite/BibTeX footer), shared by
/// the Classic detail panel and the Board drawer (which renders its own header
/// + close button above this body).
pub(super) fn detail_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    reading: bool,
) {
    let locked = st.locked;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Title (serif).
            edit_text(ui, theme, st, actions, "title", locked, true);
            ui.add_space(10.0);

            // Authors byline (no label) — compact line locked; editable raw line
            // when unlocked (the Zotero per-author rows are a planned refinement).
            if locked {
                ui.label(
                    RichText::new(authors_line(e))
                        .size(14.0)
                        .color(theme.text_2),
                );
            } else {
                edit_field_raw(ui, theme, st, actions, "author", false);
            }
            ui.add_space(12.0);

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
                    .color(theme.accent),
            );
            ui.add_space(10.0);

            // Reading status + stars (Board drawer only; the Classic detail omits
            // it to match the design).
            if reading {
                meta_label(ui, theme, "Reading");
                status_stars(ui, theme, e, actions);
                ui.add_space(10.0);
            }

            // Tags.
            meta_label(ui, theme, "Tags");
            tags_editor(ui, theme, e, locked, st, actions);
            ui.add_space(12.0);

            // Abstract (serif).
            meta_label(ui, theme, "Abstract");
            if locked {
                let abs = e.fields.get("abstract").map(String::as_str).unwrap_or("—");
                crate::tex::runs_label(ui, abs, theme::serif(13.5), theme.text_2);
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
                if primary_btn(ui, theme, Some(Glyph::Quote), "Cite").clicked() {
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
            ui.add_space(8.0);
            let w = ui.available_width();
            if ghost_btn(ui, theme, Some(Glyph::Book), "BibTeX", w).clicked() {
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
            let (glyph, col) = if n <= cur {
                (Glyph::StarFilled, theme.amber)
            } else {
                (Glyph::Star, theme.faint)
            };
            if icon_btn_colored(ui, theme, glyph, col, false).clicked() {
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
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let dot = ui
                            .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                            .0;
                        ui.painter()
                            .rect_filled(dot, egui::CornerRadius::same(2), tag_color(t));
                        ui.add_space(5.0);
                        // Full namespace:value, with the namespace dimmed (spec §4·A).
                        if let Some((ns, val)) = t.split_once(':') {
                            ui.label(
                                RichText::new(format!("{ns}: "))
                                    .size(11.0)
                                    .color(theme.muted),
                            );
                            ui.label(RichText::new(val).size(11.0).color(theme.text));
                        } else {
                            ui.label(RichText::new(value).size(11.0).color(theme.text));
                        }
                        if !locked {
                            ui.add_space(4.0);
                            if ui
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
        let raw = st.buffers.get(field).cloned().unwrap_or_default();
        // DOI is an identifier (no LaTeX); other fields get the display transform.
        let v = if field == "doi" {
            raw
        } else {
            crate::tex::display(&raw)
        };
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
        crate::tex::runs_label(ui, &v, theme::serif(19.0), theme.text);
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
    icons::paint_at(ui, rect.shrink(8.0), glyph, color);
    resp
}

fn primary_btn(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Glyph>,
    label: &str,
) -> egui::Response {
    egui::Frame::default()
        .fill(theme.accent)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                if let Some(g) = icon {
                    icons::show(ui, g, 16.0, Color32::WHITE);
                }
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

fn ghost_btn(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Glyph>,
    label: &str,
    w: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
    let fill = if resp.hovered() {
        theme.surface_2
    } else {
        theme.surface
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        fill,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    // center icon + label
    let label_w = label.len() as f32 * 7.0;
    let icon_w = if icon.is_some() { 22.0 } else { 0.0 };
    let mut x = rect.center().x - (icon_w + label_w) * 0.5;
    if let Some(g) = icon {
        icons::paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(x + 8.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            g,
            theme.text,
        );
        x += icon_w;
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        theme.text,
    );
    resp
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
    // Split the raw `author` on " and " first, then de-TeX each name for display
    // (so `M\"uller`/`{\v S}imek` render as Müller / Šimek). Search/edit still use
    // the raw field — this is display-only.
    e.fields
        .get("author")
        .map(|a| {
            a.split(" and ")
                .map(|s| crate::tex::display(s.trim()))
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
        "book" => "Book",
        "incollection" | "inbook" => "Book Chapter",
        "proceedings" => "Proceedings",
        "phdthesis" => "PhD Thesis",
        "mastersthesis" => "Master's Thesis",
        "techreport" => "Technical Report",
        "manual" => "Manual",
        "booklet" => "Booklet",
        "unpublished" => "Unpublished",
        "online" | "electronic" => "Online",
        "misc" => "Preprint",
        _ => "Document",
    }
}

fn type_glyph(theme: &Theme, t: &str) -> (Glyph, Color32) {
    match t {
        "inproceedings" | "conference" | "proceedings" => (Glyph::Book, theme.accent),
        "article" => (Glyph::Doc, theme.blue),
        "book" | "incollection" | "inbook" | "manual" | "booklet" => (Glyph::Book, theme.purple),
        "phdthesis" | "mastersthesis" => (Glyph::Doc, theme.teal),
        "misc" | "online" | "electronic" => (Glyph::Doc, theme.amber),
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

pub(crate) fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
