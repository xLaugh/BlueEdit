//! Docked tools: trimmer (source monitor), mixer console, video scopes,
//! colour grading, edit details, notes, loudness, history, surround panner,
//! plug-in manager.

use crate::app::{App, MediaPayload};
use crate::i18n::tr;
use crate::model::*;
use crate::panels::fr_num;
use crate::theme;
use egui::{pos2, vec2, Color32, FontId, Rect, Sense, Stroke};

/// Source monitor state (Trimmer window).
#[derive(Default)]
pub struct Trimmer {
    pub media: Option<Id>,
    pub pos: f64,
    pub mark_in: Option<f64>,
    pub mark_out: Option<f64>,
    pub history: Vec<Id>,
    pub tex: Option<egui::TextureHandle>,
    pub tex_sig: (Id, i64),
}

pub fn db(v: f32) -> String {
    if v > 0.0 {
        format!("{} dB", fr_num(20.0 * v.log10() as f64, 1))
    } else {
        "-Inf.".into()
    }
}

/// Small editor for a list of audio effects; returns true when changed.
pub fn afx_list_ui(ui: &mut egui::Ui, list: &mut Vec<crate::audio_fx::AudioFx>, salt: &str) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (i, fx) in list.iter_mut().enumerate() {
        egui::CollapsingHeader::new(fx.label()).id_salt((salt, i)).default_open(true).show(ui, |ui| {
            changed |= fx.ui(ui);
            if ui.small_button(tr("Retirer")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    ui.menu_button(tr("Ajouter un effet audio…"), |ui| {
        for f in crate::audio_fx::AudioFx::presets() {
            if ui.button(f.label()).clicked() {
                list.push(f);
                changed = true;
                ui.close_menu();
            }
        }
    });
    changed
}

impl App {
    // ------------------------------------------------------------- trimmer

    pub(crate) fn trimmer_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(mid) = self.trimmer.media.or_else(|| self.p.media.iter().find(|m| m.has_video || m.has_audio).map(|m| m.id)) else {
            ui.label(egui::RichText::new(tr("Glissez un média ici ou double-cliquez sur un média du projet pour l'ouvrir dans la rogneuse.")).color(theme::TEXT_DIM()));
            return;
        };
        let Some(m) = self.p.media(mid).cloned() else {
            self.trimmer.media = None;
            return;
        };
        self.trimmer.media = Some(mid);
        let dur = if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN };
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("trim_hist").width(200.0).selected_text(&m.name).show_ui(ui, |ui| {
                for h in self.trimmer.history.clone() {
                    let name = self.p.media(h).map(|x| x.name.clone());
                    if let Some(name) = name {
                        if ui.selectable_label(h == mid, name).clicked() {
                            self.trimmer.media = Some(h);
                            self.trimmer.pos = 0.0;
                            self.trimmer.mark_in = None;
                            self.trimmer.mark_out = None;
                        }
                    }
                }
            });
            ui.label(egui::RichText::new(format!("{} / {}", self.fmt_time(self.trimmer.pos), self.fmt_time(dur))).monospace());
        });
        let avail = ui.available_rect_before_wrap();
        let pic_h = (avail.height() - 64.0).max(40.0);
        let pic = Rect::from_min_size(avail.min, vec2(avail.width(), pic_h));
        let resp = ui.allocate_rect(pic, Sense::click_and_drag());
        let painter = ui.painter_at(pic);
        painter.rect_filled(pic, 0.0, Color32::from_rgb(12, 12, 14));
        if m.has_video {
            let frame = (self.trimmer.pos * self.p.fps) as i64;
            if self.trimmer.tex_sig != (mid, frame) || self.trimmer.tex.is_none() {
                let w = 320u32;
                let h = ((320.0 * m.height.max(1) as f64 / m.width.max(1) as f64) as u32).max(2) & !1;
                let mut tmp = Project::empty();
                tmp.width = w;
                tmp.height = h;
                tmp.fps = self.p.fps;
                let mut mm = m.clone();
                mm.id = 1;
                tmp.media.push(mm.clone());
                let ti = tmp.add_track(TrackKind::Video, None);
                let mut e = tmp.new_event(&mm, 0.0, dur.max(0.04));
                e.offset = self.trimmer.pos;
                e.looped = false;
                e.id = mid ^ 0x2000_0000;
                tmp.tracks[ti].events.push(e);
                let mut src = crate::compositor::PreviewSource { server: &self.server, store: &self.store };
                let img = self.comp.compose(&tmp, 0.0, w, h, false, &mut src);
                let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                match &mut self.trimmer.tex {
                    Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                    None => self.trimmer.tex = Some(ctx.load_texture("trimmer", ci, egui::TextureOptions::LINEAR)),
                }
                self.trimmer.tex_sig = (mid, frame);
            }
            if let Some(t) = &self.trimmer.tex {
                let s = t.size_vec2();
                let k = (pic.width() / s.x).min(pic.height() / s.y);
                painter.image(t.id(), Rect::from_center_size(pic.center(), s * k), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        } else if let Some(a) = self.store.audio(mid) {
            let n = a.peaks.len().max(1);
            let wpx = pic.width().max(1.0) as usize;
            for x in 0..wpx {
                if let Some(p) = a.peaks.get(x * n / wpx) {
                    let v = (p[1] as f32 - p[0] as f32) / 65536.0 / a.max_peak.max(0.05);
                    let cx = pic.left() + x as f32;
                    painter.line_segment([pos2(cx, pic.center().y - v * pic.height() * 0.45), pos2(cx, pic.center().y + v * pic.height() * 0.45)], Stroke::new(1.0, theme::WAVE()));
                }
            }
        }
        resp.dnd_set_drag_payload(MediaPayload(mid));
        if let Some(pl) = resp.dnd_release_payload::<MediaPayload>() {
            self.open_in_trimmer(pl.0);
        }
        let bar = Rect::from_min_size(pos2(avail.left(), pic.bottom() + 4.0), vec2(avail.width(), 14.0));
        let bresp = ui.interact(bar, ui.id().with("trim_bar"), Sense::click_and_drag());
        let bp = ui.painter_at(bar);
        bp.rect_filled(bar, 2.0, theme::ui_color(19));
        let x_of = |t: f64| bar.left() + (t / dur) as f32 * bar.width();
        if let (Some(a), Some(b)) = (self.trimmer.mark_in, self.trimmer.mark_out) {
            bp.rect_filled(Rect::from_min_max(pos2(x_of(a), bar.top()), pos2(x_of(b), bar.bottom())), 0.0, theme::ACCENT().gamma_multiply(0.6));
        }
        bp.line_segment([pos2(x_of(self.trimmer.pos), bar.top()), pos2(x_of(self.trimmer.pos), bar.bottom())], Stroke::new(2.0, theme::CURSOR()));
        if let Some(p) = bresp.interact_pointer_pos() {
            if bresp.dragged() || bresp.clicked() {
                self.trimmer.pos = (((p.x - bar.left()) / bar.width()) as f64 * dur).clamp(0.0, dur);
            }
        }
        ui.add_space(bar.height() + 6.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Entrée (I)")).clicked() {
                self.trimmer.mark_in = Some(self.trimmer.pos);
            }
            if ui.button(tr("Sortie (O)")).clicked() {
                self.trimmer.mark_out = Some(self.trimmer.pos);
            }
            let fr = 1.0 / self.p.fps;
            if ui.button("◀").clicked() {
                self.trimmer.pos = (self.trimmer.pos - fr).max(0.0);
            }
            if ui.button("▶").clicked() {
                self.trimmer.pos = (self.trimmer.pos + fr).min(dur);
            }
            ui.separator();
            let (a, b) = self.trimmer_range(dur);
            if ui.button(tr("Insérer au curseur")).on_hover_text(tr("Ajoute la sélection au curseur et décale la suite (insertion)")).clicked() {
                self.trimmer_to_timeline(mid, a, b, true);
            }
            if ui.button(tr("Écraser")).on_hover_text(tr("Remplace ce qui se trouve sous le curseur (écrasement)")).clicked() {
                self.trimmer_to_timeline(mid, a, b, false);
            }
            if ui.button(tr("Créer un sous-clip")).clicked() {
                self.trimmer_subclip(mid, a, b, ctx);
            }
        });
    }

    fn trimmer_range(&self, dur: f64) -> (f64, f64) {
        let a = self.trimmer.mark_in.unwrap_or(0.0);
        let b = self.trimmer.mark_out.filter(|b| *b > a).unwrap_or(dur);
        (a, b)
    }

    /// A subclip: a nested project holding just the selected part.
    fn trimmer_subclip(&mut self, mid: Id, a: f64, b: f64, ctx: &egui::Context) {
        let Some(m) = self.p.media(mid).cloned() else { return };
        let mut sub = Project::empty();
        sub.width = if m.width > 0 { m.width } else { self.p.width };
        sub.height = if m.height > 0 { m.height } else { self.p.height };
        sub.fps = self.p.fps;
        sub.name = format!("{} ({}–{})", m.name, self.fmt_time(a), self.fmt_time(b));
        let id = sub.add_media(m.clone());
        let mm = sub.media(id).unwrap().clone();
        let len = (b - a).max(1.0 / self.p.fps);
        for kind in [TrackKind::Video, TrackKind::Audio] {
            if (kind == TrackKind::Video && m.has_video) || (kind == TrackKind::Audio && m.has_audio) {
                let ti = sub.add_track(kind, None);
                let mut e = sub.new_event(&mm, 0.0, len);
                e.offset = a;
                e.looped = false;
                sub.tracks[ti].events.push(e);
            }
        }
        let dir = crate::plugins::data_dir().join("sous-clips");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}.{}", sub.name.replace(['/', '\\', ':'], "_"), crate::app::PROJECT_EXT));
        if let Ok(s) = serde_json::to_string_pretty(&sub) {
            let _ = std::fs::write(&path, s);
        }
        match crate::media::probe_nested(&path.to_string_lossy()) {
            Ok(mut nm) => {
                nm.name = sub.name.clone();
                let nid = self.p.add_media(nm);
                let nm = self.p.media(nid).unwrap().clone();
                self.store.ensure(&nm, ctx);
                self.status = tr("Sous-clip créé dans le média du projet.").into();
            }
            Err(e) => self.status = e,
        }
    }

    pub fn open_in_trimmer(&mut self, id: Id) {
        self.trimmer.media = Some(id);
        self.trimmer.pos = 0.0;
        self.trimmer.mark_in = None;
        self.trimmer.mark_out = None;
        self.trimmer.history.retain(|h| *h != id);
        self.trimmer.history.insert(0, id);
        self.trimmer.history.truncate(20);
        self.show_tab(crate::docks::Tab::Trimmer);
    }

    /// Insert (ripple) or overwrite the trimmer selection at the cursor.
    fn trimmer_to_timeline(&mut self, mid: Id, a: f64, b: f64, insert: bool) {
        let Some(m) = self.p.media(mid).cloned() else { return };
        let len = (b - a).max(1.0 / self.p.fps);
        let at = self.p.quantize(self.cursor);
        self.push_undo();
        let group = if m.has_video && m.has_audio { Some(self.p.new_id()) } else { None };
        let mut kinds = vec![];
        if m.has_video {
            kinds.push(TrackKind::Video);
        }
        if m.has_audio {
            kinds.push(TrackKind::Audio);
        }
        for kind in kinds {
            let ti = self.p.tracks.iter().position(|t| t.kind == kind).unwrap_or_else(|| self.p.add_track(kind, None));
            let track = &mut self.p.tracks[ti];
            let mut extra = vec![];
            if insert {
                for e in &mut track.events {
                    if e.start < at && e.end() > at {
                        let mut r = e.clone();
                        let cut = at - e.start;
                        r.start = at;
                        r.offset = e.offset + cut * e.rate;
                        r.length = e.length - cut;
                        r.fade_in = 0.0;
                        e.length = cut;
                        e.fade_out = 0.0;
                        extra.push(r);
                    }
                }
                for e in &mut track.events {
                    if e.start >= at - 1e-6 {
                        e.start += len;
                    }
                }
                for mut r in extra.drain(..) {
                    r.id = 0;
                    r.start += len;
                    track.events.push(r);
                }
            } else {
                let (s, t1) = (at, at + len);
                track.events.retain(|e| !(e.start >= s && e.end() <= t1));
                for e in &mut track.events {
                    if e.start < s && e.end() > t1 {
                        let mut r = e.clone();
                        r.offset = e.offset + (t1 - e.start) * e.rate;
                        r.start = t1;
                        r.length = e.end() - t1;
                        extra.push(r);
                        e.length = s - e.start;
                    } else if e.start < s && e.end() > s {
                        e.length = s - e.start;
                    } else if e.start < t1 && e.end() > t1 {
                        let d = t1 - e.start;
                        e.offset += d * e.rate;
                        e.start = t1;
                        e.length -= d;
                    }
                }
                for mut r in extra.drain(..) {
                    r.id = 0;
                    track.events.push(r);
                }
            }
            let e = Event { id: 0, media: m.id, name: m.name.clone(), start: at, length: len, offset: a, group, looped: false, ..Event::blank() };
            self.p.tracks[ti].events.push(e);
        }
        let mut next = self.p.next_id;
        for t in &mut self.p.tracks {
            for e in &mut t.events {
                if e.id == 0 {
                    e.id = next;
                    next += 1;
                }
            }
        }
        self.p.next_id = next;
        self.p.sort_events();
        self.cursor = at + len;
        self.status = if insert { tr("Inséré depuis la rogneuse.").into() } else { tr("Écrasé depuis la rogneuse.").into() };
    }

    // --------------------------------------------------------------- mixer

    pub(crate) fn mixer_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(tr("Ajouter un bus audio")).clicked() {
                self.push_undo();
                let n = self.p.buses.len();
                self.p.buses.push(Bus { name: format!("Bus {}", (b'A' + (n % 26) as u8) as char), level: 1.0, pan: 0.0, mute: false });
            }
            ui.checkbox(&mut self.dim, tr("Atténuation (dim)"));
            ui.checkbox(&mut self.mute_audio, tr("Couper tout l'audio"));
            ui.checkbox(&mut self.metronome, tr("Métronome"));
        });
        ui.separator();
        let bus_names: Vec<String> = self.p.buses.iter().map(|b| b.name.clone()).collect();
        let mut changed = false;
        let mut fx_open = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for ti in 0..self.p.tracks.len() {
                    if self.p.tracks[ti].kind != TrackKind::Audio {
                        continue;
                    }
                    egui::Frame::new().fill(theme::HEADER()).inner_margin(4.0).corner_radius(3.0).show(ui, |ui| {
                        ui.set_width(88.0);
                        ui.vertical(|ui| {
                            let t = &mut self.p.tracks[ti];
                            let name = if t.name.is_empty() { format!("{} {}", tr("Audio"), ti + 1) } else { t.name.clone() };
                            ui.label(egui::RichText::new(name).strong().size(11.0));
                            if ui.small_button(format!("FX ({})", t.afx.len())).clicked() {
                                fx_open = Some(ti);
                            }
                            let cur = t.bus.and_then(|b| bus_names.get(b).cloned()).unwrap_or_else(|| tr("Maître").into());
                            egui::ComboBox::from_id_salt(("bus", ti)).width(80.0).selected_text(cur).show_ui(ui, |ui| {
                                if ui.selectable_label(t.bus.is_none(), tr("Maître")).clicked() {
                                    t.bus = None;
                                    changed = true;
                                }
                                for (i, b) in bus_names.iter().enumerate() {
                                    if ui.selectable_label(t.bus == Some(i), b).clicked() {
                                        t.bus = Some(i);
                                        changed = true;
                                    }
                                }
                            });
                            changed |= ui.add(egui::Slider::new(&mut t.pan, -1.0..=1.0).show_value(false)).on_hover_text(tr("Panoramique")).changed();
                            ui.horizontal(|ui| {
                                changed |= ui.toggle_value(&mut t.mute, "M").changed();
                                changed |= ui.toggle_value(&mut t.solo, "S").changed();
                                changed |= ui.toggle_value(&mut t.armed, "●").on_hover_text(tr("Armer pour l'enregistrement")).changed();
                            });
                            let r = ui.add(egui::Slider::new(&mut t.level, 0.0..=2.0).vertical().show_value(false));
                            changed |= r.changed();
                            ui.label(egui::RichText::new(db(t.level)).size(10.0));
                            egui::ComboBox::from_id_salt(("auto", ti)).width(80.0).selected_text(t.auto_mode.label()).show_ui(ui, |ui| {
                                for m in AutoMode::ALL {
                                    changed |= ui.selectable_value(&mut t.auto_mode, m, m.label()).changed();
                                }
                            });
                            if r.changed() {
                                let lv = t.level;
                                self.automation_writes.push((ti, false, lv));
                            }
                        });
                    });
                }
                for b in 0..self.p.buses.len() {
                    egui::Frame::new().fill(theme::HEADER_SEL()).inner_margin(4.0).corner_radius(3.0).show(ui, |ui| {
                        ui.set_width(88.0);
                        ui.vertical(|ui| {
                            let bus = &mut self.p.buses[b];
                            ui.add(egui::TextEdit::singleline(&mut bus.name).desired_width(80.0));
                            changed |= ui.add(egui::Slider::new(&mut bus.pan, -1.0..=1.0).show_value(false)).changed();
                            changed |= ui.toggle_value(&mut bus.mute, "M").changed();
                            changed |= ui.add(egui::Slider::new(&mut bus.level, 0.0..=2.0).vertical().show_value(false)).changed();
                            ui.label(egui::RichText::new(db(bus.level)).size(10.0));
                        });
                    });
                }
                egui::Frame::new().fill(theme::ui_color(23)).inner_margin(4.0).corner_radius(3.0).show(ui, |ui| {
                    ui.set_width(88.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(tr("Maître")).strong());
                        changed |= ui.add(egui::Slider::new(&mut self.p.master_volume, 0.0..=2.0).vertical().show_value(false)).changed();
                        ui.label(egui::RichText::new(db(self.p.master_volume)).size(10.0));
                        ui.label(egui::RichText::new(format!("L {}", db(self.meters[0]))).size(9.5));
                        ui.label(egui::RichText::new(format!("R {}", db(self.meters[1]))).size(9.5));
                    });
                });
            });
        });
        if changed {
            self.touch();
        }
        if let Some(ti) = fx_open {
            self.win_track_fx = Some(ti);
        }
    }

    // -------------------------------------------------------------- scopes

    pub(crate) fn scopes_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for k in crate::scopes::ScopeKind::ALL {
                ui.selectable_value(&mut self.scope_kind, k, k.label());
            }
        });
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let Some((w, h, img)) = self.preview_rgba.clone() else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tr("Aucune image"), FontId::proportional(12.0), theme::TEXT_DIM());
            return;
        };
        let sig = (self.preview_serial, self.scope_kind as u8);
        if self.scope_cache.as_ref().map(|c| c.0 != sig).unwrap_or(true) {
            let d = crate::scopes::analyze(&img, w as usize, h as usize, self.scope_kind);
            self.scope_cache = Some((sig, d));
        }
        if let Some((_, d)) = &self.scope_cache {
            crate::scopes::draw(ui, rect, d);
        }
    }

    // ------------------------------------------------------- colour grading

    pub(crate) fn grading_tab(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.sel.iter().copied().find(|id| self.p.find_event(*id).map(|(t, _)| self.p.tracks[t].kind == TrackKind::Video).unwrap_or(false)) else {
            ui.label(egui::RichText::new(tr("Sélectionnez un événement vidéo pour l'étalonner.")).color(theme::TEXT_DIM()));
            return;
        };
        let mut g = self
            .p
            .event(id)
            .and_then(|e| e.fx.iter().find_map(|f| if let VideoFx::ColorGrade(g) = f { Some(g.clone()) } else { None }))
            .unwrap_or_default();
        let name = self.p.event(id).map(|e| e.name.clone()).unwrap_or_default();
        ui.label(egui::RichText::new(name).strong());
        if crate::color::grade_ui(ui, &mut g) {
            if !self.grading_dirty {
                self.push_undo();
                self.label_last_undo(tr("Étalonnage"));
                self.grading_dirty = true;
            }
            if let Some(e) = self.p.event_mut(id) {
                if let Some(slot) = e.fx.iter_mut().find(|f| matches!(f, VideoFx::ColorGrade(_))) {
                    *slot = VideoFx::ColorGrade(g);
                } else {
                    e.fx.insert(0, VideoFx::ColorGrade(g));
                }
            }
            self.touch();
        } else if !ui.input(|i| i.pointer.any_down()) {
            self.grading_dirty = false;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tr("Table de correspondance (LUT) :"));
            if ui.button(tr("Charger un fichier .cube…")).clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("LUT .cube", &["cube"]).pick_file() {
                    let path = f.to_string_lossy().to_string();
                    match crate::color::load_cube(&path) {
                        Ok(_) => {
                            self.push_undo();
                            if let Some(e) = self.p.event_mut(id) {
                                e.fx.retain(|x| !matches!(x, VideoFx::Lut { .. }));
                                e.fx.push(VideoFx::Lut { path, amount: 1.0 });
                            }
                        }
                        Err(e) => self.status = e,
                    }
                }
            }
        });
    }

    // -------------------------------------------------------- edit details

    pub(crate) fn edit_details_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (k, l) in [(0u8, tr("Événements")), (1, tr("Médias")), (2, tr("Marqueurs")), (3, tr("Régions"))] {
                ui.selectable_value(&mut self.details_view, k, l);
            }
            if ui.button(tr("Exporter la liste des événements…")).clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter(tr("Texte"), &["txt"]).set_file_name("evenements.txt").save_file() {
                    self.status = match std::fs::write(&f, event_list_text(&self.p)) {
                        Ok(()) => format!("{} {}", tr("Liste des événements exportée :"), f.display()),
                        Err(e) => e.to_string(),
                    };
                }
            }
        });
        ui.separator();
        let fmt = self.ruler_fmt;
        let pr = Project { fps: self.p.fps, tempo: self.p.tempo, sample_rate: self.p.sample_rate, ..Project::empty() };
        let ft = |t: f64| fmt.format(t, &pr);
        match self.details_view {
            0 => {
                let mut select = None;
                let mut edits: Vec<(Id, f64, f64)> = vec![];
                egui::Grid::new("details_ev").striped(true).show(ui, |ui| {
                    for h in [tr("Piste"), tr("Nom"), tr("Début"), tr("Longueur"), tr("Fin"), tr("Décalage"), tr("Vitesse"), tr("Verrou")] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (ti, t) in self.p.tracks.iter().enumerate() {
                        for e in &t.events {
                            if ui.selectable_label(self.sel.contains(&e.id), format!("{}", ti + 1)).clicked() {
                                select = Some((e.id, e.start));
                            }
                            ui.label(&e.name);
                            let mut st = e.start;
                            let mut ln = e.length;
                            let r1 = ui.add(egui::DragValue::new(&mut st).speed(0.01).range(0.0..=1e6).custom_formatter(|v, _| ft(v)));
                            let r2 = ui.add(egui::DragValue::new(&mut ln).speed(0.01).range(0.01..=1e6).custom_formatter(|v, _| ft(v)));
                            if r1.changed() || r2.changed() {
                                edits.push((e.id, st, ln));
                            }
                            ui.label(ft(e.end()));
                            ui.label(ft(e.offset));
                            ui.label(format!("{} ×", fr_num(e.rate, 2)));
                            ui.label(if e.locked { tr("oui") } else { "" });
                            ui.end_row();
                        }
                    }
                });
                if let Some((id, st)) = select {
                    self.sel.clear();
                    self.sel.insert(id);
                    self.cursor = st;
                }
                if !edits.is_empty() {
                    if !self.details_dirty {
                        self.push_undo();
                        self.details_dirty = true;
                    }
                    for (id, st, ln) in edits {
                        if let Some(e) = self.p.event_mut(id) {
                            e.start = st;
                            e.length = ln;
                        }
                    }
                    self.p.sort_events();
                    self.touch();
                } else if !ui.input(|i| i.pointer.any_down()) {
                    self.details_dirty = false;
                }
            }
            1 => {
                egui::Grid::new("details_media").striped(true).show(ui, |ui| {
                    for h in [tr("Nom"), tr("Type"), tr("Durée"), tr("Taille"), tr("Images/s"), tr("Chemin")] {
                        ui.label(egui::RichText::new(h).strong());
                    }
                    ui.end_row();
                    for m in &self.p.media {
                        ui.label(&m.name);
                        ui.label(m.type_label());
                        ui.label(ft(m.duration));
                        ui.label(format!("{}x{}", m.width, m.height));
                        ui.label(fr_num(m.fps, 3));
                        ui.label(m.path().unwrap_or(""));
                        ui.end_row();
                    }
                });
            }
            2 => {
                let mut del = None;
                egui::Grid::new("details_mk").striped(true).show(ui, |ui| {
                    for (i, mk) in self.p.markers.iter_mut().enumerate() {
                        ui.label(if mk.command { tr("Commande") } else { tr("Marqueur") });
                        ui.add(egui::TextEdit::singleline(&mut mk.name).desired_width(200.0));
                        ui.label(ft(mk.time));
                        if ui.small_button("✕").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = del {
                    self.push_undo();
                    self.p.markers.remove(i);
                }
            }
            _ => {
                let mut del = None;
                let mut go = None;
                egui::Grid::new("details_rg").striped(true).show(ui, |ui| {
                    for (i, r) in self.p.regions.iter_mut().enumerate() {
                        ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(200.0));
                        ui.label(format!("{} → {}", ft(r.start), ft(r.end)));
                        if ui.small_button(tr("Sélectionner")).clicked() {
                            go = Some((r.start, r.end));
                        }
                        if ui.small_button("✕").clicked() {
                            del = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some((a, b)) = go {
                    self.p.loop_region = Some((a, b));
                    self.cursor = a;
                }
                if let Some(i) = del {
                    self.push_undo();
                    self.p.regions.remove(i);
                }
            }
        }
    }

    pub(crate) fn notes_tab(&mut self, ui: &mut egui::Ui) {
        let r = ui.add_sized(ui.available_size(), egui::TextEdit::multiline(&mut self.p.notes).hint_text(tr("Notes du projet (enregistrées avec le projet)…")));
        if r.changed() {
            self.dirty = true;
        }
    }

    // ------------------------------------------------------------ loudness

    pub(crate) fn loudness_tab(&mut self, ui: &mut egui::Ui) {
        if self.playing || (self.loud_live_at - self.cursor).abs() > 1e-9 {
            let rate = 12_000.0f32;
            let n = (3.0 * rate) as usize;
            let mut buf = vec![0f32; n * 2];
            crate::audio::mix_into(&self.mix, (self.cursor - 3.0).max(0.0), rate as f64, &mut buf);
            self.loud_live = crate::loudness::momentary_short(&buf, rate);
            self.loud_live_at = self.cursor;
        }
        crate::loudness::meter_ui(ui, self.loud_live.0, self.loud_live.1, self.loud_report.as_ref());
        ui.horizontal(|ui| {
            if ui.button(tr("Analyser tout le projet")).clicked() {
                let rate = 24_000.0f32;
                let len = self.p.end_time();
                let mut buf = vec![0f32; (len * rate as f64) as usize * 2];
                crate::audio::mix_into(&self.mix, 0.0, rate as f64, &mut buf);
                self.loud_report = Some(crate::loudness::analyze(&buf, rate));
                self.loud_log = crate::loudness::short_term_series(&buf, rate, 1.0);
            }
            if ui.add_enabled(!self.loud_log.is_empty(), egui::Button::new(tr("Enregistrer le journal de sonie…"))).clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("sonie.csv").save_file() {
                    let mut s = String::from("temps;court_terme_LUFS\n");
                    for (t, v) in &self.loud_log {
                        s.push_str(&format!("{};{}\n", fr_num(*t as f64, 1), if v.is_finite() { fr_num(*v as f64, 1) } else { "-inf".into() }));
                    }
                    self.status = match std::fs::write(&f, s) {
                        Ok(()) => format!("{} {}", tr("Journal de sonie enregistré :"), f.display()),
                        Err(e) => e.to_string(),
                    };
                }
            }
        });
    }

    // ------------------------------------------------------------- history

    pub(crate) fn history_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(tr("Tout annuler"))).clicked() {
                let n = self.undo.len();
                self.undo_steps(n);
            }
            if ui.button(tr("Effacer l'historique")).clicked() {
                self.clear_history();
            }
        });
        ui.separator();
        let n = self.undo_labels.len();
        let mut go = None;
        for (i, l) in self.undo_labels.iter().enumerate().rev() {
            let label = if l.is_empty() { tr("Modification") } else { l.as_str() };
            if ui.selectable_label(false, format!("{}. {}", i + 1, label)).on_hover_text(tr("Cliquez pour annuler jusqu'ici")).clicked() {
                go = Some(n - i);
            }
        }
        if let Some(k) = go {
            self.undo_steps(k);
        }
        if !self.redo_labels.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new(tr("Rétablissables :")).color(theme::TEXT_DIM()));
            for l in self.redo_labels.iter().rev() {
                ui.label(egui::RichText::new(if l.is_empty() { tr("Modification") } else { l.as_str() }).color(theme::TEXT_DIM()));
            }
        }
    }

    // ------------------------------------------------------------ surround

    pub(crate) fn surround_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Canaux du projet :"));
            let mut ch = self.p.channels;
            ui.selectable_value(&mut ch, 2, tr("Stéréo"));
            ui.selectable_value(&mut ch, 6, "5.1");
            if ch != self.p.channels {
                self.push_undo();
                self.p.channels = ch;
            }
        });
        let Some(cur) = self.p.tracks.get(self.focus_track).filter(|t| t.kind == TrackKind::Audio).map(|t| t.surround) else {
            ui.label(egui::RichText::new(tr("Choisissez une piste audio.")).color(theme::TEXT_DIM()));
            return;
        };
        let mut s = cur;
        let size = ui.available_width().min(ui.available_height() - 10.0).clamp(80.0, 260.0);
        let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
        let p = ui.painter_at(r);
        p.rect_filled(r, 4.0, theme::ui_color(19));
        p.circle_stroke(r.center(), size * 0.45, Stroke::new(1.0, theme::TEXT_DIM()));
        for (lbl, x, y) in [("L", -0.7, -0.7), ("C", 0.0, -1.0), ("R", 0.7, -0.7), ("Ls", -0.7, 0.7), ("Rs", 0.7, 0.7)] {
            p.text(r.center() + vec2(x * size * 0.45, y * size * 0.45), egui::Align2::CENTER_CENTER, lbl, FontId::proportional(11.0), theme::TEXT());
        }
        if let Some(pp) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                s = [((pp.x - r.center().x) / (size * 0.45)).clamp(-1.0, 1.0), ((pp.y - r.center().y) / (size * 0.45)).clamp(-1.0, 1.0)];
            }
        }
        if resp.double_clicked() {
            s = [0.0, 0.0];
        }
        p.circle_filled(r.center() + vec2(s[0] * size * 0.45, s[1] * size * 0.45), 6.0, theme::ACCENT());
        if s != cur {
            if resp.drag_started() || resp.clicked() || resp.double_clicked() {
                self.push_undo();
            }
            self.p.tracks[self.focus_track].surround = s;
            self.touch();
        }
        if self.p.channels != 6 {
            ui.label(egui::RichText::new(tr("Passez le projet en 5.1 pour entendre la position surround au rendu.")).color(theme::TEXT_DIM()).size(11.0));
        }
    }

    pub(crate) fn plugins_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(tr("Effets vidéo intégrés")).strong());
        for f in VideoFx::presets().iter().chain(VideoFx::pack().iter()).chain(VideoFx::pro().iter()) {
            ui.label(format!("• {}  ({})", f.label(), f.category()));
        }
        ui.separator();
        ui.label(egui::RichText::new(tr("Effets audio intégrés")).strong());
        for f in crate::audio_fx::AudioFx::presets() {
            ui.label(format!("• {}", f.label()));
        }
        ui.separator();
        if ui.button(tr("Gérer les packs et les plugins OFX…")).clicked() {
            self.ai.win_plugins = true;
        }
    }
}

