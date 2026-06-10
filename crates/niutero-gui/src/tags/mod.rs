//! Tags tool (design: a dedicated 5th rail tool — a master/detail tag manager,
//! not just the in-sidebar filter).
//!
//! Left: a searchable tag table grouped by namespace (Topics / Workflow / …),
//! each row a color dot · mono `ns:value` · usage bar + entry count · last-used
//! date; click-to-sort column headers; footer tallies. Right: a detail panel for
//! the selected tag — recolor (session-local) + custom hex, namespace pills
//! (selecting one *moves* the tag, a real rename), rename via the lock toggle,
//! Merge-into / Delete, and the list of entries carrying the tag.
//!
//! Engine-touching requests come back as [`TagAction`]s the app applies
//! (`rename_tag` / `delete_tag`), mirroring the Library views.

use std::collections::{BTreeMap, HashMap, HashSet};

use eframe::egui::{self, Color32, RichText};
use niutero_engine::EntryView;

use crate::icons::{self, Glyph};
use crate::library::{creator_of, ellipsize, tag_color, type_glyph};
use crate::theme::{self, Theme};
use crate::widgets;

mod wizards;

pub use wizards::{wizard_ui, ApplySummary, Wizard, WizardOutcome};

/// Which wizard the toolbar launched. `TexTag` is the "Tag from LaTeX" flow
/// (formerly named Import — nothing is imported; it tags what a manuscript
/// cites, and the name collided with the real `.bib` import surface).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WizardKind {
    Organize,
    Autotag,
    TexTag,
}

/// Tag-table sort column.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TagSort {
    Name,
    #[default]
    Used,
    Recent,
}

/// An engine-touching request from the Tags tool; the app applies it.
pub enum TagAction {
    /// Rename a tag everywhere (`rename_tag`).
    Rename { from: String, to: String },
    /// Merge one tag into another (`rename_tag` onto an existing name).
    Merge { from: String, into: String },
    /// Delete a tag from every entry (`delete_tag`).
    Delete(String),
    /// Jump to an entry in the Library (Classic) view.
    Jump(String),
    /// Open one of the three wizards.
    Wizard(WizardKind),
}

/// One tag's derived stats (cached; rebuilt only when the library changes).
struct TagInfo {
    name: String,
    ns: String,
    value: String,
    count: usize,
    last_used: Option<String>,
}

/// View-local state for the Tags tool.
pub struct TagsState {
    pub selected: Option<String>,
    pub search: String,
    pub sort: TagSort,
    /// Detail rename lock (editable only when unlocked).
    pub locked: bool,
    collapsed: HashSet<String>,
    /// Session-local recolor overrides (persisting needs sidecar storage — later).
    colors: HashMap<String, Color32>,
    rename_buf: String,
    rename_for: Option<String>,
    hex_open: bool,
    hex_buf: String,
    /// Cached tag model + a signature of the library it was built from.
    model: Vec<TagInfo>,
    model_sig: Option<u64>,
}

impl Default for TagsState {
    fn default() -> Self {
        TagsState {
            selected: None,
            search: String::new(),
            sort: TagSort::default(),
            locked: true,
            collapsed: HashSet::new(),
            colors: HashMap::new(),
            rename_buf: String::new(),
            rename_for: None,
            hex_open: false,
            hex_buf: String::new(),
            model: Vec::new(),
            model_sig: None,
        }
    }
}

impl TagsState {
    fn color_of(&self, name: &str) -> Color32 {
        self.colors
            .get(name)
            .copied()
            .unwrap_or_else(|| tag_color(name))
    }

    /// Move a session-local recolor across a rename/merge. Won't clobber a color
    /// already chosen for the destination (a merge keeps the target's color).
    pub fn migrate_color(&mut self, from: &str, to: &str) {
        if let Some(c) = self.colors.remove(from) {
            self.colors.entry(to.to_string()).or_insert(c);
        }
    }
}

/// Split a `ns:value` tag into (namespace, value); a tag with no colon has an
/// empty namespace.
fn split_tag(name: &str) -> (String, String) {
    match name.split_once(':') {
        Some((ns, val)) => (ns.to_string(), val.to_string()),
        None => (String::new(), name.to_string()),
    }
}

