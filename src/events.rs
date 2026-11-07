//! Events on the timeline: drawing (thumbnails, waveforms, fades) and
//! mouse editing (move, trim, fades, gain, drop from panels).

use crate::i18n::tr;

use crate::app::{App, FxPayload, MediaPayload, TransitionPayload};
use crate::media::{self, PEAK_BLOCK};
use crate::model::*;
use crate::theme;
use crate::timeline::{ruler_step, Drag, Geo};
use egui::{pos2, vec2, Color32, CursorIcon, FontId, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind};
use std::collections::HashSet;

const EVENT_HEADER_H: f32 = 14.0;
const EDGE_PX: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zone {
    Body,
    Left,
    Right,
    FadeIn,
    FadeOut,
    Gain,
    FxButton,
    PanButton,
}

impl App {
    fn event_zone(&self, g: &Geo, ys: &[(f32, f32)], pos: Pos2) -> Option<(usize, usize, Zone)> {
        let ti = self.track_at_y(ys, pos.y)?;
        let (ty, th) = ys[ti];
        let track = &self.p.tracks[ti];
        for ei in (0..track.events.len()).rev() {
            let e = &track.events[ei];
            let x0 = g.x(e.start);
            let x1 = g.x(e.end());
            if pos.x < x0 - 2.0 || pos.x > x1 + 2.0 {
                continue;
            }
            let top = ty + 1.0;
            let (fin, fout) = effective_fades(track, ei, self.auto_xf);
            let fin_x = g.x(e.start + fin);
            let fout_x = g.x(e.end() - fout);
            if track.kind == TrackKind::Video && th >= 40.0 && x1 - x0 > 60.0 {
                let fx_r = Rect::from_min_size(pos2(x1 - 30.0, top + 1.0), vec2(13.0, 12.0));
                let pan_r = Rect::from_min_size(pos2(x1 - 15.0, top + 1.0), vec2(13.0, 12.0));
                if fx_r.contains(pos) {
                    return Some((ti, ei, Zone::FxButton));
                }
                if pan_r.contains(pos) {
                    return Some((ti, ei, Zone::PanButton));
                }
            }
            if pos.y < top + 9.0 {
                if (pos.x - fin_x).abs() < 7.0 || (fin <= 0.0 && pos.x - x0 < 10.0) {
                    return Some((ti, ei, Zone::FadeIn));
                }
                if (pos.x - fout_x).abs() < 7.0 || (fout <= 0.0 && x1 - pos.x < 10.0) {
                    return Some((ti, ei, Zone::FadeOut));
                }
                return Some((ti, ei, Zone::Gain));
            }
            if pos.x - x0 < EDGE_PX {
                return Some((ti, ei, Zone::Left));
            }
            if x1 - pos.x < EDGE_PX {
                return Some((ti, ei, Zone::Right));
            }
            return Some((ti, ei, Zone::Body));
        }
        None
    }

