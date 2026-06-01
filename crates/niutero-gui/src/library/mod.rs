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

/// Which Classic-list column the rows are sorted by (spec §4·A header).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// File / import order — the `.bib`'s own order, no reordering.
    #[default]
    None,
    Title,
    Creator,
    Year,
}

/// The Classic list sort: a column plus a direction. Clicking a column header
/// cycles that column ascending → descending → off (back to file order).
#[derive(Clone, Copy, Default)]
pub struct SortState {
    pub key: SortKey,
    /// `true` = descending (Z→A, newest→oldest); `false` = ascending.
    pub desc: bool,
}

impl SortState {
    /// Advance the sort when `key`'s header is clicked: a fresh column starts
    /// ascending, the active column flips to descending, then clears.
    fn click(&mut self, key: SortKey) {
        if self.key != key {
            self.key = key;
            self.desc = false;
        } else if !self.desc {
            self.desc = true;
        } else {
            *self = SortState::default();
        }
    }
}

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
    /// Classic: which column the list is sorted by, and the direction.
    pub sort: SortState,
    /// Cached Classic row order — indices into the loaded entries — plus a
    /// signature of the inputs that produced it. Filtering + sorting de-TeX's
    /// every title/author, so this runs only when an input changes (search,
    /// tag, sort, or a library reload), never on a plain scroll frame.
    shown_cache: Vec<usize>,
    shown_sig: Option<u64>,
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
            sort: SortState::default(),
            shown_cache: Vec::new(),
            shown_sig: None,
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
            .default_width(232.0)
            .width_range(190.0..=360.0)
            .resizable(true)
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
            .default_width(384.0)
            .width_range(320.0..=560.0)
            .resizable(true)
            // No inner margin — `detail_panel` lays out its own pinned footer +
            // padded scroll via nested panels.
            .frame(egui::Frame::default().fill(theme.surface))
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

/// Minimum content width for the list before it scrolls horizontally — enough
/// for a readable title plus the Creator/Year/clip columns.
const LIST_MIN_W: f32 = 520.0;

fn item_list(ui: &mut egui::Ui, theme: &Theme, entries: &[EntryView], st: &mut LibState) {
    // Toolbar: collapse-left · new · add-by-id · search · collapse-right.
    // The collapse-right button is reserved on the right (the search is width-
    // bounded) so it can't be pushed off-screen by the search box.
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if icon_btn(ui, theme, Glyph::PanelLeft, st.hide_tags)
                    .on_hover_text(if st.hide_tags {
                        "Show tags panel"
                    } else {
                        "Hide tags panel"
                    })
                    .clicked()
                {
                    st.hide_tags = !st.hide_tags;
                }
                let (dr, _) = ui.allocate_exact_size(egui::vec2(7.0, 20.0), egui::Sense::hover());
                ui.painter().vline(
                    dr.center().x,
                    (dr.center().y - 10.0)..=(dr.center().y + 10.0),
                    egui::Stroke::new(1.0, theme.border),
                );
                // New-entry / add-by-identifier: rendered now, wired in a later wave.
                let _ = icon_btn(ui, theme, Glyph::Plus, false).on_hover_text("New item");
                let _ = icon_btn(ui, theme, Glyph::Link, false)
                    .on_hover_text("Add by identifier (DOI / arXiv)");
                // search box fills the middle, but bounded so the right toggle fits
                let search_w = (ui.available_width() - 42.0).max(80.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(search_w, 32.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::Frame::default()
                            .fill(theme.surface_2)
                            .corner_radius(9.0)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
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
                    },
                );
                if icon_btn(ui, theme, Glyph::PanelRight, st.hide_detail)
                    .on_hover_text(if st.hide_detail {
                        "Show details panel"
                    } else {
                        "Hide details panel"
                    })
                    .clicked()
                {
                    st.hide_detail = !st.hide_detail;
                }
            });
        });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.min_rect().bottom(),
        egui::Stroke::new(1.0, theme.border),
    );

    // Recompute the filtered+sorted order only when an input changed; on a plain
    // scroll the signature is stable, so we reuse the cache (no per-frame de-TeX).
    let search = st.search.to_lowercase();
    let sig = order_sig(entries, &st.active_tag, &search, st.sort);
    if st.shown_sig != Some(sig) {
        st.shown_cache = compute_order(entries, &st.active_tag, &search, st.sort);
        st.shown_sig = Some(sig);
    }
    // Borrow the order out of `st` so the row closure can still mutate `st`
    // (selection); restored before returning.
    let order = std::mem::take(&mut st.shown_cache);

    if order.is_empty() {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No matching entries").color(theme.muted));
                });
            });
        st.shown_cache = order;
        return;
    }

    // The header + rows live in one horizontal scroll so a narrow pane scrolls
    // instead of crowding the columns; the body is vertically row-virtualized
    // inside (only the visible ~20 of 1,292 rows are built per frame).
    let viewport_w = ui.available_width();
    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let content_w = viewport_w.max(LIST_MIN_W);
            ui.set_min_width(content_w);
            // Sortable column header (TITLE · CREATOR · YEAR) aligned to the rows.
            list_header(ui, theme, st, content_w);
            // Rows are flush (each draws its own bottom hairline), so the pitch
            // must be exactly 56 — `show_rows` reads this spacing.
            ui.spacing_mut().item_spacing.y = 0.0;
            let body_h = ui.available_height();
            egui::ScrollArea::vertical()
                .id_salt("niu-classic-rows")
                .auto_shrink([false, false])
                .max_height(body_h)
                .show_rows(ui, 56.0, order.len(), |ui, range| {
                    for i in range {
                        let e = &entries[order[i]];
                        let sel = st.selected.as_deref() == Some(&e.citekey);
                        if list_row(ui, theme, content_w, e, sel).clicked() {
                            st.selected = Some(e.citekey.clone());
                            st.buffers_for = None; // re-load edit buffers for the new pick
                        }
                    }
                });
        });
    st.shown_cache = order;
}