/// A display label for a namespace (`topics` → "Topics", `wf` → "Workflow").
fn ns_label(ns: &str) -> String {
    match ns {
        "topics" => "Topics".into(),
        "wf" => "Workflow".into(),
        "pp" => "Paper projects".into(),
        "" => "Ungrouped".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => other.into(),
            }
        }
    }
}

fn fmt_date(d: Option<&str>) -> String {
    match d {
        None => "—".into(),
        Some(s) => {
            // Stored as YYYY-MM-DD; show "Mon D, YYYY" (best-effort, else raw).
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() == 3 {
                const M: [&str; 12] = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                if let (Ok(mo), Ok(day)) = (parts[1].parse::<usize>(), parts[2].parse::<u32>()) {
                    if (1..=12).contains(&mo) {
                        return format!("{} {}, {}", M[mo - 1], day, parts[0]);
                    }
                }
            }
            s.to_string()
        }
    }
}

/// (Re)compute the tag model only when the library changed. `gen` is the
/// library's explicit reload generation — an integer the app bumps on every
/// reload, immune to allocator coincidences (the previous pointer-identity
/// signature silently froze if a reload ever reused the same buffer).
fn ensure_model(st: &mut TagsState, entries: &[EntryView], gen: u64) {
    if st.model_sig == Some(gen) {
        return;
    }
    let sig = gen;
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    for e in entries {
        for t in &e.tags {
            *count.entry(t.clone()).or_insert(0) += 1;
            if let Some(added) = e.added.as_deref() {
                let slot = last.entry(t.clone()).or_default();
                if added > slot.as_str() {
                    *slot = added.to_string();
                }
            }
        }
    }
    st.model = count
        .into_iter()
        .map(|(name, count)| {
            let (ns, value) = split_tag(&name);
            let last_used = last.get(&name).cloned();
            TagInfo {
                name,
                ns,
                value,
                count,
                last_used,
            }
        })
        .collect();
    st.model_sig = Some(sig);
}

/// Render the Tags tool. Engine requests go into `actions`. `gen` is the
/// library's reload generation (the tag-model cache key).
pub fn tags_tab(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    gen: u64,
    st: &mut TagsState,
    actions: &mut Vec<TagAction>,
) {
    ensure_model(st, entries, gen);
    if st.selected.is_none() {
        st.selected = st.model.first().map(|t| t.name.clone());
    }

    // Right: detail panel for the selected tag (380px).
    let sel = st.selected.clone();
    if let Some(name) = sel
        .as_ref()
        .filter(|n| st.model.iter().any(|t| &t.name == *n))
    {
        let name = name.clone();
        egui::SidePanel::right("niu-tags-detail")
            .exact_width(380.0)
            .resizable(false)
            .frame(egui::Frame::default().fill(theme.surface))
            .show(ctx, |ui| detail(ui, theme, entries, st, &name, actions));
    }

    // Left: toolbar + sortable header + grouped table + footer.
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            toolbar(ui, theme, st, actions);
            col_header(ui, theme, st);
            table(ui, theme, st);
        });
}

// --------------------------------------------------------------------- toolbar

