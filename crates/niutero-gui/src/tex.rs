//! GUI glue for `niutero_engine::texdisplay` — turning BibTeX/LaTeX field text
//! into something presentable (Tiers A+B).
//!
//! [`display`] is the plain Tier-A transform (used wherever we paint a single
//! string: list rows, cards, author/venue lines, locked metadata). [`runs_label`]
//! renders the styled runs (Tier B) for the rich spots (titles, abstracts):
//! italic uses egui's built-in synthetic slant; bold is a faux double-strike,
//! since no bold face is bundled (and the global `override_text_color` would
//! mask egui's color-only `strong`).
//!
//! Always display-only — the stored `.bib` value is never modified.

use eframe::egui::{self, Color32, FontId, RichText};
use niutero_engine::texdisplay;

/// Plain Tier-A display string (braces stripped, accents/specials decoded).
pub fn display(s: &str) -> String {
    texdisplay::to_display(s)
}

/// Render `text` as styled runs in `font`/`color` (Tier B). Plain text takes a
/// fast path through a single wrapping `Label`.
pub fn runs_label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) {
    let runs = texdisplay::to_runs(text);
    if runs.iter().all(|r| !r.bold && !r.italic) {
        let s: String = runs.into_iter().map(|r| r.text).collect();
        ui.label(RichText::new(s).font(font).color(color));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for r in &runs {
            // Split into word-tokens (keeping trailing spaces) so wrapping can
            // still happen between words of an emphasized span.
            for tok in r.text.split_inclusive(' ') {
                styled_token(ui, tok, font.clone(), color, r.bold, r.italic);
            }
        }
    });
}

fn styled_token(
    ui: &mut egui::Ui,
    tok: &str,
    font: FontId,
    color: Color32,
    bold: bool,
    italic: bool,
) {
    if tok.is_empty() {
        return;
    }
    let mut rt = RichText::new(tok).font(font.clone()).color(color);
    if italic {
        rt = rt.italics();
    }
    let resp = ui.label(rt);
    if bold {
        // Faux bold: redraw the glyphs offset by ~half a pixel to thicken them
        // (no bold font face is bundled).
        let galley = ui.painter().layout_no_wrap(tok.to_string(), font, color);
        ui.painter()
            .galley(resp.rect.min + egui::vec2(0.5, 0.0), galley, color);
    }
}
