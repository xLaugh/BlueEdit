//! Small painted toolbar / transport icons (no image assets).

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    Render,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    Split,
    Snap,
    Ripple,
    AutoXfade,
    Properties,
    Record,
    Loop,
    PlayFromStart,
    Play,
    Pause,
    Stop,
    ToStart,
    ToEnd,
    PrevFrame,
    NextFrame,
    Marker,
    Grid,
    Safe,
    Snapshot,
    ZoomIn,
    ZoomOut,
}

/// Icon colour follows the interface text colour (theme level).
pub fn icon_color() -> Color32 {
    crate::theme::TEXT()
}

pub fn button(ui: &mut Ui, icon: Icon, tip: &str, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    let p = ui.painter_at(rect);
    if active {
        p.rect_filled(rect.shrink(1.0), 3.0, crate::theme::ACCENT().gamma_multiply(0.6));
    } else if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 3.0, crate::theme::HEADER_SEL());
    }
    let col = if resp.is_pointer_button_down_on() { crate::theme::ACCENT() } else { icon_color() };
    draw(&p, icon, rect.shrink2(vec2(6.0, 5.0)), col);
    resp.on_hover_text(tip)
}

fn tri(p: &Painter, pts: [Pos2; 3], c: Color32) {
    p.add(Shape::convex_polygon(pts.to_vec(), c, Stroke::NONE));
}

