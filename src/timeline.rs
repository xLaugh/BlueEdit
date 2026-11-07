//! The timeline: time display, track headers, ruler, markers, loop region.
//! Events are drawn and edited in `events.rs`.

use crate::i18n::tr;

use crate::app::App;
use crate::model::*;
use crate::theme;
use egui::{pos2, vec2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};
use std::collections::HashSet;

pub const HEADER_W: f32 = 252.0;
pub const LOOP_H: f32 = 12.0;
pub const MARKER_H: f32 = 16.0;
pub const RULER_H: f32 = 28.0;
pub const TOP_H: f32 = LOOP_H + MARKER_H + RULER_H;
pub const SCROLL_H: f32 = 14.0;

pub struct TlView {
    /// Time at the left edge of the events area.
    pub scroll: f64,
    /// Pixels per second.
    pub pps: f64,
    pub scroll_y: f32,
    pub width: f32,
    pub ctx_event: Option<Id>,
    pub ctx_time: f64,
    pub ctx_track: Option<usize>,
    pub snap_line: Option<f64>,
    pub rename_buf: String,
}

impl Default for TlView {
    fn default() -> Self {
        TlView {
            scroll: 0.0,
            pps: 62.0,
            scroll_y: 0.0,
            width: 1300.0,
            ctx_event: None,
            ctx_time: 0.0,
            ctx_track: None,
            snap_line: None,
            rename_buf: String::new(),
        }
    }
}

impl TlView {
    pub fn visible_secs(&self) -> f64 {
        self.width as f64 / self.pps
    }
    pub fn zoom_at(&mut self, factor: f64, anchor: f64) {
        let ax = (anchor - self.scroll) * self.pps;
        self.pps = (self.pps * factor).clamp(0.5, 4000.0);
        self.scroll = (anchor - ax / self.pps).max(0.0);
    }
    pub fn fit(&mut self, len: f64) {
        self.pps = (self.width as f64 / (len * 1.05)).clamp(0.5, 4000.0);
        self.scroll = 0.0;
    }
}

#[derive(Clone)]
pub enum Drag {
    Move {
        anchor: Id,
        grab_dt: f64,
        orig: Vec<(Id, usize, f64)>,
        anchor_track: usize,
    },
    TrimL {
        id: Id,
        start: f64,
        len: f64,
        off: f64,
    },
    TrimR {
        id: Id,
        len: f64,
        later: Vec<(Id, f64)>,
    },
    FadeIn {
        id: Id,
    },
    FadeOut {
        id: Id,
    },
    Gain {
        id: Id,
    },
    Scrub,
    LoopNew {
        from: f64,
    },
    LoopMove {
        grab: f64,
        orig: (f64, f64),
    },
    LoopEdge {
        left: bool,
    },
    Marker {
        idx: usize,
    },
    TrackHeight {
        track: usize,
        orig: f32,
        y0: f32,
    },
    Slider,
    HScroll {
        grab: f64,
    },
    /// Selection tool rectangle (screen positions).
    RectSel {
        from: Pos2,
    },
    /// Zoom tool rectangle.
    ZoomRect {
        from: Pos2,
    },
    /// Envelope point being dragged: track (or event for velocity) and index.
    Env {
        track: usize,
        event: Option<Id>,
        idx: usize,
    },
    /// Slip: move the media inside the event.
    Slip {
        id: Id,
        off0: f64,
        t0: f64,
    },
    /// Slide: move the event, neighbours follow.
    Slide {
        id: Id,
        start0: f64,
        t0: f64,
        prev: Option<(Id, f64)>,
        next: Option<(Id, f64, f64, f64)>,
    },
    /// Shuffle: reorder events.
    Shuffle {
        id: Id,
    },
    /// Time stretch: change the length and the rate together.
    Stretch {
        id: Id,
        len0: f64,
        rate0: f64,
        left: bool,
        start0: f64,
    },
}

pub struct Geo {
    pub events: Rect,
    pub left: f32,
    pub tracks_top: f32,
    pub scroll: f64,
    pub pps: f64,
}