fn toolbar(ui: &mut egui::Ui, theme: &Theme, st: &mut TagsState, actions: &mut Vec<TagAction>) {
    egui::TopBottomPanel::top("niu-tags-toolbar")
        .exact_height(52.0)
        .frame(
            egui::Frame::default()
                .fill(theme.bg)
                .inner_margin(egui::Margin::symmetric(16, 0)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal_centered(|ui| {
                // search (bounded width)
                ui.allocate_ui_with_layout(
                    egui::vec2(260.0, 34.0),
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
                                            .hint_text("Filter tags…")
                                            .desired_width(f32::INFINITY)
                                            .frame(false),
                                    );
                                });
                            });
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::pri_btn(ui, theme, Glyph::Plus, "New tag").clicked() {
                        // Standalone-tag creation has no entry to attach to; the
                        // Organize wizard is the real "grow the vocabulary" path.
                        actions.push(TagAction::Wizard(WizardKind::Organize));
                    }
                    divider(ui, theme);
                    // "Tag from LaTeX", not "Import project": nothing is
                    // imported — the wizard tags entries a manuscript cites.
                    if wz_btn(ui, theme, Glyph::Download, "Tag from LaTeX", false).clicked() {
                        actions.push(TagAction::Wizard(WizardKind::TexTag));
                    }
                    if wz_btn(ui, theme, Glyph::Ai, "Auto-tag", true).clicked() {
                        actions.push(TagAction::Wizard(WizardKind::Autotag));
                    }
                    if wz_btn(ui, theme, Glyph::Sparkle, "Organize", true).clicked() {
                        actions.push(TagAction::Wizard(WizardKind::Organize));
                    }
                });
            });
        });
}

fn col_header(ui: &mut egui::Ui, theme: &Theme, st: &mut TagsState) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    let cy = rect.center().y;
    let right = rect.right();
    let used_x = right - 18.0 - 26.0 - 116.0 - 150.0;
    let recent_x = right - 18.0 - 26.0 - 116.0;
    let sort_head = |ui: &mut egui::Ui, st: &mut TagsState, x: f32, label: &str, key: TagSort| {
        let on = st.sort == key;
        let color = if on { theme.accent } else { theme.muted };
        let font = egui::FontId::proportional(11.0);
        ui.painter().text(
            egui::pos2(x, cy),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            color,
        );
        if on {
            let w = ui
                .painter()
                .layout_no_wrap(label.to_string(), font, color)
                .size()
                .x;
            icons::paint_at(
                ui,
                egui::Rect::from_center_size(egui::pos2(x + w + 6.0, cy), egui::vec2(11.0, 11.0)),
                Glyph::ChevronDown,
                theme.accent,
            );
        }
    };
    sort_head(ui, st, rect.left() + 18.0, "TAG", TagSort::Name);
    sort_head(ui, st, used_x, "USED IN", TagSort::Used);
    sort_head(ui, st, recent_x, "LAST USED", TagSort::Recent);
    // hit regions
    let hit = |a: f32, b: f32| {
        egui::Rect::from_min_max(egui::pos2(a, rect.top()), egui::pos2(b, rect.bottom()))
    };
    for (r, key) in [
        (hit(rect.left(), used_x - 8.0), TagSort::Name),
        (hit(used_x - 4.0, recent_x - 8.0), TagSort::Used),
        (hit(recent_x - 4.0, right), TagSort::Recent),
    ] {
        if ui
            .interact(
                r,
                ui.id().with(("tsort", label_of(key))),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            st.sort = key;
        }
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.border_2),
    );
}

fn label_of(s: TagSort) -> &'static str {
    match s {
        TagSort::Name => "name",
        TagSort::Used => "used",
        TagSort::Recent => "recent",
    }
}

fn table(ui: &mut egui::Ui, theme: &Theme, st: &mut TagsState) {
    let q = st.search.to_lowercase();
    let max_count = st.model.iter().map(|t| t.count).max().unwrap_or(1).max(1);
    // namespaces in first-seen (sorted) order
    let mut order: Vec<String> = Vec::new();
    for t in &st.model {
        if !order.contains(&t.ns) {
            order.push(t.ns.clone());
        }
    }

    // footer counts
    let total = st.model.len();

    egui::TopBottomPanel::bottom("niu-tags-footer")
        .exact_height(30.0)
        .frame(
            egui::Frame::default()
                .fill(theme.bg)
                .inner_margin(egui::Margin::symmetric(18, 0)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(
                    RichText::new(format!("{total} tags"))
                        .size(12.0)
                        .color(theme.text_2),
                );
                ui.label(RichText::new("·").color(theme.faint));
                ui.label(
                    RichText::new("manage the library vocabulary")
                        .size(12.0)
                        .color(theme.muted),
                );
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for ns in &order {
                        // Gather owned row data first so the `st.model` borrow ends
                        // before we mutate `st` (collapse / selection) below.
                        let data: Vec<(String, usize, Color32, String)> = {
                            let rows = sorted_rows(st, ns, &q);
                            rows.iter()
                                .map(|t| {
                                    (
                                        t.name.clone(),
                                        t.count,
                                        st.color_of(&t.name),
                                        fmt_date(t.last_used.as_deref()),
                                    )
                                })
                                .collect()
                        };
                        if data.is_empty() {
                            continue;
                        }
                        let collapsed = st.collapsed.contains(ns);
                        if group_header(ui, theme, &ns_label(ns), data.len(), collapsed) {
                            if collapsed {
                                st.collapsed.remove(ns);
                            } else {
                                st.collapsed.insert(ns.clone());
                            }
                        }
                        if collapsed {
                            continue;
                        }
                        for (name, count, color, last) in &data {
                            let selected = st.selected.as_deref() == Some(name.as_str());
                            if tag_row(ui, theme, name, *count, max_count, *color, last, selected) {
                                st.selected = Some(name.clone());
                                st.rename_for = None;
                            }
                        }
                    }
                });
        });
}

