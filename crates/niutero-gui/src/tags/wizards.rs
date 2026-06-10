//! Wizards — three multi-step flows launched from the Tags toolbar. All real:
//!   Organize  : `engine::organize_tags` proposes merges + new tags; the app
//!               applies the accepted merges via `rename_tag`.
//!   Auto-tag  : `engine::suggest_tags` per untagged entry; the app applies the
//!               accepted assignments via `set_tags`.
//!   Import    : `tex_scan` resolves `\cite{}` keys, then tags the matched
//!               entries `pp:<project>`.
//!
//! The model runs off-thread (see `app::start_organize` / `start_auto_tag`); the
//! wizard shows a spinner (`busy`) until the app feeds results back via
//! `set_organize` / `set_autotag`, or resets it with `fail`.

use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText};
use niutero_engine::{EntryView, TagMerge, TagSuggestion};

use crate::icons::{self, Glyph};
use crate::library::{ellipsize, tag_color, type_glyph};
use crate::theme::{self, Theme};
use crate::widgets;

use super::{ghost, split_tag, WizardKind};

/// What a wizard asks the app to do this frame.
pub enum WizardOutcome {
    /// Stay open.
    Keep,
    /// Dismiss.
    Close,
    /// Run a LaTeX cite-scan over these files (the app has the vault; it fills
    /// the wizard's matched/unmatched lists and advances to the review step).
    ScanImport { files: Vec<std::path::PathBuf> },
    /// Tag these matched cite keys with `tag` (`pp:<project>`).
    ApplyImport { tag: String, keys: Vec<String> },
    /// Ask the model to tidy the vocabulary (off-thread); results come back via
    /// [`Wizard::set_organize`].
    RunOrganize { instructions: String },
    /// Apply the accepted merges (`from` → `into`); `new_tags` are advisory.
    ApplyOrganize {
        merges: Vec<(String, String)>,
        new_tags: Vec<String>,
    },
    /// Auto-tag these entries (off-thread); results come back via
    /// [`Wizard::set_autotag`].
    RunAutotag { keys: Vec<String> },
    /// Apply the accepted per-entry tag assignments.
    ApplyAutotag {
        assignments: Vec<(String, Vec<String>)>,
    },
}

/// What a footer button does. The `Run*`/`Apply*` actions read their data from
/// the wizard at apply time, so the body only needs to name the action.
#[derive(Clone, Copy)]
enum Nav {
    None,
    Close,
    Step(usize),
    RunOrganize,
    ApplyOrganize,
    RunAutotag,
    ApplyAutotag,
    ScanImport,
    ApplyImport,
}

/// The pinned footer the current step wants: a left (Back/Cancel) button and a
/// right primary button, each with an action. Built by the body, rendered by the
/// shell so it sits *below* the scrolling content rather than inside it.
struct Footer {
    back: Option<&'static str>,
    primary: String,
    enabled: bool,
    on_back: Nav,
    on_primary: Nav,
}

/// Entries auto-tagged per run are capped so an "Auto-tag" doesn't fan out into
/// hundreds of sequential model calls; we target the *untagged* entries first.
const AUTOTAG_MAX: usize = 20;

/// What actually happened when the app applied a wizard's output. The Done
/// step reports these real counts — never the accepted counts, which would
/// claim success for entries that vanished or merges the engine rejected.
#[derive(Default, Clone)]
pub struct ApplySummary {
    /// Entries changed (auto-tag/import) or merges that moved entries.
    pub applied: usize,
    /// No-ops: unknown citekeys, merges whose `from` matched nothing.
    pub skipped: usize,
    /// Engine errors (the loop continued past them).
    pub failed: usize,
    /// The first error string, for the recap.
    pub first_error: Option<String>,
    /// Auto-tag/import: tags actually added across `applied` entries.
    pub tags: usize,
}

/// One open wizard's state.
pub struct Wizard {
    kind: WizardKind,
    step: usize, // 0 setup · 1 review · 2 done
    /// True while an off-thread model call is in flight (shows the spinner).
    busy: bool,
    /// Real apply outcome, fed back by the app before the Done step renders.
    applied: Option<ApplySummary>,
    // organize (real — filled by `set_organize`)
    org_opts: [bool; 2], // merge, suggest
    org_prompt: String,
    org_merges: Vec<TagMerge>,
    org_new: Vec<TagSuggestion>,
    org_merge_acc: Vec<bool>,
    org_new_acc: Vec<bool>,
    // auto-tag (real — `at_keys` set at run time, `at_assign` by `set_autotag`)
    at_keys: Vec<String>,
    at_assign: Vec<(String, Vec<String>)>, // (citekey, suggested tags)
    at_drop: HashSet<(usize, usize)>,      // (paper index, tag index) dropped
    // import (real)
    imp_files: Vec<std::path::PathBuf>,
    imp_proj: String,
    imp_scanned: bool,
    imp_matched: Vec<(String, bool)>, // citekey, accepted
    imp_unmatched: Vec<String>,
}

