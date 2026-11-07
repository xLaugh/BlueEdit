//! Dialogs: track FX and motion, event audio, tracking/stabilisation,
//! multicam, media properties, output FX, toolbar customisation and the
//! external monitor window.

use crate::app::App;
use crate::i18n::tr;
use crate::model::*;
use crate::panels::fr_num;
use crate::theme;
use egui::{pos2, vec2, Color32, Rect};

fn set_key(keys: &mut Vec<PanKey>, t: f64, pan: PanCrop) {
    if let Some(k) = keys.iter_mut().find(|k| (k.t - t).abs() < 1e-3) {
        k.pan = pan;
    } else {
        let i = keys.partition_point(|k| k.t < t);
        keys.insert(i, PanKey { t, pan });
    }
}

/// Keyframe helper shared with the pan/crop window.
pub fn set_pan_key(keys: &mut Vec<PanKey>, t: f64, pan: PanCrop) {
    set_key(keys, t, pan)
}

/// Sets / adds an effect keyframe at local time t with the current chain.
pub fn set_fx_key(keys: &mut Vec<FxKey>, t: f64, fx: Vec<VideoFx>) {
    if let Some(k) = keys.iter_mut().find(|k| (k.t - t).abs() < 1e-3) {
        k.fx = fx;
    } else {
        let i = keys.partition_point(|k| k.t < t);
        keys.insert(i, FxKey { t, fx });
    }
}

impl App {
    pub fn dialogs2(&mut self, ctx: &egui::Context) {
        self.track_fx_window(ctx);
        self.track_motion_window(ctx);
        self.event_audio_window(ctx);
        self.tracking_window(ctx);
        self.multicam_window(ctx);
        self.media_props_window(ctx);
        self.output_fx_window(ctx);
        self.customize_window(ctx);
        self.monitor_window(ctx);
    }

    fn track_fx_window(&mut self, ctx: &egui::Context) {
        let Some(ti) = self.win_track_fx.filter(|t| *t < self.p.tracks.len()) else {
            self.win_track_fx = None;
            return;
        };
        let mut open = true;
        let kind = self.p.tracks[ti].kind;
        let before = self.p.tracks[ti].clone();
        let title = format!("{} — {}", tr("Effets de piste"), if before.name.is_empty() { format!("{}", ti + 1) } else { before.name.clone() });
        egui::Window::new(title).id(egui::Id::new("track_fx")).open(&mut open).default_width(360.0).show(ctx, |ui| {
            let t = &mut self.p.tracks[ti];
            if kind == TrackKind::Audio {
                crate::tabs::afx_list_ui(ui, &mut t.afx, "tafx");
            } else {
                ui.checkbox(&mut t.adjustment, tr("Piste d'ajustement (ses effets s'appliquent aux pistes du dessous)"));
                ui.horizontal(|ui| {
                    ui.label(tr("Mode de composition :"));
                    egui::ComboBox::from_id_salt("blend").selected_text(t.blend.label()).show_ui(ui, |ui| {
                        for b in BlendMode::ALL {
                            ui.selectable_value(&mut t.blend, b, b.label());
                        }
                    });
                });
                ui.checkbox(&mut t.child, tr("Piste enfant (composée avec la piste parente au-dessus)"));
                ui.separator();
                let mut remove = None;
                for (i, fx) in t.fx.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(fx.label());
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    t.fx.remove(i);
                }
                ui.menu_button(tr("Ajouter un effet de piste…"), |ui| {
                    for f in VideoFx::presets().into_iter().chain(VideoFx::pack()).chain(VideoFx::pro()) {
                        if ui.button(f.label()).clicked() {
                            t.fx.push(f);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                ui.label(tr("Fondu vers une couleur : couleurs haute et basse"));
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut t.fade_colors[0]);
                    ui.color_edit_button_srgb(&mut t.fade_colors[1]);
                });
            }
        });
        if self.p.tracks[ti] != before {
            let now = std::mem::replace(&mut self.p.tracks[ti], before);
            self.push_undo();
            self.p.tracks[ti] = now;
            self.label_last_undo(tr("Effets de piste"));
        }
        if !open {
            self.win_track_fx = None;
        }
    }

