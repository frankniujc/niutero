//! Painted icons — egui has no SVG, and the three text fonts don't carry symbol
//! glyphs (a `☾`/`⎇`/`↻` renders as tofu), so the design's simple stroke icons
//! (`app/shared.jsx`'s `Icon` set) are reproduced here with the `egui::Painter`.
//!
//! Each fn paints a 24×24-design-space icon scaled into `rect`, stroked in
//! `color`. Keep these in step with the design's `Icon` paths.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};

/// Map a point in the design's 0..24 viewBox into `rect`.
fn p(rect: Rect, x: f32, y: f32) -> Pos2 {
    rect.min + Vec2::new(x / 24.0 * rect.width(), y / 24.0 * rect.height())
}

fn line(painter: &egui::Painter, rect: Rect, color: Color32, w: f32, pts: &[(f32, f32)]) {
    let stroke = Stroke::new(w, color);
    let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| p(rect, x, y)).collect();
    painter.add(Shape::line(pts, stroke));
}

/// Stroke width scaled to the icon size (design uses ~1.7 at 24px).
fn sw(rect: Rect) -> f32 {
    (rect.width() / 24.0 * 1.7).max(1.0)
}

/// Which glyph to paint. Names mirror the design's `Icon` keys. The full set is
/// defined here; a few (drawer close, tree chevron) are first used in G3.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Glyph {
    Library,
    Normalize,
    Ai,
    Settings,
    Sync,
    Sun,
    Moon,
    Branch,
    Search,
    Plus,
    Link,
    PanelLeft,
    PanelRight,
    Star,
    Doc,
    Book,
    Attach,
    Lock,
    Unlock,
    Close,
    ChevronRight,
}