    fn thumb_texture(&mut self, ctx: &egui::Context, media_id: Id, idx: usize) -> Option<egui::TextureHandle> {
        if let Some(t) = self.thumb_tex.get(&(media_id, idx)) {
            return Some(t.clone());
        }
        let th = self.store.thumbs(media_id)?;
        let (_, img) = th.frames.get(idx)?;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.data);
        let tex = ctx.load_texture(format!("thumb{}_{}", media_id, idx), ci, egui::TextureOptions::LINEAR);
        self.thumb_tex.insert((media_id, idx), tex.clone());
        Some(tex)
    }

    pub fn events_area(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, painter: &egui::Painter, g: &Geo, ys: &[(f32, f32)]) {
        let area = g.events;
        let p = painter.with_clip_rect(area);
        for (ti, (y, h)) in ys.iter().enumerate() {
            let r = Rect::from_min_size(pos2(area.left(), *y), vec2(area.width(), *h - 1.0));
            p.rect_filled(r, 0.0, if ti % 2 == 0 { theme::TRACK_BG() } else { theme::TRACK_BG_ALT() });
            p.line_segment([pos2(r.left(), r.bottom() + 0.5), pos2(r.right(), r.bottom() + 0.5)], Stroke::new(1.0, Color32::from_gray(28)));
        }
        let (major, _) = ruler_step(g.pps, self.p.fps);
        let mut t = (g.t(area.left()) / major).floor().max(0.0) * major;
        while g.x(t) < area.right() {
            let x = g.x(t);
            p.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 10)));
            t += major;
        }
        if let Some((a, b)) = self.p.loop_region {
            let r = Rect::from_min_max(pos2(g.x(a), area.top()), pos2(g.x(b), area.bottom()));
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 150, 220, if self.loop_on { 26 } else { 14 }));
        }
        // named regions: a thin band and the name at the top of the events area
        for rg in &self.p.regions {
            let r = Rect::from_min_max(pos2(g.x(rg.start), area.top()), pos2(g.x(rg.end), area.top() + 12.0));
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(80, 200, 120, 60));
            p.text(r.left_center() + vec2(3.0, 0.0), egui::Align2::LEFT_CENTER, &rg.name, egui::FontId::proportional(10.0), theme::TEXT());
        }
        for m in &self.p.markers {
            let x = g.x(m.time);
            let mut y = area.top();
            while y < area.bottom() {
                p.line_segment([pos2(x, y), pos2(x, (y + 3.0).min(area.bottom()))], Stroke::new(1.0, theme::MARKER()));
                y += 6.0;
            }
        }
        let pulse = (ui.input(|i| i.time) as f32 * SELECTION_PULSE_RATE).sin() * 0.5 + 0.5;
        for ti in 0..self.p.tracks.len() {
            let (ty, th) = ys[ti];
            if ty > area.bottom() || ty + th < area.top() {
                continue;
            }
            for ei in 0..self.p.tracks[ti].events.len() {
                self.draw_event(ctx, &p, g, ti, ei, ty, th, pulse);
            }
        }

        let resp = ui.interact(area, ui.id().with("events"), Sense::click_and_drag());
        let hover = resp.hover_pos();
        if self.drag.is_none() {
            if let Some(hp) = hover {
                if let Some((_, _, z)) = self.event_zone(g, ys, hp) {
                    let icon = match z {
                        Zone::Left | Zone::Right => CursorIcon::ResizeHorizontal,
                        Zone::FadeIn | Zone::FadeOut => CursorIcon::ResizeColumn,
                        Zone::Gain => CursorIcon::ResizeVertical,
                        Zone::FxButton | Zone::PanButton => CursorIcon::PointingHand,
                        Zone::Body => CursorIcon::Grab,
                    };
                    ctx.set_cursor_icon(icon);
                }
            }
        } else if matches!(self.drag, Some(Drag::Move { .. })) {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        }

        // wheel: zoom (plain), horizontal scroll (Shift), vertical scroll (Ctrl)
        if resp.hovered() {
            let (scroll, zoom, shift) = ui.input(|i| (i.raw_scroll_delta, i.zoom_delta(), i.modifiers.shift));
            let content_h: f32 = self.p.tracks.iter().map(|t| t.height).sum();
            let max_y = (content_h - area.height() + 20.0).max(0.0);
            if (zoom - 1.0).abs() > 1e-4 {
                self.tl.scroll_y -= zoom.ln() * 400.0;
            } else if scroll != egui::Vec2::ZERO {
                if shift {
                    let d = if scroll.x != 0.0 { scroll.x } else { scroll.y };
                    self.tl.scroll = (self.tl.scroll - d as f64 / self.tl.pps).max(0.0);
                } else if scroll.y != 0.0 {
                    let anchor = hover.map(|h| g.t(h.x)).unwrap_or(self.cursor);
                    self.tl.zoom_at(if scroll.y > 0.0 { 1.15 } else { 1.0 / 1.15 }, anchor);
                } else {
                    self.tl.scroll = (self.tl.scroll - scroll.x as f64 / self.tl.pps).max(0.0);
                }
            }
            self.tl.scroll_y = self.tl.scroll_y.clamp(0.0, max_y);
        }

        if self.prefs.show_envelopes {
            self.draw_envelopes(&p, g, ys);
        }
        if let (Some(Drag::RectSel { from } | Drag::ZoomRect { from }), Some(pp)) = (self.drag.clone(), resp.interact_pointer_pos()) {
            p.rect_stroke(Rect::from_two_pos(from, pp), 0.0, Stroke::new(1.0, theme::ACCENT()), StrokeKind::Inside);
            p.rect_filled(Rect::from_two_pos(from, pp), 0.0, theme::ACCENT().gamma_multiply(0.12));
        }

        let press = ui.input(|i| i.pointer.press_origin());
        let (ctrl, alt, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.alt, i.modifiers.shift));
        if resp.drag_started() {
            if let Some(o) = press {
                self.begin_drag_tool(g, ys, o, ctrl, alt);
            }
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                self.update_drag(g, ys, pp);
            }
        }
        if resp.drag_stopped() {
            if let Some(pp) = resp.interact_pointer_pos() {
                self.end_drag_tool(g, ys, pp, ctrl, shift);
            }
            self.p.sort_events();
            self.drag = None;
            self.tl.snap_line = None;
        }
        if resp.clicked() && self.tool != crate::tools::EditTool::Normal {
            if let Some(pp) = resp.interact_pointer_pos() {
                if self.tool_click(g, ys, pp, shift) {
                    return;
                }
            }
        }
        if resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                match self.event_zone(g, ys, pp) {
                    Some((ti, ei, z)) => {
                        let id = self.p.tracks[ti].events[ei].id;
                        match z {
                            Zone::FxButton => {
                                self.select_event(id, false);
                                self.win_fx = Some(id);
                            }
                            Zone::PanButton => {
                                self.select_event(id, false);
                                self.win_pan = Some(id);
                            }
                            _ => {
                                self.select_event(id, ctrl);
                                self.focus_track = ti;
                                self.cursor = self.click_time(g.t(pp.x));
                            }
                        }
                    }
                    None => {
                        if !ctrl {
                            self.sel.clear();
                        }
                        if let Some(ti) = self.track_at_y(ys, pp.y) {
                            self.focus_track = ti;
                        }
                        self.cursor = self.click_time(g.t(pp.x));
                    }
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                if let Some((ti, ei, _)) = self.event_zone(g, ys, pp) {
                    let e = &self.p.tracks[ti].events[ei];
                    self.p.loop_region = Some((e.start, e.end()));
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                self.tl.ctx_time = g.t(pp.x);
                self.tl.ctx_track = self.track_at_y(ys, pp.y);
                self.tl.ctx_event = self
                    .event_zone(g, ys, pp)
                    .map(|(ti, ei, _)| self.p.tracks[ti].events[ei].id);
                if let Some(id) = self.tl.ctx_event {
                    if !self.sel.contains(&id) {
                        self.select_event(id, false);
                    }
                }
            }
        }
        resp.context_menu(|ui| {
            self.event_menu(ui);
            if self.tl.ctx_event.is_some() {
                ui.separator();
                self.event_menu_extra(ui);
            }
        });

        if let Some(hp) = hover {
            if let Some(pl) = resp.dnd_hover_payload::<MediaPayload>() {
                if let Some(m) = self.p.media(pl.0) {
                    let len = if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN };
                    let x1 = hp.x + (len * g.pps) as f32;
                    let (y, h) = self.track_at_y(ys, hp.y).map(|t| ys[t]).unwrap_or((hp.y - 20.0, 40.0));
                    p.rect_stroke(Rect::from_min_max(pos2(hp.x, y + 1.0), pos2(x1, y + h - 2.0)), 2.0, Stroke::new(2.0, theme::ACCENT()), StrokeKind::Inside);
                }
            }
        }
        if let Some(pl) = resp.dnd_release_payload::<MediaPayload>() {
            if let Some(hp) = hover.or(resp.interact_pointer_pos()) {
                let t = self.snap_time(g.t(hp.x).max(0.0), &HashSet::new());
                let ti = self.track_at_y(ys, hp.y);
                self.insert_media(pl.0, t, ti, true);
                self.tl.snap_line = None;
            }
        }
        if let Some(pl) = resp.dnd_release_payload::<TransitionPayload>() {
            if let Some((ti, ei, _)) = hover.and_then(|hp| self.event_zone(g, ys, hp)) {
                self.push_undo();
                let e = &mut self.p.tracks[ti].events[ei];
                e.transition = pl.0;
                if e.fade_in <= 0.0 {
                    e.fade_in = 1.0_f64.min(e.length / 2.0);
                }
                self.status = format!("Transition « {} » appliquée.", pl.0.label());
            }
        }
        if let Some(pl) = resp.dnd_release_payload::<FxPayload>() {
            if let Some((ti, ei, _)) = hover.and_then(|hp| self.event_zone(g, ys, hp)) {
                if self.p.tracks[ti].kind == TrackKind::Video {
                    self.push_undo();
                    let e = &mut self.p.tracks[ti].events[ei];
                    e.fx.extend(pl.0.iter().cloned());
                    self.win_fx = Some(e.id);
                    self.status = format!("Effet « {} » ajouté.", pl.0.iter().map(|f| f.label()).collect::<Vec<_>>().join(" + "));
                }
            }
        }
    }

    fn event_menu(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.tl.ctx_event else {
            if ui.add_enabled(!self.clipboard.is_empty(), egui::Button::new(tr("Coller")).shortcut_text(crate::platform::shortcut(tr("Ctrl+V")))).clicked() {
                self.cursor = self.click_time(self.tl.ctx_time);
                self.paste();
                ui.close_menu();
            }
            if ui.button(tr("Insérer un marqueur ici")).clicked() {
                self.cursor = self.click_time(self.tl.ctx_time);
                self.add_marker();
                ui.close_menu();
            }
            if ui.button(tr("Insérer un texte ici")).clicked() {
                let ctx = ui.ctx().clone();
                let mid = self.add_generator(crate::app::default_text(), &ctx);
                let t = self.click_time(self.tl.ctx_time);
                self.insert_media(mid, t, self.tl.ctx_track, true);
                self.win_gen = Some(mid);
                ui.close_menu();
            }
            ui.separator();
            if ui.button(tr("Insérer une piste vidéo")).clicked() {
                self.push_undo();
                self.focus_track = self.p.add_track(TrackKind::Video, Some(0));
                ui.close_menu();
            }
            if ui.button(tr("Insérer une piste audio")).clicked() {
                self.push_undo();
                self.focus_track = self.p.add_track(TrackKind::Audio, None);
                ui.close_menu();
            }
            return;
        };
        if ui.add(egui::Button::new(tr("Couper")).shortcut_text(crate::platform::shortcut(tr("Ctrl+X")))).clicked() {
            self.cut_selected();
            ui.close_menu();
        }
        if ui.add(egui::Button::new(tr("Copier")).shortcut_text(crate::platform::shortcut(tr("Ctrl+C")))).clicked() {
            self.copy_selected();
            ui.close_menu();
        }
        if ui.add(egui::Button::new(tr("Supprimer")).shortcut_text(crate::platform::shortcut(tr("Suppr")))).clicked() {
            self.delete_selected();
            ui.close_menu();
        }
        if ui.add(egui::Button::new(tr("Diviser au curseur")).shortcut_text(crate::platform::shortcut(tr("S")))).clicked() {
            self.split_at_cursor();
            ui.close_menu();
        }
        ui.separator();
        let is_video = self
            .p
            .find_event(id)
            .map(|(t, _)| self.p.tracks[t].kind == TrackKind::Video)
            .unwrap_or(false);
        for (title, out) in [("Type de fondu d'entrée", false), ("Type de fondu de sortie", true)] {
            ui.menu_button(title, |ui| {
                for c in Curve::ALL {
                    if ui.button(c.label()).clicked() {
                        self.push_undo();
                        if let Some(e) = self.p.event_mut(id) {
                            if out {
                                e.curve_out = c;
                            } else {
                                e.curve_in = c;
                            }
                        }
                        ui.close_menu();
                    }
                }
            });
        }
        if is_video {
            ui.menu_button(tr("Transition d'entrée"), |ui| {
                for t in Transition::ALL {
                    if ui.button(t.label()).clicked() {
                        self.push_undo();
                        if let Some(e) = self.p.event_mut(id) {
                            e.transition = t;
                        }
                        ui.close_menu();
                    }
                }
            });
            if ui.button(tr("Effets vidéo de l'événement…")).clicked() {
                self.win_fx = Some(id);
                ui.close_menu();
            }
            if ui.button(tr("Panoramique/Recadrage…")).clicked() {
                self.win_pan = Some(id);
                ui.close_menu();
            }
        }
        let (muted, looped, gen, media_id) = self
            .p
            .event(id)
            .map(|e| {
                let gen = self
                    .p
                    .media(e.media)
                    .map(|m| matches!(m.source, MediaSource::Generated(_)))
                    .unwrap_or(false);
                (e.mute, e.looped, gen, e.media)
            })
            .unwrap_or((false, false, false, 0));
        if gen && is_video && ui.button(tr("Modifier le média généré…")).clicked() {
            self.win_gen = Some(media_id);
            ui.close_menu();
        }
        ui.separator();
        if ui.selectable_label(muted, tr("Muet")).clicked() {
            self.push_undo();
            if let Some(e) = self.p.event_mut(id) {
                e.mute = !e.mute;
            }
            ui.close_menu();
        }
        if ui.selectable_label(looped, tr("Boucle")).clicked() {
            self.push_undo();
            if let Some(e) = self.p.event_mut(id) {
                e.looped = !e.looped;
            }
            ui.close_menu();
        }
        if ui.button(tr("Dissocier du groupe")).clicked() {
            self.push_undo();
            if let Some(e) = self.p.event_mut(id) {
                e.group = None;
            }
            ui.close_menu();
        }
        if ui.button(tr("Sélectionner comme région de boucle")).clicked() {
            if let Some(e) = self.p.event(id) {
                self.p.loop_region = Some((e.start, e.end()));
            }
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr("Propriétés…")).clicked() {
            self.win_props = Some(id);
            ui.close_menu();
        }
    }

    fn begin_drag(&mut self, g: &Geo, ys: &[(f32, f32)], o: Pos2, ctrl: bool) {
        let t = g.t(o.x);
        let Some((ti, ei, z)) = self.event_zone(g, ys, o) else {
            if !ctrl {
                self.sel.clear();
            }
            let t = self.click_time(t);
            self.drag = Some(Drag::LoopNew { from: t });
            self.cursor = t;
            return;
        };
        let id = self.p.tracks[ti].events[ei].id;
        if !self.sel.contains(&id) {
            self.select_event(id, ctrl);
        }
        self.push_undo();
        let e = self.p.tracks[ti].events[ei].clone();
        self.drag = Some(match z {
            Zone::Left => Drag::TrimL {
                id,
                start: e.start,
                len: e.length,
                off: e.offset,
            },
            Zone::Right => {
                let later = self.p.tracks[ti]
                    .events
                    .iter()
                    .filter(|o| o.start >= e.end() - 1e-6 && o.id != id)
                    .map(|o| (o.id, o.start))
                    .collect();
                Drag::TrimR { id, len: e.length, later }
            }
            Zone::FadeIn => Drag::FadeIn { id },
            Zone::FadeOut => Drag::FadeOut { id },
            Zone::Gain => Drag::Gain { id },
            _ => {
                let mut anchor = id;
                let mut anchor_track = ti;
                if ctrl {
                    // Ctrl+drag copies the selection
                    let mut copies = vec![];
                    for (tj, tr) in self.p.tracks.iter().enumerate() {
                        for ev in &tr.events {
                            if self.sel.contains(&ev.id) {
                                copies.push((tj, ev.clone()));
                            }
                        }
                    }
                    let mut new_sel = HashSet::new();
                    for (tj, mut ev) in copies {
                        let old = ev.id;
                        ev.id = self.p.new_id();
                        if old == id {
                            anchor = ev.id;
                            anchor_track = tj;
                        }
                        new_sel.insert(ev.id);
                        self.p.tracks[tj].events.push(ev);
                    }
                    self.sel = new_sel;
                }
                Drag::Move {
                    anchor,
                    grab_dt: t - e.start,
                    orig: self.selection_positions(),
                    anchor_track,
                }
            }
        });
    }

    fn selection_positions(&self) -> Vec<(Id, usize, f64)> {
        let mut v = vec![];
        for (ti, t) in self.p.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel.contains(&e.id) {
                    v.push((e.id, ti, e.start));
                }
            }
        }
        v
    }

    fn update_drag(&mut self, g: &Geo, ys: &[(f32, f32)], pp: Pos2) {
        let t = g.t(pp.x);
        let frame = 1.0 / self.p.fps;
        self.tl.snap_line = None;
        match self.drag.clone() {
            Some(Drag::Move {
                anchor,
                grab_dt,
                orig,
                anchor_track,
            }) => {
                let Some(&(_, _, a_start)) = orig.iter().find(|(id, _, _)| *id == anchor) else { return };
                let a_len = self.p.event(anchor).map(|e| e.length).unwrap_or(0.0);
                let ex: HashSet<Id> = orig.iter().map(|o| o.0).collect();
                let raw = t - grab_dt;
                let s1 = self.snap_time(raw, &ex);
                let line1 = self.tl.snap_line.take();
                let s2 = self.snap_time(raw + a_len, &ex) - a_len;
                let line2 = self.tl.snap_line.take();
                let (new_start, line) = if line2.is_none() || (line1.is_some() && (s1 - raw).abs() <= (s2 - raw).abs()) {
                    (s1, line1)
                } else {
                    (s2, line2)
                };
                self.tl.snap_line = line;
                let min_start = orig.iter().map(|o| o.2).fold(f64::MAX, f64::min);
                let delta = (new_start - a_start).max(-min_start);
                let target = self.track_at_y(ys, pp.y).unwrap_or(anchor_track);
                let dtrack = target as i64 - anchor_track as i64;
                let ok = orig.iter().all(|(_, ti, _)| {
                    let nt = *ti as i64 + dtrack;
                    nt >= 0
                        && (nt as usize) < self.p.tracks.len()
                        && self.p.tracks[nt as usize].kind == self.p.tracks[*ti].kind
                });
                let dtrack = if ok { dtrack } else { 0 };
                for (id, ti, start) in &orig {
                    let Some((cur_t, cur_e)) = self.p.find_event(*id) else { continue };
                    let mut e = self.p.tracks[cur_t].events.remove(cur_e);
                    e.start = start + delta;
                    let nt = (*ti as i64 + dtrack) as usize;
                    self.p.tracks[nt].events.push(e);
                }
                self.p.sort_events();
                self.touch();
            }
            Some(Drag::TrimL { id, start, len, off }) => {
                let ex: HashSet<Id> = [id].into_iter().collect();
                let end = start + len;
                let mut ns = self.snap_time(t, &ex).min(end - frame).max(0.0);
                let (looped, still) = self.event_media_flags(id);
                if !looped && !still {
                    ns = ns.max(start - off);
                }
                if let Some(e) = self.p.event_mut(id) {
                    e.start = ns;
                    e.length = end - ns;
                    e.offset = if still { 0.0 } else { (off + (ns - start)).max(0.0) };
                    e.fade_in = e.fade_in.min(e.length);
                    e.fade_out = e.fade_out.min(e.length - e.fade_in);
                }
                self.touch();
            }
            Some(Drag::TrimR { id, len, later }) => {
                let ex: HashSet<Id> = [id].into_iter().collect();
                let Some(e) = self.p.event(id) else { return };
                let (start, off) = (e.start, e.offset);
                let dur = self.p.media(e.media).map(|m| m.duration).unwrap_or(0.0);
                let ne = self.snap_time(t, &ex);
                let mut nl = (ne - start).max(frame);
                let (looped, still) = self.event_media_flags(id);
                if !looped && !still && dur > 0.0 {
                    nl = nl.min(dur - off);
                }
                if let Some(e) = self.p.event_mut(id) {
                    e.length = nl;
                    e.fade_out = e.fade_out.min(nl);
                    e.fade_in = e.fade_in.min(nl - e.fade_out);
                }
                if self.ripple {
                    let d = nl - len;
                    for (oid, ostart) in later {
                        if let Some(o) = self.p.event_mut(oid) {
                            o.start = (ostart + d).max(0.0);
                        }
                    }
                }
                self.touch();
            }
            Some(Drag::FadeIn { id }) => {
                let fps = self.p.fps;
                if let Some(e) = self.p.event_mut(id) {
                    let v = (t - e.start).clamp(0.0, e.length - e.fade_out);
                    e.fade_in = (v * fps).round() / fps;
                }
                self.touch();
            }
            Some(Drag::FadeOut { id }) => {
                let fps = self.p.fps;
                if let Some(e) = self.p.event_mut(id) {
                    let v = (e.end() - t).clamp(0.0, e.length - e.fade_in);
                    e.fade_out = (v * fps).round() / fps;
                }
                self.touch();
            }
            Some(Drag::Gain { id }) => {
                if let Some((ti, _)) = self.p.find_event(id) {
                    let (ty, th) = ys[ti];
                    let gv = 1.0 - ((pp.y - ty - 1.0) / (th - 4.0)).clamp(0.0, 1.0);
                    if let Some(e) = self.p.event_mut(id) {
                        e.gain = gv;
                    }
                    self.status = format!("Gain de l'événement : {:.0} %", gv * 100.0);
                    self.touch();
                }
            }
            Some(Drag::LoopNew { from }) => {
                let t = self.snap_time(t.max(0.0), &HashSet::new());
                if (t - from).abs() * g.pps > 3.0 {
                    self.p.loop_region = Some((from.min(t), from.max(t)));
                }
            }
            Some(other) => self.update_drag_tool(other, g, ys, pp),
            None => {}
        }
    }

    fn event_media_flags(&self, id: Id) -> (bool, bool) {
        let Some(e) = self.p.event(id) else { return (true, true) };
        let still = self
            .p
            .media(e.media)
            .map(|m| m.duration <= 0.0 || matches!(m.source, MediaSource::Generated(_)))
            .unwrap_or(true);
        (e.looped, still)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_event(&mut self, ctx: &egui::Context, p: &egui::Painter, g: &Geo, ti: usize, ei: usize, ty: f32, th: f32, pulse: f32) {
        let track = &self.p.tracks[ti];
        let e = track.events[ei].clone();
        let kind = track.kind;
        let color = track.color;
        let (fin, fout) = effective_fades(track, ei, self.auto_xf);
        let x0 = g.x(e.start);
        let x1 = g.x(e.end());
        if x1 < g.events.left() || x0 > g.events.right() {
            return;
        }
        let r = Rect::from_min_max(pos2(x0, ty + 1.0), pos2(x1.max(x0 + 1.0), ty + th - 2.0));
        let ep = p.with_clip_rect(r.intersect(g.events));
        let col = TRACK_COLORS[color % TRACK_COLORS.len()];
        let body = match kind {
            TrackKind::Video => theme::EVENT_VIDEO(),
            TrackKind::Audio => theme::EVENT_AUDIO(),
        };
        ep.rect_filled(r, 2.0, body);
        let content = Rect::from_min_max(pos2(r.left(), r.top() + EVENT_HEADER_H), r.max);
        let media = self.p.media(e.media).cloned();
        match kind {
            TrackKind::Video if content.height() > 6.0 => {
                if let (Some(m), Some(thumbs)) = (&media, media.as_ref().and_then(|m| self.store.thumbs(m.id))) {
                    if let Some((_, first)) = thumbs.frames.first() {
                        let dw = (first.w as f32 * content.height() / first.h as f32).max(8.0);
                        let step = if thumbs.frames.len() > 1 { thumbs.frames[1].0 - thumbs.frames[0].0 } else { 0.0 };
                        let skip = ((g.events.left() - r.left()) / dw).floor().max(0.0);
                        let mut x = r.left() + skip * dw;
                        let cp = ep.with_clip_rect(content.intersect(g.events));
                        while x < r.right().min(g.events.right()) {
                            let mut st = e.offset + (g.t(x) - e.start);
                            if m.duration > 0.0 && e.looped {
                                st %= m.duration;
                            }
                            let idx = if step > 0.0 {
                                ((st / step).round().max(0.0) as usize).min(thumbs.frames.len() - 1)
                            } else {
                                0
                            };
                            if let Some(tex) = self.thumb_texture(ctx, m.id, idx) {
                                let tr = Rect::from_min_size(pos2(x, content.top()), vec2(dw, content.height()));
                                cp.image(tex.id(), tr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            x += dw;
                        }
                    }
                }
            }
            TrackKind::Audio if content.height() > 6.0 => {
                if let Some(pcm) = self.store.audio(e.media) {
                    let stereo = content.height() > 34.0;
                    let lanes: Vec<(Rect, usize)> = if stereo {
                        let h2 = content.height() / 2.0;
                        vec![
                            (Rect::from_min_size(content.min, vec2(content.width(), h2)), 0),
                            (Rect::from_min_size(pos2(content.left(), content.top() + h2), vec2(content.width(), h2)), 2),
                        ]
                    } else {
                        vec![(content, 0)]
                    };
                    let mut mesh = Mesh::default();
                    let x_start = content.left().max(g.events.left()).floor();
                    let x_end = content.right().min(g.events.right());
                    let n_blocks = pcm.peaks.len();
                    let dur = pcm.duration();
                    let block_secs = PEAK_BLOCK as f64 / media::AUDIO_RATE as f64;
                    let mut x = x_start;
                    while x < x_end && n_blocks > 0 {
                        let lt0 = g.t(x) - e.start;
                        let lt1 = g.t(x + 1.0) - e.start;
                        let mut s0 = e.offset + lt0;
                        let mut s1 = e.offset + lt1;
                        if e.looped && dur > 0.0 {
                            let base = (s0 / dur).floor() * dur;
                            s0 -= base;
                            s1 -= base;
                        }
                        let b0 = (s0 / block_secs).floor().max(0.0) as usize;
                        let b1 = ((s1 / block_secs).ceil() as usize).max(b0 + 1);
                        if b0 < n_blocks {
                            for (lane, ch) in &lanes {
                                let mut lo = 0i32;
                                let mut hi = 0i32;
                                for pk in &pcm.peaks[b0..b1.min(n_blocks)] {
                                    lo = lo.min(pk[*ch] as i32);
                                    hi = hi.max(pk[*ch + 1] as i32);
                                }
                                let cy = lane.center().y;
                                // fDrawNormalizedWaves = 1: scale to the media's loudest peak
                                let norm = if DRAW_NORMALIZED_WAVES { 1.0 / pcm.max_peak.max(0.01) } else { 1.0 };
                                let half = lane.height() * 0.48 * e.gain * norm;
                                let y0 = cy - hi as f32 / 32768.0 * half;
                                let y1 = cy - lo as f32 / 32768.0 * half;
                                mesh.add_colored_rect(
                                    Rect::from_min_max(pos2(x, y0.min(cy - 0.5)), pos2(x + 1.0, y1.max(cy + 0.5))),
                                    theme::WAVE(),
                                );
                            }
                        }
                        x += 1.0;
                    }
                    ep.add(egui::Shape::mesh(mesh));
                    for (lane, _) in &lanes {
                        ep.line_segment(
                            [pos2(lane.left(), lane.center().y), pos2(lane.right(), lane.center().y)],
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
                        );
                    }
                }
            }
            _ => {}
        }
        let hr = Rect::from_min_max(r.min, pos2(r.right(), r.top() + EVENT_HEADER_H));
        let dim = |c: u8| (c as u16 * 2 / 5) as u8 + 30;
        ep.rect_filled(hr, 0.0, Color32::from_rgb(dim(col[0]), dim(col[1]), dim(col[2])));
        let name_x = r.left().max(g.events.left()) + 4.0;
        ep.with_clip_rect(Rect::from_min_max(hr.min, pos2(hr.right() - 32.0, hr.bottom())).intersect(g.events))
            .text(pos2(name_x, hr.center().y), egui::Align2::LEFT_CENTER, &e.name, FontId::proportional(11.0), theme::TEXT());
        if let Some(m) = &media {
            if m.duration > 0.0 && e.looped {
                let mut k = ((e.offset / m.duration).floor() + 1.0) * m.duration;
                while k < e.offset + e.length {
                    let x = g.x(e.start + k - e.offset);
                    ep.add(egui::Shape::convex_polygon(
                        vec![pos2(x - 4.0, r.top()), pos2(x + 4.0, r.top()), pos2(x, r.top() + 6.0)],
                        Color32::from_gray(20),
                        Stroke::NONE,
                    ));
                    k += m.duration;
                }
            }
        }
        if e.gain < 0.999 {
            let y = r.top() + (1.0 - e.gain) * (r.height() - 2.0);
            ep.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, Color32::from_rgb(120, 200, 255)));
        }
        // fades: shade above the curve, then the curve itself
        let h = r.height();
        let mut shade = Mesh::default();
        let shade_col = Color32::from_rgba_unmultiplied(0, 0, 0, 110);
        let mut curve_in = vec![];
        let mut curve_out = vec![];
        let n = 24;
        if fin > 0.0 {
            for i in 0..=n {
                let u = i as f64 / n as f64;
                curve_in.push(pos2(g.x(e.start + u * fin), r.bottom() - e.curve_in.gain_in(u) as f32 * h));
            }
        }
        if fout > 0.0 {
            for i in 0..=n {
                let u = i as f64 / n as f64;
                curve_out.push(pos2(g.x(e.end() - fout + u * fout), r.bottom() - e.curve_out.gain_out(u) as f32 * h));
            }
        }
        for c in [&curve_in, &curve_out] {
            for w in c.windows(2) {
                let (a, b) = (w[0], w[1]);
                let i0 = shade.vertices.len() as u32;
                shade.colored_vertex(pos2(a.x, r.top()), shade_col);
                shade.colored_vertex(pos2(b.x, r.top()), shade_col);
                shade.colored_vertex(b, shade_col);
                shade.colored_vertex(a, shade_col);
                shade.add_triangle(i0, i0 + 1, i0 + 2);
                shade.add_triangle(i0, i0 + 2, i0 + 3);
            }
        }
        ep.add(egui::Shape::mesh(shade));
        for c in [curve_in, curve_out] {
            if c.len() > 1 {
                ep.add(egui::Shape::line(c, Stroke::new(1.2, Color32::from_rgb(235, 235, 235))));
            }
        }
        for hx in [g.x(e.start + fin), g.x(e.end() - fout)] {
            ep.rect_filled(Rect::from_center_size(pos2(hx, r.top() + 3.0), vec2(5.0, 5.0)), 0.0, Color32::from_gray(200));
        }
        if kind == TrackKind::Video && th >= 40.0 && x1 - x0 > 60.0 {
            let fx_r = Rect::from_min_size(pos2(x1 - 30.0, r.top() + 1.0), vec2(13.0, 12.0));
            let pan_r = Rect::from_min_size(pos2(x1 - 15.0, r.top() + 1.0), vec2(13.0, 12.0));
            let on = Color32::from_rgb(70, 150, 90);
            ep.rect_filled(fx_r, 2.0, if e.fx.is_empty() { Color32::from_gray(60) } else { on });
            ep.text(fx_r.center(), egui::Align2::CENTER_CENTER, "fx", FontId::proportional(9.0), theme::TEXT());
            ep.rect_filled(pan_r, 2.0, if e.pan.is_identity() { Color32::from_gray(60) } else { on });
            ep.rect_stroke(pan_r.shrink(3.0), 0.0, Stroke::new(1.0, theme::TEXT()), StrokeKind::Inside);
        }
        if e.mute {
            ep.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
        }
        if self.sel.contains(&e.id) {
            let a = LIGHT_SELECTION_ALPHA as f32 + (HEAVY_SELECTION_ALPHA - LIGHT_SELECTION_ALPHA) as f32 * pulse;
            ep.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(110, 170, 255, (a * 0.45) as u8));
            ep.rect_stroke(r, 2.0, Stroke::new(1.5, Color32::from_rgb(140, 195, 255)), StrokeKind::Inside);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else {
            ep.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(20)), StrokeKind::Inside);
        }
    }
}