    fn track_motion_window(&mut self, ctx: &egui::Context) {
        let Some(ti) = self.win_track_motion.filter(|t| *t < self.p.tracks.len()) else {
            self.win_track_motion = None;
            return;
        };
        let mut open = true;
        let before = self.p.tracks[ti].clone();
        let lt = self.cursor;
        egui::Window::new(tr("Mouvement de piste")).open(&mut open).default_width(320.0).show(ctx, |ui| {
            let t = &mut self.p.tracks[ti];
            let mut m = t.motion_at(lt);
            let mut ch = false;
            egui::Grid::new("tm").num_columns(2).show(ui, |ui| {
                ui.label(tr("Échelle"));
                ch |= ui.add(egui::Slider::new(&mut m.zoom, 0.05..=4.0)).changed();
                ui.end_row();
                ui.label(tr("Position X"));
                ch |= ui.add(egui::Slider::new(&mut m.x, -1.0..=1.0)).changed();
                ui.end_row();
                ui.label(tr("Position Y"));
                ch |= ui.add(egui::Slider::new(&mut m.y, -1.0..=1.0)).changed();
                ui.end_row();
                ui.label(tr("Rotation (°)"));
                ch |= ui.add(egui::Slider::new(&mut m.rotation, -180.0..=180.0)).changed();
                ui.end_row();
            });
            if ch {
                if t.motion_keys.is_empty() {
                    t.motion = m;
                } else {
                    set_key(&mut t.motion_keys, lt, m);
                }
            }
            ui.horizontal(|ui| {
                if ui.button(tr("Ajouter une image clé au curseur")).clicked() {
                    if t.motion_keys.is_empty() {
                        t.motion_keys.push(PanKey { t: 0.0, pan: t.motion });
                    }
                    set_key(&mut t.motion_keys, lt, m);
                }
                if ui.button(tr("Supprimer les images clés")).clicked() {
                    t.motion_keys.clear();
                }
                if ui.button(tr("Réinitialiser")).clicked() {
                    t.motion = PanCrop::default();
                    t.motion_keys.clear();
                }
            });
            ui.label(egui::RichText::new(format!("{} {}", t.motion_keys.len(), tr("image(s) clé(s)"))).color(theme::TEXT_DIM()));
        });
        if self.p.tracks[ti] != before {
            let now = std::mem::replace(&mut self.p.tracks[ti], before);
            self.push_undo();
            self.p.tracks[ti] = now;
            self.label_last_undo(tr("Mouvement de piste"));
        }
        if !open {
            self.win_track_motion = None;
        }
    }