/// The list's column geometry (shared by the header and rows). Columns are
/// fixed-width and left-aligned, anchored to the content's right edge (design
/// §4·A: Creator 110 · Year 48 · clip 22); the title flexes to fill the rest.
struct ListCols {
    title_left: f32,
    title_right: f32,
    creator_left: f32,
    year_left: f32,
    clip_center: f32,
}

fn list_cols(rect: egui::Rect) -> ListCols {
    let r = rect.right();
    let clip_left = r - 16.0 - 22.0; // right pad + clip column
    let year_left = clip_left - 11.0 - 48.0; // gap + year column
    let creator_left = year_left - 11.0 - 110.0; // gap + creator column
    ListCols {
        title_left: rect.left() + 45.0, // 16 pad + 18 icon + 11 gap
        title_right: creator_left - 11.0,
        creator_left,
        year_left,
        clip_center: clip_left + 11.0,
    }
}

/// Sortable column header (TITLE · CREATOR · YEAR), aligned to the row columns
/// (spec §4·A). Every column is clickable; the active sort column is accent and
/// carries an up/down chevron, the rest are muted.
fn list_header(ui: &mut egui::Ui, theme: &Theme, st: &mut LibState, content_w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w, 34.0), egui::Sense::hover());
    let cols = list_cols(rect);
    let cy = rect.center().y;
    let top = rect.top();
    let bot = rect.bottom();
    // (label, left x, SortKey, hit rect) for each column.
    let columns = [
        (
            "TITLE",
            rect.left() + 16.0,
            SortKey::Title,
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), top),
                egui::pos2(cols.creator_left - 8.0, bot),
            ),
        ),
        (
            "CREATOR",
            cols.creator_left,
            SortKey::Creator,
            egui::Rect::from_min_max(
                egui::pos2(cols.creator_left - 4.0, top),
                egui::pos2(cols.year_left - 6.0, bot),
            ),
        ),
        (
            "YEAR",
            cols.year_left,
            SortKey::Year,
            egui::Rect::from_min_max(
                egui::pos2(cols.year_left - 4.0, top),
                egui::pos2(cols.clip_center - 8.0, bot),
            ),
        ),
    ];
    let hfont = egui::FontId::proportional(11.0);
    for (label, x, key, hit) in columns {
        let active = st.sort.key == key;
        let color = if active { theme.accent } else { theme.muted };
        ui.painter().text(
            egui::pos2(x, cy),
            egui::Align2::LEFT_CENTER,
            label,
            hfont.clone(),
            color,
        );
        if active {
            let w = ui
                .painter()
                .layout_no_wrap(label.to_string(), hfont.clone(), color)
                .size()
                .x;
            let g = if st.sort.desc {
                Glyph::ChevronDown
            } else {
                Glyph::ChevronUp
            };
            icons::paint_at(
                ui,
                egui::Rect::from_center_size(egui::pos2(x + w + 6.0, cy), egui::vec2(11.0, 11.0)),
                g,
                theme.accent,
            );
        }
        let resp = ui
            .interact(hit, ui.id().with(("sort", label)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Sort by {}", label.to_lowercase()));
        if resp.clicked() {
            st.sort.click(key);
        }
    }
    ui.painter()
        .hline(rect.x_range(), bot, egui::Stroke::new(1.0, theme.border_2));
}

/// One 56px list row (spec §4·A): type glyph · serif title with a colored
/// tag-hash line under it · left-aligned CREATOR and YEAR columns · PDF clip.
/// Drawn `content_w` wide so a narrow pane scrolls horizontally.
fn list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    content_w: f32,
    e: &EntryView,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(content_w, 56.0), egui::Sense::click());
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
    let (glyph, gcolor) = type_glyph(theme, &e.entry_type);
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(egui::pos2(rect.left() + 25.0, cy), egui::vec2(18.0, 18.0)),
        glyph,
        gcolor,
    );

    // Title (serif) with the first three tags as colored #hashes underneath.
    let title_w = (cols.title_right - cols.title_left).max(60.0);
    let title = crate::tex::display(
        e.fields
            .get("title")
            .map(String::as_str)
            .unwrap_or("(untitled)"),
    );
    let tags: Vec<&String> = e.tags.iter().take(3).collect();
    let has_tags = !tags.is_empty();
    let title_y = if has_tags { cy - 9.0 } else { cy };
    ui.painter().text(
        egui::pos2(cols.title_left, title_y),
        egui::Align2::LEFT_CENTER,
        ellipsize(&title, (title_w / 7.2) as usize),
        theme::serif(15.5),
        theme.text,
    );
    if has_tags {
        let mut x = cols.title_left;
        for t in &tags {
            let label = format!("#{}", t.rsplit(':').next().unwrap_or(t));
            let gal = ui.painter().layout_no_wrap(
                label.clone(),
                egui::FontId::proportional(10.5),
                tag_color(t),
            );
            let w = gal.size().x;
            if x + w > cols.title_right {
                break;
            }
            ui.painter().text(
                egui::pos2(x, cy + 11.0),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::proportional(10.5),
                tag_color(t),
            );
            x += w + 8.0;
        }
    }

    // CREATOR and YEAR columns (left-aligned in fixed-width slots).
    ui.painter().text(
        egui::pos2(cols.creator_left, cy),
        egui::Align2::LEFT_CENTER,
        ellipsize(&creator_of(e), 16),
        egui::FontId::proportional(13.0),
        theme.text_2,
    );
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    ui.painter().text(
        egui::pos2(cols.year_left, cy),
        egui::Align2::LEFT_CENTER,
        year,
        egui::FontId::proportional(13.0),
        theme.text_2,
    );

    // PDF clip (in the entry type's color), else nothing.
    if has_pdf(e) {
        icons::paint_at(
            ui,
            egui::Rect::from_center_size(egui::pos2(cols.clip_center, cy), egui::vec2(16.0, 16.0)),
            Glyph::Attach,
            gcolor,
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

/// The lock/unlock toggle button (icon flips with state).
pub(super) fn lock_toggle(ui: &mut egui::Ui, theme: &Theme, locked: bool) -> egui::Response {
    let g = if locked { Glyph::Lock } else { Glyph::Unlock };
    let col = if locked { theme.muted } else { theme.accent };
    icon_btn_colored(ui, theme, g, col, !locked).on_hover_text(if locked {
        "Locked — click to edit"
    } else {
        "Editing — click to lock"
    })
}

/// Classic detail header (inside the scroll): type pill · "Locked/Editing" · lock.
fn detail_header(ui: &mut egui::Ui, theme: &Theme, st: &mut LibState, e: &EntryView) {
    let locked = st.locked;
    ui.horizontal(|ui| {
        type_badge(ui, theme, e);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if lock_toggle(ui, theme, locked).clicked() {
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
    ui.add_space(6.0);
}

pub(super) fn detail_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
) {
    ensure_buffers(st, e);
    // Footer pinned at the bottom (design §4·A); the fields scroll above it.
    egui::TopBottomPanel::bottom("niu-detail-footer")
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
        .show_inside(ui, |ui| detail_footer(ui, theme, e, actions));
    egui::CentralPanel::default()
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin {
                    left: 22,
                    right: 22,
                    top: 16,
                    bottom: 8,
                }),
        )
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    detail_header(ui, theme, st, e);
                    // Classic detail omits the reading row (it's in the Board).
                    detail_fields(ui, theme, e, st, actions, false);
                });
        });
}

