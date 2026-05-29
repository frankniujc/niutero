//! Visual system — a faithful port of the design's CSS tokens (`app/shared.jsx`)
//! to egui. Light and dark share token *roles* with different values; the green
//! accent, the four-step text hierarchy, and the three type families
//! (Hanken Grotesk / Newsreader / JetBrains Mono) all come straight from the
//! design spec §2.

use eframe::egui::{self, Color32, FontFamily};

/// The serif family (Newsreader) — paper titles and abstracts.
pub const SERIF: &str = "niu-serif";
/// The monospace family (JetBrains Mono) — cite keys, DOIs, git text.
pub const MONO: &str = "niu-mono";

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
/// A token defined in CSS as `rgba(r,g,b,a)` over the surface — egui blends with
/// the real background at paint time, so an unmultiplied alpha is faithful.
fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (a * 255.0).round() as u8)
}

/// The resolved palette for one theme. Field names mirror the CSS custom
/// properties on `.niu` so the mapping stays auditable. The full token set is
/// defined now (it's the design's source of truth); some tokens aren't consumed
/// until the later view waves (G2–G5), hence the allow.
#[derive(Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_2: Color32,
    pub raise: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub border: Color32,
    pub border_2: Color32,
    pub accent: Color32,
    pub accent_press: Color32,
    pub accent_tint: Color32,
    pub accent_tint_2: Color32,
    pub sel: Color32,
    pub sel_line: Color32,
    // Semantic/status colors (literal hex, theme-independent — spec §2).
    pub amber: Color32,
    pub rose: Color32,
    pub blue: Color32,
    pub purple: Color32,
    pub teal: Color32,
}

impl Theme {
    pub fn light() -> Self {
        Theme {
            dark: false,
            bg: rgb(0xFA, 0xFA, 0xF8),
            surface: rgb(0xFF, 0xFF, 0xFF),
            surface_2: rgb(0xF4, 0xF4, 0xF1),
            raise: rgb(0xFF, 0xFF, 0xFF),
            text: rgb(0x1B, 0x1C, 0x19),
            text_2: rgb(0x54, 0x57, 0x4F),
            muted: rgb(0x88, 0x8B, 0x82),
            faint: rgb(0xB6, 0xB8, 0xB0),
            border: rgba(20, 22, 18, 0.09),
            border_2: rgba(20, 22, 18, 0.06),
            accent: rgb(0x1F, 0x8A, 0x5B),
            accent_press: rgb(0x17, 0x6F, 0x49),
            accent_tint: rgba(31, 138, 91, 0.10),
            accent_tint_2: rgba(31, 138, 91, 0.16),
            sel: rgba(31, 138, 91, 0.12),
            sel_line: rgb(0x1F, 0x8A, 0x5B),
            amber: rgb(0xB6, 0x79, 0x2B),
            rose: rgb(0xC2, 0x53, 0x6B),
            blue: rgb(0x2A, 0x6F, 0xDB),
            purple: rgb(0x8A, 0x5B, 0xD9),
            teal: rgb(0x2F, 0x8E, 0x8A),
        }
    }

    pub fn dark() -> Self {
        Theme {
            dark: true,
            bg: rgb(0x12, 0x14, 0x11),
            surface: rgb(0x19, 0x1C, 0x17),
            surface_2: rgb(0x1F, 0x23, 0x1D),
            raise: rgb(0x22, 0x26, 0x1F),
            text: rgb(0xE9, 0xEB, 0xE4),
            text_2: rgb(0xAE, 0xB2, 0xA6),
            muted: rgb(0x7E, 0x83, 0x78),
            faint: rgb(0x5A, 0x5F, 0x53),
            border: rgba(255, 255, 255, 0.09),
            border_2: rgba(255, 255, 255, 0.05),
            accent: rgb(0x3B, 0xB1, 0x78),
            accent_press: rgb(0x2E, 0x98, 0x66),
            accent_tint: rgba(59, 177, 120, 0.13),
            accent_tint_2: rgba(59, 177, 120, 0.20),
            sel: rgba(59, 177, 120, 0.15),
            sel_line: rgb(0x3B, 0xB1, 0x78),
            amber: rgb(0xB6, 0x79, 0x2B),
            rose: rgb(0xC2, 0x53, 0x6B),
            blue: rgb(0x2A, 0x6F, 0xDB),
            purple: rgb(0x8A, 0x5B, 0xD9),
            teal: rgb(0x2F, 0x8E, 0x8A),
        }
    }

    pub fn of(dark: bool) -> Self {
        if dark {
            Theme::dark()
        } else {
            Theme::light()
        }
    }

    /// Push this palette into egui's `Visuals` so default-styled widgets pick up
    /// the right fills, strokes, selection, and text colors.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.dark_mode = self.dark;
        v.override_text_color = Some(self.text);
        v.panel_fill = self.surface;
        v.window_fill = self.bg;
        v.window_stroke = egui::Stroke::new(1.0, self.border);
        v.extreme_bg_color = self.surface_2;
        v.faint_bg_color = self.surface_2;
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.sel;
        v.selection.stroke = egui::Stroke::new(1.0, self.accent);
        v.window_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if self.dark { 90 } else { 18 }),
        };
        v.popup_shadow = v.window_shadow;
        v.window_corner_radius = egui::CornerRadius::same(12);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.surface;
        w.noninteractive.weak_bg_fill = self.surface;
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.text_2);
        w.inactive.bg_fill = self.surface_2;
        w.inactive.weak_bg_fill = self.surface_2;
        w.inactive.bg_stroke = egui::Stroke::new(1.0, self.border);
        w.inactive.fg_stroke = egui::Stroke::new(1.0, self.text);
        w.hovered.bg_fill = self.surface_2;
        w.hovered.weak_bg_fill = self.surface_2;
        w.hovered.bg_stroke = egui::Stroke::new(1.0, self.faint);
        w.hovered.fg_stroke = egui::Stroke::new(1.0, self.text);
        w.active.bg_fill = self.accent_tint;
        w.active.weak_bg_fill = self.accent_tint;
        w.active.bg_stroke = egui::Stroke::new(1.0, self.accent);
        w.active.fg_stroke = egui::Stroke::new(1.0, self.accent);
        for s in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            s.corner_radius = egui::CornerRadius::same(8);
        }

        ctx.set_visuals(v);
    }
}

/// Install the three design type families. Hanken Grotesk is the proportional
/// default (all chrome); Newsreader is the `SERIF` family (titles/abstracts);
/// JetBrains Mono is monospace (cite keys, DOIs, git). Fonts are bundled into
/// the binary so the app is self-contained and cross-platform.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::FontData;
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "hanken".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/HankenGrotesk.ttf")).into(),
    );
    fonts.font_data.insert(
        "newsreader".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Newsreader.ttf")).into(),
    );
    fonts.font_data.insert(
        "jbmono".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono.ttf")).into(),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "hanken".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jbmono".to_owned());
    fonts.families.insert(
        FontFamily::Name(SERIF.into()),
        vec!["newsreader".to_owned()],
    );
    fonts
        .families
        .insert(FontFamily::Name(MONO.into()), vec!["jbmono".to_owned()]);

    ctx.set_fonts(fonts);
}

/// Convenience: a `FontId` in the serif family at `size`.
pub fn serif(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name(SERIF.into()))
}
/// Convenience: a `FontId` in the mono family at `size`.
pub fn mono(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name(MONO.into()))
}