/// Paint `glyph` centered in `rect`, stroked in `color`.
pub fn paint(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let w = sw(rect);
    let st = Stroke::new(w, color);
    match glyph {
        // book + tall slab + tilted spine (≈ the design's library icon)
        Glyph::Library => {
            painter.rect_stroke(
                Rect::from_min_max(p(rect, 4.0, 5.0), p(rect, 9.0, 19.0)),
                egui::CornerRadius::ZERO,
                st,
                egui::StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(rect, 10.0, 5.0), p(rect, 14.0, 19.0)),
                egui::CornerRadius::ZERO,
                st,
                egui::StrokeKind::Middle,
            );
            line(
                painter,
                rect,
                color,
                w,
                &[(16.0, 6.0), (20.0, 7.0), (17.0, 19.0), (13.0, 18.0)],
            );
        }
        // diagonal wand + a 4-point sparkle (auto-tidy)
        Glyph::Normalize => {
            line(painter, rect, color, w, &[(3.0, 21.0), (13.5, 10.5)]);
            line(
                painter,
                rect,
                color,
                w,
                &[
                    (16.5, 4.2),
                    (17.5, 6.5),
                    (19.8, 7.5),
                    (17.5, 8.5),
                    (16.5, 10.8),
                    (15.5, 8.5),
                    (13.2, 7.5),
                    (15.5, 6.5),
                    (16.5, 4.2),
                ],
            );
        }
        // big + small 4-point stars (AI sparkle)
        Glyph::Ai => {
            line(
                painter,
                rect,
                color,
                w,
                &[
                    (12.0, 3.0),
                    (13.6, 7.4),
                    (18.0, 9.0),
                    (13.6, 10.6),
                    (12.0, 15.0),
                    (10.4, 10.6),
                    (6.0, 9.0),
                    (10.4, 7.4),
                    (12.0, 3.0),
                ],
            );
            line(
                painter,
                rect,
                color,
                w * 0.8,
                &[
                    (18.0, 15.0),
                    (18.8, 17.2),
                    (21.0, 18.0),
                    (18.8, 18.8),
                    (18.0, 21.0),
                    (17.2, 18.8),
                    (15.0, 18.0),
                    (17.2, 17.2),
                    (18.0, 15.0),
                ],
            );
        }
        // gear: ring + 8 teeth (approximate the cog)
        Glyph::Settings => {
            let c = p(rect, 12.0, 12.0);
            let r_in = rect.width() / 24.0 * 3.2;
            let r_out = rect.width() / 24.0 * 8.2;
            painter.circle_stroke(c, r_in, st);
            for k in 0..8 {
                let a = std::f32::consts::TAU * (k as f32) / 8.0;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment([c + d * (r_in * 1.5), c + d * r_out], st);
            }
        }
        // two opposing curved arrows ≈ refresh/sync
        Glyph::Sync => {
            let c = p(rect, 12.0, 12.0);
            let r = rect.width() / 24.0 * 7.0;
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: arc(c, r, 0.6, 4.2, 22),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: arc(
                    c,
                    r,
                    0.6 + std::f32::consts::PI,
                    4.2 + std::f32::consts::PI,
                    22,
                ),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
        }
        // sun: disc + 8 rays
        Glyph::Sun => {
            let c = p(rect, 12.0, 12.0);
            painter.circle_stroke(c, rect.width() / 24.0 * 4.0, st);
            for k in 0..8 {
                let a = std::f32::consts::TAU * (k as f32) / 8.0;
                let d = Vec2::new(a.cos(), a.sin());
                let r0 = rect.width() / 24.0 * 6.5;
                let r1 = rect.width() / 24.0 * 9.0;
                painter.line_segment([c + d * r0, c + d * r1], st);
            }
        }
        // crescent moon
        Glyph::Moon => {
            let c = p(rect, 12.0, 12.0);
            let r = rect.width() / 24.0 * 8.0;
            let outer = arc(c, r, 1.9, 1.9 + std::f32::consts::PI * 1.25, 20);
            let c2 = c + Vec2::new(rect.width() / 24.0 * 3.0, -rect.width() / 24.0 * 2.5);
            let mut inner = arc(c2, r * 0.95, 1.9 + std::f32::consts::PI * 1.25, 1.9, 20);
            let mut pts = outer;
            pts.append(&mut inner);
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: pts,
                closed: true,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
        }
        // git branch: two nodes + a merge curve
        Glyph::Branch => {
            painter.circle_stroke(p(rect, 6.0, 6.0), rect.width() / 24.0 * 2.4, st);
            painter.circle_stroke(p(rect, 6.0, 18.0), rect.width() / 24.0 * 2.4, st);
            painter.circle_stroke(p(rect, 18.0, 8.0), rect.width() / 24.0 * 2.4, st);
            line(painter, rect, color, w, &[(6.0, 8.4), (6.0, 15.6)]);
            line(painter, rect, color, w, &[(18.0, 10.4), (18.0, 12.0)]);
            line(
                painter,
                rect,
                color,
                w,
                &[(6.0, 12.0), (15.6, 12.0), (18.0, 11.0)],
            );
        }
        Glyph::Search => {
            painter.circle_stroke(p(rect, 11.0, 11.0), rect.width() / 24.0 * 7.0, st);
            line(painter, rect, color, w, &[(20.0, 20.0), (16.5, 16.5)]);
        }
        Glyph::Plus => {
            line(painter, rect, color, w, &[(12.0, 5.0), (12.0, 19.0)]);
            line(painter, rect, color, w, &[(5.0, 12.0), (19.0, 12.0)]);
        }
        Glyph::Link => {
            line(painter, rect, color, w, &[(10.0, 14.0), (14.0, 10.0)]);
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: arc(p(rect, 8.5, 15.5), rect.width() / 24.0 * 3.5, -0.8, 2.4, 14),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: arc(p(rect, 15.5, 8.5), rect.width() / 24.0 * 3.5, 2.3, 5.5, 14),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
        }
        Glyph::PanelLeft | Glyph::PanelRight => {
            painter.rect_stroke(
                Rect::from_min_max(p(rect, 3.0, 4.0), p(rect, 21.0, 20.0)),
                egui::CornerRadius::same(2),
                st,
                egui::StrokeKind::Middle,
            );
            let x = if matches!(glyph, Glyph::PanelLeft) {
                9.0
            } else {
                15.0
            };
            line(painter, rect, color, w * 0.9, &[(x, 4.0), (x, 20.0)]);
        }
        Glyph::Star => {
            line(
                painter,
                rect,
                color,
                w,
                &[
                    (12.0, 4.0),
                    (14.3, 8.8),
                    (19.5, 9.5),
                    (15.7, 13.1),
                    (16.6, 18.3),
                    (12.0, 16.6),
                    (7.4, 18.3),
                    (8.3, 13.1),
                    (4.5, 9.5),
                    (9.7, 8.8),
                    (12.0, 4.0),
                ],
            );
        }
        Glyph::Doc => {
            line(
                painter,
                rect,
                color,
                w,
                &[
                    (6.0, 3.0),
                    (14.0, 3.0),
                    (18.0, 7.0),
                    (18.0, 21.0),
                    (6.0, 21.0),
                    (6.0, 3.0),
                ],
            );
            line(
                painter,
                rect,
                color,
                w * 0.8,
                &[(14.0, 3.0), (14.0, 7.0), (18.0, 7.0)],
            );
        }
        Glyph::Book => {
            line(
                painter,
                rect,
                color,
                w,
                &[(4.0, 5.0), (12.0, 5.0), (12.0, 19.0), (6.0, 19.0)],
            );
            line(painter, rect, color, w, &[(20.0, 5.0), (12.0, 5.0)]);
            line(
                painter,
                rect,
                color,
                w,
                &[(20.0, 5.0), (20.0, 19.0), (12.0, 19.0)],
            );
        }
        Glyph::Attach => {
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: vec![p(rect, 18.0, 8.0), p(rect, 9.0, 17.0)],
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
            line(painter, rect, color, w, &[(18.0, 8.0), (11.0, 15.0)]);
            painter.add(Shape::Path(egui::epaint::PathShape {
                points: arc(p(rect, 12.0, 12.0), rect.width() / 24.0 * 6.0, 2.0, 5.6, 16),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: st.into(),
            }));
        }
        Glyph::Lock | Glyph::Unlock => {
            painter.rect_stroke(
                Rect::from_min_max(p(rect, 5.0, 11.0), p(rect, 19.0, 20.0)),
                egui::CornerRadius::same(2),
                st,
                egui::StrokeKind::Middle,
            );
            if matches!(glyph, Glyph::Lock) {
                painter.add(Shape::Path(egui::epaint::PathShape {
                    points: arc(
                        p(rect, 12.0, 11.0),
                        rect.width() / 24.0 * 4.0,
                        std::f32::consts::PI,
                        std::f32::consts::TAU,
                        12,
                    ),
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: st.into(),
                }));
            } else {
                painter.add(Shape::Path(egui::epaint::PathShape {
                    points: arc(
                        p(rect, 12.0, 11.0),
                        rect.width() / 24.0 * 4.0,
                        std::f32::consts::PI,
                        std::f32::consts::PI * 1.6,
                        10,
                    ),
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: st.into(),
                }));
            }
        }
        Glyph::Close => {
            line(painter, rect, color, w, &[(6.0, 6.0), (18.0, 18.0)]);
            line(painter, rect, color, w, &[(18.0, 6.0), (6.0, 18.0)]);
        }
        Glyph::ChevronRight => {
            line(
                painter,
                rect,
                color,
                w,
                &[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)],
            );
        }
    }
}

/// Polyline approximating a circular arc from `a0` to `a1` radians.
fn arc(center: Pos2, radius: f32, a0: f32, a1: f32, segs: usize) -> Vec<Pos2> {
    (0..=segs)
        .map(|i| {
            let t = a0 + (a1 - a0) * (i as f32) / (segs as f32);
            center + Vec2::new(t.cos() * radius, t.sin() * radius)
        })
        .collect()
}

/// Allocate a `size`×`size` cell and paint `glyph` in it (no interaction).
pub fn show(ui: &mut egui::Ui, glyph: Glyph, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint(ui.painter(), rect.shrink(size * 0.08), glyph, color);
    resp
}
