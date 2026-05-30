//! Normalize — the cleanup engine (design spec §6), wired to the real engine.
//!
//! Sub-navigation: **Overview · Review changes · Ruleset · Re-key**.
//! - Overview's Health report is `engine::analyze` (one offline scan, per check
//!   the entries that fail it); "Offline cleanup → Run" jumps to Review.
//! - Review renders `engine::normalize_preview` (field-level diffs); per-entry
//!   Accept applies that entry's changes via `engine::edit`, Apply-all uses
//!   `engine::normalize_apply` when nothing is rejected (a single atomic pass).
//! - Re-key renders `engine::rekey_preview`; Apply uses `engine::rekey_apply`.
//! - Ruleset shows the real rule classes; the toggles are display-only for now
//!   (the engine has no per-rule persistence API — runs use `.niutero/norm.toml`).
//!
//! Like the Library views, this is a pure render over engine data ([`NormCache`],
//! refreshed by the app) plus view-local [`NormalizeState`]; engine-touching
//! requests come back as [`NormAction`]s the app applies after rendering.

use std::collections::HashMap;

use eframe::egui::{self, RichText};
use niutero_engine::{AnalysisReport, EntryView, NormChange, Rekey};

use crate::icons::{self, Glyph};
use crate::theme::{self, Theme};
use crate::widgets;

/// The four Normalize sub-views.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NormView {
    Overview,
    Review,
    Ruleset,
    Rekey,
}

/// The rule classes the cleanup engine applies (display + their defaults). The
/// toggles are not yet persisted — see the module doc.
const RULES: [(&str, &str, &str, bool); 6] = [
    (
        "Venue canonicalization",
        "146 aliases",
        "Map venue aliases to one canonical name (e.g. “Proc. of ICML” → “ICML”).",
        true,
    ),
    (
        "Title casing",
        "Title Case",
        "Normalize paper titles to a consistent Title Case.",
        true,
    ),
    (
        "Promote arXiv → published",
        "online",
        "When a published version exists, promote the arXiv preprint to it.",
        true,
    ),
    (
        "Required fields",
        "url · year · author",
        "Flag entries missing fields the library expects.",
        true,
    ),
    (
        "Duplicate detection",
        "off",
        "Cluster likely-duplicate entries by title/DOI similarity.",
        false,
    ),
    (
        "Author name format",
        "Last, First",
        "Normalize author names to “Last, First”.",
        true,
    ),
]; // NB: keep in sync with the engine's offline passes.

/// View-local UI state for the Normalize tool.
pub struct NormalizeState {
    pub view: NormView,
    /// Selected health row (Overview).
    pub picked: Option<String>,
    /// Per-entry accept (`true`) / reject (`false`) decisions (Review).
    pub done: HashMap<String, bool>,
    /// Local ruleset toggles (display-only; see module doc).
    pub rules: [bool; 6],
}

impl Default for NormalizeState {
    fn default() -> Self {
        NormalizeState {
            view: NormView::Overview,
            picked: None,
            done: HashMap::new(),
            rules: RULES.map(|r| r.3),
        }
    }
}

/// Engine data the tab renders, refreshed by the app (see `app::ensure_norm`).
pub struct NormCache {
    pub report: AnalysisReport,
    pub diffs: Vec<NormChange>,
    pub rekey: Vec<Rekey>,
    pub pattern: String,
    pub total: usize,
}

/// An engine-touching request the tab makes; the app applies it post-render.
pub enum NormAction {
    /// Run offline cleanup (the preview is cached) and jump to Review.
    RunOffline,
    /// Start the Online-enrich background task (an online feature).
    StartEnrich,
    /// Apply every staged change not rejected.
    ApplyAll,
    /// Copy the staged diff as a text patch.
    CopyPatch,
    /// Rewrite citation keys from the pattern (`engine::rekey_apply`).
    ApplyRekey,
}

/// Render the Normalize tool.
pub fn normalize(
    ctx: &egui::Context,
    theme: &Theme,
    entries: &[EntryView],
    cache: &NormCache,
    st: &mut NormalizeState,
    actions: &mut Vec<NormAction>,
) {
    egui::SidePanel::left("niu-norm-nav")
        .exact_width(224.0)
        .resizable(false)
        .frame(
            egui::Frame::default()
                .fill(theme.surface)
                .inner_margin(egui::Margin::symmetric(12, 18)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let staged = cache.diffs.len();
            for (icon, label, badge, v) in [
                (Glyph::CheckCircle, "Overview", None, NormView::Overview),
                (
                    Glyph::Copy,
                    "Review changes",
                    Some(staged),
                    NormView::Review,
                ),
                (Glyph::Filter, "Ruleset", None, NormView::Ruleset),
                (Glyph::Key, "Re-key", None, NormView::Rekey),
            ] {
                if widgets::subnav_item(ui, theme, icon, label, badge, st.view == v) {
                    st.view = v;
                }
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::default()
                        .inner_margin(egui::Margin {
                            left: 38,
                            right: 38,
                            top: 30,
                            bottom: 48,
                        })
                        .show(ui, |ui| {
                            widgets::centered_column(ui, 880.0, |ui| match st.view {
                                NormView::Overview => overview(ui, theme, cache, st, actions),
                                NormView::Review => review(ui, theme, entries, cache, st, actions),
                                NormView::Ruleset => ruleset(ui, theme, st),
                                NormView::Rekey => rekey(ui, theme, cache, actions),
                            });
                        });
                });
        });
}