// ---------------------------------------------------------------------------
// Edit tools, envelopes and the extra event menu entries.

use crate::tools::{EditTool, EnvKind};

impl App {
    /// Envelope points of a track for the current envelope kind.
    fn env_points(&self, ti: usize) -> (&[EnvPoint], f32, f32, f32) {
        let t = &self.p.tracks[ti];
        let (lo, hi) = self.env_kind.range(t.kind);
        match self.env_kind {
            EnvKind::Pan => (&t.pan_env, lo, hi, t.pan),
            EnvKind::FadeToColor => (&t.fade_env, lo, hi, 0.0),
            _ => (&t.vol_env, lo, hi, t.level),
        }
    }

    fn env_points_mut(&mut self, ti: usize) -> &mut Vec<EnvPoint> {
        let kind = self.env_kind;
        let t = &mut self.p.tracks[ti];
        match kind {
            EnvKind::Pan => &mut t.pan_env,
            EnvKind::FadeToColor => &mut t.fade_env,
            _ => &mut t.vol_env,
        }
    }

    fn y_to_val(y: f32, ty: f32, th: f32, lo: f32, hi: f32) -> f32 {
        let f = 1.0 - ((y - ty - 2.0) / (th - 4.0)).clamp(0.0, 1.0);
        lo + (hi - lo) * f
    }