impl Wizard {
    pub fn new(kind: WizardKind) -> Self {
        Wizard {
            kind,
            step: 0,
            busy: false,
            applied: None,
            org_opts: [true, true],
            org_prompt: String::new(),
            org_merges: Vec::new(),
            org_new: Vec::new(),
            org_merge_acc: Vec::new(),
            org_new_acc: Vec::new(),
            at_keys: Vec::new(),
            at_assign: Vec::new(),
            at_drop: HashSet::new(),
            imp_files: Vec::new(),
            imp_proj: String::new(),
            imp_scanned: false,
            imp_matched: Vec::new(),
            imp_unmatched: Vec::new(),
        }
    }

    /// Called by the app after a `ScanImport`: record the tex-scan result and
    /// advance to the review step.
    pub fn set_scan(&mut self, matched: Vec<String>, missing: Vec<String>) {
        self.imp_matched = matched.into_iter().map(|k| (k, true)).collect();
        self.imp_unmatched = missing;
        self.imp_scanned = true;
        self.step = 1;
    }

    /// Record an Organize plan from the model and advance to the review step.
    pub fn set_organize(&mut self, merges: Vec<TagMerge>, new_tags: Vec<TagSuggestion>) {
        self.org_merge_acc = vec![true; merges.len()];
        self.org_new_acc = vec![true; new_tags.len()];
        self.org_merges = merges;
        self.org_new = new_tags;
        self.busy = false;
        self.step = 1;
    }

    /// Record Auto-tag assignments from the model and advance to the review step.
    pub fn set_autotag(&mut self, results: Vec<(String, Vec<String>)>) {
        self.at_assign = results;
        self.at_drop.clear();
        self.busy = false;
        self.step = 1;
    }

    /// The model call failed (or couldn't start): drop the spinner and return to
    /// setup. The app surfaces the actual error as a toast.
    pub fn fail(&mut self) {
        self.busy = false;
        self.step = 0;
    }

    /// The app reports what its apply actually did (counts + first error), so
    /// the Done step tells the truth.
    pub fn set_applied(&mut self, summary: ApplySummary) {
        self.applied = Some(summary);
    }

    /// Which wizard this is — the app routes off-thread results by kind so a
    /// stale Organize result can't land in a freshly opened Import wizard.
    pub fn kind(&self) -> WizardKind {
        self.kind
    }

    fn proj_tag(&self) -> String {
        let p = self
            .imp_proj
            .trim()
            .to_lowercase()
            .replace(char::is_whitespace, "-");
        format!("pp:{}", if p.is_empty() { "project" } else { &p })
    }

    /// The accepted per-entry assignments (review minus dropped chips).
    fn autotag_kept(&self) -> Vec<(String, Vec<String>)> {
        self.at_assign
            .iter()
            .enumerate()
            .filter_map(|(pi, (key, tags))| {
                let kept: Vec<String> = tags
                    .iter()
                    .enumerate()
                    .filter(|(ti, _)| !self.at_drop.contains(&(pi, *ti)))
                    .map(|(_, t)| t.clone())
                    .collect();
                (!kept.is_empty()).then(|| (key.clone(), kept))
            })
            .collect()
    }
}

/// Render the open wizard as a modal; returns what the app should do.
pub fn wizard_ui(
    ctx: &egui::Context,
    theme: &Theme,
    wiz: &mut Wizard,
    entries: &[EntryView],
) -> WizardOutcome {
    let mut outcome = WizardOutcome::Keep;
    let modal = egui::Modal::new(egui::Id::new("niu-tag-wizard")).show(ctx, |ui| {
        ui.set_width(600.0);
        outcome = wizard_inner(ui, theme, wiz, entries);
    });
    if modal.should_close() && matches!(outcome, WizardOutcome::Keep) {
        WizardOutcome::Close
    } else {
        outcome
    }
}