/// Curve code used in the event list (hold 0, linear 1,
/// fast 2, slow -2, smooth 4, sharp -4).
fn curve_internal(c: Curve) -> i32 {
    match c {
        Curve::Hold => 0,
        Curve::Linear => 1,
        Curve::Fast => 2,
        Curve::Slow => -2,
        Curve::Smooth => 4,
        Curve::Sharp => -4,
    }
}

/// Event list as text: fields separated by
/// "; ", times in milliseconds,
/// booleans TRUE/FALSE, media type VIDEO/AUDIO, CRLF line ends. Tracks are
/// numbered per kind (video and audio counted separately), ids from 1.
pub fn event_list_text(p: &Project) -> String {
    let mut s = String::from("\"ID\";\"Track\";\"StartTime\";\"Length\";\"PlayRate\";\"Locked\";\"Normalized\";\"StretchMethod\";\"Looped\";\"OnRuler\";\"MediaType\";\"FileName\";\"Stream\";\"StreamStart\";\"StreamLength\";\"FadeTimeIn\";\"FadeTimeOut\";\"SustainGain\";\"CurveIn\";\"GainIn\";\"CurveOut\";\"GainOut\";\"Layer\";\"Color\";\"CurveInR\";\"CurveOutR\";\"PlayPitch\";\"LockPitch\";\"FirstChannel\";\"Channels\"
");
    let ms = |t: f64| t * 1000.0;
    let b = |v: bool| if v { "TRUE" } else { "FALSE" };
    let (mut nv, mut na, mut id) = (0, 0, 0);
    for t in &p.tracks {
        let (kind, tn) = match t.kind {
            TrackKind::Video => {
                nv += 1;
                ("VIDEO", nv - 1)
            }
            TrackKind::Audio => {
                na += 1;
                ("AUDIO", na - 1)
            }
        };
        for e in &t.events {
            id += 1;
            let file = p.media(e.media).and_then(|m| m.path().map(|x| x.to_string())).unwrap_or_else(|| e.name.clone());
            let audio = t.kind == TrackKind::Audio;
            s.push_str(&format!("{}; {}; {:.4}; {:.4}; {:.6}; ", id, tn, ms(e.start), ms(e.length), e.rate));
            s.push_str(&format!("{}; {}; {}; {}; {}; ", b(e.locked), b(e.normalize), 0, b(e.looped), b(false)));
            s.push_str(&format!("{}; ", kind));
            s.push_str(&format!("\"{}\"; {}; {:.4}; {:.4}; ", file, 0, ms(e.offset), ms(e.source_span())));
            s.push_str(&format!("{:.4}; {:.4}; {:.6}; {}; {:.6}; {}; {:.6}; ", ms(e.fade_in), ms(e.fade_out), e.gain, curve_internal(e.curve_in), 0.0, curve_internal(e.curve_out), 0.0));
            s.push_str(&format!("{}; {}; ", 0, t.color));
            s.push_str(&format!("{}; {}; ", curve_internal(e.curve_in), curve_internal(e.curve_out)));
            s.push_str(&format!("{:.6}; {}", if audio { e.pitch as f64 } else { 0.0 }, b(audio && e.preserve_pitch)));
            s.push_str(&format!("; {}; {}", 0, if audio { 2 } else { 0 }));
            s.push_str("
");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_list_format() {
        let p = crate::app::demo_project();
        let txt = event_list_text(&p);
        let mut lines = txt.split("
");
        assert!(lines.next().unwrap().starts_with("\"ID\";\"Track\";\"StartTime\""));
        let first = lines.next().unwrap();
        let f: Vec<&str> = first.split("; ").collect();
        assert_eq!(f[0], "1");
        assert_eq!(f[10], "VIDEO");
        // 0.5 s start = 500 ms, 1 s fade in = 1000 ms, smooth curve = 4
        assert!(f[2].starts_with("50"));
        assert_eq!(f[15], "1000.0000");
        assert_eq!(f[18], "4");
    }
}