    fn val_to_y(v: f32, ty: f32, th: f32, lo: f32, hi: f32) -> f32 {
        let f = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        ty + 2.0 + (1.0 - f) * (th - 4.0)
    }

    /// Draws track envelopes (volume/level, pan, fade to colour) and event
    /// velocity envelopes as lines with handles.
    pub(crate) fn draw_envelopes(&self, p: &egui::Painter, g: &Geo, ys: &[(f32, f32)]) {
        let area = g.events;
        for (ti, t) in self.p.tracks.iter().enumerate() {
            let (ty, th) = ys[ti];
            if ty > area.bottom() || ty + th < area.top() || th < 20.0 {
                continue;
            }
            let (vlo, vhi) = EnvKind::Volume.range(t.kind);
            for (pts, lo, hi, col) in [
                (&t.vol_env, vlo, vhi, Color32::from_rgb(90, 160, 255)),
                (&t.pan_env, -1.0, 1.0, Color32::from_rgb(230, 90, 90)),
                (&t.fade_env, -1.0, 1.0, Color32::from_rgb(200, 200, 200)),
            ] {
                if pts.is_empty() {
                    continue;
                }
                let mut line = vec![];
                let mut x = area.left();
                while x <= area.right() {
                    let tt = g.t(x);
                    line.push(pos2(x, Self::val_to_y(env_value(pts, tt, 0.0), ty, th, lo, hi)));
                    x += 3.0;
                }
                p.add(egui::Shape::line(line, Stroke::new(1.5, col)));
                for pt in pts.iter() {
                    let c = pos2(g.x(pt.t), Self::val_to_y(pt.v, ty, th, lo, hi));
                    p.rect_filled(Rect::from_center_size(c, vec2(6.0, 6.0)), 1.0, col);
                }
            }
            for e in &t.events {
                if e.velocity.is_empty() {
                    continue;
                }
                let line: Vec<Pos2> = (0..=40)
                    .map(|k| {
                        let lt = e.length * k as f64 / 40.0;
                        pos2(g.x(e.start + lt), Self::val_to_y(env_value(&e.velocity, lt, 100.0), ty, th, -100.0, 1000.0))
                    })
                    .collect();
                p.add(egui::Shape::line(line, Stroke::new(1.5, Color32::from_rgb(110, 220, 120))));
                for pt in &e.velocity {
                    let c = pos2(g.x(e.start + pt.t), Self::val_to_y(pt.v, ty, th, -100.0, 1000.0));
                    p.rect_filled(Rect::from_center_size(c, vec2(6.0, 6.0)), 1.0, Color32::from_rgb(110, 220, 120));
                }
            }
        }
    }