impl Geo {
    pub fn x(&self, t: f64) -> f32 {
        self.left + ((t - self.scroll) * self.pps) as f32
    }
    pub fn t(&self, x: f32) -> f64 {
        self.scroll + (x - self.left) as f64 / self.pps
    }
}

pub fn ruler_step(pps: f64, fps: f64) -> (f64, f64) {
    let cands = [
        1.0 / fps,
        2.0 / fps,
        5.0 / fps,
        10.0 / fps,
        0.5,
        1.0,
        2.0,
        5.0,
        10.0,
        15.0,
        30.0,
        60.0,
        120.0,
        300.0,
        600.0,
        1800.0,
        3600.0,
    ];
    // ticks fall on whole timecode frames (e.g. 60 frames at 29.97 = 00:00:02;00)
    let nominal = fps.round().max(1.0);
    let to_frames = |s: f64| (s * nominal).round().max(1.0);
    let major_s = cands.iter().copied().find(|s| s * pps >= 90.0).unwrap_or(3600.0);
    let major_f = to_frames(major_s);
    let mut minor_f = (major_f / 5.0).max(1.0);
    if major_f >= 10.0 && major_f % 5.0 != 0.0 {
        minor_f = (major_f / 2.0).max(1.0);
    }
    if minor_f / fps * pps < MIN_VISIBLE_GRID_PX as f64 {
        minor_f = (major_f / 2.0).max(1.0);
    }
    (major_f / fps, minor_f / fps)
}

fn bar_rect(r: Rect, f: f32, centred: bool) -> Rect {
    if centred {
        let c = r.left() + r.width() * 0.5;
        let x = r.left() + r.width() * f;
        Rect::from_min_max(pos2(c.min(x), r.top()), pos2(c.max(x), r.bottom()))
    } else {
        Rect::from_min_max(r.min, pos2(r.left() + r.width() * f, r.bottom()))
    }
}

impl App {
    pub fn track_ys(&self, tracks_top: f32) -> Vec<(f32, f32)> {
        let mut y = tracks_top - self.tl.scroll_y;
        (0..self.p.tracks.len())
            .map(|ti| {
                let h = self.shown_height(ti);
                let r = (y, h);
                y += h;
                r
            })
            .collect()
    }

    pub fn track_at_y(&self, ys: &[(f32, f32)], y: f32) -> Option<usize> {
        ys.iter().position(|(ty, h)| y >= *ty && y < ty + h)
    }

    /// Snap a time to nearby edges (cxSnapWidth = 7 px), then to frames.
    pub fn snap_time(&mut self, t: f64, exclude: &HashSet<Id>) -> f64 {
        let mut best = t;
        let mut line = None;
        if self.snap {
            let thr = SNAP_WIDTH_PX as f64 / self.tl.pps;
            let mut pts = vec![self.cursor, 0.0];
            pts.extend(self.p.markers.iter().map(|m| m.time));
            if let Some((a, b)) = self.p.loop_region {
                pts.push(a);
                pts.push(b);
            }
            for tr in &self.p.tracks {
                for e in &tr.events {
                    if !exclude.contains(&e.id) {
                        pts.push(e.start);
                        pts.push(e.end());
                    }
                }
            }
            if self.snap_grid {
                let (_, minor) = ruler_step(self.tl.pps, self.p.fps);
                pts.push((t / minor).round() * minor);
            }
            let mut best_d = thr;
            for p in pts {
                let d = (p - t).abs();
                if d < best_d {
                    best_d = d;
                    best = p;
                    line = Some(p);
                }
            }
        }
        if line.is_some() {
            self.tl.snap_line = line;
        }
        if self.quantize {
            self.p.quantize(best)
        } else {
            best
        }
    }

    pub fn click_time(&self, t: f64) -> f64 {
        let t = t.max(0.0);
        if self.quantize {
            self.p.quantize(t)
        } else {
            t
        }
    }