// ------------------------------------------------------------------ Overview

fn overview(
    ui: &mut egui::Ui,
    theme: &Theme,
    cache: &NormCache,
    st: &mut NormalizeState,
    actions: &mut Vec<NormAction>,
) {
    widgets::tab_header(
        ui,
        theme,
        "Overview",
        "Plan and run the cleanup, and see what needs attention.",
        &format!("{} entries", cache.total),
    );

    // Recommended plan.
    widgets::card(theme).show(ui, |ui| {
        widgets::card_head(ui, theme, Glyph::Sparkle, "Recommended plan", None);
        if plan_row(
            ui,
            theme,
            Glyph::Refresh,
            "Offline cleanup",
            "Run local rule passes — venue canonicalization, title casing, required fields.",
            &format!("{} changes", cache.diffs.len()),
            "Run",
            false,
            true,
        ) {
            actions.push(NormAction::RunOffline);
        }
        if plan_row(
            ui,
            theme,
            Glyph::Download,
            "Online enrich",
            "Fetch published versions & metadata for arXiv preprints.",
            "network · rate-limited",
            "Run",
            true,
            false,
        ) {
            actions.push(NormAction::StartEnrich);
        }
    });
    ui.add_space(22.0);

    // Health.
    let need = cache
        .report
        .checks
        .iter()
        .filter(|c| !c.keys.is_empty())
        .count();
    let total_checks = cache.report.checks.len();
    widgets::card(theme).show(ui, |ui| {
        widgets::card_head(
            ui,
            theme,
            Glyph::CheckCircle,
            "Health",
            Some(&format!("{need} of {total_checks} checks need attention")),
        );
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                for c in &cache.report.checks {
                    let count = c.keys.len();
                    let picked = st.picked.as_deref() == Some(c.id.as_str());
                    match health_row(ui, theme, &c.label, &c.hint, count, picked) {
                        HealthClick::Pick => st.picked = Some(c.id.clone()),
                        HealthClick::Fix => actions.push(NormAction::RunOffline),
                        HealthClick::None => {}
                    }
                }
            });
    });
}

#[allow(clippy::too_many_arguments)]
fn plan_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Glyph,
    title: &str,
    desc: &str,
    tag: &str,
    btn_label: &str,
    primary: bool,
    border_below: bool,
) -> bool {
    let mut clicked = false;
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // icon box
                let (bx, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(bx, egui::CornerRadius::same(10), theme.surface_2);
                icons::paint_at(ui, bx.shrink(9.0), icon, theme.text_2);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(15.0).strong().color(theme.text));
                    ui.add_space(2.0);
                    ui.label(RichText::new(desc).size(13.0).color(theme.text_2));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, theme, None, btn_label, primary, 32.0).clicked() {
                        clicked = true;
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(tag).size(11.5).strong().color(theme.muted));
                });
            });
        });
    if border_below {
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1.0, theme.border_2),
        );
    }
    clicked
}

enum HealthClick {
    None,
    Pick,
    Fix,
}

fn health_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    hint: &str,
    count: usize,
    picked: bool,
) -> HealthClick {
    let issue = count > 0;
    let mut out = HealthClick::None;
    let resp = egui::Frame::default()
        .fill(if picked && issue {
            theme.sel
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(12, 11))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (g, c) = if issue {
                    (Glyph::Warn, theme.amber)
                } else {
                    (Glyph::Check, theme.accent)
                };
                icons::show(ui, g, 17.0, c);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(label).size(14.0).strong().color(theme.text));
                    ui.label(RichText::new(hint).size(12.0).color(theme.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if issue {
                        if widgets::button(ui, theme, None, "Fix", true, 28.0).clicked() {
                            out = HealthClick::Fix;
                        }
                        if widgets::button(ui, theme, None, "View", false, 28.0).clicked() {
                            out = HealthClick::Pick;
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(count.to_string())
                            .size(16.0)
                            .strong()
                            .color(if issue { theme.amber } else { theme.faint }),
                    );
                });
            });
        });
    if issue
        && matches!(out, HealthClick::None)
        && ui
            .interact(
                resp.response.rect,
                ui.id().with(("health", label)),
                egui::Sense::click(),
            )
            .clicked()
    {
        out = HealthClick::Pick;
    }
    out
}