/// Rows for one namespace, filtered by search and ordered by the active sort.
fn sorted_rows<'a>(st: &'a TagsState, ns: &str, q: &str) -> Vec<&'a TagInfo> {
    // Match the full `ns:value` so typing a visible namespace prefix works too.
    let mut rows: Vec<&TagInfo> = st
        .model
        .iter()
        .filter(|t| t.ns == ns && t.name.to_lowercase().contains(q))
        .collect();
    match st.sort {
        TagSort::Name => rows.sort_by(|a, b| a.value.to_lowercase().cmp(&b.value.to_lowercase())),
        TagSort::Used => rows.sort_by(|a, b| b.count.cmp(&a.count)),
        TagSort::Recent => rows.sort_by(|a, b| b.last_used.cmp(&a.last_used)),
    }
    rows
}

fn group_header(ui: &mut egui::Ui, theme: &Theme, label: &str, n: usize, collapsed: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, theme.surface_2);
    let g = if collapsed {
        Glyph::ChevronRight
    } else {
        Glyph::ChevronDown
    };
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            egui::vec2(12.0, 12.0),
        ),
        g,
        theme.muted,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label.to_uppercase(),
        egui::FontId::proportional(11.0),
        theme.muted,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(16.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{n} tags"),
        theme::mono(11.0),
        theme.muted,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.border_2),
    );
    resp.clicked()
}

#[allow(clippy::too_many_arguments)]
fn tag_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    count: usize,
    max_count: usize,
    color: Color32,
    last: &str,
    selected: bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 50.0), egui::Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme.sel);
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
            egui::CornerRadius::ZERO,
            theme.sel_line,
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, theme.surface_2);
    }
    let cy = rect.center().y;
    let right = rect.right();
    let used_x = right - 18.0 - 26.0 - 116.0 - 150.0;
    let recent_x = right - 18.0 - 26.0 - 116.0;
    // color dot
    ui.painter().rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 18.0 + 6.0, cy),
            egui::vec2(12.0, 12.0),
        ),
        egui::CornerRadius::same(4),
        color,
    );
    // mono ns:value
    let (ns, value) = split_tag(name);
    let mut x = rect.left() + 18.0 + 23.0;
    if !ns.is_empty() {
        let nss = format!("{ns}:");
        let gal = ui
            .painter()
            .layout_no_wrap(nss.clone(), theme::mono(13.5), theme.faint);
        ui.painter().text(
            egui::pos2(x, cy),
            egui::Align2::LEFT_CENTER,
            &nss,
            theme::mono(13.5),
            theme.faint,
        );
        x += gal.size().x;
    }
    ui.painter().text(
        egui::pos2(x, cy),
        egui::Align2::LEFT_CENTER,
        &value,
        theme::mono(13.5),
        theme.text,
    );
    // usage bar + count
    let bar = egui::Rect::from_min_size(egui::pos2(used_x, cy - 3.0), egui::vec2(64.0, 6.0));
    ui.painter()
        .rect_filled(bar, egui::CornerRadius::same(3), theme.surface_2);
    let frac = (count as f32 / max_count as f32).clamp(0.08, 1.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(bar.min, egui::vec2(64.0 * frac, 6.0)),
        egui::CornerRadius::same(3),
        color,
    );
    ui.painter().text(
        egui::pos2(used_x + 64.0 + 10.0, cy),
        egui::Align2::LEFT_CENTER,
        count.to_string(),
        egui::FontId::proportional(12.5),
        theme.text_2,
    );
    // last-used date
    ui.painter().text(
        egui::pos2(recent_x, cy),
        egui::Align2::LEFT_CENTER,
        last,
        egui::FontId::proportional(12.5),
        theme.muted,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, theme.border_2),
    );
    resp.clicked()
}