    /// Drag start, depending on the active edit tool (and Alt / Ctrl).
    fn begin_drag_tool(&mut self, g: &Geo, ys: &[(f32, f32)], o: Pos2, ctrl: bool, alt: bool) {
        let t = g.t(o.x);
        match self.tool {
            EditTool::Selection => {
                self.drag = Some(Drag::RectSel { from: o });
                return;
            }
            EditTool::Zoom => {
                self.drag = Some(Drag::ZoomRect { from: o });
                return;
            }
            EditTool::Envelope => {
                let Some(ti) = self.track_at_y(ys, o.y) else { return };
                let (ty, th) = ys[ti];
                if self.env_kind == EnvKind::Velocity {
                    let Some((_, ei, _)) = self.event_zone(g, ys, o) else { return };
                    let e = self.p.tracks[ti].events[ei].clone();
                    self.push_undo();
                    let lt = (t - e.start).clamp(0.0, e.length);
                    let v = Self::y_to_val(o.y, ty, th, -100.0, 1000.0);
                    let ev = self.p.event_mut(e.id).unwrap();
                    if ev.velocity.is_empty() {
                        ev.velocity.push(EnvPoint { t: 0.0, v: 100.0, curve: Curve::Linear });
                        ev.velocity.push(EnvPoint { t: e.length, v: 100.0, curve: Curve::Linear });
                    }
                    let idx = match ev.velocity.iter().position(|p| ((p.t - lt) * g.pps).abs() < 6.0) {
                        Some(i) => i,
                        None => {
                            env_set(&mut ev.velocity, lt, v);
                            ev.velocity.iter().position(|p| (p.t - lt).abs() < 1e-4).unwrap_or(0)
                        }
                    };
                    self.drag = Some(Drag::Env { track: ti, event: Some(e.id), idx });
                    return;
                }
                let (pts, lo, hi, def) = self.env_points(ti);
                let near = pts.iter().position(|p| ((p.t - t) * g.pps).abs() < 6.0);
                let empty = pts.is_empty();
                self.push_undo();
                let v = Self::y_to_val(o.y, ty, th, lo, hi);
                let env = self.env_points_mut(ti);
                if empty {
                    env.push(EnvPoint { t: 0.0, v: def, curve: Curve::Linear });
                }
                let idx = match near {
                    Some(i) => i,
                    None => {
                        env_set(env, t.max(0.0), v);
                        env.iter().position(|p| (p.t - t.max(0.0)).abs() < 1e-4).unwrap_or(0)
                    }
                };
                self.focus_track = ti;
                self.drag = Some(Drag::Env { track: ti, event: None, idx });
                return;
            }
            _ => {}
        }
        if let Some((ti, ei, z)) = self.event_zone(g, ys, o) {
            let e = self.p.tracks[ti].events[ei].clone();
            if e.locked {
                self.select_event(e.id, ctrl);
                self.status = tr("Événement verrouillé : déverrouillez-le pour le modifier.").into();
                return;
            }
            let body = matches!(z, Zone::Body);
            let edge = matches!(z, Zone::Left | Zone::Right);
            if body && (alt || self.tool == EditTool::Slip) {
                self.push_undo();
                self.select_event(e.id, false);
                self.drag = Some(Drag::Slip { id: e.id, off0: e.offset, t0: t });
                return;
            }
            if body && self.tool == EditTool::Slide {
                self.push_undo();
                self.select_event(e.id, false);
                let track = &self.p.tracks[ti];
                let prev = track.events.iter().filter(|o| o.id != e.id && (o.end() - e.start).abs() < 0.05).map(|o| (o.id, o.length)).next();
                let next = track.events.iter().filter(|o| o.id != e.id && (o.start - e.end()).abs() < 0.05).map(|o| (o.id, o.start, o.offset, o.length)).next();
                self.drag = Some(Drag::Slide { id: e.id, start0: e.start, t0: t, prev, next });
                return;
            }
            if body && self.tool == EditTool::Shuffle {
                self.push_undo();
                self.select_event(e.id, false);
                self.drag = Some(Drag::Shuffle { id: e.id });
                return;
            }
            if edge && (ctrl || self.tool == EditTool::Stretch) {
                self.push_undo();
                self.select_event(e.id, false);
                self.drag = Some(Drag::Stretch { id: e.id, len0: e.length, rate0: e.rate, left: matches!(z, Zone::Left), start0: e.start });
                return;
            }
        }
        self.begin_drag(g, ys, o, ctrl);
    }