// -------------------------------------------------------------------- Review

fn review(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    cache: &NormCache,
    st: &mut NormalizeState,
    actions: &mut Vec<NormAction>,
) {
    // Success banner.
    egui::Frame::default()
        .fill(theme.accent_tint)
        .stroke(egui::Stroke::new(1.0, theme.accent))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(18, 13))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, Glyph::CheckCircle, 19.0, theme.accent);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Offline cleanup finished. {} change{} staged — nothing is written to \
                         disk until you apply.",
                        cache.diffs.len(),
                        if cache.diffs.len() == 1 { "" } else { "s" }
                    ))
                    .size(13.5)
                    .color(theme.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, theme, None, "Back to overview", false, 30.0).clicked() {
                        st.view = NormView::Overview;
                    }
                });
            });
        });
    ui.add_space(18.0);

    // Bulk actions.
    ui.horizontal(|ui| {
        if widgets::button(ui, theme, Some(Glyph::Check), "Apply all", true, 32.0).clicked() {
            actions.push(NormAction::ApplyAll);
        }
        if widgets::button(ui, theme, None, "Reject all", false, 32.0).clicked() {
            for d in &cache.diffs {
                st.done.insert(d.citekey.clone(), false);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, theme, Some(Glyph::Copy), "Copy as patch", false, 32.0).clicked()
            {
                actions.push(NormAction::CopyPatch);
            }
        });
    });
    ui.add_space(18.0);

    if cache.diffs.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Nothing to change — the library is already clean.")
                    .color(theme.muted),
            );
        });
        return;
    }

    for d in &cache.diffs {
        diff_card(ui, theme, entries, d, st);
        ui.add_space(16.0);
    }
}

fn diff_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[EntryView],
    d: &NormChange,
    st: &mut NormalizeState,
) {
    let rejected = st.done.get(&d.citekey) == Some(&false);
    let accepted = st.done.get(&d.citekey) == Some(&true);
    let title = entries
        .iter()
        .find(|e| e.citekey == d.citekey)
        .and_then(|e| e.fields.get("title"))
        .map(String::as_str)
        .unwrap_or("(untitled)");
    let rule = d
        .notes
        .first()
        .cloned()
        .unwrap_or_else(|| "Normalize".into());

    widgets::card(theme).show(ui, |ui| {
        if rejected {
            ui.disable();
        }
        // header
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(18, 13))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(crate::library::ellipsize(title, 64))
                                .font(theme::serif(15.5))
                                .color(theme.text),
                        );
                        ui.label(
                            RichText::new(&d.citekey)
                                .font(theme::mono(11.5))
                                .color(theme.accent),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        chip(ui, theme, &rule);
                    });
                });
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1.0, theme.border_2),
        );
        // changes
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(0, 6))
            .show(ui, |ui| {
                for c in &d.diffs {
                    diff_row(ui, theme, &c.field, c.from.as_deref(), c.to.as_deref());
                }
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1.0, theme.border_2),
        );
        // footer
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(18, 11))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Accept/Reject are *staging* decisions only — nothing is
                    // written until "Apply all" (so the banner's promise holds).
                    let acc_label = if accepted { "Accepted ✓" } else { "Accept" };
                    if widgets::button(ui, theme, None, acc_label, true, 30.0).clicked() {
                        st.done.insert(d.citekey.clone(), true);
                    }
                    if widgets::button(ui, theme, None, "Reject", false, 30.0).clicked() {
                        st.done.insert(d.citekey.clone(), false);
                    }
                });
            });
    });
}

fn diff_row(ui: &mut egui::Ui, theme: &Theme, field: &str, from: Option<&str>, to: Option<&str>) {
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(18, 8))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (fr, _) = ui.allocate_exact_size(egui::vec2(108.0, 18.0), egui::Sense::hover());
                ui.painter().text(
                    fr.left_top() + egui::vec2(0.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    field,
                    theme::mono(12.0),
                    theme.muted,
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    // old value (rose, struck through unless absent)
                    let old = from.unwrap_or("—");
                    value_chip(ui, old, theme.rose, from.is_some());
                    icons::show(ui, Glyph::ArrowRight, 14.0, theme.faint);
                    // new value (accent)
                    let new = to.unwrap_or("(removed)");
                    value_chip(ui, new, theme.accent, false);
                });
            });
        });
}

fn value_chip(ui: &mut egui::Ui, text: &str, color: egui::Color32, strike: bool) {
    let bg = color.gamma_multiply(0.14);
    egui::Frame::default()
        .fill(bg)
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            let mut rt = RichText::new(crate::library::ellipsize(text, 80))
                .size(13.0)
                .color(color);
            if strike {
                rt = rt.strikethrough();
            }
            ui.label(rt);
        });
}