fn wizard_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    wiz: &mut Wizard,
    entries: &[EntryView],
) -> WizardOutcome {
    let (icon, title, sub, steps) = match wiz.kind {
        WizardKind::Organize => (
            Glyph::Sparkle,
            "Organize tags with AI",
            "Merge equivalent tags and discover new ones.",
            ["Scope", "Review", "Done"],
        ),
        WizardKind::Autotag => (
            Glyph::Ai,
            "Auto-tag all papers",
            "Apply your existing tags across the whole library.",
            ["Setup", "Review", "Done"],
        ),
        WizardKind::Import => (
            Glyph::Doc,
            "Import paper project",
            "Tag every entry cited by a LaTeX manuscript.",
            ["Source", "Citations", "Done"],
        ),
    };

    // Header: accent-tint icon box · title + subtitle · close ✕.
    let mut close = false;
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 22,
            right: 18,
            top: 20,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (bx, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(bx, egui::CornerRadius::same(11), theme.accent_tint);
                icons::paint_at(ui, bx.shrink(9.0), icon, theme.accent);
                ui.add_space(11.0);
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.label(RichText::new(title).size(16.5).strong().color(theme.text));
                    ui.add_space(2.0);
                    ui.label(RichText::new(sub).size(12.5).color(theme.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if widgets::icbtn(ui, theme, Glyph::Close, 30.0, 7.0).clicked() {
                        close = true;
                    }
                });
            });
        });

    // Stepper.
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 0,
            bottom: 16,
        })
        .show(ui, |ui| wz_steps(ui, theme, &steps, wiz.step));

    // A hairline separates the stepper from the body (design: body borderTop).
    let top = ui.cursor().min.y;
    ui.painter().hline(
        ui.max_rect().x_range(),
        top,
        egui::Stroke::new(1.0, theme.border_2),
    );

    // Scrolling body (fixed-height region; the footer below stays pinned).
    let mut footer = Footer {
        back: None,
        primary: String::new(),
        enabled: false,
        on_back: Nav::None,
        on_primary: Nav::None,
    };
    egui::Frame::default()
        .inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 18,
            bottom: 6,
        })
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Hard width bound: the modal is 600 wide, but inside the
                    // auto-sizing Area a wrapped row would otherwise measure
                    // against the screen width and never wrap (blowing the modal
                    // wide). A fixed cap makes `horizontal_wrapped` wrap reliably.
                    ui.set_max_width(540.0);
                    if wiz.busy {
                        wz_scan(ui, theme, wiz.kind);
                        footer = Footer {
                            back: None,
                            primary: "Working…".into(),
                            enabled: false,
                            on_back: Nav::None,
                            on_primary: Nav::None,
                        };
                    } else {
                        footer = match wiz.kind {
                            WizardKind::Organize => organize_body(ui, theme, wiz),
                            WizardKind::Autotag => autotag_body(ui, theme, wiz, entries),
                            WizardKind::Import => import_body(ui, theme, wiz),
                        };
                    }
                });
        });

    // Pinned footer bar.
    let nav = footer_bar(ui, theme, &footer);

    if close {
        return WizardOutcome::Close;
    }
    match nav {
        None | Some(Nav::None) => WizardOutcome::Keep,
        Some(Nav::Close) => WizardOutcome::Close,
        Some(Nav::Step(n)) => {
            wiz.step = n;
            WizardOutcome::Keep
        }
        Some(Nav::RunOrganize) => {
            // The app starts the off-thread call; we show the spinner meanwhile.
            wiz.busy = true;
            WizardOutcome::RunOrganize {
                instructions: organize_instructions(wiz),
            }
        }
        Some(Nav::ApplyOrganize) => {
            let merges = wiz
                .org_merges
                .iter()
                .enumerate()
                .filter(|(i, _)| wiz.org_merge_acc[*i])
                .map(|(_, m)| (m.from.clone(), m.into.clone()))
                .collect();
            let new_tags = wiz
                .org_new
                .iter()
                .enumerate()
                .filter(|(i, _)| wiz.org_new_acc[*i])
                .map(|(_, s)| s.name.clone())
                .collect();
            wiz.step = 2;
            WizardOutcome::ApplyOrganize { merges, new_tags }
        }
        Some(Nav::RunAutotag) => {
            wiz.busy = true;
            WizardOutcome::RunAutotag {
                keys: wiz.at_keys.clone(),
            }
        }
        Some(Nav::ApplyAutotag) => {
            let assignments = wiz.autotag_kept();
            wiz.step = 2;
            WizardOutcome::ApplyAutotag { assignments }
        }
        Some(Nav::ScanImport) => WizardOutcome::ScanImport {
            files: wiz.imp_files.clone(),
        },
        Some(Nav::ApplyImport) => {
            let keys: Vec<String> = wiz
                .imp_matched
                .iter()
                .filter(|(_, a)| *a)
                .map(|(k, _)| k.clone())
                .collect();
            let tag = wiz.proj_tag();
            wiz.step = 2;
            WizardOutcome::ApplyImport { tag, keys }
        }
    }
}

// ----------------------------------------------------------------- shell bits

fn wz_steps(ui: &mut egui::Ui, theme: &Theme, steps: &[&str; 3], step: usize) {
    ui.horizontal(|ui| {
        for (i, label) in steps.iter().enumerate() {
            let active = i == step;
            let done = i < step;
            let (bg, fg) = if active {
                (theme.accent, Color32::WHITE)
            } else if done {
                (theme.accent_tint, theme.accent)
            } else {
                (theme.surface_2, theme.muted)
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
            // Active step gets a soft accent-tint ring.
            if active {
                ui.painter()
                    .circle_filled(rect.center(), 16.0, theme.accent_tint);
            }
            ui.painter().circle_filled(rect.center(), 12.0, bg);
            if done {
                icons::paint_at(ui, rect.shrink(5.0), Glyph::Check, fg);
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    egui::FontId::proportional(12.0),
                    fg,
                );
            }
            ui.add_space(9.0);
            ui.label(RichText::new(*label).size(12.5).strong().color(if active {
                theme.text
            } else {
                theme.muted
            }));
            if i < steps.len() - 1 {
                ui.add_space(12.0);
                let (lr, _) = ui.allocate_exact_size(egui::vec2(40.0, 2.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(lr.center(), egui::vec2(lr.width(), 2.0)),
                    egui::CornerRadius::same(1),
                    if i < step { theme.accent } else { theme.border },
                );
                ui.add_space(12.0);
            }
        }
    });
}

fn wz_scan(ui: &mut egui::Ui, theme: &Theme, kind: WizardKind) {
    ui.add_space(34.0);
    ui.vertical_centered(|ui| {
        ui.add(egui::Spinner::new().size(34.0).color(theme.accent));
        ui.add_space(14.0);
        let label = match kind {
            WizardKind::Organize => "Analyzing your tag vocabulary…",
            WizardKind::Autotag => "Reading papers…",
            WizardKind::Import => "Reading the LaTeX project…",
        };
        ui.label(RichText::new(label).size(15.0).strong().color(theme.text));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Asking Claude — this can take a few seconds.")
                .size(12.5)
                .color(theme.muted),
        );
    });
    ui.add_space(34.0);
}