    fn update_drag_tool(&mut self, d: Drag, g: &Geo, ys: &[(f32, f32)], pp: Pos2) {
        let t = g.t(pp.x);
        match d {
            Drag::Env { track, event, idx } => {
                let (ty, th) = ys[track];
                if let Some(id) = event {
                    let st = self.p.event(id).map(|e| (e.start, e.length)).unwrap_or((0.0, 0.0));
                    let v = Self::y_to_val(pp.y, ty, th, -100.0, 1000.0);
                    if let Some(e) = self.p.event_mut(id) {
                        let n = e.velocity.len();
                        if let Some(p) = e.velocity.get_mut(idx) {
                            let lo = if idx > 0 { 0.0 } else { 0.0 };
                            p.t = if idx == 0 { 0.0 } else if idx + 1 == n { st.1 } else { (t - st.0).clamp(lo, st.1) };
                            p.v = (v / 5.0).round() * 5.0;
                            self.status = format!("{} {:.0} %", tr("Vitesse :"), p.v);
                        }
                        e.velocity.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                    }
                } else {
                    let (_, lo, hi, _) = self.env_points(track);
                    let v = Self::y_to_val(pp.y, ty, th, lo, hi);
                    let kind = self.env_kind;
                    let env = self.env_points_mut(track);
                    if let Some(p) = env.get_mut(idx) {
                        p.t = t.max(0.0);
                        p.v = v;
                    }
                    env.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
                    self.status = match kind {
                        EnvKind::Volume => format!("{} {:.0} %", tr("Niveau :"), v * 100.0),
                        _ => format!("{} {:.0} %", tr("Valeur :"), v * 100.0),
                    };
                }
                self.touch();
            }
            Drag::Slip { id, off0, t0 } => {
                if let Some(e) = self.p.event_mut(id) {
                    e.offset = (off0 - (t - t0) * e.rate).max(0.0);
                }
                self.status = tr("Glisser : le contenu se déplace dans l'événement.").into();
                self.touch();
            }
            Drag::Slide { id, start0, t0, prev, next } => {
                let mut d = t - t0;
                if let Some((_, plen)) = prev {
                    d = d.max(-plen + 1.0 / self.p.fps);
                }
                if let Some((_, _, _, nlen)) = next {
                    d = d.min(nlen - 1.0 / self.p.fps);
                }
                let d = self.p.quantize(d);
                if let Some(e) = self.p.event_mut(id) {
                    e.start = (start0 + d).max(0.0);
                }
                if let Some((pid, plen)) = prev {
                    if let Some(o) = self.p.event_mut(pid) {
                        o.length = plen + d;
                    }
                }
                if let Some((nid, nstart, noff, nlen)) = next {
                    if let Some(o) = self.p.event_mut(nid) {
                        o.start = nstart + d;
                        o.offset = (noff + d * o.rate).max(0.0);
                        o.length = nlen - d;
                    }
                }
                self.touch();
            }
            Drag::Shuffle { .. } => {
                self.tl.snap_line = Some(t);
            }
            Drag::Stretch { id, len0, rate0, left, start0 } => {
                let end0 = start0 + len0;
                let new_len = if left { (end0 - t).max(1.0 / self.p.fps) } else { (t - start0).max(1.0 / self.p.fps) };
                // the same media span over a new length: rate = old span / new length
                let rate = (rate0 * len0 / new_len).clamp(0.25, 4.0);
                let new_len = rate0 * len0 / rate;
                if let Some(e) = self.p.event_mut(id) {
                    e.rate = rate;
                    e.length = new_len;
                    if left {
                        e.start = end0 - new_len;
                    }
                    e.fade_in = e.fade_in.min(e.length);
                    e.fade_out = e.fade_out.min(e.length - e.fade_in);
                }
                self.status = format!("{} {:.0} %", tr("Vitesse :"), rate * 100.0);
                self.touch();
            }
            _ => {}
        }
    }