/// The scrollable detail content (title … tags), shared by the Classic detail
/// panel and the Board drawer. The header and footer are rendered by the caller
/// (pinned). `reading` adds the Reading status+stars row (Board only).
pub(super) fn detail_fields(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    reading: bool,
) {
    let locked = st.locked;

    // Title (serif 24).
    edit_text(ui, theme, st, actions, "title", locked, true);
    ui.add_space(6.0);

    // Authors byline (no label) — compact line locked; editable raw line when
    // unlocked (the Zotero per-author rows are a planned refinement).
    if locked {
        ui.label(
            RichText::new(authors_line(e))
                .size(14.0)
                .color(theme.text_2),
        );
    } else {
        edit_field_raw(ui, theme, st, actions, "author", false);
    }
    ui.add_space(14.0);

    // Divided metadata rows.
    let pub_field = if e.fields.contains_key("journal") {
        "journal"
    } else {
        "booktitle"
    };
    meta_row(ui, theme, st, actions, "Publication", pub_field, locked);
    meta_row(ui, theme, st, actions, "Year", "year", locked);
    meta_row(ui, theme, st, actions, "DOI", "doi", locked);
    // Citation key (read-only — re-keying is a Normalize action).
    divided_meta(ui, theme, "Citation Key", |ui| {
        ui.label(
            RichText::new(&e.citekey)
                .font(theme::mono(12.0))
                .color(theme.accent),
        );
    });

    // Reading status + stars (Board drawer only).
    if reading {
        ui.add_space(14.0);
        meta_label(ui, theme, "Reading");
        status_stars(ui, theme, e, actions);
    }

    // Abstract.
    ui.add_space(14.0);
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

    // Tags.
    ui.add_space(14.0);
    meta_label(ui, theme, "Tags");
    tags_editor(ui, theme, e, locked, st, actions);
}