// ---------------------------------------------------------------------- detail

#[allow(clippy::too_many_arguments)]
fn detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    st: &mut TagsState,
    name: &str,
    actions: &mut Vec<TagAction>,
) {
    let (ns, value) = split_tag(name);
    let color = st.color_of(name);
    let count = st
        .model
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.count)
        .unwrap_or(0);
    let last = st
        .model
        .iter()
        .find(|t| t.name == name)
        .and_then(|t| t.last_used.clone());

    // Header (pill + lock) + name + stats.
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 22,
            right: 22,
            top: 16,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                type_pill(ui, theme, Glyph::Tag, "TAG");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::library::lock_toggle(ui, theme, st.locked).clicked() {
                        st.locked = !st.locked;
                        st.rename_for = None;
                    }
                    ui.label(
                        RichText::new(if st.locked { "Locked" } else { "Editing" })
                            .size(11.0)
                            .strong()
                            .color(theme.muted),
                    );
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let (sw, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(sw, egui::CornerRadius::same(8), color);
                ui.add_space(6.0);
                if !ns.is_empty() {
                    ui.label(
                        RichText::new(format!("{ns}:"))
                            .font(theme::mono(21.0))
                            .color(theme.faint),
                    );
                }
                if st.locked {
                    ui.label(
                        RichText::new(&value)
                            .font(theme::mono(21.0))
                            .color(theme.text),
                    );
                } else {
                    if st.rename_for.as_deref() != Some(name) {
                        st.rename_buf = value.clone();
                        st.rename_for = Some(name.to_string());
                    }
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut st.rename_buf)
                            .font(theme::mono(21.0))
                            .desired_width(200.0),
                    );
                    // Commit only on Enter (the project's submit idiom): a
                    // rename rewrites every carrying entry, so Esc or a stray
                    // click elsewhere must revert, not silently commit.
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let nv = st.rename_buf.trim();
                            if !nv.is_empty() && nv != value {
                                let to = if ns.is_empty() {
                                    nv.to_string()
                                } else {
                                    format!("{ns}:{nv}")
                                };
                                actions.push(TagAction::Rename {
                                    from: name.to_string(),
                                    to,
                                });
                            }
                        } else {
                            st.rename_buf = value.clone();
                        }
                    }
                }
            });
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 26.0;
                td_stat(ui, theme, &count.to_string(), "entries");
                td_stat(ui, theme, &fmt_date(last.as_deref()), "last used");
            });
            ui.add_space(6.0);
        });

    // Controls (color + namespace) — only meaningful when editing.
    controls(ui, theme, st, name, &ns, &value, actions);

    // Actions (Merge into… · Delete).
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(22, 16))
        .show(ui, |ui| {
            let others: Vec<String> = st
                .model
                .iter()
                .map(|t| t.name.clone())
                .filter(|n| n != name)
                .collect();
            ui.menu_button(RichText::new("  Merge into…").color(theme.text), |ui| {
                if others.is_empty() {
                    ui.label(RichText::new("(no other tags)").color(theme.muted));
                }
                for other in &others {
                    if ui.button(other).clicked() {
                        actions.push(TagAction::Merge {
                            from: name.to_string(),
                            into: other.clone(),
                        });
                        ui.close();
                    }
                }
            });
            ui.add_space(8.0);
            let del = egui::Button::new(
                RichText::new("Delete tag")
                    .size(13.0)
                    .strong()
                    .color(theme.rose),
            )
            .fill(theme.surface)
            .stroke(egui::Stroke::new(1.0, theme.rose.gamma_multiply(0.45)))
            .corner_radius(8.0)
            .min_size(egui::vec2(ui.available_width(), 32.0));
            if ui.add(del).clicked() {
                actions.push(TagAction::Delete(name.to_string()));
            }
        });

    // Tagged entries (virtualized).
    tagged_entries(ui, theme, entries, name, actions);
}