    fn end_drag_tool(&mut self, g: &Geo, ys: &[(f32, f32)], pp: Pos2, ctrl: bool, shift: bool) {
        match self.drag.clone() {
            Some(Drag::RectSel { from }) => {
                let (t0, t1) = (g.t(from.x), g.t(pp.x));
                let tr0 = self.track_at_y(ys, from.y).unwrap_or(0);
                let tr1 = self.track_at_y(ys, pp.y).unwrap_or(self.p.tracks.len().saturating_sub(1));
                self.select_rect(t0, t1, tr0, tr1, ctrl || shift);
                self.p.loop_region = Some((t0.min(t1).max(0.0), t0.max(t1)));
            }
            Some(Drag::ZoomRect { from }) => {
                let (a, b) = (g.t(from.x).min(g.t(pp.x)), g.t(from.x).max(g.t(pp.x)));
                if (b - a) * g.pps > 8.0 {
                    self.tl.pps = (self.tl.width as f64 / (b - a)).clamp(0.5, 4000.0);
                    self.tl.scroll = a.max(0.0);
                }
            }
            Some(Drag::Shuffle { id }) => self.shuffle_to(id, g.t(pp.x)),
            _ => {}
        }
    }

    /// Single clicks with the zoom and split/trim tools. Returns true when
    /// the click was used.
    fn tool_click(&mut self, g: &Geo, ys: &[(f32, f32)], pp: Pos2, shift: bool) -> bool {
        let t = g.t(pp.x);
        match self.tool {
            EditTool::Zoom => {
                self.tl.zoom_at(if shift { 0.5 } else { 2.0 }, t);
                true
            }
            EditTool::SplitTrim => {
                if let Some((ti, ei, _)) = self.event_zone(g, ys, pp) {
                    let id = self.p.tracks[ti].events[ei].id;
                    self.sel.clear();
                    self.sel.insert(id);
                    self.cursor = self.click_time(t);
                    self.split_at_cursor();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Extra entries of the event context menu.
    fn event_menu_extra(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.tl.ctx_event else { return };
        let Some(e) = self.p.event(id).cloned() else { return };
        let is_video = self.p.find_event(id).map(|(t, _)| self.p.tracks[t].kind == TrackKind::Video).unwrap_or(false);
        let media = self.p.media(e.media).cloned();
        if ui.selectable_label(e.locked, tr("Verrouiller")).clicked() {
            self.toggle_lock();
            ui.close_menu();
        }
        ui.menu_button(tr("Vitesse de lecture"), |ui| {
            for r in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 4.0] {
                if ui.selectable_label((e.rate - r).abs() < 1e-6, format!("{} %", (r * 100.0) as i32)).clicked() {
                    self.push_undo();
                    if let Some(x) = self.p.event_mut(id) {
                        let span = x.length * x.rate;
                        x.rate = r;
                        x.length = span / r;
                    }
                    ui.close_menu();
                }
            }
            if ui.selectable_label(e.reverse, tr("Lecture inversée")).clicked() {
                self.push_undo();
                if let Some(x) = self.p.event_mut(id) {
                    x.reverse = !x.reverse;
                }
                ui.close_menu();
            }
            if ui.button(tr("Enveloppe de vitesse")).clicked() {
                self.insert_envelope(EnvKind::Velocity);
                self.env_kind = EnvKind::Velocity;
                self.tool = EditTool::Envelope;
                ui.close_menu();
            }
        });
        if is_video {
            ui.menu_button(tr("Rééchantillonnage"), |ui| {
                for (r, l) in [(Resample::Smart, tr("Intelligent")), (Resample::Force, tr("Forcé")), (Resample::Disable, tr("Désactivé"))] {
                    if ui.selectable_label(e.resample == r, l).clicked() {
                        self.push_undo();
                        if let Some(x) = self.p.event_mut(id) {
                            x.resample = r;
                        }
                        ui.close_menu();
                    }
                }
            });
            if ui.selectable_label(e.keep_aspect, tr("Conserver le format d'image")).clicked() {
                self.push_undo();
                if let Some(x) = self.p.event_mut(id) {
                    x.keep_aspect = !x.keep_aspect;
                }
                ui.close_menu();
            }
            let has_deflicker = e.fx.iter().any(|f| matches!(f, VideoFx::Deflicker { .. }));
            if ui.selectable_label(has_deflicker, tr("Réduire le scintillement d'entrelacement")).clicked() {
                self.push_undo();
                if let Some(x) = self.p.event_mut(id) {
                    if has_deflicker {
                        x.fx.retain(|f| !matches!(f, VideoFx::Deflicker { .. }));
                    } else {
                        x.fx.push(VideoFx::Deflicker { amount: 0.5 });
                    }
                }
                ui.close_menu();
            }
            if ui.button(tr("Convertir la transition en coupe")).clicked() {
                self.transition_to_cut(id);
                ui.close_menu();
            }
            if ui.button(tr("Stabilisation et suivi…")).clicked() {
                self.win_tracking = Some(id);
                ui.close_menu();
            }
        } else {
            if ui.button(tr("Audio de l'événement (effets, hauteur)…")).clicked() {
                self.win_event_audio = Some(id);
                ui.close_menu();
            }
            if ui.selectable_label(e.normalize, tr("Normaliser")).clicked() {
                self.push_undo();
                if let Some(x) = self.p.event_mut(id) {
                    x.normalize = !x.normalize;
                }
                ui.close_menu();
            }
            if ui.selectable_label(e.invert_phase, tr("Inverser la phase")).clicked() {
                self.push_undo();
                if let Some(x) = self.p.event_mut(id) {
                    x.invert_phase = !x.invert_phase;
                }
                ui.close_menu();
            }
            if ui.button(tr("Ouvrir dans l'éditeur audio")).clicked() {
                self.open_in_sound_editor(id);
                ui.close_menu();
            }
        }
        ui.menu_button(tr("Prises"), |ui| {
            ui.label(format!("{} {}", e.takes.len() + 1, tr("prise(s)")));
            if ui.button(tr("Prise suivante (T)")).clicked() {
                self.cycle_take(true);
                ui.close_menu();
            }
            if ui.button(tr("Prise précédente (Maj+T)")).clicked() {
                self.cycle_take(false);
                ui.close_menu();
            }
        });
        if let Some(m) = &media {
            if ui.button(tr("Ouvrir dans la rogneuse")).clicked() {
                self.open_in_trimmer(m.id);
                ui.close_menu();
            }
            if ui.button(tr("Propriétés du média…")).clicked() {
                self.win_media_props = Some(m.id);
                ui.close_menu();
            }
            if matches!(m.source, MediaSource::Nested(_)) && ui.button(tr("Ouvrir la timeline imbriquée")).clicked() {
                let ctx = ui.ctx().clone();
                self.open_nested(m.id, &ctx);
                ui.close_menu();
            }
        }
        if ui.button(tr("Sélectionner l'événement semblable suivant")).clicked() {
            self.select_similar(true);
            ui.close_menu();
        }
    }
}