/// The pinned detail footer: Cite (primary) + open-link on row 1, BibTeX on
/// row 2 — constrained to a 248px column (design §4·A).
pub(super) fn detail_footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &EntryView,
    actions: &mut Vec<LibAction>,
) {
    ui.add_space(4.0);
    ui.vertical(|ui| {
        ui.set_max_width(248.0);
        let has_link = e.fields.get("url").is_some_and(|u| !u.is_empty());
        ui.horizontal(|ui| {
            let cite_w = if has_link { 248.0 - 40.0 } else { 248.0 };
            if pri_btn_centered(ui, theme, Glyph::Quote, "Cite", cite_w).clicked() {
                actions.push(LibAction::Cite);
            }
            if has_link {
                let url = e.fields.get("url").cloned().unwrap_or_default();
                if icbtn_bordered(ui, theme, Glyph::Link)
                    .on_hover_text("Open source")
                    .clicked()
                {
                    actions.push(LibAction::OpenUrl(url));
                }
            }
        });
        ui.add_space(8.0);
        if ghost_btn(ui, theme, Some(Glyph::Book), "BibTeX", 248.0).clicked() {
            actions.push(LibAction::Bibtex);
        }
    });
}

/// A fixed-width primary button with centered icon + label.
fn pri_btn_centered(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Glyph,
    label: &str,
    w: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
    let fill = if resp.hovered() {
        theme.accent_press
    } else {
        theme.accent
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    let label_w = label.len() as f32 * 7.0;
    let mut x = rect.center().x - (22.0 + label_w) * 0.5;
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0)),
        icon,
        Color32::WHITE,
    );
    x += 22.0;
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        Color32::WHITE,
    );
    resp
}

/// A 32×32 bordered icon button (the footer's open-link button).
fn icbtn_bordered(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let bg = if resp.hovered() {
        theme.surface_2
    } else {
        theme.surface
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        bg,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    icons::paint_at(ui, rect.shrink(8.0), glyph, theme.muted);
    resp
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

/// An uppercase section label (Abstract / Tags / …) — design's `secLabel` style.
fn meta_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(6.0);
}

/// A divided metadata row (design `metaRow`): a top hairline, then a fixed 92px
/// label and a flexing value, 9px padding top & bottom.
fn divided_meta(ui: &mut egui::Ui, theme: &Theme, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    let top = ui.cursor().min.y;
    ui.painter().hline(
        ui.max_rect().x_range(),
        top,
        egui::Stroke::new(1.0, theme.border_2),
    );
    ui.add_space(9.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let (lr, _) = ui.allocate_exact_size(egui::vec2(92.0, 18.0), egui::Sense::hover());
        ui.painter().text(
            lr.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            theme.muted,
        );
        value(ui);
    });
    ui.add_space(9.0);
}