// ------------------------------------------------------------------- Ruleset

fn ruleset(ui: &mut egui::Ui, theme: &Theme, st: &mut NormalizeState) {
    widgets::tab_header(
        ui,
        theme,
        "Ruleset",
        "The passes the cleanup engine runs. Stored in .niutero/norm.toml.",
        "",
    );
    widgets::card(theme).show(ui, |ui| {
        for (i, (name, meta, desc, _)) in RULES.iter().enumerate() {
            egui::Frame::default()
                .inner_margin(egui::Margin::symmetric(20, 16))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(*name).size(14.5).strong().color(theme.text),
                                );
                                meta_tag(ui, theme, meta);
                            });
                            ui.add_space(3.0);
                            ui.label(RichText::new(*desc).size(13.0).color(theme.text_2));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::toggle(ui, theme, st.rules[i]) {
                                st.rules[i] = !st.rules[i];
                            }
                        });
                    });
                });
            if i + 1 < RULES.len() {
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1.0, theme.border_2),
                );
            }
        }
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new(
            "Toggle changes aren't persisted yet — runs use the library's norm.toml profile.",
        )
        .size(11.5)
        .color(theme.faint),
    );
}

// -------------------------------------------------------------------- Re-key

fn rekey(ui: &mut egui::Ui, theme: &Theme, cache: &NormCache, actions: &mut Vec<NormAction>) {
    let clashes = cache.rekey.iter().filter(|r| r.disambiguated).count();
    widgets::tab_header(
        ui,
        theme,
        "Re-key",
        "Regenerate citation keys from the library pattern.",
        &format!("{} entries", cache.total),
    );

    // Pattern info card.
    widgets::card(theme).show(ui, |ui| {
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    icons::show(ui, Glyph::Key, 18.0, theme.accent);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(&cache.pattern)
                            .font(theme::mono(13.5))
                            .color(theme.text),
                    );
                    if clashes > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{clashes} collision{} — +suffix will be added",
                                    if clashes == 1 { "" } else { "s" }
                                ))
                                .size(12.5)
                                .color(theme.amber),
                            );
                            icons::show(ui, Glyph::Warn, 14.0, theme.amber);
                        });
                    }
                });
            });
    });
    ui.add_space(18.0);

    // Preview table.
    widgets::card(theme).show(ui, |ui| {
        // header
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(20, 11))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("CURRENT KEY")
                            .size(11.0)
                            .strong()
                            .color(theme.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(ui.available_width() * 0.5 - 30.0);
                        ui.label(
                            RichText::new("NEW KEY")
                                .size(11.0)
                                .strong()
                                .color(theme.muted),
                        );
                    });
                });
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1.0, theme.border_2),
        );
        if cache.rekey.is_empty() {
            egui::Frame::default()
                .inner_margin(egui::Margin::symmetric(20, 20))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("All citation keys already match the pattern.")
                            .color(theme.muted),
                    );
                });
        }
        for (i, r) in cache.rekey.iter().enumerate() {
            rekey_row(ui, theme, &r.citekey, &r.new_key, r.disambiguated);
            if i + 1 < cache.rekey.len() {
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1.0, theme.border_2),
                );
            }
        }
    });
    ui.add_space(18.0);

    // Actions.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if widgets::button(ui, theme, Some(Glyph::Key), "Apply re-key", true, 32.0).clicked() {
            actions.push(NormAction::ApplyRekey);
        }
        let label = format!("Preview all {}", cache.total);
        let _ = widgets::button(ui, theme, None, &label, false, 32.0);
    });
}

fn rekey_row(ui: &mut egui::Ui, theme: &Theme, old: &str, new: &str, clash: bool) {
    egui::Frame::default()
        .inner_margin(egui::Margin::symmetric(20, 12))
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                cols[0].label(
                    RichText::new(old)
                        .font(theme::mono(12.5))
                        .color(theme.muted)
                        .strikethrough(),
                );
                cols[1].horizontal(|ui| {
                    icons::show(ui, Glyph::ArrowRight, 15.0, theme.faint);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(new)
                            .font(theme::mono(12.5))
                            .color(theme.accent),
                    );
                    if clash {
                        clash_badge(ui, theme);
                    }
                });
            });
        });
}

// ------------------------------------------------------------- tiny widgets

fn chip(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    egui::Frame::default()
        .fill(theme.accent_tint)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(theme.accent));
        });
}

fn meta_tag(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    egui::Frame::default()
        .fill(theme.surface_2)
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(theme::mono(10.5))
                    .color(theme.muted),
            );
        });
}

fn clash_badge(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::default()
        .fill(theme.amber.gamma_multiply(0.14))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new("+a").size(10.0).color(theme.amber));
        });
}