fn wz_done(ui: &mut egui::Ui, theme: &Theme, title: &str, sub: &str, recap: &[String]) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 26.0, theme.accent_tint);
        icons::paint_at(ui, rect.shrink(14.0), Glyph::Check, theme.accent);
        ui.add_space(14.0);
        ui.label(RichText::new(title).size(18.0).strong().color(theme.text));
        ui.add_space(5.0);
        ui.label(RichText::new(sub).size(13.5).color(theme.muted));
        if !recap.is_empty() {
            ui.add_space(16.0);
            egui::Frame::default()
                .fill(theme.surface_2)
                .corner_radius(12.0)
                .inner_margin(egui::Margin::symmetric(18, 14))
                .show(ui, |ui| {
                    for r in recap {
                        ui.horizontal(|ui| {
                            icons::show(ui, Glyph::CheckCircle, 16.0, theme.accent);
                            ui.label(RichText::new(r).size(13.0).color(theme.text_2));
                        });
                    }
                });
        }
    });
    ui.add_space(16.0);
}

/// A checkbox toggle (square or round).
fn wz_check(ui: &mut egui::Ui, theme: &Theme, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), theme.accent);
        icons::paint_at(ui, rect.shrink(3.0), Glyph::Check, Color32::WHITE);
    } else {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            egui::Stroke::new(1.5, theme.faint),
            egui::StrokeKind::Inside,
        );
    }
    resp.clicked()
}

/// An option card (checkbox + title + description).
fn wz_option(ui: &mut egui::Ui, theme: &Theme, on: bool, title: &str, desc: &str) -> bool {
    let mut clicked = false;
    let r = egui::Frame::default()
        .fill(if on { theme.accent_tint } else { theme.surface })
        .stroke(egui::Stroke::new(
            1.0,
            if on {
                Color32::TRANSPARENT
            } else {
                theme.border
            },
        ))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(15, 13))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                if wz_check(ui, theme, on) {
                    clicked = true;
                }
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(14.0).strong().color(theme.text));
                    ui.add_space(2.0);
                    ui.label(RichText::new(desc).size(12.5).color(theme.muted));
                });
            });
        });
    if r.response.interact(egui::Sense::click()).clicked() {
        clicked = true;
    }
    clicked
}

fn wz_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(8.0);
}

/// The rendered width of a `chip` (matches the layout in `chip`): frame margin
/// 8+8, dot 7, 4px gaps, plus the `ns:` and value text. Used by `chip_grid` to
/// wrap deterministically (egui's `horizontal_wrapped` won't wrap reliably
/// inside the modal's auto-sizing area).
fn chip_width(ui: &egui::Ui, name: &str) -> f32 {
    let text_w = |s: &str| -> f32 {
        ui.painter()
            .layout_no_wrap(
                s.to_string(),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            )
            .size()
            .x
    };
    let (ns, value) = split_tag(name);
    let mut w = 16.0 + 7.0 + 4.0; // margins + dot + gap
    if !ns.is_empty() {
        w += text_w(&format!("{ns}:")) + 4.0;
    }
    w + text_w(&value) + 2.0
}

/// Lay out chips in rows, breaking to a new row at `max_w` — a manual wrap that
/// doesn't rely on egui's heuristic (which misfires inside an auto-sized modal).
fn chip_grid(ui: &mut egui::Ui, theme: &Theme, names: &[String], max_w: f32) {
    let gap = 7.0;
    let mut i = 0;
    while i < names.len() {
        // Greedily pack one row.
        let mut end = i;
        let mut w = 0.0;
        while end < names.len() {
            let cw = chip_width(ui, &names[end]);
            let add = if end == i { cw } else { cw + gap };
            if end > i && w + add > max_w {
                break;
            }
            w += add;
            end += 1;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for n in &names[i..end] {
                chip(ui, theme, n, false);
            }
        });
        ui.add_space(gap);
        i = end;
    }
}

/// A chip showing a tag with its dot (and an optional NEW badge).
fn chip(ui: &mut egui::Ui, theme: &Theme, name: &str, is_new: bool) {
    egui::Frame::default()
        .fill(theme.surface_2)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (d, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(d, egui::CornerRadius::same(2), tag_color(name));
                let (ns, value) = split_tag(name);
                if !ns.is_empty() {
                    ui.label(
                        RichText::new(format!("{ns}:"))
                            .size(11.0)
                            .color(theme.muted),
                    );
                }
                ui.label(RichText::new(value).size(11.0).color(theme.text));
                if is_new {
                    ui.label(RichText::new("NEW").size(9.0).strong().color(theme.accent));
                }
            });
        });
}

