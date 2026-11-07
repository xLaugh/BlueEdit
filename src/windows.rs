//! Floating windows: event FX, pan/crop, event properties, project
//! properties, render, generated media, help.

use crate::i18n::tr;

use crate::app::App;
use crate::export;
use crate::model::*;
use crate::panels::fr_num;
use crate::theme;

fn color_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 4]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut col = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
        let r = egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::OnlyBlend);
        if r.changed() {
            *c = col.to_srgba_unmultiplied();
        }
        r.changed()
    })
    .inner
}

impl App {
    /// One undo step per gesture in a window (a slider drag, a typed text...).
    fn begin_window_edit(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new("window_edit_open");
        let open: bool = ctx.data(|d| d.get_temp(id).unwrap_or(false));
        if !open {
            self.push_undo();
            ctx.data_mut(|d| d.insert_temp(id, true));
        }
    }

    pub fn windows(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("window_edit_open"), false));
        }
        self.fx_window(ctx);
        self.pan_window(ctx);
        self.props_window(ctx);
        self.project_window(ctx);
        self.render_window(ctx);
        self.gen_window(ctx);
        self.about_window(ctx);
    }

    fn fx_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_fx else { return };
        let Some(ev) = self.p.event(id).cloned() else {
            self.win_fx = None;
            return;
        };
        let mut open = true;
        let lt = (self.cursor - ev.start).clamp(0.0, ev.length);
        // with keyframes, the window edits the chain at the cursor
        let mut fx = ev.fx_at(lt).into_owned();
        let keyed = !ev.fx_keys.is_empty();
        let mut add_key = false;
        let mut clear_keys = false;
        let mut changed = false;
        let mut remove = None;
        egui::Window::new(format!("Effets vidéo de l'événement : {}", ev.name))
            .open(&mut open)
            .default_width(360.0)
            .resizable(false)
            .show(ctx, |ui| {
                if fx.is_empty() {
                    ui.label(egui::RichText::new(tr("Aucun effet. Ajoutez-en un ci-dessous ou glissez-le depuis l'onglet « Effets vidéo ».")).color(theme::TEXT_DIM()));
                }
                for (i, f) in fx.iter_mut().enumerate() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(f.label());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(tr("Retirer")).clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                        let r = match f {
                            VideoFx::BrightnessContrast { brightness, contrast } => {
                                let a = ui.add(egui::Slider::new(brightness, -1.0..=1.0).text(tr("Luminosité"))).changed();
                                let b = ui.add(egui::Slider::new(contrast, -1.0..=1.0).text(tr("Contraste"))).changed();
                                a || b
                            }
                            VideoFx::Saturation { amount } => ui.add(egui::Slider::new(amount, 0.0..=3.0).text(tr("Saturation"))).changed(),
                            VideoFx::BlackWhite { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                            VideoFx::Sepia { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                            VideoFx::Invert { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                            VideoFx::Blur { radius } => ui.add(egui::Slider::new(radius, 0.0..=30.0).text(tr("Rayon"))).changed(),
                            VideoFx::Vignette { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                            VideoFx::Glow { radius, intensity, threshold } => {
                                let a = ui.add(egui::Slider::new(radius, 1.0..=60.0).text(tr("Rayon"))).changed();
                                let b = ui.add(egui::Slider::new(intensity, 0.0..=3.0).text(tr("Intensité"))).changed();
                                let c = ui.add(egui::Slider::new(threshold, 0.0..=0.95).text(tr("Seuil"))).changed();
                                a || b || c
                            }
                            VideoFx::RgbSplit { amount }
                            | VideoFx::Glitch { amount }
                            | VideoFx::Vhs { amount }
                            | VideoFx::Grain { amount }
                            | VideoFx::Shake { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                            VideoFx::Duotone { dark, light } => {
                                let a = color_edit(ui, "Ombres", dark);
                                let b = color_edit(ui, "Lumières", light);
                                a || b
                            }
                            VideoFx::Pixelate { size } => ui.add(egui::Slider::new(size, 2.0..=80.0).text(tr("Taille des blocs"))).changed(),
                            VideoFx::Noir { contrast } => ui.add(egui::Slider::new(contrast, 0.0..=1.0).text(tr("Contraste"))).changed(),
                            VideoFx::ColorGrade(g) => crate::color::grade_ui(ui, g),
                            VideoFx::Lut { path, amount } => {
                                let mut c = false;
                                ui.horizontal(|ui| {
                                    ui.label(if path.is_empty() { tr("(aucun fichier)").to_string() } else { std::path::Path::new(path.as_str()).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() });
                                    if ui.button(tr("Choisir un .cube…")).clicked() {
                                        if let Some(f) = rfd::FileDialog::new().add_filter("LUT .cube", &["cube"]).pick_file() {
                                            *path = f.to_string_lossy().to_string();
                                            c = true;
                                        }
                                    }
                                });
                                c | ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed()
                            }
                            VideoFx::ChromaKey { color, tolerance, softness, spill } => {
                                let mut c = ui.horizontal(|ui| {
                                    ui.label(tr("Couleur clé"));
                                    ui.color_edit_button_srgb(color).changed()
                                }).inner;
                                c |= ui.add(egui::Slider::new(tolerance, 0.0..=1.0).text(tr("Tolérance"))).changed();
                                c |= ui.add(egui::Slider::new(softness, 0.0..=0.5).text(tr("Douceur"))).changed();
                                c |= ui.add(egui::Slider::new(spill, 0.0..=1.0).text(tr("Suppression du débordement"))).changed();
                                c
                            }
                            VideoFx::Mask { shape, x, y, w, h, feather, invert, points } => {
                                let mut c = false;
                                ui.horizontal(|ui| {
                                    for (s, l) in [(MaskShape::Rectangle, tr("Rectangle")), (MaskShape::Ellipse, tr("Ellipse")), (MaskShape::Polygon, tr("Forme libre"))] {
                                        c |= ui.selectable_value(shape, s, l).changed();
                                    }
                                });
                                if *shape == MaskShape::Polygon {
                                    let mut txt = points.iter().map(|p| format!("{:.3} {:.3}", p[0], p[1])).collect::<Vec<_>>().join("; ");
                                    ui.label(tr("Points (x y ; x y ; …, de 0 à 1) :"));
                                    if ui.text_edit_singleline(&mut txt).changed() {
                                        *points = txt
                                            .split(';')
                                            .filter_map(|p| {
                                                let v: Vec<f32> = p.split_whitespace().filter_map(|n| n.replace(',', ".").parse().ok()).collect();
                                                if v.len() == 2 { Some([v[0], v[1]]) } else { None }
                                            })
                                            .collect();
                                        c = true;
                                    }
                                } else {
                                    c |= ui.add(egui::Slider::new(x, 0.0..=1.0).text("X")).changed();
                                    c |= ui.add(egui::Slider::new(y, 0.0..=1.0).text("Y")).changed();
                                    c |= ui.add(egui::Slider::new(w, 0.0..=1.0).text(tr("Largeur"))).changed();
                                    c |= ui.add(egui::Slider::new(h, 0.0..=1.0).text(tr("Hauteur"))).changed();
                                }
                                c |= ui.add(egui::Slider::new(feather, 0.0..=0.3).text(tr("Contour progressif"))).changed();
                                c |= ui.checkbox(invert, tr("Inverser le masque")).changed();
                                c
                            }
                            VideoFx::Stabilize { amount, zoom, path, .. } => {
                                if path.is_empty() {
                                    ui.label(egui::RichText::new(tr("Pas encore analysé : Outils > Vidéo > Stabilisation et suivi.")).color(theme::TEXT_DIM()));
                                }
                                let a = ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed();
                                let b = ui.add(egui::Slider::new(zoom, 1.0..=1.5).text(tr("Zoom de compensation"))).changed();
                                a || b
                            }
                            VideoFx::Deflicker { amount } => ui.add(egui::Slider::new(amount, 0.0..=1.0).text(tr("Intensité"))).changed(),
                        };
                        changed |= r;
                    });
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("Ajouter :"));
                    for p in VideoFx::presets().into_iter().chain(VideoFx::pack()).chain(VideoFx::pro()) {
                        if ui.small_button(p.label()).clicked() {
                            fx.push(p);
                            changed = true;
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    add_key = ui.button(tr("Image clé au curseur")).on_hover_text(tr("Enregistre les réglages actuels à la position du curseur ; ils sont interpolés entre deux images clés")).clicked();
                    clear_keys = ui.add_enabled(keyed, egui::Button::new(tr("Supprimer les images clés"))).clicked();
                    if keyed {
                        ui.label(egui::RichText::new(format!("{} {}", ev.fx_keys.len(), tr("image(s) clé(s)"))).color(theme::TEXT_DIM()));
                    }
                });
            });
        if let Some(i) = remove {
            fx.remove(i);
            changed = true;
        }
        if changed || add_key || clear_keys {
            self.begin_window_edit(ctx);
            if let Some(e) = self.p.event_mut(id) {
                let structure_changed = fx.len() != e.fx.len() || fx.iter().zip(e.fx.iter()).any(|(a, b)| a.key() != b.key());
                if clear_keys {
                    e.fx_keys.clear();
                    e.fx = fx;
                } else if add_key || (keyed && !structure_changed) {
                    if e.fx_keys.is_empty() {
                        e.fx_keys.push(FxKey { t: 0.0, fx: e.fx.clone() });
                    }
                    crate::dialogs2::set_fx_key(&mut e.fx_keys, lt, fx.clone());
                    e.fx = fx;
                } else {
                    // effects added/removed: keyframes no longer match the chain
                    e.fx_keys.clear();
                    e.fx = fx;
                }
            }
            self.touch();
        }
        if !open {
            self.win_fx = None;
        }
    }

    fn pan_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_pan else { return };
        let Some(ev) = self.p.event(id).cloned() else {
            self.win_pan = None;
            return;
        };
        let mut open = true;
        let lt = (self.cursor - ev.start).clamp(0.0, ev.length);
        let mut pc = ev.pan_at(lt);
        let orig_pc = pc;
        let keyed = !ev.pan_keys.is_empty();
        let mut add_key = false;
        let mut clear_keys = false;
        egui::Window::new(format!("Panoramique/Recadrage : {}", ev.name))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(tr("Le résultat s'affiche dans l'aperçu vidéo.")).color(theme::TEXT_DIM()));
                ui.add(egui::Slider::new(&mut pc.zoom, 0.1..=5.0).logarithmic(true).text(tr("Zoom")));
                ui.add(egui::Slider::new(&mut pc.x, -1.0..=1.0).text(tr("Position X")));
                ui.add(egui::Slider::new(&mut pc.y, -1.0..=1.0).text(tr("Position Y")));
                ui.add(egui::Slider::new(&mut pc.rotation, -180.0..=180.0).text(tr("Rotation (°)")));
                if ui.button(tr("Réinitialiser")).clicked() {
                    pc = PanCrop::default();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    add_key = ui.button(tr("Image clé au curseur")).clicked();
                    clear_keys = ui.add_enabled(keyed, egui::Button::new(tr("Supprimer les images clés"))).clicked();
                });
                if keyed {
                    ui.label(egui::RichText::new(format!("{} {}", ev.pan_keys.len(), tr("image(s) clé(s)"))).color(theme::TEXT_DIM()));
                }
            });
        if pc != orig_pc || add_key || clear_keys {
            self.begin_window_edit(ctx);
            if let Some(e) = self.p.event_mut(id) {
                if clear_keys {
                    e.pan_keys.clear();
                    e.pan = pc;
                } else if keyed || add_key {
                    if e.pan_keys.is_empty() {
                        e.pan_keys.push(PanKey { t: 0.0, pan: e.pan });
                    }
                    crate::dialogs2::set_pan_key(&mut e.pan_keys, lt, pc);
                } else {
                    e.pan = pc;
                }
            }
            self.touch();
        }
        if !open {
            self.win_pan = None;
        }
    }

    fn props_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.win_props else { return };
        let Some(ev) = self.p.event(id).cloned() else {
            self.win_props = None;
            return;
        };
        let is_video = self
            .p
            .find_event(id)
            .map(|(t, _)| self.p.tracks[t].kind == TrackKind::Video)
            .unwrap_or(false);
        let fps = self.p.fps;
        let mut open = true;
        let mut e = ev.clone();
        egui::Window::new(tr("Propriétés de l'événement"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("props").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(tr("Nom"));
                    ui.text_edit_singleline(&mut e.name);
                    ui.end_row();
                    ui.label(tr("Début"));
                    ui.monospace(timecode(e.start, fps));
                    ui.end_row();
                    ui.label(tr("Longueur (s)"));
                    ui.add(egui::DragValue::new(&mut e.length).speed(1.0 / fps).range(1.0 / fps..=36000.0).max_decimals(3));
                    ui.end_row();
                    ui.label(tr("Décalage dans le média (s)"));
                    ui.add(egui::DragValue::new(&mut e.offset).speed(1.0 / fps).range(0.0..=36000.0).max_decimals(3));
                    ui.end_row();
                    ui.label(if is_video { tr("Opacité") } else { tr("Volume") });
                    ui.add(egui::Slider::new(&mut e.gain, 0.0..=1.0));
                    ui.end_row();
                    ui.label(tr("Fondu d'entrée (s)"));
                    ui.add(egui::DragValue::new(&mut e.fade_in).speed(1.0 / fps).range(0.0..=e.length).max_decimals(3));
                    ui.end_row();
                    ui.label(tr("Courbe d'entrée"));
                    egui::ComboBox::from_id_salt("cin").selected_text(e.curve_in.label()).show_ui(ui, |ui| {
                        for c in Curve::ALL {
                            ui.selectable_value(&mut e.curve_in, c, c.label());
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Fondu de sortie (s)"));
                    ui.add(egui::DragValue::new(&mut e.fade_out).speed(1.0 / fps).range(0.0..=e.length).max_decimals(3));
                    ui.end_row();
                    ui.label(tr("Courbe de sortie"));
                    egui::ComboBox::from_id_salt("cout").selected_text(e.curve_out.label()).show_ui(ui, |ui| {
                        for c in Curve::ALL {
                            ui.selectable_value(&mut e.curve_out, c, c.label());
                        }
                    });
                    ui.end_row();
                    if is_video {
                        ui.label(tr("Transition d'entrée"));
                        egui::ComboBox::from_id_salt("trn").selected_text(e.transition.label()).show_ui(ui, |ui| {
                            for t in Transition::ALL {
                                ui.selectable_value(&mut e.transition, t, t.label());
                            }
                        });
                        ui.end_row();
                    }
                    ui.label(tr("Options"));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut e.looped, tr("Boucle"));
                        ui.checkbox(&mut e.mute, tr("Muet"));
                    });
                    ui.end_row();
                });
            });
        if e != ev {
            self.begin_window_edit(ctx);
            if let Some(t) = self.p.event_mut(id) {
                *t = e;
            }
            self.touch();
        }
        if !open {
            self.win_props = None;
        }
    }

    fn project_window(&mut self, ctx: &egui::Context) {
        if !self.win_project {
            return;
        }
        let mut open = true;
        let mut p = self.p.clone();
        egui::Window::new(tr("Propriétés du projet"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("proj").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(tr("Nom du projet"));
                    ui.text_edit_singleline(&mut p.name);
                    ui.end_row();
                    ui.label(tr("Modèle"));
                    let cur = TEMPLATES
                        .iter()
                        .find(|t| t.width == p.width && t.height == p.height && (exact_fps(t.fps) - p.fps).abs() < 0.01)
                        .map(|t| t.name)
                        .unwrap_or("Personnalisé");
                    egui::ComboBox::from_id_salt("tpl").width(300.0).selected_text(cur).show_ui(ui, |ui| {
                        for t in TEMPLATES {
                            if ui.selectable_label(cur == t.name, t.name).clicked() {
                                p.width = t.width;
                                p.height = t.height;
                                p.fps = exact_fps(t.fps);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Largeur"));
                    ui.add(egui::DragValue::new(&mut p.width).range(16..=8192));
                    ui.end_row();
                    ui.label(tr("Hauteur"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut p.height).range(16..=8192));
                        if ui.button(tr("Inverser (horizontal / vertical)")).clicked() {
                            std::mem::swap(&mut p.width, &mut p.height);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Orientation"));
                    ui.label(if p.height > p.width { tr("Vertical") } else if p.height == p.width { tr("Carré") } else { tr("Horizontal") });
                    ui.end_row();
                    ui.label(tr("Fréquence d'images"));
                    egui::ComboBox::from_id_salt("fps").selected_text(format!("{} (i/s)", fr_num(p.fps, 3))).show_ui(ui, |ui| {
                        for f in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0] {
                            let ef = exact_fps(f);
                            if ui.selectable_label((ef - p.fps).abs() < 0.001, fr_num(ef, 3)).clicked() {
                                p.fps = ef;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Fréquence audio"));
                    ui.label(format!("{} Hz", p.sample_rate));
                    ui.end_row();
                    ui.label(tr("Tempo"));
                    ui.add(egui::DragValue::new(&mut p.tempo).range(20.0..=300.0).suffix(" BPM"));
                    ui.end_row();
                    ui.label(tr("Format des pixels"));
                    egui::ComboBox::from_id_salt("par").selected_text(fr_num(p.pixel_aspect as f64, 4)).show_ui(ui, |ui| {
                        for (v, l) in [(1.0f32, tr("1,0000 (carré)")), (0.9091, "0,9091 (NTSC DV)"), (1.2121, "1,2121 (NTSC DV 16:9)"), (1.0926, "1,0926 (PAL DV)"), (1.4568, "1,4568 (PAL DV 16:9)"), (1.3333, "1,3333 (HDV 1440)")] {
                            ui.selectable_value(&mut p.pixel_aspect, v, l);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Ordre des trames"));
                    egui::ComboBox::from_id_salt("field").selected_text(match p.field_order {
                        FieldOrder::Progressive => tr("Aucun (balayage progressif)"),
                        FieldOrder::UpperFirst => tr("Trame supérieure en premier"),
                        FieldOrder::LowerFirst => tr("Trame inférieure en premier"),
                    }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.field_order, FieldOrder::Progressive, tr("Aucun (balayage progressif)"));
                        ui.selectable_value(&mut p.field_order, FieldOrder::UpperFirst, tr("Trame supérieure en premier"));
                        ui.selectable_value(&mut p.field_order, FieldOrder::LowerFirst, tr("Trame inférieure en premier"));
                    });
                    ui.end_row();
                    ui.label(tr("Désentrelacement"));
                    egui::ComboBox::from_id_salt("deint").selected_text(match p.deinterlace {
                        Deinterlace::None => tr("Aucun"),
                        Deinterlace::Blend => tr("Mélanger les trames"),
                        Deinterlace::Interpolate => tr("Interpoler les trames"),
                    }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.deinterlace, Deinterlace::None, tr("Aucun"));
                        ui.selectable_value(&mut p.deinterlace, Deinterlace::Blend, tr("Mélanger les trames"));
                        ui.selectable_value(&mut p.deinterlace, Deinterlace::Interpolate, tr("Interpoler les trames"));
                    });
                    ui.end_row();
                    ui.label(tr("Qualité de rendu"));
                    egui::ComboBox::from_id_salt("rq").selected_text(match p.render_quality {
                        RenderQuality::Draft => tr("Ébauche"),
                        RenderQuality::Preview => tr("Aperçu"),
                        RenderQuality::Good => tr("Bonne"),
                        RenderQuality::Best => tr("Optimale"),
                    }).show_ui(ui, |ui| {
                        for (q, l) in [(RenderQuality::Draft, tr("Ébauche")), (RenderQuality::Preview, tr("Aperçu")), (RenderQuality::Good, tr("Bonne")), (RenderQuality::Best, tr("Optimale"))] {
                            ui.selectable_value(&mut p.render_quality, q, l);
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Canaux audio"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut p.channels, 2, tr("Stéréo"));
                        ui.selectable_value(&mut p.channels, 6, "5.1");
                    });
                    ui.end_row();
                    ui.label(tr("Flou de mouvement"));
                    ui.add(egui::Slider::new(&mut p.motion_blur, 0.0..=1.0));
                    ui.end_row();
                    ui.label(tr("Suréchantillonnage"));
                    ui.add(egui::Slider::new(&mut p.supersample, 1..=8));
                    ui.end_row();
                });
            });
        if p != self.p {
            self.begin_window_edit(ctx);
            self.p = p;
        }
        if !open {
            self.win_project = false;
        }
    }

    fn render_window(&mut self, ctx: &egui::Context) {
        // progress of a running / finished job
        if let Some(job) = &self.job {
            let (done, frac, msg, err) = {
                let p = job.progress.lock().unwrap();
                (p.done, p.fraction, p.message.clone(), p.error.clone())
            };
            let mut close = false;
            egui::Window::new(tr("Rendu"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Fichier : {}", job.path));
                    ui.add(egui::ProgressBar::new(frac).show_percentage());
                    if let Some(e) = &err {
                        ui.colored_label(egui::Color32::from_rgb(240, 110, 90), e);
                    } else {
                        ui.label(msg);
                    }
                    ui.horizontal(|ui| {
                        if done {
                            if err.is_none() && ui.button(tr("Ouvrir le dossier")).clicked() {
                                if let Some(dir) = std::path::Path::new(&job.path).parent() {
                                    crate::platform::open_folder(dir);
                                }
                            }
                            if ui.button(tr("Fermer")).clicked() {
                                close = true;
                            }
                        } else if ui.button(tr("Annuler")).clicked() {
                            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                });
            if !done {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            if close {
                self.job = None;
            }
            return;
        }
        let Some(mut dlg) = self.win_render.take() else { return };
        let mut open = true;
        let mut start = false;
        let p_name = self.p.name.clone();
        egui::Window::new(tr("Rendre en tant que"))
            .open(&mut open)
            .resizable(false)
            .default_width(620.0)
            .show(ctx, |ui| {
                let s = &mut dlg.settings;
                egui::Grid::new("render_out").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(tr("Format"));
                    egui::ComboBox::from_id_salt("fmt").width(300.0).selected_text(s.format.label()).show_ui(ui, |ui| {
                        for f in export::Format::ALL {
                            if ui.selectable_value(&mut s.format, f, f.label()).changed() {
                                let pb = std::path::PathBuf::from(&dlg.path).with_extension(f.ext());
                                dlg.path = pb.to_string_lossy().to_string();
                                if let Some(c) = export::Codec::for_format(f).first() {
                                    if !export::Codec::for_format(f).contains(&s.codec) {
                                        s.codec = *c;
                                    }
                                }
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Fichier"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut dlg.path).desired_width(360.0));
                        if ui.button(tr("Parcourir…")).clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter(s.format.label(), &[s.format.ext()])
                                .set_file_name(format!("{}.{}", p_name, s.format.ext()))
                                .save_file()
                            {
                                dlg.path = p.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Plage"));
                    ui.add_enabled(self.p.loop_region.is_some(), egui::Checkbox::new(&mut dlg.loop_only, tr("Rendre seulement la région de boucle")));
                    ui.end_row();
                });
                if s.format.has_video() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong(tr("Modèle"));
                        let cur = if s.use_project {
                            "(Utiliser les paramètres du projet)".to_string()
                        } else {
                            export::TEMPLATES
                                .iter()
                                .find(|t| t.width == s.width && t.height == s.height && (exact_fps(t.fps) - s.fps).abs() < 0.01 && t.avg_bps == s.avg_bps && t.max_bps == s.max_bps)
                                .map(|t| t.name.to_string())
                                .unwrap_or_else(|| "Personnalisé".into())
                        };
                        egui::ComboBox::from_id_salt("tpl_render").width(420.0).selected_text(cur).show_ui(ui, |ui| {
                            if ui.selectable_label(s.use_project, tr("(Utiliser les paramètres du projet)")).clicked() {
                                s.use_project = true;
                                let (a, m) = export::suggested_rates(self.p.width, self.p.height, self.p.fps);
                                s.avg_bps = a;
                                s.max_bps = m;
                                s.cbr_bps = a;
                            }
                            for t in export::TEMPLATES {
                                let label = format!("{}  —  {}x{}, {} / {}", t.name, t.width, t.height, export::fmt_bps(t.avg_bps), export::fmt_bps(t.max_bps));
                                if ui.selectable_label(false, label).clicked() {
                                    s.apply_template(t);
                                }
                            }
                        });
                    });
                    egui::Grid::new("render_video").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label(tr("Taille de l'image"));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut s.use_project, tr("Celle du projet"));
                            ui.add_enabled_ui(!s.use_project, |ui| {
                                ui.add(egui::DragValue::new(&mut s.width).range(16..=8192).suffix(" px"));
                                ui.label(tr("×"));
                                ui.add(egui::DragValue::new(&mut s.height).range(16..=8192).suffix(" px"));
                                if ui.button(tr("Inverser")).on_hover_text(tr("Inverser largeur et hauteur (horizontal / vertical)")).clicked() {
                                    std::mem::swap(&mut s.width, &mut s.height);
                                }
                            });
                        });
                        ui.end_row();
                        ui.label(tr("Fréquence d'images"));
                        ui.add_enabled_ui(!s.use_project, |ui| {
                            egui::ComboBox::from_id_salt("rfps").selected_text(format!("{} i/s", fr_num(s.fps, 3))).show_ui(ui, |ui| {
                                for f in [23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0] {
                                    let ef = exact_fps(f);
                                    if ui.selectable_label((ef - s.fps).abs() < 0.001, fr_num(ef, 3)).clicked() {
                                        s.fps = ef;
                                    }
                                }
                            });
                        });
                        ui.end_row();
                        ui.label(tr("Codec vidéo"));
                        egui::ComboBox::from_id_salt("rcodec").selected_text(s.codec.label()).show_ui(ui, |ui| {
                            for c in export::Codec::for_format(s.format) {
                                ui.selectable_value(&mut s.codec, *c, c.label());
                            }
                        });
                        ui.end_row();
                        ui.label(tr("Encodeur"));
                        egui::ComboBox::from_id_salt("renc").selected_text(s.encoder.label()).show_ui(ui, |ui| {
                            for e in export::Encoder::ALL {
                                let ok = export::encoder_available(e, s.codec);
                                let label = if ok { e.label().to_string() } else { format!("{} (non disponible)", e.label()) };
                                ui.add_enabled_ui(ok, |ui| {
                                    ui.selectable_value(&mut s.encoder, e, label);
                                });
                            }
                        });
                        ui.end_row();
                        if s.codec != export::Codec::ProRes {
                            ui.label(tr("Débit"));
                            ui.vertical(|ui| {
                                ui.radio_value(&mut s.rate_mode, export::RateMode::Constant, tr("Débit constant (bps)"));
                                ui.add_enabled_ui(s.rate_mode == export::RateMode::Constant, |ui| {
                                    ui.add(egui::DragValue::new(&mut s.cbr_bps).range(100_000..=800_000_000).speed(100_000).suffix(" bps"));
                                });
                                ui.radio_value(&mut s.rate_mode, export::RateMode::Variable, tr("Débit variable"));
                                ui.add_enabled_ui(s.rate_mode == export::RateMode::Variable, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(tr("Maximum (bps) :"));
                                        ui.add(egui::DragValue::new(&mut s.max_bps).range(100_000..=800_000_000).speed(100_000));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(tr("Moyenne (bps) :"));
                                        ui.add(egui::DragValue::new(&mut s.avg_bps).range(100_000..=800_000_000).speed(100_000));
                                    });
                                });
                                if s.avg_bps > s.max_bps {
                                    s.max_bps = s.avg_bps;
                                }
                                if s.codec == export::Codec::Hevc {
                                    ui.checkbox(&mut s.ten_bit, tr("10 bits (débit ×1,25, comme les modèles HDR)"));
                                }
                            });
                            ui.end_row();
                        }
                        ui.label(tr("Cadrage"));
                        ui.checkbox(&mut s.stretch, tr("Étirer la vidéo pour remplir l'image (sinon bandes noires)"));
                        ui.end_row();
                    });
                }
                ui.separator();
                egui::Grid::new("render_audio").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(tr("Audio : fréquence"));
                    egui::ComboBox::from_id_salt("rar").selected_text(format!("{} Hz", s.audio_rate)).show_ui(ui, |ui| {
                        for r in export::SAMPLE_RATES {
                            ui.selectable_value(&mut s.audio_rate, r, format!("{r} Hz"));
                        }
                    });
                    ui.end_row();
                    if s.format.has_video() && !(s.format == export::Format::Mov && s.codec == export::Codec::ProRes) {
                        ui.label(tr("Audio : débit"));
                        let cap = export::max_aac_for_rate(s.audio_rate);
                        if s.audio_bps > cap {
                            s.audio_bps = export::AAC_BITRATES.iter().copied().filter(|b| *b <= cap).last().unwrap_or(128_000);
                        }
                        egui::ComboBox::from_id_salt("rab").selected_text(export::fmt_bps(s.audio_bps)).show_ui(ui, |ui| {
                            for b in export::AAC_BITRATES.iter().copied().filter(|b| *b <= cap) {
                                ui.selectable_value(&mut s.audio_bps, b, export::fmt_bps(b));
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.separator();
                let (ow, oh, ofps) = s.output_size(&self.p);
                let dur = match (dlg.loop_only, self.p.loop_region) {
                    (true, Some((a, b))) if b > a => b - a,
                    _ => self.p.end_time(),
                };
                if s.format.has_video() {
                    let v = if s.rate_mode == export::RateMode::Constant { s.cbr_bps } else { s.avg_bps };
                    let v = if s.ten_bit && s.codec == export::Codec::Hevc { export::ten_bit_rate(v) } else { v };
                    let mb = (v as f64 + s.audio_bps as f64) * dur / 8.0 / 1e6;
                    ui.label(format!(
                        "Sortie : {}x{} {} ; {} i/s ; {} ; durée {:.1} s ; taille estimée {:.0} Mo",
                        ow,
                        oh,
                        if oh > ow { "(vertical)" } else { "" },
                        fr_num(ofps, 3),
                        s.codec.label(),
                        dur,
                        mb
                    ));
                }
                ui.horizontal(|ui| {
                    if ui.button(tr("Rendre")).clicked() {
                        start = true;
                    }
                });
            });
        if start {
            let range = match (dlg.loop_only, self.p.loop_region) {
                (true, Some((a, b))) if b > a => (a, b),
                _ => (0.0, self.p.end_time().max(1.0 / self.p.fps)),
            };
            self.job = Some(export::start(
                self.p.clone(),
                self.store.clone(),
                dlg.path.clone(),
                dlg.settings.clone(),
                range,
                self.auto_xf,
            ));
            self.status = format!("Rendu vers {}…", dlg.path);
            self.last_render = Some(dlg.settings.clone());
            return;
        }
        if open {
            self.win_render = Some(dlg);
        }
    }

    fn gen_window(&mut self, ctx: &egui::Context) {
        let Some(mid) = self.win_gen else { return };
        let Some(m) = self.p.media(mid).cloned() else {
            self.win_gen = None;
            return;
        };
        let MediaSource::Generated(mut g) = m.source.clone() else {
            self.win_gen = None;
            return;
        };
        let orig = g.clone();
        let mut open = true;
        egui::Window::new(format!("Média généré : {}", m.name))
            .open(&mut open)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| match &mut g {
                Generator::Text { text, size_pct, color, background } => {
                    ui.label(tr("Texte :"));
                    ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(330.0));
                    ui.add(egui::Slider::new(size_pct, 2..=40).text(tr("Taille (% de la hauteur)")));
                    color_edit(ui, "Couleur du texte", color);
                    color_edit(ui, "Arrière-plan", background);
                }
                Generator::Subtitle { text, size_pct } => {
                    ui.label(tr("Sous-titre :"));
                    ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(330.0));
                    ui.add(egui::Slider::new(size_pct, 2..=20).text(tr("Taille (% de la hauteur)")));
                }
                Generator::SolidColor(c) => {
                    color_edit(ui, "Couleur", c);
                }
                Generator::Gradient(a, b) => {
                    color_edit(ui, "Couleur de départ", a);
                    color_edit(ui, "Couleur d'arrivée", b);
                }
                Generator::Tone { freq } => {
                    ui.add(egui::Slider::new(freq, 20..=8000).text(tr("Fréquence (Hz)")));
                }
                Generator::Title { text, size_pct, color, anim, anim_ms } => {
                    ui.label(tr("Titre :"));
                    ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(330.0));
                    ui.add(egui::Slider::new(size_pct, 2..=40).text(tr("Taille (% de la hauteur)")));
                    color_edit(ui, "Couleur du texte", color);
                    egui::ComboBox::from_label(tr("Animation")).selected_text(anim.label()).show_ui(ui, |ui| {
                        for a in TitleAnim::ALL {
                            ui.selectable_value(anim, a, a.label());
                        }
                    });
                    ui.add(egui::Slider::new(anim_ms, 100..=5000).text(tr("Durée de l'animation (ms)")));
                }
                Generator::ColorBars | Generator::DemoMusic { .. } | Generator::Adjustment => {
                    ui.label(tr("Ce générateur n'a pas de réglage."));
                }
            });
        if g != orig {
            self.begin_window_edit(ctx);
            let name = g.label();
            if let Some(mm) = self.p.media.iter_mut().find(|x| x.id == mid) {
                mm.source = MediaSource::Generated(g);
                mm.name = name.clone();
            }
            for t in &mut self.p.tracks {
                for e in &mut t.events {
                    if e.media == mid {
                        e.name = name.clone();
                    }
                }
            }
            // refresh thumbnails
            self.store.thumbs.lock().unwrap().remove(&mid);
            self.store.audio.lock().unwrap().remove(&mid);
            self.store.loading.lock().unwrap().remove(&mid);
            self.thumb_tex.retain(|(id, _), _| *id != mid);
            if let Some(mm) = self.p.media(mid).cloned() {
                self.store.ensure(&mm, ctx);
            }
            self.touch();
        }
        if !open {
            self.win_gen = None;
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.win_about {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Blue Edit – raccourcis"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(tr("Blue Edit (« Blue ») : logiciel de montage vidéo à pistes, écrit en Rust."));
                ui.separator();
                egui::Grid::new("keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (k, d) in [
                        ("Espace", "Lecture / arrêt (retour au point de départ)"),
                        ("Maj+Espace", "Lire depuis le début"),
                        ("Entrée", "Lecture / pause"),
                        ("Échap", "Arrêt"),
                        ("J / K / L", "Arrière / pause / avant (appuis répétés : plus vite)"),
                        ("S", "Diviser au curseur"),
                        ("Suppr", "Supprimer la sélection"),
                        ("Ctrl+Z / Ctrl+Y", "Annuler / rétablir"),
                        ("Ctrl+X / C / V", "Couper / copier / coller au curseur"),
                        ("M", "Insérer un marqueur"),
                        ("I / O", "Début / fin de la région de boucle au curseur"),
                        ("Q", "Lecture en boucle"),
                        ("G / U", "Grouper / dissocier la sélection"),
                        ("Z / X", "Muet / solo de la piste sélectionnée"),
                        ("Ctrl+Q / Ctrl+Maj+Q", "Insérer une piste audio / vidéo"),
                        ("F8 / Ctrl+F8 / Alt+F8", "Aimantation / grille / quantifier aux images"),
                        ("Ctrl+L", "Montage en cascade automatique"),
                        ("Ctrl+Maj+X", "Fondu enchaîné automatique"),
                        ("Alt+1 / 5 / 7 / 8 / 9", "Explorateur / média / transitions / effets / générateurs"),
                        ("Alt+Entrée", "Propriétés du projet"),
                        ("Haut / Bas", "Zoom avant / arrière"),
                        ("Ctrl+Maj+Haut / Bas", "Hauteur des pistes"),
                        ("Gauche / Droite", "Image précédente / suivante"),
                        ("Molette", "Zoom ; Maj+molette : défiler ; Ctrl+molette : pistes"),
                        ("Ctrl+glisser", "Copier les événements"),
                        ("Ctrl+R", "Rendre en tant que…"),
                    ] {
                        ui.monospace(k);
                        ui.label(tr(d));
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new(tr("Haut d'un événement : glisser les coins pour les fondus, le milieu pour le gain.")).color(theme::TEXT_DIM()));
            });
        if !open {
            self.win_about = false;
        }
    }
}