/// A labelled meta field (Publication/Year/DOI) as a divided row, editable when
/// unlocked. The DOI is mono + accent (an identifier, no LaTeX).
fn meta_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut LibState,
    actions: &mut Vec<LibAction>,
    label: &str,
    field: &str,
    locked: bool,
) {
    let is_doi = field == "doi";
    divided_meta(ui, theme, label, |ui| {
        if locked {
            let raw = st.buffers.get(field).cloned().unwrap_or_default();
            let v = if is_doi {
                raw
            } else {
                crate::tex::display(&raw)
            };
            let shown = if v.is_empty() { "—".to_string() } else { v };
            let font = if is_doi {
                theme::mono(12.5)
            } else {
                egui::FontId::proportional(13.0)
            };
            let col = if is_doi { theme.accent } else { theme.text };
            ui.label(RichText::new(shown).font(font).color(col));
        } else {
            edit_field_raw(ui, theme, st, actions, field, is_doi);
        }
    });
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
        crate::tex::runs_label(ui, &v, theme::serif(24.0), theme.text);
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

/// Compute the visible row order: filter to the active tag/search, then sort by
/// the active column. Returns indices into `entries`. `SortKey::None` keeps the
/// `.bib`'s own order. Keys are built once per entry (`sort_by_cached_key`).
/// Cached by the caller — this is the de-TeX-heavy work kept off scroll frames.
fn compute_order(
    entries: &[EntryView],
    active_tag: &Option<String>,
    search: &str,
    sort: SortState,
) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len())
        .filter(|&i| matches_filter(&entries[i], active_tag, search))
        .collect();
    match sort.key {
        SortKey::None => return idx,
        SortKey::Title => idx.sort_by_cached_key(|&i| title_of(&entries[i]).to_lowercase()),
        // Surname of the first author, then year, so same-author entries are
        // chronological within the alphabetical block.
        SortKey::Creator => idx.sort_by_cached_key(|&i| {
            let last = authors_vec(&entries[i])
                .first()
                .map(|a| last_name(a))
                .unwrap_or_default();
            (last.to_lowercase(), year_of(&entries[i]))
        }),
        SortKey::Year => idx.sort_by_cached_key(|&i| year_of(&entries[i])),
    }
    if sort.desc {
        idx.reverse();
    }
    idx
}

/// A cheap signature of everything that determines the row order. When it's
/// unchanged frame-to-frame (e.g. during a scroll) the cached order is reused.
/// The entries' data pointer + length stand in for "the library reloaded", so
/// any edit (which replaces the `Vec`) busts the cache.
fn order_sig(entries: &[EntryView], tag: &Option<String>, search: &str, sort: SortState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (entries.as_ptr() as usize).hash(&mut h);
    entries.len().hash(&mut h);
    tag.hash(&mut h);
    search.hash(&mut h);
    (sort.key as u8).hash(&mut h);
    sort.desc.hash(&mut h);
    h.finish()
}

/// De-TeX'd title for sorting (matches what the row shows).
fn title_of(e: &EntryView) -> String {
    crate::tex::display(e.fields.get("title").map(String::as_str).unwrap_or(""))
}

/// The entry's year as a number for sorting; missing/non-numeric years sort as
/// `0` (first ascending, last descending).
fn year_of(e: &EntryView) -> i32 {
    e.fields
        .get("year")
        .and_then(|y| {
            let digits: String = y.chars().filter(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0)
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

/// The surname of one author, handling both BibTeX name orderings:
/// `"Last, First"` → `Last`, and `"First Middle Last"` → `Last` (the last
/// whitespace-separated token). This is what the Creator column shows and what
/// the creator sort orders by — so `"John Smith"` sorts under **S**, not **J**.
fn last_name(author: &str) -> String {
    let a = author.trim();
    if let Some((last, _first)) = a.split_once(',') {
        return last.trim().to_string();
    }
    a.rsplit(char::is_whitespace)
        .find(|t| !t.is_empty())
        .unwrap_or(a)
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