/// The pinned footer bar (surface-2, top border): left Back/Cancel + right
/// primary. Returns the `Nav` of whichever button was clicked.
fn footer_bar(ui: &mut egui::Ui, theme: &Theme, f: &Footer) -> Option<Nav> {
    let mut nav = None;
    egui::Frame::default()
        .fill(theme.surface_2)
        .inner_margin(egui::Margin::symmetric(22, 13))
        .show(ui, |ui| {
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.min_rect().top(),
                egui::Stroke::new(1.0, theme.border),
            );
            ui.horizontal(|ui| {
                if let Some(back) = f.back {
                    if ghost(ui, theme, back).clicked() {
                        nav = Some(f.on_back);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if wiz_primary(ui, theme, &f.primary, f.enabled) {
                        nav = Some(f.on_primary);
                    }
                });
            });
        });
    nav
}

/// A footer primary button (no leading icon; dimmed + inert when disabled).
fn wiz_primary(ui: &mut egui::Ui, theme: &Theme, label: &str, enabled: bool) -> bool {
    let fill = if enabled {
        theme.accent
    } else {
        theme.accent.gamma_multiply(0.45)
    };
    let resp = egui::Frame::default()
        .fill(fill)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.label(
                RichText::new(label)
                    .size(13.0)
                    .strong()
                    .color(Color32::WHITE),
            );
        })
        .response
        .interact(egui::Sense::click());
    enabled && resp.clicked()
}

// ------------------------------------------------------------ organize (real)

/// Fold the setup toggles + free-text box into one instruction string for the
/// model (the engine prompt always asks for both merges and new tags; these
/// narrow it).
fn organize_instructions(wiz: &Wizard) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !wiz.org_opts[0] {
        parts.push("Do not propose any merges.".into());
    }
    if !wiz.org_opts[1] {
        parts.push("Do not propose any new tags.".into());
    }
    let extra = wiz.org_prompt.trim();
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    parts.join(" ")
}

fn organize_body(ui: &mut egui::Ui, theme: &Theme, wiz: &mut Wizard) -> Footer {
    match wiz.step {
        0 => {
            ui.label(
                RichText::new(
                    "Claude reviews every tag, folds together tags that mean the same thing, and \
                     proposes new ones. You approve each change before anything is written.",
                )
                .size(13.5)
                .color(theme.text_2),
            );
            ui.add_space(10.0);
            for (i, (t, d)) in [
                (
                    "Merge equivalent & duplicate tags",
                    "Detect spelling, casing, and abbreviation variants and combine them.",
                ),
                (
                    "Suggest new tags",
                    "Propose tags for recurring topics that don't have one yet.",
                ),
            ]
            .iter()
            .enumerate()
            {
                if wz_option(ui, theme, wiz.org_opts[i], t, d) {
                    wiz.org_opts[i] = !wiz.org_opts[i];
                }
                ui.add_space(8.0);
            }
            wz_label(ui, theme, "Instructions for Claude — optional");
            ui.add(
                egui::TextEdit::multiline(&mut wiz.org_prompt)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("e.g. Keep method tags separate from application tags. Prefer hyphenated lowercase."),
            );
            let any = wiz.org_opts.iter().any(|&b| b);
            Footer {
                back: Some("Cancel"),
                primary: "Scan library".into(),
                enabled: any,
                on_back: Nav::Close,
                on_primary: Nav::RunOrganize,
            }
        }
        1 => {
            if wiz.org_merges.is_empty() && wiz.org_new.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Claude found nothing to change — your vocabulary looks tidy.")
                        .size(13.5)
                        .color(theme.text_2),
                );
                return Footer {
                    back: Some("Back"),
                    primary: "Done".into(),
                    enabled: true,
                    on_back: Nav::Step(0),
                    on_primary: Nav::Close,
                };
            }
            ui.label(
                RichText::new("Suggestions — toggle what to apply.")
                    .size(13.0)
                    .color(theme.text_2),
            );
            ui.add_space(14.0);
            if !wiz.org_merges.is_empty() {
                wz_label(ui, theme, "Merge equivalent tags");
                for i in 0..wiz.org_merges.len() {
                    let m = &wiz.org_merges[i];
                    let (from, into, reason) = (m.from.clone(), m.into.clone(), m.reason.clone());
                    review_row(ui, theme, wiz.org_merge_acc[i], |ui| {
                        chip(ui, theme, &from, false);
                        icons::show(ui, Glyph::ArrowRight, 15.0, theme.muted);
                        chip(ui, theme, &into, false);
                        if !reason.is_empty() {
                            ui.label(
                                RichText::new(format!("· {reason}"))
                                    .size(12.0)
                                    .color(theme.muted),
                            );
                        }
                    })
                    .then(|| wiz.org_merge_acc[i] = !wiz.org_merge_acc[i]);
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
            }
            if !wiz.org_new.is_empty() {
                wz_label(ui, theme, "Suggested new tags");
                for i in 0..wiz.org_new.len() {
                    let s = &wiz.org_new[i];
                    let (name, reason) = (s.name.clone(), s.reason.clone());
                    review_row(ui, theme, wiz.org_new_acc[i], |ui| {
                        chip(ui, theme, &name, true);
                        if !reason.is_empty() {
                            ui.label(RichText::new(reason).size(12.0).color(theme.muted));
                        }
                    })
                    .then(|| wiz.org_new_acc[i] = !wiz.org_new_acc[i]);
                    ui.add_space(8.0);
                }
            }
            let merges = wiz.org_merge_acc.iter().filter(|&&b| b).count();
            let news = wiz.org_new_acc.iter().filter(|&&b| b).count();
            Footer {
                back: Some("Back"),
                // New-tag suggestions are advisory (a tag exists only on entries),
                // so only the merges are "changes" we apply.
                primary: if merges > 0 {
                    format!("Apply {merges} merge{}", if merges == 1 { "" } else { "s" })
                } else {
                    "Finish".into()
                },
                enabled: merges + news > 0,
                on_back: Nav::Step(0),
                on_primary: Nav::ApplyOrganize,
            }
        }
        _ => {
            let news = wiz.org_new_acc.iter().filter(|&&b| b).count();
            let mut recap = Vec::new();
            // Report what actually happened, not what was accepted.
            match wiz.applied.as_ref() {
                Some(s) => {
                    recap.push(format!(
                        "{} merge{} applied",
                        s.applied,
                        if s.applied == 1 { "" } else { "s" }
                    ));
                    if s.skipped > 0 {
                        recap.push(format!("{} matched nothing (skipped)", s.skipped));
                    }
                    if s.failed > 0 {
                        recap.push(format!(
                            "{} failed{}",
                            s.failed,
                            s.first_error
                                .as_deref()
                                .map(|e| format!(" — {e}"))
                                .unwrap_or_default()
                        ));
                    }
                }
                None => recap.push("No merges were applied".into()),
            }
            if news > 0 {
                recap.push(format!(
                    "{news} new tag{} suggested — add them as you tag",
                    if news == 1 { "" } else { "s" }
                ));
            }
            wz_done(
                ui,
                theme,
                "Vocabulary tidied",
                // Tags live in the .niutero sidecar — references.bib is never
                // touched by tag operations (the project's core invariant).
                "Merges were applied to your library's tags (the .niutero sidecar); \
                 references.bib is untouched.",
                &recap,
            );
            done_footer()
        }
    }
}