    pub fn timeline(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let painter = ui.painter_at(full);
        let events_rect = Rect::from_min_max(
            pos2(full.left() + HEADER_W, full.top() + TOP_H),
            pos2(full.right(), full.bottom() - SCROLL_H),
        );
        self.tl.width = events_rect.width();
        let g = Geo {
            events: events_rect,
            left: events_rect.left(),
            tracks_top: events_rect.top(),
            scroll: self.tl.scroll,
            pps: self.tl.pps,
        };
        let ys = self.track_ys(g.tracks_top);
        painter.rect_filled(full, 0.0, theme::BG());

        self.draw_time_display(&painter, Rect::from_min_size(full.min, vec2(HEADER_W, TOP_H)));
        self.top_strips(ui, &painter, &g, full);
        self.events_area(ui, ctx, &painter, &g, &ys);
        self.headers(ui, &painter, &g, &ys, full);
        self.hscroll(ui, &painter, &g, full);

        let cx = g.x(self.cursor);
        if cx >= events_rect.left() && cx <= events_rect.right() {
            painter.line_segment(
                [pos2(cx, full.top() + LOOP_H), pos2(cx, events_rect.bottom())],
                Stroke::new(1.0, theme::CURSOR()),
            );
            let tri = vec![
                pos2(cx - 5.0, full.top() + TOP_H - 9.0),
                pos2(cx + 5.0, full.top() + TOP_H - 9.0),
                pos2(cx, full.top() + TOP_H - 2.0),
            ];
            painter.add(egui::Shape::convex_polygon(tri, theme::CURSOR(), Stroke::NONE));
        }
        if let (Some(t), Some(_)) = (self.tl.snap_line, &self.drag) {
            let x = g.x(t);
            painter.line_segment(
                [pos2(x, events_rect.top()), pos2(x, events_rect.bottom())],
                Stroke::new(1.0, Color32::from_rgb(120, 200, 255)),
            );
        }
    }