    fn event_audio_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_event_audio else { return };
        let Some(before) = self.p.event(id).cloned() else {
            self.win_event_audio = None;
            return;
        };
        let mut open = true;
        let mut ext = false;
        egui::Window::new(format!("{} — {}", tr("Audio de l'événement"), before.name)).id(egui::Id::new("ev_audio")).open(&mut open).default_width(360.0).show(ctx, |ui| {
            let Some(e) = self.p.event_mut(id) else { return };
            egui::Grid::new("ea").num_columns(2).show(ui, |ui| {
                ui.label(tr("Vitesse de lecture"));
                ui.add(egui::Slider::new(&mut e.rate, 0.25..=4.0).logarithmic(true));
                ui.end_row();
                ui.label(tr("Conserver la hauteur"));
                ui.checkbox(&mut e.preserve_pitch, "");
                ui.end_row();
                ui.label(tr("Lecture inversée"));
                ui.checkbox(&mut e.reverse, "");
                ui.end_row();
                ui.label(tr("Hauteur (demi-tons)"));
                ui.add(egui::Slider::new(&mut e.pitch, -24.0..=24.0).step_by(0.01));
                ui.end_row();
                ui.label(tr("Normaliser"));
                ui.checkbox(&mut e.normalize, "");
                ui.end_row();
                ui.label(tr("Inverser la phase"));
                ui.checkbox(&mut e.invert_phase, "");
                ui.end_row();
            });
            ui.separator();
            crate::tabs::afx_list_ui(ui, &mut e.afx, "eafx");
            ui.separator();
            ext = ui.button(tr("Ouvrir dans l'éditeur audio externe")).clicked();
        });
        if self.p.event(id) != Some(&before) {
            if let Some(e) = self.p.event_mut(id) {
                let now = std::mem::replace(e, before);
                self.push_undo();
                if let Some(e) = self.p.event_mut(id) {
                    *e = now;
                }
            }
            self.label_last_undo(tr("Audio de l'événement"));
        }
        if ext {
            self.open_in_sound_editor(id);
        }
        if !open {
            self.win_event_audio = None;
        }
    }

    fn tracking_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_tracking else { return };
        let mut open = true;
        let mut go = false;
        let mut stab = false;
        egui::Window::new(tr("Suivi de mouvement et stabilisation")).open(&mut open).default_width(380.0).show(ctx, |ui| {
            ui.label(tr("Stabilisation : analyse le tremblement de la vidéo et le compense."));
            stab = ui.button(tr("Stabiliser cet événement")).clicked();
            ui.separator();
            ui.label(tr("Suivi : zone à suivre (en fraction de l'image) :"));
            egui::Grid::new("trk").num_columns(4).show(ui, |ui| {
                for (i, l) in ["X", "Y", tr("Largeur"), tr("Hauteur")].iter().enumerate() {
                    ui.label(*l);
                    ui.add(egui::DragValue::new(&mut self.tracking_rect[i]).range(0.0..=1.0).speed(0.005));
                }
            });
            let others: Vec<(Id, String)> = self.p.tracks.iter().filter(|t| t.kind == TrackKind::Video).flat_map(|t| t.events.iter()).filter(|e| e.id != id).map(|e| (e.id, e.name.clone())).collect();
            let cur = self.tracking_target.and_then(|t| others.iter().find(|o| o.0 == t).map(|o| o.1.clone())).unwrap_or_else(|| tr("(aucun)").into());
            egui::ComboBox::from_label(tr("Appliquer le mouvement à")).selected_text(cur).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.tracking_target, None, tr("(aucun)"));
                for (oid, n) in &others {
                    ui.selectable_value(&mut self.tracking_target, Some(*oid), n);
                }
            });
            go = ui.button(tr("Lancer le suivi")).clicked();
            for t in &self.tasks {
                ui.add(egui::ProgressBar::new(*t.progress.lock().unwrap()).text(&t.name));
            }
        });
        if stab {
            self.stabilize_event(id);
        }
        if go {
            self.track_motion(id, self.tracking_rect, self.tracking_target);
        }
        if !open {
            self.win_tracking = None;
        }
    }

    fn multicam_window(&mut self, ctx: &egui::Context) {
        if !self.win_multicam {
            return;
        }
        let mut open = true;
        let mut sync = false;
        let mut begin = false;
        let mut finish = false;
        let mut cut = None;
        egui::Window::new(tr("Montage multicaméra")).open(&mut open).default_width(360.0).show(ctx, |ui| {
            sync = ui.button(tr("Synchroniser les événements sélectionnés par l'audio")).clicked();
            ui.separator();
            match &self.multicam {
                None => {
                    begin = ui.button(tr("Activer le montage multicaméra")).clicked();
                }
                Some(mc) => {
                    ui.label(format!("{} {} — {} {}", mc.tracks.len(), tr("angles"), mc.cuts.len(), tr("coupe(s)")));
                    ui.horizontal_wrapped(|ui| {
                        for a in 0..mc.tracks.len() {
                            if ui.selectable_label(a == mc.current, format!("{} {}", tr("Angle"), a + 1)).clicked() {
                                cut = Some(a);
                            }
                        }
                    });
                    ui.label(egui::RichText::new(tr("Pendant la lecture, les touches 1 à 9 changent d'angle.")).color(theme::TEXT_DIM()));
                    finish = ui.button(tr("Terminer et créer le montage")).clicked();
                }
            }
        });
        if sync {
            self.sync_by_audio();
        }
        if begin {
            self.multicam_begin();
        }
        if let Some(a) = cut {
            self.multicam_cut(a);
        }
        if finish {
            self.multicam_finish();
        }
        if !open {
            self.win_multicam = false;
        }
    }

    fn media_props_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_media_props else { return };
        let Some(m) = self.p.media(id).cloned() else {
            self.win_media_props = None;
            return;
        };
        let mut open = true;
        let mut nm = m.clone();
        let bins = self.p.bins.clone();
        let fps = self.p.fps;
        egui::Window::new(format!("{} — {}", tr("Propriétés du média"), m.name)).id(egui::Id::new("media_props")).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("mp").num_columns(2).show(ui, |ui| {
                ui.label(tr("Nom"));
                ui.text_edit_singleline(&mut nm.name);
                ui.end_row();
                ui.label(tr("Type"));
                ui.label(m.type_label());
                ui.end_row();
                ui.label(tr("Chemin"));
                ui.label(m.path().unwrap_or("—"));
                ui.end_row();
                ui.label(tr("Durée"));
                ui.label(timecode(m.duration, fps));
                ui.end_row();
                ui.label(tr("Image"));
                ui.label(format!("{}x{} ; {} i/s", m.width, m.height, fr_num(m.fps, 3)));
                ui.end_row();
                if let Some(Ok(md)) = m.path().map(std::fs::metadata) {
                    ui.label(tr("Taille du fichier"));
                    ui.label(format!("{} Mo", fr_num(md.len() as f64 / 1_048_576.0, 1)));
                    ui.end_row();
                }
                ui.label(tr("Proxy"));
                ui.label(m.proxy.clone().unwrap_or_else(|| tr("aucun").into()));
                ui.end_row();
                ui.label(tr("Chutier"));
                egui::ComboBox::from_id_salt("bin").selected_text(if nm.bin.is_empty() { tr("(racine)").to_string() } else { nm.bin.clone() }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut nm.bin, String::new(), tr("(racine)"));
                    for b in &bins {
                        ui.selectable_value(&mut nm.bin, b.clone(), b);
                    }
                });
                ui.end_row();
                ui.label(tr("Étiquettes"));
                let mut tags = nm.tags.join(", ");
                if ui.text_edit_singleline(&mut tags).changed() {
                    nm.tags = tags.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
                ui.end_row();
            });
            ui.separator();
            ui.label(tr("Effets du média (appliqués à tous ses événements) :"));
            let mut remove = None;
            for (i, f) in nm.fx.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(f.label());
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                nm.fx.remove(i);
            }
            ui.menu_button(tr("Ajouter un effet de média…"), |ui| {
                for f in VideoFx::presets().into_iter().chain(VideoFx::pro()) {
                    if ui.button(f.label()).clicked() {
                        nm.fx.push(f);
                        ui.close_menu();
                    }
                }
            });
        });
        if nm != m {
            self.push_undo();
            if let Some(x) = self.p.media.iter_mut().find(|x| x.id == id) {
                *x = nm;
            }
        }
        if !open {
            self.win_media_props = None;
        }
    }

    fn output_fx_window(&mut self, ctx: &egui::Context) {
        if !self.win_output_fx {
            return;
        }
        let mut open = true;
        let before = self.p.output_fx.clone();
        egui::Window::new(tr("Effets de sortie vidéo")).open(&mut open).show(ctx, |ui| {
            ui.label(egui::RichText::new(tr("Appliqués à l'image finale (aperçu et rendu).")).color(theme::TEXT_DIM()));
            let mut remove = None;
            for (i, f) in self.p.output_fx.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(f.label());
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.p.output_fx.remove(i);
            }
            ui.menu_button(tr("Ajouter un effet de sortie…"), |ui| {
                for f in VideoFx::presets().into_iter().chain(VideoFx::pack()).chain(VideoFx::pro()) {
                    if ui.button(f.label()).clicked() {
                        self.p.output_fx.push(f);
                        ui.close_menu();
                    }
                }
            });
        });
        if self.p.output_fx != before {
            let now = std::mem::replace(&mut self.p.output_fx, before);
            self.push_undo();
            self.p.output_fx = now;
        }
        if !open {
            self.win_output_fx = false;
        }
    }

    fn customize_window(&mut self, ctx: &egui::Context) {
        if !self.win_customize {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Personnaliser les barres d'outils")).open(&mut open).show(ctx, |ui| {
            ui.label(tr("Boutons affichés :"));
            for (key, label) in crate::app::TOOLBAR_ITEMS {
                let mut on = !self.prefs.toolbar_hidden.iter().any(|k| k == key);
                if ui.checkbox(&mut on, tr(label)).changed() {
                    if on {
                        self.prefs.toolbar_hidden.retain(|k| k != key);
                    } else {
                        self.prefs.toolbar_hidden.push(key.to_string());
                    }
                    self.prefs.save();
                }
            }
        });
        if !open {
            self.win_customize = false;
        }
    }

    /// External monitor: the preview in its own window (put it on a second screen).
    fn monitor_window(&mut self, ctx: &egui::Context) {
        if !self.ext_monitor {
            return;
        }
        let tex = self.preview_tex.clone();
        let (pw, ph) = (self.p.width as f32, self.p.height as f32);
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("blue_monitor"),
            egui::ViewportBuilder::default().with_title(tr("Moniteur externe — Blue Edit")).with_inner_size([960.0, 540.0]),
            |ctx, _| {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK)).show(ctx, |ui| {
                    let r = ui.available_rect_before_wrap();
                    if let Some(t) = &tex {
                        let k = (r.width() / pw).min(r.height() / ph);
                        ui.painter().image(t.id(), Rect::from_center_size(r.center(), vec2(pw * k, ph * k)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            },
        );
        if close {
            self.ext_monitor = false;
        }
    }
}