#[allow(clippy::too_many_arguments)]
fn controls(
    ui: &mut egui::Ui,
    theme: &Theme,
    st: &mut TagsState,
    name: &str,
    ns: &str,
    value: &str,
    actions: &mut Vec<TagAction>,
) {
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 22,
            right: 22,
            top: 6,
            bottom: 18,
        })
        .show(ui, |ui| {
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.min_rect().top(),
                egui::Stroke::new(1.0, theme.border_2),
            );
            // Colors are session-local until the sidecar can store them —
            // say so, or users will pick colors and silently lose them.
            widgets::section_label(ui, theme, "Color (this session only)", 12.0, 9.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(9.0, 9.0);
                let cur = st.color_of(name);
                for (r, g, b) in PALETTE {
                    let c = Color32::from_rgb(r, g, b);
                    if widgets::swatch(ui, c, cur == c, 26.0, 2.0) {
                        st.colors.insert(name.to_string(), c);
                    }
                }
                divider(ui, theme);
                if hex_swatch(ui, theme, st.hex_open) {
                    st.hex_open = !st.hex_open;
                    if st.hex_open && st.hex_buf.is_empty() {
                        st.hex_buf = "#".into();
                    }
                }
            });
            if st.hex_open {
                ui.add_space(11.0);
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut st.hex_buf)
                            .font(theme::mono(13.0))
                            .desired_width(120.0)
                            .hint_text("#RRGGBB"),
                    );
                    if r.changed() {
                        if let Some(c) = parse_hex(&st.hex_buf) {
                            st.colors.insert(name.to_string(), c);
                        }
                    }
                    if ghost(ui, theme, "Done").clicked() {
                        st.hex_open = false;
                    }
                });
            }
            ui.add_space(18.0);
            widgets::section_label(ui, theme, "Namespace", 12.0, 9.0);
            // Selecting a different namespace *moves* the tag (a real rename).
            let nslist: Vec<String> = {
                let mut v: Vec<String> = st.model.iter().map(|t| t.ns.clone()).collect();
                v.sort();
                v.dedup();
                v.retain(|n| !n.is_empty());
                v
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(7.0, 7.0);
                for other in &nslist {
                    let on = other == ns;
                    if ns_pill(ui, theme, &ns_label(other), on) && !on {
                        actions.push(TagAction::Rename {
                            from: name.to_string(),
                            to: format!("{other}:{value}"),
                        });
                    }
                }
            });
        });
}

fn tagged_entries(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    name: &str,
    actions: &mut Vec<TagAction>,
) {
    let shown: Vec<usize> = {
        let mut v: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].tags.iter().any(|t| t == name))
            .collect();
        v.sort_by(|&a, &b| entries[b].added.cmp(&entries[a].added)); // newest first
        v
    };
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 22,
            right: 22,
            top: 4,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("TAGGED ENTRIES")
                        .size(11.0)
                        .strong()
                        .color(theme.muted),
                );
                ui.label(
                    RichText::new(shown.len().to_string())
                        .font(theme::mono(11.0))
                        .color(theme.faint),
                );
            });
        });
    ui.add_space(6.0);
    let row_h = 52.0;
    egui::ScrollArea::vertical()
        .id_salt("niu-tags-entries")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, shown.len(), |ui, range| {
            for i in range {
                let e = &entries[shown[i]];
                if entry_row(ui, theme, e, row_h) {
                    actions.push(TagAction::Jump(e.citekey.clone()));
                }
            }
        });
}