pub fn draw(p: &Painter, icon: Icon, r: Rect, c: Color32) {
    let s = Stroke::new(1.6, c);
    let (l, t, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
    let cx = r.center().x;
    let cy = r.center().y;
    match icon {
        Icon::New => {
            p.rect_stroke(Rect::from_min_max(pos2(l + 2.0, t), pos2(rr - 2.0, b)), 1.0, s, egui::StrokeKind::Inside);
            p.line_segment([pos2(rr - 6.0, t), pos2(rr - 2.0, t + 4.0)], s);
        }
        Icon::Open => {
            p.rect_stroke(Rect::from_min_max(pos2(l, t + 3.0), pos2(rr, b)), 1.0, s, egui::StrokeKind::Inside);
            p.line_segment([pos2(l, t + 3.0), pos2(l + 5.0, t)], s);
            p.line_segment([pos2(l + 5.0, t), pos2(cx, t + 3.0)], s);
        }
        Icon::Save => {
            p.rect_stroke(r, 1.0, s, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_min_max(pos2(l + 3.0, t), pos2(rr - 3.0, t + 5.0)), 0.0, c);
            p.rect_stroke(Rect::from_min_max(pos2(l + 3.0, cy + 1.0), pos2(rr - 3.0, b)), 0.0, s, egui::StrokeKind::Inside);
        }
        Icon::Render => {
            p.rect_stroke(r, 1.0, s, egui::StrokeKind::Inside);
            for i in 0..3 {
                let y = t + 2.0 + i as f32 * 4.5;
                p.rect_filled(Rect::from_min_size(pos2(l + 1.5, y), vec2(2.0, 2.0)), 0.0, c);
                p.rect_filled(Rect::from_min_size(pos2(rr - 3.5, y), vec2(2.0, 2.0)), 0.0, c);
            }
            tri(p, [pos2(cx - 2.0, cy - 3.5), pos2(cx + 3.5, cy), pos2(cx - 2.0, cy + 3.5)], c);
        }
        Icon::Undo | Icon::Redo => {
            let dir = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let a = if dir > 0.0 { l + 1.0 } else { rr - 1.0 };
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let ang = std::f32::consts::PI * (i as f32 / 10.0);
                    pos2(cx + dir * -(ang.cos()) * 5.0, cy + 1.0 - ang.sin() * 4.0)
                })
                .collect();
            p.add(Shape::line(pts, s));
            tri(p, [pos2(a, cy + 1.0), pos2(a + dir * 6.0, cy - 1.0), pos2(a + dir * 4.0, cy + 5.0)], c);
        }
        Icon::Cut => {
            p.circle_stroke(pos2(l + 3.0, b - 3.0), 2.5, s);
            p.circle_stroke(pos2(rr - 3.0, b - 3.0), 2.5, s);
            p.line_segment([pos2(l + 4.5, b - 5.0), pos2(rr - 2.0, t)], s);
            p.line_segment([pos2(rr - 4.5, b - 5.0), pos2(l + 2.0, t)], s);
        }
        Icon::Copy => {
            p.rect_stroke(Rect::from_min_max(pos2(l, t), pos2(rr - 4.0, b - 4.0)), 1.0, s, egui::StrokeKind::Inside);
            p.rect_stroke(Rect::from_min_max(pos2(l + 4.0, t + 4.0), pos2(rr, b)), 1.0, s, egui::StrokeKind::Inside);
        }
        Icon::Paste => {
            p.rect_stroke(Rect::from_min_max(pos2(l + 1.0, t + 2.0), pos2(rr - 1.0, b)), 1.0, s, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_min_max(pos2(cx - 3.0, t), pos2(cx + 3.0, t + 4.0)), 1.0, c);
        }
        Icon::Delete => {
            p.line_segment([pos2(l + 2.0, t + 2.0), pos2(rr - 2.0, b - 2.0)], Stroke::new(2.0, c));
            p.line_segment([pos2(rr - 2.0, t + 2.0), pos2(l + 2.0, b - 2.0)], Stroke::new(2.0, c));
        }
        Icon::Split => {
            p.line_segment([pos2(cx, t), pos2(cx, b)], s);
            tri(p, [pos2(l, cy - 3.0), pos2(cx - 2.0, cy), pos2(l, cy + 3.0)], c);
            tri(p, [pos2(rr, cy - 3.0), pos2(cx + 2.0, cy), pos2(rr, cy + 3.0)], c);
        }
        Icon::Snap => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let ang = std::f32::consts::PI * (i as f32 / 12.0);
                    pos2(cx - ang.cos() * 5.0, cy + ang.sin() * 5.0 - 1.0)
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.5, c)));
            p.line_segment([pos2(cx - 5.0, cy - 1.0), pos2(cx - 5.0, t)], Stroke::new(2.5, Color32::from_rgb(220, 70, 70)));
            p.line_segment([pos2(cx + 5.0, cy - 1.0), pos2(cx + 5.0, t)], Stroke::new(2.5, Color32::from_rgb(220, 70, 70)));
        }
        Icon::Ripple => {
            p.rect_stroke(Rect::from_min_max(pos2(l, t + 2.0), pos2(cx - 2.0, b - 2.0)), 1.0, s, egui::StrokeKind::Inside);
            p.line_segment([pos2(cx, cy), pos2(rr, cy)], s);
            tri(p, [pos2(rr - 4.0, cy - 3.0), pos2(rr + 1.0, cy), pos2(rr - 4.0, cy + 3.0)], c);
        }
        Icon::AutoXfade => {
            p.line_segment([pos2(l, b), pos2(rr, t)], s);
            p.line_segment([pos2(l, t), pos2(rr, b)], s);
        }
        Icon::Properties => {
            p.circle_stroke(r.center(), 4.5, Stroke::new(2.0, c));
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([r.center() + d * 5.0, r.center() + d * 7.0], Stroke::new(2.0, c));
            }
        }
        Icon::Record => {
            p.circle_filled(r.center(), 5.0, Color32::from_rgb(200, 60, 60));
        }
        Icon::Loop => {
            p.rect_stroke(Rect::from_min_max(pos2(l, t + 3.0), pos2(rr, b - 3.0)), 3.0, s, egui::StrokeKind::Inside);
            tri(p, [pos2(cx - 1.0, t), pos2(cx + 4.0, t + 3.0), pos2(cx - 1.0, t + 6.0)], c);
        }
        Icon::PlayFromStart => {
            p.line_segment([pos2(l + 1.0, t), pos2(l + 1.0, b)], Stroke::new(2.0, c));
            tri(p, [pos2(l + 4.0, t), pos2(rr, cy), pos2(l + 4.0, b)], c);
        }
        Icon::Play => tri(p, [pos2(l + 2.0, t), pos2(rr, cy), pos2(l + 2.0, b)], c),
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(pos2(l + 2.0, t), pos2(cx - 1.5, b)), 0.0, c);
            p.rect_filled(Rect::from_min_max(pos2(cx + 1.5, t), pos2(rr - 2.0, b)), 0.0, c);
        }
        Icon::Stop => {
            p.rect_filled(r.shrink(1.0), 0.0, c);
        }
        Icon::ToStart => {
            p.line_segment([pos2(l + 1.0, t), pos2(l + 1.0, b)], Stroke::new(2.0, c));
            tri(p, [pos2(rr, t), pos2(l + 3.0, cy), pos2(rr, b)], c);
        }
        Icon::ToEnd => {
            p.line_segment([pos2(rr - 1.0, t), pos2(rr - 1.0, b)], Stroke::new(2.0, c));
            tri(p, [pos2(l, t), pos2(rr - 3.0, cy), pos2(l, b)], c);
        }
        Icon::PrevFrame => {
            tri(p, [pos2(cx, t), pos2(l, cy), pos2(cx, b)], c);
            tri(p, [pos2(rr, t), pos2(cx, cy), pos2(rr, b)], c);
        }
        Icon::NextFrame => {
            tri(p, [pos2(l, t), pos2(cx, cy), pos2(l, b)], c);
            tri(p, [pos2(cx, t), pos2(rr, cy), pos2(cx, b)], c);
        }
        Icon::Marker => {
            p.line_segment([pos2(l + 2.0, t), pos2(l + 2.0, b)], s);
            p.rect_filled(Rect::from_min_max(pos2(l + 2.0, t), pos2(rr, cy)), 0.0, Color32::from_rgb(230, 150, 40));
        }
        Icon::Grid => {
            for i in 0..3 {
                let f = i as f32 / 2.0;
                p.line_segment([pos2(l + f * r.width(), t), pos2(l + f * r.width(), b)], Stroke::new(1.0, c));
                p.line_segment([pos2(l, t + f * r.height()), pos2(rr, t + f * r.height())], Stroke::new(1.0, c));
            }
        }
        Icon::Safe => {
            p.rect_stroke(r, 0.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
            p.rect_stroke(r.shrink(3.0), 0.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
        }
        Icon::Snapshot => {
            p.rect_stroke(Rect::from_min_max(pos2(l, t + 3.0), pos2(rr, b)), 2.0, s, egui::StrokeKind::Inside);
            p.circle_stroke(pos2(cx, cy + 1.5), 3.0, s);
            p.rect_filled(Rect::from_min_max(pos2(cx - 3.0, t), pos2(cx + 3.0, t + 3.0)), 0.0, c);
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(pos2(cx - 1.5, cy - 1.5), 4.5, s);
            p.line_segment([pos2(cx + 2.0, cy + 2.0), pos2(rr, b)], Stroke::new(2.0, c));
            p.line_segment([pos2(cx - 4.0, cy - 1.5), pos2(cx + 1.0, cy - 1.5)], s);
            if icon == Icon::ZoomIn {
                p.line_segment([pos2(cx - 1.5, cy - 4.0), pos2(cx - 1.5, cy + 1.0)], s);
            }
        }
    }
}