/// The footer for any wizard's terminal "Done" step.
fn done_footer() -> Footer {
    Footer {
        back: None,
        primary: "Finish".into(),
        enabled: true,
        on_back: Nav::None,
        on_primary: Nav::Close,
    }
}

// ------------------------------------------------------------- autotag (real)

/// Entries to auto-tag: the *untagged* ones (where the model adds the most), in
/// library order, capped at [`AUTOTAG_MAX`].
fn autotag_targets(entries: &[EntryView]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.tags.is_empty())
        .take(AUTOTAG_MAX)
        .map(|e| e.citekey.clone())
        .collect()
}

fn autotag_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    wiz: &mut Wizard,
    entries: &[EntryView],
) -> Footer {
    let vocab: Vec<String> = {
        let mut s: Vec<String> = entries.iter().flat_map(|e| e.tags.clone()).collect();
        s.sort();
        s.dedup();
        s
    };

    match wiz.step {
        0 => {
            // The keys the run will read — recomputed each frame so it tracks the
            // current library, and ready when the "Run" button fires.
            wiz.at_keys = autotag_targets(entries);
            let n = wiz.at_keys.len();
            let untagged = entries.iter().filter(|e| e.tags.is_empty()).count();
            ui.label(
                RichText::new(
                    "Claude reads each paper and assigns tags only from your existing tag set — \
                     it won't invent new tags. You review every assignment before applying.",
                )
                .size(13.5)
                .color(theme.text_2),
            );
            ui.add_space(16.0);
            wz_label(ui, theme, "Tag set Claude will choose from");
            egui::Frame::default()
                .fill(theme.surface_2)
                .corner_radius(12.0)
                .inner_margin(13)
                .show(ui, |ui| {
                    if vocab.is_empty() {
                        ui.label(RichText::new("(no tags yet)").color(theme.muted));
                    } else {
                        // Manual wrap at a fixed width (see `chip_grid`).
                        chip_grid(ui, theme, &vocab, 500.0);
                    }
                });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                icons::show(ui, Glyph::Ai, 15.0, theme.accent);
                ui.label(
                    RichText::new(if untagged == 0 {
                        "Every entry already has tags — nothing to auto-tag.".to_string()
                    } else if untagged > n {
                        format!("Tags the first {n} of {untagged} untagged entries.")
                    } else {
                        format!("Tags your {n} untagged entr{}.", widgets::plural_y(n))
                    })
                    .size(12.5)
                    .color(theme.muted),
                );
            });
            Footer {
                back: Some("Cancel"),
                primary: format!("Run on {n} papers"),
                enabled: !vocab.is_empty() && n > 0,
                on_back: Nav::Close,
                on_primary: Nav::RunAutotag,
            }
        }
        1 => {
            if wiz.at_assign.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Claude didn't find confident tags for these entries.")
                        .size(13.5)
                        .color(theme.text_2),
                );
                return Footer {
                    back: Some("Back"),
                    primary: "Done".into(),
                    enabled: true,
                    on_back: Nav::Step(0),
                    on_primary: Nav::Close,
                };
            }
            let mut total = 0usize;
            ui.label(
                RichText::new("Proposed assignments — remove any you don't want.")
                    .size(13.0)
                    .color(theme.text_2),
            );
            ui.add_space(14.0);
            for pi in 0..wiz.at_assign.len() {
                let (key, tags) = wiz.at_assign[pi].clone();
                let kept: Vec<(usize, String)> = tags
                    .iter()
                    .enumerate()
                    .filter(|(ti, _)| !wiz.at_drop.contains(&(pi, *ti)))
                    .map(|(ti, t)| (ti, t.clone()))
                    .collect();
                total += kept.len();
                let entry = entries.iter().find(|e| e.citekey == key);
                egui::Frame::default()
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .corner_radius(11.0)
                    .inner_margin(egui::Margin::symmetric(13, 11))
                    .show(ui, |ui| {
                        let glyph = entry
                            .map(|e| type_glyph(theme, &e.entry_type))
                            .unwrap_or((Glyph::Doc, theme.muted));
                        ui.horizontal_top(|ui| {
                            icons::show(ui, glyph.0, 17.0, glyph.1);
                            ui.vertical(|ui| {
                                let title = entry
                                    .and_then(|e| e.fields.get("title"))
                                    .map(|t| crate::tex::display(t))
                                    .unwrap_or_else(|| key.clone());
                                ui.label(
                                    RichText::new(ellipsize(&title, 64))
                                        .font(theme::serif(14.0))
                                        .color(theme.text),
                                );
                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    if kept.is_empty() {
                                        ui.label(
                                            RichText::new("skipped")
                                                .size(12.0)
                                                .italics()
                                                .color(theme.faint),
                                        );
                                    }
                                    for (ti, t) in &kept {
                                        if drop_chip(ui, theme, t) {
                                            wiz.at_drop.insert((pi, *ti));
                                        }
                                    }
                                });
                            });
                        });
                    });
                ui.add_space(8.0);
            }
            Footer {
                back: Some("Back"),
                primary: format!("Apply {total} tags"),
                enabled: total > 0,
                on_back: Nav::Step(0),
                on_primary: Nav::ApplyAutotag,
            }
        }
        _ => {
            // Real outcome from the app's apply, falling back to the accepted
            // sets only if the summary never arrived.
            let (papers, tags, skipped) = match wiz.applied.as_ref() {
                Some(s) => (s.applied, s.tags, s.skipped),
                None => {
                    let kept = wiz.autotag_kept();
                    (kept.len(), kept.iter().map(|(_, t)| t.len()).sum(), 0)
                }
            };
            let mut recap = vec![format!(
                "{tags} tag{} across {papers} entr{}",
                if tags == 1 { "" } else { "s" },
                widgets::plural_y(papers)
            )];
            if skipped > 0 {
                recap.push(format!(
                    "{skipped} entr{} not found (skipped)",
                    widgets::plural_y(skipped)
                ));
            }
            wz_done(
                ui,
                theme,
                "Papers tagged",
                "Tags were applied to your library (the .niutero sidecar); references.bib \
                 is untouched.",
                &recap,
            );
            done_footer()
        }
    }
}