    fn draw_time_display(&self, painter: &egui::Painter, r: Rect) {
        painter.rect_filled(r, 0.0, Color32::from_rgb(18, 18, 20));
        painter.text(
            pos2(r.left() + 12.0, r.center().y - 4.0),
            egui::Align2::LEFT_CENTER,
            self.fmt_time(self.cursor),
            FontId::monospace(if self.fmt_time(self.cursor).len() > 12 { 20.0 } else { 28.0 }),
            Color32::from_rgb(235, 235, 235),
        );
        painter.text(
            pos2(r.left() + 14.0, r.bottom() - 8.0),
            egui::Align2::LEFT_CENTER,
            format!("{} {}", tr("Image :"), self.p.frame_of(self.cursor)),
            FontId::proportional(11.0),
            theme::TEXT_DIM(),
        );
        painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, Color32::from_gray(20)));
    }

    fn marker_at(&self, g: &Geo, x: f32) -> Option<usize> {
        self.p
            .markers
            .iter()
            .position(|m| x >= g.x(m.time) - 3.0 && x <= g.x(m.time) + 16.0)
    }

    fn top_strips(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, g: &Geo, full: Rect) {
        let left = g.events.left();
        let right = g.events.right();
        let loop_r = Rect::from_min_max(pos2(left, full.top()), pos2(right, full.top() + LOOP_H));
        let mark_r = Rect::from_min_max(pos2(left, loop_r.bottom()), pos2(right, loop_r.bottom() + MARKER_H));
        let ruler_r = Rect::from_min_max(pos2(left, mark_r.bottom()), pos2(right, mark_r.bottom() + RULER_H));
        let sp = painter.with_clip_rect(Rect::from_min_max(loop_r.min, ruler_r.max));
        sp.rect_filled(loop_r, 0.0, theme::ui_color(20));
        sp.rect_filled(mark_r, 0.0, theme::ui_color(21));
        sp.rect_filled(ruler_r, 0.0, theme::RULER());

        if let Some((a, b)) = self.p.loop_region {
            let (xa, xb) = (g.x(a), g.x(b));
            let col = if self.loop_on { theme::LOOP() } else { Color32::from_rgb(90, 90, 100) };
            let lr = Rect::from_min_max(pos2(xa, loop_r.top() + 1.0), pos2(xb, loop_r.bottom() - 1.0));
            if lr.width() > 0.0 {
                sp.rect_filled(lr, 2.0, col);
            }
            for x in [xa, xb] {
                sp.rect_filled(Rect::from_center_size(pos2(x, loop_r.center().y), vec2(4.0, LOOP_H)), 0.0, Color32::WHITE);
            }
        }
        for m in &self.p.markers {
            let x = g.x(m.time);
            let flag = Rect::from_min_size(pos2(x, mark_r.top() + 1.0), vec2(16.0, MARKER_H - 2.0));
            sp.rect_filled(flag, 1.0, theme::MARKER());
            sp.text(flag.center(), egui::Align2::CENTER_CENTER, &m.name, FontId::proportional(10.5), Color32::BLACK);
            sp.line_segment([pos2(x, mark_r.top()), pos2(x, mark_r.bottom())], Stroke::new(1.0, theme::MARKER()));
        }
        let (major, minor) = ruler_step(g.pps, self.p.fps);
        let t0 = g.t(left).max(0.0);
        let t1 = g.t(right);
        let mut k = (t0 / minor).floor() as i64;
        loop {
            let t = k as f64 * minor;
            if t > t1 {
                break;
            }
            if t >= 0.0 {
                let x = g.x(t);
                let is_major = ((t / major).round() * major - t).abs() < minor * 0.01;
                let h = if is_major { 12.0 } else { 5.0 };
                sp.line_segment(
                    [pos2(x, ruler_r.bottom() - h), pos2(x, ruler_r.bottom())],
                    Stroke::new(1.0, if is_major { theme::TEXT() } else { theme::TEXT_DIM() }),
                );
                if is_major {
                    sp.text(
                        pos2(x + 3.0, ruler_r.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        self.fmt_time(t),
                        FontId::monospace(10.0),
                        theme::TEXT(),
                    );
                }
            }
            k += 1;
        }

        let strip = Rect::from_min_max(loop_r.min, ruler_r.max);
        let resp = ui.interact(strip, ui.id().with("topstrips"), Sense::click_and_drag());
        let press = ui.input(|i| i.pointer.press_origin());
        let pos = resp.interact_pointer_pos().or(resp.hover_pos());
        if let Some(p) = resp.hover_pos() {
            if loop_r.contains(p) {
                if let Some((a, b)) = self.p.loop_region {
                    if (p.x - g.x(a)).abs() < 5.0 || (p.x - g.x(b)).abs() < 5.0 {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    }
                }
            }
        }
        if resp.drag_started() {
            if let Some(o) = press {
                let t = g.t(o.x).max(0.0);
                if loop_r.contains(o) {
                    self.push_undo();
                    self.drag = match self.p.loop_region {
                        Some((a, _)) if (o.x - g.x(a)).abs() < 5.0 => Some(Drag::LoopEdge { left: true }),
                        Some((_, b)) if (o.x - g.x(b)).abs() < 5.0 => Some(Drag::LoopEdge { left: false }),
                        Some((a, b)) if t > a && t < b => Some(Drag::LoopMove { grab: t, orig: (a, b) }),
                        _ => Some(Drag::LoopNew { from: self.p.quantize(t) }),
                    };
                } else if mark_r.contains(o) {
                    if let Some(idx) = self.marker_at(g, o.x) {
                        self.push_undo();
                        self.drag = Some(Drag::Marker { idx });
                    } else {
                        self.drag = Some(Drag::Scrub);
                    }
                } else {
                    self.drag = Some(Drag::Scrub);
                }
            }
        }
        if resp.dragged() {
            if let Some(p) = pos {
                let t = g.t(p.x).max(0.0);
                let ex = HashSet::new();
                match self.drag.clone() {
                    Some(Drag::Scrub) => {
                        self.cursor = self.click_time(t);
                    }
                    Some(Drag::LoopNew { from }) => {
                        let t = self.snap_time(t, &ex);
                        self.p.loop_region = Some((from.min(t), from.max(t)));
                    }
                    Some(Drag::LoopMove { grab, orig }) => {
                        let d = (t - grab).max(-orig.0);
                        let a = self.p.quantize(orig.0 + d);
                        self.p.loop_region = Some((a, a + (orig.1 - orig.0)));
                    }
                    Some(Drag::LoopEdge { left }) => {
                        let t = self.snap_time(t, &ex);
                        if let Some((a, b)) = self.p.loop_region {
                            self.p.loop_region = Some(if left { (t.min(b), b) } else { (a, t.max(a)) });
                        }
                    }
                    Some(Drag::Marker { idx }) => {
                        let t = self.snap_time(t, &ex);
                        if let Some(m) = self.p.markers.get_mut(idx) {
                            m.time = t;
                        }
                    }
                    _ => {}
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            self.tl.snap_line = None;
            self.p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
        }
        if resp.clicked() {
            if let Some(p) = pos {
                let t = g.t(p.x).max(0.0);
                let marker = if mark_r.contains(p) { self.marker_at(g, p.x) } else { None };
                self.cursor = match marker {
                    Some(i) => self.p.markers[i].time,
                    None => self.click_time(t),
                };
            }
        }
        if resp.secondary_clicked() {
            if let Some(p) = pos {
                if mark_r.contains(p) {
                    if let Some(i) = self.marker_at(g, p.x) {
                        self.push_undo();
                        self.p.markers.remove(i);
                    }
                } else if loop_r.contains(p) {
                    self.loop_on = !self.loop_on;
                }
            }
        }
        resp.on_hover_text_at_pointer(
            "Règle : cliquez ou glissez pour placer le curseur.\nBarre du haut : glissez pour définir la région de boucle (clic droit : activer la boucle).\nBarre des marqueurs : glissez un marqueur, clic droit pour le supprimer.",
        );
    }

    fn headers(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, g: &Geo, ys: &[(f32, f32)], full: Rect) {
        let area = Rect::from_min_max(pos2(full.left(), g.tracks_top), pos2(full.left() + HEADER_W, g.events.bottom()));
        let hp = painter.with_clip_rect(area);
        hp.rect_filled(area, 0.0, theme::ui_color(21));
        let mut v_index = 0;
        let mut a_index = 0;
        let mut action: Option<(usize, &'static str)> = None;
        for ti in 0..self.p.tracks.len() {
            let (y, h) = ys[ti];
            let r = Rect::from_min_size(pos2(area.left(), y), vec2(HEADER_W - 1.0, h - 1.0));
            let kind = self.p.tracks[ti].kind;
            let number = match kind {
                TrackKind::Video => {
                    v_index += 1;
                    v_index
                }
                TrackKind::Audio => {
                    a_index += 1;
                    a_index
                }
            };
            if r.bottom() < area.top() || r.top() > area.bottom() {
                continue;
            }
            let focused = ti == self.focus_track;
            hp.rect_filled(r, 0.0, if focused { theme::HEADER_SEL() } else { theme::HEADER() });
            let col = TRACK_COLORS[self.p.tracks[ti].color % TRACK_COLORS.len()];
            let strip = Rect::from_min_size(r.min, vec2(20.0, r.height()));
            hp.rect_filled(strip, 0.0, Color32::from_rgb(col[0], col[1], col[2]));
            hp.text(
                pos2(strip.center().x, strip.top() + 10.0),
                egui::Align2::CENTER_CENTER,
                format!("{}", ti + 1),
                FontId::proportional(12.0),
                Color32::BLACK,
            );
            let gx = strip.right() + 6.0;
            let gy = r.top() + 6.0;
            match kind {
                TrackKind::Video => {
                    hp.rect_stroke(Rect::from_min_size(pos2(gx, gy), vec2(14.0, 10.0)), 1.0, Stroke::new(1.2, theme::TEXT_DIM()), StrokeKind::Inside);
                    hp.rect_filled(Rect::from_min_size(pos2(gx + 2.0, gy + 2.0), vec2(4.0, 6.0)), 0.0, theme::TEXT_DIM());
                }
                TrackKind::Audio => {
                    hp.rect_filled(Rect::from_min_size(pos2(gx, gy + 3.0), vec2(4.0, 5.0)), 0.0, theme::TEXT_DIM());
                    hp.add(egui::Shape::convex_polygon(
                        vec![pos2(gx + 4.0, gy + 3.0), pos2(gx + 10.0, gy), pos2(gx + 10.0, gy + 11.0), pos2(gx + 4.0, gy + 8.0)],
                        theme::TEXT_DIM(),
                        Stroke::NONE,
                    ));
                }
            }
            let name_r = Rect::from_min_max(pos2(gx + 20.0, r.top() + 2.0), pos2(r.right() - 52.0, r.top() + 18.0));
            if self.rename_track == Some(ti) {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_r));
                let te = child.add(egui::TextEdit::singleline(&mut self.tl.rename_buf).desired_width(name_r.width()));
                te.request_focus();
                if te.lost_focus() {
                    self.push_undo();
                    self.p.tracks[ti].name = self.tl.rename_buf.clone();
                    self.rename_track = None;
                }
            } else {
                let name = &self.p.tracks[ti].name;
                let (txt, c) = if name.is_empty() {
                    (
                        match kind {
                            TrackKind::Video => format!("{} {}", tr("Vidéo"), number),
                            TrackKind::Audio => format!("{} {}", tr("Audio"), number),
                        },
                        theme::TEXT_DIM(),
                    )
                } else {
                    (name.clone(), theme::TEXT())
                };
                hp.with_clip_rect(name_r).text(name_r.left_center(), egui::Align2::LEFT_CENTER, txt, FontId::proportional(12.5), c);
            }
            for (k, label) in [(0, "M"), (1, "S")] {
                let br = Rect::from_min_size(pos2(r.right() - 44.0 + k as f32 * 21.0, r.top() + 3.0), vec2(19.0, 15.0));
                let on = if k == 0 { self.p.tracks[ti].mute } else { self.p.tracks[ti].solo };
                let resp = ui.interact(br, ui.id().with(("ms", ti, k)), Sense::click());
                let bg = if on {
                    if k == 0 {
                        Color32::from_rgb(200, 80, 60)
                    } else {
                        Color32::from_rgb(220, 190, 60)
                    }
                } else if resp.hovered() {
                    Color32::from_gray(90)
                } else {
                    Color32::from_gray(70)
                };
                hp.rect_filled(br, 2.0, bg);
                hp.text(br.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.0), if on { Color32::BLACK } else { theme::TEXT() });
                if resp.clicked() {
                    self.push_undo();
                    if k == 0 {
                        self.p.tracks[ti].mute = !self.p.tracks[ti].mute;
                    } else {
                        self.p.tracks[ti].solo = !self.p.tracks[ti].solo;
                    }
                }
                resp.on_hover_text(if k == 0 { tr("Muet") } else { tr("Solo") });
            }
            let sx0 = strip.right() + 6.0;
            let sw = r.right() - 8.0 - sx0;
            if h >= 40.0 {
                let sr = Rect::from_min_size(pos2(sx0, r.top() + 23.0), vec2(sw, 15.0));
                let v = self.p.tracks[ti].level;
                let (label, min, max) = match kind {
                    TrackKind::Video => (format!("{} {:.1} %", tr("Niveau :"), v * 100.0), 0.0, 1.0),
                    TrackKind::Audio => {
                        let db = if v > 0.0 { 20.0 * v.log10() } else { f32::NEG_INFINITY };
                        let txt = if db.is_finite() { format!("{} {:.1} dB", tr("Vol. :"), db) } else { format!("{} -Inf.", tr("Vol. :")) };
                        (txt, 0.0, 2.0)
                    }
                };
                if let Some(nv) = self.header_slider(ui, &hp, sr, ("lvl", ti), v, min, max, 1.0, &label) {
                    self.p.tracks[ti].level = nv;
                    self.touch();
                }
            }
            if kind == TrackKind::Audio && h >= 60.0 {
                let sr = Rect::from_min_size(pos2(sx0, r.top() + 42.0), vec2(sw, 15.0));
                let v = self.p.tracks[ti].pan;
                let label = if v.abs() < 0.005 {
                    format!("{} {}", tr("Pan. :"), tr("Centre"))
                } else if v < 0.0 {
                    format!("{} {:.0} % {}", tr("Pan. :"), -v * 100.0, tr("G"))
                } else {
                    format!("{} {:.0} % {}", tr("Pan. :"), v * 100.0, tr("D"))
                };
                if let Some(nv) = self.header_slider(ui, &hp, sr, ("pan", ti), v, -1.0, 1.0, 0.0, &label) {
                    self.p.tracks[ti].pan = nv;
                    self.touch();
                }
            }
            let resize_r = Rect::from_min_max(pos2(r.left(), r.bottom() - 3.0), pos2(r.right(), r.bottom() + 2.0));
            let rresp = ui.interact(resize_r, ui.id().with(("resize", ti)), Sense::drag());
            if rresp.hovered() || rresp.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if rresp.drag_started() {
                self.drag = Some(Drag::TrackHeight {
                    track: ti,
                    orig: self.p.tracks[ti].height,
                    y0: rresp.interact_pointer_pos().map(|p| p.y).unwrap_or(0.0),
                });
            }
            if rresp.dragged() {
                if let (Some(Drag::TrackHeight { track, orig, y0 }), Some(p)) = (self.drag.clone(), rresp.interact_pointer_pos()) {
                    self.p.tracks[track].height = (orig + p.y - y0).clamp(MIN_TRACK_HEIGHT, 300.0);
                }
            }
            if rresp.drag_stopped() {
                self.drag = None;
            }
            let name_resp = ui.interact(name_r, ui.id().with(("name", ti)), Sense::click());
            if name_resp.double_clicked() {
                self.rename_track = Some(ti);
                self.tl.rename_buf = self.p.tracks[ti].name.clone();
            }
            let body_r = Rect::from_min_max(r.min, pos2(strip.right() + 4.0, r.bottom() - 3.0));
            let body = ui.interact(body_r, ui.id().with(("hdr", ti)), Sense::click());
            if body.clicked() || name_resp.clicked() {
                self.focus_track = ti;
            }
            // badges: armed, adjustment, child, blend, group
            {
                let t = &self.p.tracks[ti];
                let mut bx = r.right() - 48.0;
                let by = r.top() + 10.0;
                let mut badge = |txt: &str, c: Color32| {
                    bx -= 7.0 * txt.chars().count() as f32 + 6.0;
                    hp.text(pos2(bx, by), egui::Align2::LEFT_CENTER, txt, FontId::proportional(10.0), c);
                };
                if t.armed {
                    badge("●", Color32::from_rgb(230, 70, 60));
                }
                if t.adjustment {
                    badge("ADJ", theme::ACCENT());
                }
                if t.child {
                    badge("↳", theme::TEXT_DIM());
                }
                if t.blend != BlendMode::Normal {
                    badge("◐", theme::TEXT_DIM());
                }
                if !t.fx.is_empty() || !t.afx.is_empty() {
                    badge("fx", theme::ACCENT());
                }
                if t.group.is_some() {
                    badge("▤", theme::TEXT_DIM());
                }
            }
            let menu_resp = name_resp.union(body);
            menu_resp.context_menu(|ui| {
                for (label, key) in [
                    (tr("Renommer"), "rename"),
                    (tr("Insérer une piste vidéo"), "addv"),
                    (tr("Insérer une piste audio"), "adda"),
                    (tr("Dupliquer la piste"), "dup"),
                ] {
                    if ui.button(label).clicked() {
                        action = Some((ti, key));
                        ui.close_menu();
                    }
                }
                ui.separator();
                self.track_menu_extra(ui, ti);
                ui.separator();
                if ui.button(tr("Supprimer la piste")).clicked() {
                    action = Some((ti, "del"));
                    ui.close_menu();
                }
            });
        }
        if let Some((ti, a)) = action {
            match a {
                "rename" => {
                    self.rename_track = Some(ti);
                    self.tl.rename_buf = self.p.tracks[ti].name.clone();
                }
                "addv" => {
                    self.push_undo();
                    self.focus_track = self.p.add_track(TrackKind::Video, Some(ti));
                }
                "adda" => {
                    self.push_undo();
                    self.focus_track = self.p.add_track(TrackKind::Audio, Some(ti + 1));
                }
                "dup" => {
                    self.push_undo();
                    let mut t = self.p.tracks[ti].clone();
                    t.id = self.p.new_id();
                    for e in &mut t.events {
                        e.id = self.p.new_id();
                        e.group = None;
                    }
                    t.color = self.p.tracks.len();
                    self.p.tracks.insert(ti + 1, t);
                }
                "del" => {
                    self.push_undo();
                    self.p.tracks.remove(ti);
                    self.focus_track = self.focus_track.min(self.p.tracks.len().saturating_sub(1));
                }
                _ => {}
            }
        }
        painter.line_segment([pos2(area.right(), area.top()), pos2(area.right(), area.bottom())], Stroke::new(1.0, Color32::from_gray(20)));
    }

    #[allow(clippy::too_many_arguments)]
    fn header_slider(
        &mut self,
        ui: &mut egui::Ui,
        p: &egui::Painter,
        r: Rect,
        id: (&str, usize),
        val: f32,
        min: f32,
        max: f32,
        default: f32,
        label: &str,
    ) -> Option<f32> {
        let resp = ui.interact(r, ui.id().with(id), Sense::click_and_drag());
        p.rect_filled(r, 2.0, theme::ui_color(19));
        let f = ((val - min) / (max - min)).clamp(0.0, 1.0);
        p.rect_filled(bar_rect(r, f, min < 0.0), 2.0, theme::ACCENT().gamma_multiply(0.55));
        p.text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.0), theme::TEXT());
        if resp.double_clicked() {
            self.push_undo();
            return Some(default);
        }
        if resp.drag_started() {
            self.push_undo();
            self.drag = Some(Drag::Slider);
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.dragged() || resp.clicked() {
            if resp.clicked() {
                self.push_undo();
            }
            if let Some(pp) = resp.interact_pointer_pos() {
                let f = ((pp.x - r.left()) / r.width()).clamp(0.0, 1.0);
                let mut v = min + f * (max - min);
                if min < 0.0 && v.abs() < 0.03 {
                    v = 0.0;
                }
                return Some(v);
            }
        }
        None
    }

    fn hscroll(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, g: &Geo, full: Rect) {
        let r = Rect::from_min_max(pos2(g.events.left(), g.events.bottom()), pos2(g.events.right(), full.bottom()));
        painter.rect_filled(r, 0.0, theme::ui_color(19));
        let total = (self.p.end_time() + 10.0)
            .max(MIN_PROJECT_DISPLAY_LEN)
            .max(self.tl.scroll + self.tl.visible_secs());
        let a = (self.tl.scroll / total) as f32;
        let b = ((self.tl.scroll + self.tl.visible_secs()) / total) as f32;
        let thumb = Rect::from_min_max(
            pos2(r.left() + a * r.width(), r.top() + 3.0),
            pos2(r.left() + b.min(1.0) * r.width(), r.bottom() - 3.0),
        );
        painter.rect_filled(thumb, 4.0, Color32::from_gray(95));
        let resp = ui.interact(r, ui.id().with("hscroll"), Sense::click_and_drag());
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                let t = (p.x - r.left()) as f64 / r.width() as f64 * total;
                self.drag = Some(Drag::HScroll { grab: t - self.tl.scroll });
            }
        }
        if resp.dragged() {
            if let (Some(Drag::HScroll { grab }), Some(p)) = (self.drag.clone(), resp.interact_pointer_pos()) {
                let t = (p.x - r.left()) as f64 / r.width() as f64 * total;
                self.tl.scroll = (t - grab).max(0.0);
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        let zr = Rect::from_min_max(pos2(full.left(), g.events.bottom()), pos2(g.events.left(), full.bottom()));
        painter.rect_filled(zr, 0.0, theme::ui_color(19));
        painter.text(
            zr.left_center() + vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{} {:.0} px/s", tr("Zoom :"), self.tl.pps),
            FontId::proportional(10.5),
            theme::TEXT_DIM(),
        );
    }

    #[allow(dead_code)]
    pub fn pos_time(&self, g: &Geo, p: Pos2) -> f64 {
        g.t(p.x)
    }
}