fn entry_row(ui: &mut egui::Ui, theme: &Theme, e: &EntryView, row_h: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(10.0, 4.0)),
            egui::CornerRadius::same(9),
            theme.surface_2,
        );
    }
    let cy = rect.center().y;
    let (g, gc) = type_glyph(theme, &e.entry_type);
    icons::paint_at(
        ui,
        egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, cy), egui::vec2(16.0, 16.0)),
        g,
        gc,
    );
    let x = rect.left() + 40.0;
    let title = crate::tex::display(
        e.fields
            .get("title")
            .map(String::as_str)
            .unwrap_or("(untitled)"),
    );
    ui.painter().text(
        egui::pos2(x, cy - 8.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(&title, ((rect.width() - 56.0) / 7.4) as usize),
        theme::serif(14.0),
        theme.text,
    );
    let year = e.fields.get("year").map(String::as_str).unwrap_or("");
    let sub = format!("{} · {}", creator_of(e), year);
    ui.painter().text(
        egui::pos2(x, cy + 9.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(&sub, ((rect.width() - 56.0) / 6.4) as usize),
        egui::FontId::proportional(11.5),
        theme.muted,
    );
    resp.clicked()
}

// ----------------------------------------------------------- tiny widgets

const PALETTE: [(u8, u8, u8); 8] = [
    (0x1F, 0x8A, 0x5B),
    (0x2A, 0x6F, 0xDB),
    (0xB6, 0x79, 0x2B),
    (0x8A, 0x5B, 0xD9),
    (0xC9, 0x8A, 0x2B),
    (0xC2, 0x53, 0x6B),
    (0x2F, 0x8E, 0x8A),
    (0xD9, 0x77, 0x57),
];

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    let h = if h.len() == 3 {
        h.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        h.to_string()
    };
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

fn td_stat(ui: &mut egui::Ui, theme: &Theme, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).size(22.0).strong().color(theme.text));
        ui.label(RichText::new(label).size(11.5).color(theme.muted));
    });
}

fn type_pill(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, label: &str) {
    egui::Frame::default()
        .fill(theme.accent_tint)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, glyph, 13.0, theme.accent);
                ui.label(RichText::new(label).size(11.0).strong().color(theme.accent));
            });
        });
}

fn hex_swatch(ui: &mut egui::Ui, theme: &Theme, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
    // a little rainbow hint + a # chip
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), theme.surface_2);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "#",
        theme::mono(12.0),
        if on { theme.accent } else { theme.text_2 },
    );
    if on {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            egui::CornerRadius::same(10),
            egui::Stroke::new(2.0, theme.accent),
            egui::StrokeKind::Outside,
        );
    }
    resp.clicked()
}

fn ns_pill(ui: &mut egui::Ui, theme: &Theme, label: &str, on: bool) -> bool {
    let fg = if on { theme.accent } else { theme.text_2 };
    let resp = egui::Frame::default()
        .fill(if on { theme.accent_tint } else { theme.surface })
        .stroke(egui::Stroke::new(
            1.0,
            if on {
                Color32::TRANSPARENT
            } else {
                theme.border
            },
        ))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 5))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(12.5).strong().color(fg));
        })
        .response
        .interact(egui::Sense::click());
    resp.clicked()
}

fn divider(ui: &mut egui::Ui, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(1.0, 22.0), egui::Sense::hover());
    ui.painter().vline(
        r.center().x,
        (r.center().y - 11.0)..=(r.center().y + 11.0),
        egui::Stroke::new(1.0, theme.border),
    );
}

fn wz_btn(ui: &mut egui::Ui, theme: &Theme, icon: Glyph, label: &str, ai: bool) -> egui::Response {
    let col = if ai { theme.accent } else { theme.text_2 };
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(11, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, icon, 16.0, col);
                ui.label(RichText::new(label).size(13.0).color(theme.text));
            });
        })
        .response
        .interact(egui::Sense::click())
}

fn ghost(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    egui::Frame::default()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(11, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(13.0).color(theme.text));
        })
        .response
        .interact(egui::Sense::click())
}