// -------------------------------------------------------------- import (real)

fn import_body(ui: &mut egui::Ui, theme: &Theme, wiz: &mut Wizard) -> Footer {
    let proj_ok = !wiz.imp_proj.trim().is_empty();
    let tag = wiz.proj_tag();
    match wiz.step {
        0 => {
            ui.label(
                RichText::new(
                    "Point Niutero at a LaTeX project. Every entry it cites gets a project tag, so \
                     you can pull up exactly the papers behind a manuscript.",
                )
                .size(13.5)
                .color(theme.text_2),
            );
            ui.add_space(14.0);
            // dropzone (a real file picker)
            let picked = !wiz.imp_files.is_empty();
            let zone = egui::Frame::default()
                .fill(if picked {
                    theme.accent_tint
                } else {
                    theme.surface_2
                })
                .stroke(egui::Stroke::new(
                    1.5,
                    if picked { theme.accent } else { theme.border },
                ))
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(16, 22))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let (g, c) = if picked {
                            (Glyph::CheckCircle, theme.accent)
                        } else {
                            (Glyph::Download, theme.muted)
                        };
                        icons::show(ui, g, 24.0, c);
                        ui.add_space(6.0);
                        if picked {
                            ui.label(
                                RichText::new(format!("{} file(s) selected", wiz.imp_files.len()))
                                    .font(theme::mono(13.0))
                                    .color(theme.text),
                            );
                        } else {
                            ui.label(
                                RichText::new("Choose your .tex / .aux files")
                                    .size(13.5)
                                    .strong()
                                    .color(theme.text),
                            );
                            ui.label(
                                RichText::new("the ones with \\cite{} commands")
                                    .size(12.0)
                                    .color(theme.muted),
                            );
                        }
                    });
                });
            if zone.response.interact(egui::Sense::click()).clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter("LaTeX / aux", &["tex", "aux", "bbl"])
                    .pick_files()
                {
                    wiz.imp_files = files;
                }
            }
            if picked {
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("PROJECT TAG")
                            .size(11.0)
                            .strong()
                            .color(theme.muted),
                    );
                    ui.label(RichText::new("*").color(theme.rose));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Required")
                                .size(11.0)
                                .strong()
                                .color(if proj_ok { theme.muted } else { theme.rose }),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    egui::Frame::default()
                        .fill(theme.surface)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if proj_ok { theme.border } else { theme.rose },
                        ))
                        .corner_radius(9.0)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("pp:")
                                        .font(theme::mono(13.5))
                                        .color(theme.faint),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut wiz.imp_proj)
                                        .font(theme::mono(13.5))
                                        .desired_width(200.0)
                                        .hint_text("project-name")
                                        .frame(false),
                                );
                            });
                        });
                    if proj_ok {
                        ui.label(RichText::new("preview").size(12.0).color(theme.muted));
                        chip(ui, theme, &tag, true);
                    }
                });
                if !proj_ok {
                    ui.add_space(7.0);
                    ui.horizontal(|ui| {
                        icons::show(ui, Glyph::Warn, 13.0, theme.rose);
                        ui.label(
                            RichText::new(
                                "Enter a project tag — every cited entry is tagged with it.",
                            )
                            .size(12.0)
                            .color(theme.rose),
                        );
                    });
                }
            }
            Footer {
                back: Some("Cancel"),
                primary: "Scan citations".into(),
                enabled: picked && proj_ok,
                on_back: Nav::Close,
                on_primary: Nav::ScanImport,
            }
        }
        1 => {
            let matched = wiz.imp_matched.len();
            let unmatched = wiz.imp_unmatched.len();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}", matched + unmatched))
                        .size(15.0)
                        .strong()
                        .color(theme.text),
                );
                ui.label(
                    RichText::new("citations found")
                        .size(13.0)
                        .color(theme.text_2),
                );
                ui.label(RichText::new("·").color(theme.faint));
                ui.label(
                    RichText::new(format!("{matched}"))
                        .size(15.0)
                        .strong()
                        .color(theme.accent),
                );
                ui.label(
                    RichText::new("in your library")
                        .size(13.0)
                        .color(theme.accent),
                );
                ui.label(RichText::new("·").color(theme.faint));
                ui.label(
                    RichText::new(format!("{unmatched}"))
                        .size(15.0)
                        .strong()
                        .color(theme.muted),
                );
                ui.label(RichText::new("not found").size(13.0).color(theme.muted));
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("MATCHED — WILL GET")
                        .size(11.0)
                        .strong()
                        .color(theme.muted),
                );
                chip(ui, theme, &tag, false);
            });
            ui.add_space(8.0);
            for i in 0..wiz.imp_matched.len() {
                let (key, acc) = wiz.imp_matched[i].clone();
                let toggled = review_row(ui, theme, acc, |ui| {
                    ui.label(
                        RichText::new(&key)
                            .font(theme::mono(11.5))
                            .color(theme.accent),
                    );
                });
                if toggled {
                    wiz.imp_matched[i].1 = !acc;
                }
                ui.add_space(6.0);
            }
            if unmatched > 0 {
                ui.add_space(8.0);
                wz_label(ui, theme, "Not in your library");
                for k in &wiz.imp_unmatched {
                    egui::Frame::default()
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .corner_radius(11.0)
                        .inner_margin(egui::Margin::symmetric(13, 9))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                icons::show(ui, Glyph::Warn, 14.0, theme.muted);
                                ui.label(
                                    RichText::new(k).font(theme::mono(11.5)).color(theme.text_2),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new("not in library")
                                                .size(11.5)
                                                .color(theme.muted),
                                        );
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
            }
            let keep = wiz.imp_matched.iter().filter(|(_, a)| *a).count();
            Footer {
                back: Some("Back"),
                primary: format!("Tag {keep} entries"),
                enabled: keep > 0,
                on_back: Nav::Step(0),
                on_primary: Nav::ApplyImport,
            }
        }
        _ => {
            let tagged = wiz
                .applied
                .as_ref()
                .map(|s| s.applied)
                .unwrap_or_else(|| wiz.imp_matched.iter().filter(|(_, a)| *a).count());
            let mut recap = vec![format!("{tagged} entries tagged {tag}")];
            if let Some(s) = wiz.applied.as_ref() {
                if s.skipped > 0 {
                    recap.push(format!(
                        "{} entr{} not found (skipped)",
                        s.skipped,
                        widgets::plural_y(s.skipped)
                    ));
                }
            }
            wz_done(
                ui,
                theme,
                "Project imported",
                "Created the project tag and applied it across the cited entries (tags live \
                 in the .niutero sidecar).",
                &recap,
            );
            done_footer()
        }
    }
}

/// A review row (checkbox + content); returns whether the checkbox was toggled.
fn review_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    on: bool,
    content: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut toggled = false;
    egui::Frame::default()
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(11.0)
        .inner_margin(egui::Margin::symmetric(13, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if wz_check(ui, theme, on) {
                    toggled = true;
                }
                ui.add_space(6.0);
                content(ui);
            });
        });
    toggled
}

/// A removable tag chip (click to drop). Returns whether clicked.
fn drop_chip(ui: &mut egui::Ui, theme: &Theme, name: &str) -> bool {
    let resp = egui::Frame::default()
        .fill(theme.surface_2)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (d, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(d, egui::CornerRadius::same(2), tag_color(name));
                let (_, value) = split_tag(name);
                ui.label(RichText::new(value).size(11.0).color(theme.text));
                icons::show(ui, Glyph::Close, 12.0, theme.muted);
            });
        })
        .response
        .interact(egui::Sense::click());
    resp.clicked()
}
