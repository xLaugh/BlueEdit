//! Docked windows: media / explorer / transitions / FX / generators tabs,
//! the video preview, the master bus, the transport and status bars.

use crate::i18n::tr;

use crate::app::{App, FxPayload, LeftTab, MediaPayload, Quality, TransitionPayload};
use crate::compositor::PreviewSource;
use crate::icons::{self, Icon};
use crate::media;
use crate::model::*;
use crate::theme;
use egui::{pos2, vec2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};

pub fn fr_num(v: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, v).replace('.', ",")
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::same(4))
}

impl App {
    pub fn top_dock(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // [tabs | video preview | master bus]; panels on the left shrink the
        // remaining space, so the preview gets an exact width and the master
        // bus takes what is left on the right.
        let full_w = ui.available_width();
        let master_w = 118.0;
        egui::SidePanel::left("tabs_dock")
            .default_width((full_w - master_w - 600.0).max(300.0))
            .width_range(260.0..=(full_w - master_w - 300.0).max(261.0))
            .resizable(true)
            .frame(panel_frame())
            .show_inside(ui, |ui| self.left_tabs(ui, ctx));
        let preview_w = (ui.available_width() - master_w - 6.0).max(200.0);
        egui::SidePanel::left("preview")
            .exact_width(preview_w)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::same(2)))
            .show_inside(ui, |ui| self.preview(ui, ctx));
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show_inside(ui, |ui| self.master_bus(ui));
    }

    fn left_tabs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("tabs")
            .exact_height(24.0)
            .frame(egui::Frame::new().fill(theme::BAR()))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (tab, name) in [
                        (LeftTab::ProjectMedia, tr("Média du projet")),
                        (LeftTab::Explorer, tr("Explorateur")),
                        (LeftTab::Transitions, tr("Transitions")),
                        (LeftTab::VideoFx, tr("Effets vidéo")),
                        (LeftTab::Generators, tr("Générateurs de médias")),
                    ] {
                        let sel = self.left_tab == tab;
                        let b = egui::Button::new(egui::RichText::new(name).size(12.0).color(if sel { theme::TEXT() } else { theme::TEXT_DIM() }))
                            .fill(if sel { theme::PANEL() } else { theme::BAR() })
                            .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 });
                        if ui.add(b).clicked() {
                            self.left_tab = tab;
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| match self.left_tab {
                LeftTab::ProjectMedia => self.media_tab(ui, ctx),
                LeftTab::Explorer => self.explorer_tab(ui, ctx),
                LeftTab::Transitions => self.transitions_tab(ui),
                LeftTab::VideoFx => self.fx_tab(ui),
                LeftTab::Generators => self.generators_tab(ui, ctx),
            });
    }

    fn media_thumb_tex(&mut self, ctx: &egui::Context, id: Id) -> Option<egui::TextureHandle> {
        let key = (id, usize::MAX);
        if let Some(t) = self.thumb_tex.get(&key) {
            return Some(t.clone());
        }
        let th = self.store.thumbs(id)?;
        let (_, img) = th.frames.get(th.frames.len() / 3)?;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.data);
        let tex = ctx.load_texture(format!("mthumb{}", id), ci, egui::TextureOptions::LINEAR);
        self.thumb_tex.insert(key, tex.clone());
        Some(tex)
    }

    pub(crate) fn media_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let sel_id = ui.id().with("media_sel");
        let mut selected: Option<Id> = ui.data(|d| d.get_temp(sel_id));
        ui.horizontal(|ui| {
            if ui.button(tr("Importer un média…")).clicked() {
                self.import_dialog(ctx);
            }
            if ui.add_enabled(selected.is_some(), egui::Button::new(tr("Ajouter au curseur"))).clicked() {
                if let Some(id) = selected {
                    self.insert_media(id, self.cursor, None, true);
                }
            }
            if ui.add_enabled(selected.is_some(), egui::Button::new(tr("Retirer du projet"))).clicked() {
                if let Some(id) = selected {
                    let used = self.p.tracks.iter().any(|t| t.events.iter().any(|e| e.media == id));
                    if used {
                        self.status = "Ce média est utilisé sur la ligne de temps : supprimez d'abord ses événements.".into();
                    } else {
                        self.push_undo();
                        self.p.media.retain(|m| m.id != id);
                        selected = None;
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("Rechercher :"));
            ui.add(egui::TextEdit::singleline(&mut self.media_search).desired_width(140.0));
            ui.label(tr("Chutier :"));
            let cur = if self.media_bin.is_empty() { tr("Tous").to_string() } else { self.media_bin.clone() };
            egui::ComboBox::from_id_salt("bin_filter").width(120.0).selected_text(cur).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.media_bin, String::new(), tr("Tous"));
                for b in self.p.bins.clone() {
                    ui.selectable_value(&mut self.media_bin, b.clone(), b);
                }
            });
            if ui.button(tr("Nouveau chutier")).clicked() {
                self.push_undo();
                let n = self.p.bins.len() + 1;
                let name = format!("{} {}", tr("Chutier"), n);
                self.p.bins.push(name.clone());
                self.media_bin = name;
            }
            if ui.button(tr("Nettoyer")).on_hover_text(tr("Retirer les médias inutilisés")).clicked() {
                self.remove_unused_media();
            }
        });
        ui.separator();
        if self.p.media.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(tr("Glissez des fichiers ici ou cliquez sur « Importer un média… »")).color(theme::TEXT_DIM()));
            });
            return;
        }
        let media: Vec<Media> = self
            .p
            .media
            .iter()
            .filter(|m| !matches!(m.source, MediaSource::Generated(Generator::Subtitle { .. })))
            .filter(|m| self.media_bin.is_empty() || m.bin == self.media_bin)
            .filter(|m| {
                let q = self.media_search.trim().to_lowercase();
                q.is_empty() || m.name.to_lowercase().contains(&q) || m.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let vw = (ui.clip_rect().right() - ui.cursor().left() - 4.0).max(130.0);
            ui.allocate_ui_with_layout(vec2(vw, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                let cols = ((vw + 8.0) / 128.0).floor().max(1.0) as usize;
                let mut col_i = 0usize;
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for m in &media {
                    if col_i > 0 && col_i % cols == 0 {
                        ui.end_row();
                    }
                    col_i += 1;
                    let tex = self.media_thumb_tex(ctx, m.id);
                    let id = egui::Id::new(("media_card", m.id));
                    let is_sel = selected == Some(m.id);
                    let r = ui
                        .dnd_drag_source(id, MediaPayload(m.id), |ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 98.0), Sense::hover());
                            let p = ui.painter_at(rect);
                            p.rect_filled(rect, 3.0, if is_sel { theme::ACCENT().gamma_multiply(0.6) } else { theme::HEADER() });
                            let img_r = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(112.0, 63.0));
                            p.rect_filled(img_r, 0.0, Color32::from_rgb(24, 24, 26));
                            if let Some(t) = &tex {
                                let s = t.size_vec2();
                                let k = (img_r.width() / s.x).min(img_r.height() / s.y);
                                let ir = Rect::from_center_size(img_r.center(), s * k);
                                p.image(t.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            } else if m.has_audio && !m.has_video {
                                // simple speaker glyph for audio-only media
                                let c = img_r.center();
                                for i in 0..12 {
                                    let x = c.x - 30.0 + i as f32 * 5.5;
                                    let h = ((i as f32 * 1.7).sin().abs() * 18.0) + 4.0;
                                    p.line_segment([pos2(x, c.y - h), pos2(x, c.y + h)], Stroke::new(2.0, theme::WAVE()));
                                }
                            }
                            let label_r = Rect::from_min_max(pos2(rect.left() + 4.0, img_r.bottom() + 2.0), pos2(rect.right() - 4.0, rect.bottom()));
                            p.with_clip_rect(label_r).text(label_r.left_top(), egui::Align2::LEFT_TOP, &m.name, FontId::proportional(11.0), theme::TEXT());
                            let info = if m.duration > 0.0 { timecode(m.duration, self.p.fps) } else { m.type_label().to_string() };
                            p.text(label_r.left_bottom() + vec2(0.0, -1.0), egui::Align2::LEFT_BOTTOM, info, FontId::proportional(10.0), theme::TEXT_DIM());
                        })
                        .response;
                    let r = ui.interact(r.rect, id.with("click"), Sense::click());
                    if r.clicked() {
                        selected = Some(m.id);
                    }
                    if r.double_clicked() {
                        self.insert_media(m.id, self.cursor, None, true);
                    }
                    let mid = m.id;
                    let bins = self.p.bins.clone();
                    let r = r.on_hover_text(format!(
                        "{}\n{}{}{}\n{}",
                        m.name,
                        m.type_label(),
                        if m.width > 0 && m.has_video { format!(" – {}x{}", m.width, m.height) } else { String::new() },
                        if m.tags.is_empty() { String::new() } else { format!("\n{}", m.tags.join(", ")) },
                        tr("Double-clic : ajouter au curseur. Glisser : placer sur la ligne de temps. Clic droit : plus d'options.")
                    ));
                    r.context_menu(|ui| {
                        if ui.button(tr("Ouvrir dans la rogneuse")).clicked() {
                            self.open_in_trimmer(mid);
                            ui.close_menu();
                        }
                        if ui.button(tr("Ajouter comme prise de l'événement sélectionné")).clicked() {
                            self.add_take(mid);
                            ui.close_menu();
                        }
                        ui.menu_button(tr("Déplacer vers le chutier"), |ui| {
                            for b in std::iter::once(String::new()).chain(bins.iter().cloned()) {
                                let label = if b.is_empty() { tr("(racine)").to_string() } else { b.clone() };
                                if ui.button(label).clicked() {
                                    self.push_undo();
                                    if let Some(x) = self.p.media.iter_mut().find(|x| x.id == mid) {
                                        x.bin = b.clone();
                                    }
                                    ui.close_menu();
                                }
                            }
                        });
                        if ui.button(tr("Propriétés du média…")).clicked() {
                            self.win_media_props = Some(mid);
                            ui.close_menu();
                        }
                    });
                }
            });
        });
        ui.data_mut(|d| d.insert_temp(sel_id, selected));
    }

    pub(crate) fn explorer_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui.button("⬆").on_hover_text(tr("Dossier parent")).clicked() {
                if let Some(p) = self.explorer_dir.parent() {
                    self.explorer_dir = p.to_path_buf();
                }
            }
            let mut s = self.explorer_dir.to_string_lossy().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(ui.available_width())).lost_focus() {
                let pb = std::path::PathBuf::from(&s);
                if pb.is_dir() {
                    self.explorer_dir = pb;
                }
            }
        });
        ui.separator();
        let mut entries: Vec<(bool, String, std::path::PathBuf)> = std::fs::read_dir(&self.explorer_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| {
                        let p = e.path();
                        (p.is_dir(), e.file_name().to_string_lossy().to_string(), p)
                    })
                    .filter(|(d, n, _)| (*d && !n.starts_with('.')) || media::is_media_file(n))
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
        let mut go: Option<std::path::PathBuf> = None;
        let mut add: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if entries.is_empty() {
                ui.label(egui::RichText::new(tr("Aucun dossier ni média ici.")).color(theme::TEXT_DIM()));
            }
            for (is_dir, name, path) in &entries {
                let label = if *is_dir { format!("📁 {}", name) } else { format!("🎞 {}", name) };
                let r = ui.selectable_label(false, label);
                if *is_dir && r.clicked() {
                    go = Some(path.clone());
                }
                if !*is_dir && r.double_clicked() {
                    add = Some(path.to_string_lossy().to_string());
                }
                if !*is_dir {
                    r.on_hover_text(tr("Double-clic : importer et ajouter au curseur"));
                }
            }
        });
        if let Some(g) = go {
            self.explorer_dir = g;
        }
        if let Some(p) = add {
            let ids = self.import_files(vec![p], ctx);
            if let Some(id) = ids.first() {
                self.insert_media(*id, self.cursor, None, true);
            }
        }
    }

    pub(crate) fn transitions_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(tr("Glissez une transition sur un événement vidéo (elle s'applique à son entrée en fondu ou au fondu enchaîné).")).color(theme::TEXT_DIM()).size(11.0));
        let t = ui.input(|i| i.time);
        let prog = ((t * 0.6) % 1.4).min(1.0) as f32;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let vw = (ui.clip_rect().right() - ui.cursor().left() - 4.0).max(130.0);
            ui.allocate_ui_with_layout(vec2(vw, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                let cols = ((vw + 8.0) / 128.0).floor().max(1.0) as usize;
                let mut col_i = 0usize;
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for tr in Transition::ALL {
                    if col_i > 0 && col_i % cols == 0 {
                        ui.end_row();
                    }
                    col_i += 1;
                    let id = egui::Id::new(("trans", tr.label()));
                    let r = ui
                        .dnd_drag_source(id, TransitionPayload(tr), |ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 88.0), Sense::hover());
                            let p = ui.painter_at(rect);
                            p.rect_filled(rect, 3.0, theme::HEADER());
                            let img = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(112.0, 63.0));
                            draw_transition_icon(&p, img, tr, prog);
                            p.text(pos2(rect.left() + 4.0, rect.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, tr.label(), FontId::proportional(11.0), theme::TEXT());
                        })
                        .response;
                    let r = ui.interact(r.rect, id.with("c"), Sense::click());
                    if r.double_clicked() && !self.sel.is_empty() {
                        self.push_undo();
                        let sel = self.sel.clone();
                        for tk in &mut self.p.tracks {
                            for e in &mut tk.events {
                                if sel.contains(&e.id) {
                                    e.transition = tr;
                                }
                            }
                        }
                        self.status = format!("Transition « {} » appliquée à la sélection.", tr.label());
                    }
                }
            });
        });
    }

    pub(crate) fn fx_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(tr("Glissez un effet sur un événement vidéo, ou double-cliquez pour l'ajouter à la sélection.")).color(theme::TEXT_DIM()).size(11.0));
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let vw = (ui.clip_rect().right() - ui.cursor().left() - 4.0).max(130.0);
            ui.allocate_ui_with_layout(vec2(vw, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                let cols = ((vw + 8.0) / 128.0).floor().max(1.0) as usize;
                let mut col_i = 0usize;
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                let mut items: Vec<(String, Vec<VideoFx>)> = VideoFx::presets().into_iter().map(|f| (f.label().to_string(), vec![f])).collect();
                for pack in self.ai.plugins.packs.iter().filter(|p| !p.disabled) {
                    for e in &pack.effects {
                        items.push((e.name.clone(), e.chain.clone()));
                    }
                }
                for (fx_name, chain) in items {
                    let fx = chain.first().cloned().unwrap_or(VideoFx::Saturation { amount: 1.0 });
                    if col_i > 0 && col_i % cols == 0 {
                        ui.end_row();
                    }
                    col_i += 1;
                    let id = egui::Id::new(("fx", fx_name.clone()));
                    let r = ui
                        .dnd_drag_source(id, FxPayload(chain.clone()), |ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 88.0), Sense::hover());
                            let p = ui.painter_at(rect);
                            p.rect_filled(rect, 3.0, theme::HEADER());
                            let img = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(112.0, 63.0));
                            draw_fx_icon(&p, img, &fx);
                            p.text(pos2(rect.left() + 4.0, rect.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, &fx_name, FontId::proportional(11.0), theme::TEXT());
                        })
                        .response;
                    let r = ui.interact(r.rect, id.with("c"), Sense::click());
                    if r.double_clicked() && !self.sel.is_empty() {
                        self.push_undo();
                        let sel = self.sel.clone();
                        let mut first = None;
                        for tk in &mut self.p.tracks {
                            if tk.kind != TrackKind::Video {
                                continue;
                            }
                            for e in &mut tk.events {
                                if sel.contains(&e.id) {
                                    e.fx.extend(chain.iter().cloned());
                                    first.get_or_insert(e.id);
                                }
                            }
                        }
                        self.win_fx = first;
                    }
                }
            });
        });
    }

    pub(crate) fn generators_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(egui::RichText::new(tr("Cliquez pour créer le média et l'ajouter au curseur (piste sélectionnée).")).color(theme::TEXT_DIM()).size(11.0));
        ui.add_space(4.0);
        let items: Vec<(&str, Generator)> = vec![
            (tr("Texte (titre)"), crate::app::default_text()),
            (tr("Couleur unie"), Generator::SolidColor([40, 70, 140, 255])),
            (tr("Dégradé de couleurs"), Generator::Gradient([20, 30, 80, 255], [200, 90, 60, 255])),
            (tr("Mire de couleurs"), Generator::ColorBars),
            (tr("Tonalité 440 Hz (audio)"), Generator::Tone { freq: 440 }),
            (tr("Musique de démonstration (audio)"), Generator::DemoMusic { seed: 1 }),
        ];
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let vw = (ui.clip_rect().right() - ui.cursor().left() - 4.0).max(130.0);
            ui.allocate_ui_with_layout(vec2(vw, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                let cols = ((vw + 8.0) / 128.0).floor().max(1.0) as usize;
                let mut col_i = 0usize;
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for (name, g) in items {
                    if col_i > 0 && col_i % cols == 0 {
                        ui.end_row();
                    }
                    col_i += 1;
                    let (rect, resp) = ui.allocate_exact_size(vec2(120.0, 88.0), Sense::click());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 3.0, if resp.hovered() { theme::HEADER_SEL() } else { theme::HEADER() });
                    let img = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(112.0, 63.0));
                    match &g {
                        Generator::SolidColor(c) => {
                            p.rect_filled(img, 0.0, Color32::from_rgb(c[0], c[1], c[2]));
                        }
                        Generator::Gradient(a, b) => {
                            let mut mesh = egui::Mesh::default();
                            let ca = Color32::from_rgb(a[0], a[1], a[2]);
                            let cb = Color32::from_rgb(b[0], b[1], b[2]);
                            mesh.colored_vertex(img.left_top(), ca);
                            mesh.colored_vertex(img.right_top(), cb);
                            mesh.colored_vertex(img.right_bottom(), cb);
                            mesh.colored_vertex(img.left_bottom(), ca);
                            mesh.add_triangle(0, 1, 2);
                            mesh.add_triangle(0, 2, 3);
                            p.add(egui::Shape::mesh(mesh));
                        }
                        Generator::ColorBars => {
                            let cols = [[192, 192, 192], [192, 192, 0], [0, 192, 192], [0, 192, 0], [192, 0, 192], [192, 0, 0], [0, 0, 192]];
                            for (i, c) in cols.iter().enumerate() {
                                let w = img.width() / 7.0;
                                p.rect_filled(Rect::from_min_size(pos2(img.left() + i as f32 * w, img.top()), vec2(w + 0.5, img.height())), 0.0, Color32::from_rgb(c[0], c[1], c[2]));
                            }
                        }
                        Generator::Text { .. } => {
                            p.rect_filled(img, 0.0, Color32::from_rgb(20, 20, 22));
                            p.text(img.center(), egui::Align2::CENTER_CENTER, tr("Aa"), FontId::proportional(28.0), Color32::WHITE);
                        }
                        _ => {
                            p.rect_filled(img, 0.0, Color32::from_rgb(20, 20, 22));
                            let pts: Vec<_> = (0..60)
                                .map(|i| {
                                    let x = img.left() + i as f32 / 59.0 * img.width();
                                    pos2(x, img.center().y + (i as f32 * 0.6).sin() * 18.0)
                                })
                                .collect();
                            p.add(egui::Shape::line(pts, Stroke::new(1.5, theme::WAVE())));
                        }
                    }
                    p.text(pos2(rect.left() + 4.0, rect.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, name, FontId::proportional(11.0), theme::TEXT());
                    if resp.clicked() {
                        let is_text = matches!(g, Generator::Text { .. });
                        let audio = g.is_audio();
                        let id = self.add_generator(g, ctx);
                        self.insert_media(id, self.cursor, None, true);
                        if !audio {
                            self.win_gen = Some(id);
                        }
                        let _ = is_text;
                    }
                }
            });
        });
    }

    // ------------------------------------------------------------- preview

    pub(crate) fn preview(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if icons::button(ui, Icon::Properties, tr("Propriétés du projet vidéo"), false).clicked() {
                self.win_project = true;
            }
            ui.add_space(4.0);
            egui::ComboBox::from_id_salt("quality")
                .width(130.0)
                .selected_text(self.quality.label())
                .show_ui(ui, |ui| {
                    for q in [Quality::Draft, Quality::Preview, Quality::Good] {
                        ui.selectable_value(&mut self.quality, q, q.label());
                    }
                });
            ui.add_space(4.0);
            if icons::button(ui, Icon::Grid, tr("Grille"), self.show_grid).clicked() {
                self.show_grid = !self.show_grid;
            }
            if icons::button(ui, Icon::Safe, tr("Zones de sécurité"), self.show_safe).clicked() {
                self.show_safe = !self.show_safe;
            }
            if icons::button(ui, Icon::Snapshot, tr("Enregistrer un instantané dans un fichier"), false).clicked() {
                self.save_snapshot();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(tr("Aperçu vidéo")).color(theme::TEXT_DIM()).size(11.0));
            });
        });
        let avail = ui.available_rect_before_wrap();
        let status_h = 34.0;
        let img_area = Rect::from_min_max(avail.min, pos2(avail.right(), avail.bottom() - status_h));
        ui.allocate_rect(avail, Sense::hover());
        let painter = ui.painter_at(avail);
        painter.rect_filled(img_area, 0.0, Color32::from_rgb(16, 16, 18));
        let (pw, ph) = (self.p.width.max(2) as f32, self.p.height.max(2) as f32);
        let fit_k = (img_area.width() / pw).min(img_area.height() / ph);
        let k = if self.preview_zoom > 0.0 { self.preview_zoom / ctx.pixels_per_point() } else { fit_k };
        let disp = Rect::from_center_size(img_area.center(), vec2(pw * k, ph * k));
        let ppp = ctx.pixels_per_point();
        let mut rw = ((self.p.width / self.quality.divisor()) as f32).min(disp.width() * ppp).min(1280.0).max(16.0) as u32 & !1;
        if rw < 16 {
            rw = 16;
        }
        let rh = ((rw as f32 * ph / pw).round() as u32).max(2) & !1;
        let frame = self.p.frame_of(self.cursor);
        let sig = (self.rev, frame, rw, rh);
        let dynamic = self.has_dynamic_video_at(self.cursor);
        if sig != self.preview_sig || dynamic || self.preview_tex.is_none() {
            let t = self.p.quantize(self.cursor);
            let mut src = PreviewSource {
                server: &self.server,
                store: &self.store,
            };
            let ram = if self.ram_sig == (self.rev, rw, rh) { self.ram_cache.get(&frame).cloned() } else { None };
            let img: std::sync::Arc<Vec<u8>> = if self.mute_video {
                std::sync::Arc::new([0u8, 0, 0, 255].repeat((rw * rh) as usize))
            } else if let Some(r) = ram {
                r
            } else {
                std::sync::Arc::new(self.comp.compose(&self.p, t, rw, rh, self.auto_xf, &mut src))
            };
            let ci = egui::ColorImage::from_rgba_unmultiplied([rw as usize, rh as usize], &img);
            self.preview_rgba = Some((rw, rh, img.clone()));
            self.preview_serial += 1;
            match &mut self.preview_tex {
                Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
                None => self.preview_tex = Some(ctx.load_texture("preview", ci, egui::TextureOptions::LINEAR)),
            }
            self.preview_sig = sig;
            self.preview_dims = (rw, rh);
        }
        if let Some(tex) = &self.preview_tex {
            let painter = painter.with_clip_rect(img_area);
            painter.image(tex.id(), disp, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if self.show_grid {
            let n = PREVIEW_GRID;
            for i in 1..n {
                let f = i as f32 / n as f32;
                let s = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 50));
                painter.line_segment([pos2(disp.left() + f * disp.width(), disp.top()), pos2(disp.left() + f * disp.width(), disp.bottom())], s);
                painter.line_segment([pos2(disp.left(), disp.top() + f * disp.height()), pos2(disp.right(), disp.top() + f * disp.height())], s);
            }
        }
        if self.show_safe {
            for (pct, c) in [(ACTION_SAFE, Color32::from_rgb(120, 200, 255)), (TITLE_SAFE, Color32::from_rgb(255, 210, 90))] {
                let m = vec2(disp.width() * pct / 2.0, disp.height() * pct / 2.0);
                painter.rect_stroke(Rect::from_min_max(disp.min + m, disp.max - m), 0.0, Stroke::new(1.0, c), StrokeKind::Inside);
            }
        }
        let sr = Rect::from_min_max(pos2(avail.left(), img_area.bottom()), avail.max);
        painter.rect_filled(sr, 0.0, theme::PANEL());
        let fps = fr_num(self.p.fps, 3);
        let lines = [
            (
                format!("{} {}x{}x32 ; {}p", tr("Projet :"), self.p.width, self.p.height, fps),
                format!("{} {}", tr("Image :"), frame),
            ),
            (
                format!("{} {}x{}x32 ; {}p", tr("Aperçu :"), self.preview_dims.0, self.preview_dims.1, fps),
                format!("{} {}x{}x32", tr("Affichage :"), disp.width().round(), disp.height().round()),
            ),
        ];
        for (i, (a, b)) in lines.iter().enumerate() {
            let y = sr.top() + 9.0 + i as f32 * 15.0;
            painter.text(pos2(sr.left() + 6.0, y), egui::Align2::LEFT_CENTER, a, FontId::proportional(11.0), theme::TEXT_DIM());
            painter.text(pos2(sr.left() + sr.width() * 0.55, y), egui::Align2::LEFT_CENTER, b, FontId::proportional(11.0), theme::TEXT_DIM());
        }
    }

    fn has_dynamic_video_at(&self, t: f64) -> bool {
        self.p.tracks.iter().any(|tr| {
            tr.kind == TrackKind::Video
                && tr.events.iter().any(|e| {
                    e.start <= t
                        && t < e.end()
                        && self
                            .p
                            .media(e.media)
                            .map(|m| matches!(m.source, MediaSource::File(_) | MediaSource::Image(_)))
                            .unwrap_or(false)
                })
        })
    }

    // ---------------------------------------------------------- master bus

    pub(crate) fn master_bus(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(tr("Maître")).strong());
        });
        let avail = ui.available_rect_before_wrap();
        let fader_h = 46.0;
        let meter_r = Rect::from_min_max(avail.min + vec2(0.0, 16.0), pos2(avail.right(), avail.bottom() - fader_h));
        let p = ui.painter_at(avail);
        let db_to_f = |db: f32| ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
        let lin_to_db = |v: f32| if v > 0.0 { 20.0 * v.log10() } else { -100.0 };
        let scale_w = 26.0;
        let bars_left = meter_r.left() + 4.0;
        let bar_w = (meter_r.width() - scale_w - 12.0) / 2.0;
        for ch in 0..2 {
            let x = bars_left + ch as f32 * (bar_w + 4.0);
            let br = Rect::from_min_max(pos2(x, meter_r.top()), pos2(x + bar_w, meter_r.bottom()));
            p.rect_filled(br, 0.0, Color32::from_rgb(18, 18, 20));
            let f = db_to_f(lin_to_db(self.meters[ch]));
            let top = br.bottom() - f * br.height();
            // green -> yellow -> red gradient by level
            let mut mesh = egui::Mesh::default();
            let steps = 24;
            for i in 0..steps {
                let y0 = br.bottom() - (i as f32 / steps as f32) * br.height();
                let y1 = br.bottom() - ((i + 1) as f32 / steps as f32) * br.height();
                if y0 <= top {
                    break;
                }
                let lvl = i as f32 / steps as f32;
                let c = if lvl > 0.9 {
                    Color32::from_rgb(230, 70, 50)
                } else if lvl > 0.75 {
                    Color32::from_rgb(230, 200, 60)
                } else {
                    Color32::from_rgb(70, 200, 90)
                };
                mesh.add_colored_rect(Rect::from_min_max(pos2(br.left(), y1.max(top)), pos2(br.right(), y0 - 1.0)), c);
            }
            p.add(egui::Shape::mesh(mesh));
            let hold_db = lin_to_db(self.peak_hold[ch]);
            let txt = if hold_db > -99.0 { fr_num(hold_db as f64, 1) } else { "-Inf.".into() };
            p.text(pos2(br.center().x, meter_r.top() - 8.0), egui::Align2::CENTER_CENTER, txt, FontId::proportional(9.5), if hold_db > -0.1 { Color32::from_rgb(240, 80, 60) } else { theme::TEXT_DIM() });
        }
        let sx = bars_left + 2.0 * (bar_w + 4.0) + 2.0;
        for db in [0, -6, -12, -18, -24, -30, -36, -42, -48, -54, -60] {
            let y = meter_r.bottom() - db_to_f(db as f32) * meter_r.height();
            p.line_segment([pos2(sx, y), pos2(sx + 4.0, y)], Stroke::new(1.0, theme::TEXT_DIM()));
            p.text(pos2(sx + 6.0, y), egui::Align2::LEFT_CENTER, format!("{}", db), FontId::proportional(9.0), theme::TEXT_DIM());
        }
        let resp = ui.interact(meter_r, ui.id().with("meters"), Sense::click());
        if resp.clicked() {
            self.peak_hold = [0.0; 2];
        }
        resp.on_hover_text(tr("Vu-mètres du bus maître (plancher -60 dB). Cliquez pour remettre les crêtes à zéro."));
        let fr = Rect::from_min_max(pos2(avail.left(), avail.bottom() - fader_h + 6.0), avail.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(fr));
        let mut v = self.p.master_volume;
        let db = lin_to_db(v);
        child.label(egui::RichText::new(if db > -99.0 { format!("{} dB", fr_num(db as f64, 1)) } else { "-Inf.".into() }).size(11.0));
        if child.add(egui::Slider::new(&mut v, 0.0..=2.0).show_value(false)).changed() {
            self.p.master_volume = v;
            self.touch();
        }
    }

    // ----------------------------------------------------- transport & status

    pub fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if icons::button(ui, Icon::Record, tr("Enregistrer l'audio dans les pistes armées"), self.recorder.is_some()).clicked() {
                self.toggle_record(ui.ctx());
            }
            if icons::button(ui, Icon::Loop, tr("Lecture en boucle (Q)"), self.loop_on).clicked() {
                self.loop_on = !self.loop_on;
            }
            if icons::button(ui, Icon::PlayFromStart, tr("Lire depuis le début (Maj+Espace)"), false).clicked() {
                self.playing = false;
                self.cursor = 0.0;
                self.play();
                self.play_origin = 0.0;
            }
            if icons::button(ui, Icon::Play, tr("Lecture (Espace)"), self.playing && self.speed > 0.0).clicked() {
                self.play();
            }
            if icons::button(ui, Icon::Pause, tr("Pause (Entrée)"), false).clicked() {
                self.pause();
            }
            if icons::button(ui, Icon::Stop, tr("Arrêt (Échap)"), false).clicked() {
                self.stop();
            }
            ui.add_space(6.0);
            if icons::button(ui, Icon::ToStart, tr("Aller au début (Origine)"), false).clicked() {
                self.cursor = 0.0;
                self.tl.scroll = 0.0;
            }
            if icons::button(ui, Icon::ToEnd, tr("Aller à la fin (Fin)"), false).clicked() {
                self.cursor = self.p.end_time();
            }
            if icons::button(ui, Icon::PrevFrame, tr("Image précédente (Gauche)"), false).clicked() {
                self.cursor = (self.p.quantize(self.cursor) - 1.0 / self.p.fps).max(0.0);
            }
            if icons::button(ui, Icon::NextFrame, tr("Image suivante (Droite)"), false).clicked() {
                self.cursor = self.p.quantize(self.cursor) + 1.0 / self.p.fps;
            }
            if self.playing && (self.speed - 1.0).abs() > 1e-6 {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(format!("{} {}×", tr("Vitesse :"), fr_num(self.speed, 1))).color(theme::ACCENT()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fps = self.p.fps;
                let (a, b) = self.p.loop_region.unwrap_or((self.cursor, self.cursor));
                ui.spacing_mut().item_spacing.x = 5.0;
                let mono = |s: String| egui::RichText::new(s).monospace().size(12.0);
                ui.label(mono(timecode(b - a, fps)));
                ui.label(egui::RichText::new(tr("Longueur")).color(theme::TEXT_DIM()).size(11.0));
                ui.add_space(6.0);
                ui.label(mono(timecode(b, fps)));
                ui.label(egui::RichText::new(tr("Fin")).color(theme::TEXT_DIM()).size(11.0));
                ui.add_space(6.0);
                ui.label(mono(timecode(a, fps)));
                ui.label(egui::RichText::new(tr("Début")).color(theme::TEXT_DIM()).size(11.0));
            });
        });
    }

    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.status).size(11.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(free) = crate::platform::free_space(&self.record_dir()) {
                    // 16-bit stereo at the project sample rate
                    let secs = free as f64 / (self.p.sample_rate as f64 * 4.0);
                    ui.label(egui::RichText::new(format!("{} {} h {:02} min", tr("Enregistrement disponible :"), (secs / 3600.0) as u64, ((secs / 60.0) as u64) % 60)).size(11.0).color(theme::TEXT_DIM()));
                    ui.separator();
                }
                if !self.tasks.is_empty() {
                    for t in &self.tasks {
                        ui.add(egui::ProgressBar::new(*t.progress.lock().unwrap()).desired_width(110.0).text(&t.name));
                    }
                    ui.separator();
                }
                let audio = if self.audio.has_device() { tr("Audio : prêt") } else { tr("Audio : aucun périphérique") };
                ui.label(egui::RichText::new(audio).size(11.0).color(theme::TEXT_DIM()));
                ui.separator();
                let ff = if media::ffmpeg_available() { tr("ffmpeg : trouvé") } else { tr("ffmpeg : absent") };
                ui.label(egui::RichText::new(ff).size(11.0).color(theme::TEXT_DIM()));
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{} Hz ; {} {} ; {} {}",
                        self.p.sample_rate,
                        self.p.tracks.len(),
                        tr("pistes"),
                        tr("durée"),
                        timecode(self.p.end_time(), self.p.fps)
                    ))
                    .size(11.0)
                    .color(theme::TEXT_DIM()),
                );
                if let Some(job) = &self.job {
                    let pr = job.progress.lock().unwrap();
                    if !pr.done {
                        ui.separator();
                        ui.add(egui::ProgressBar::new(pr.fraction).desired_width(140.0).text(format!("{} {:.0} %", tr("Rendu"), pr.fraction * 100.0)));
                    }
                }
            });
        });
    }
}

fn draw_transition_icon(p: &egui::Painter, r: Rect, t: Transition, prog: f32) {
    let a = Color32::from_rgb(60, 110, 190);
    let b = Color32::from_rgb(220, 140, 50);
    let lerp = |x: Color32, y: Color32, f: f32| {
        Color32::from_rgb(
            (x.r() as f32 * (1.0 - f) + y.r() as f32 * f) as u8,
            (x.g() as f32 * (1.0 - f) + y.g() as f32 * f) as u8,
            (x.b() as f32 * (1.0 - f) + y.b() as f32 * f) as u8,
        )
    };
    p.rect_filled(r, 0.0, a);
    let cp = p.with_clip_rect(r);
    match t {
        Transition::Dissolve => {
            cp.rect_filled(r, 0.0, lerp(a, b, prog));
        }
        Transition::WipeLeft => {
            cp.rect_filled(Rect::from_min_max(r.min, pos2(r.left() + r.width() * prog, r.bottom())), 0.0, b);
        }
        Transition::WipeDown => {
            cp.rect_filled(Rect::from_min_max(r.min, pos2(r.right(), r.top() + r.height() * prog)), 0.0, b);
        }
        Transition::Push => {
            let off = r.width() * prog;
            cp.rect_filled(r.translate(vec2(-off, 0.0)), 0.0, a);
            cp.rect_filled(r.translate(vec2(r.width() - off, 0.0)), 0.0, b);
        }
        Transition::Iris => {
            let maxr = (r.width().powi(2) + r.height().powi(2)).sqrt() / 2.0;
            cp.circle_filled(r.center(), maxr * prog, b);
        }
        Transition::ThroughBlack => {
            let c = if prog < 0.5 { lerp(a, Color32::BLACK, prog * 2.0) } else { lerp(Color32::BLACK, b, (prog - 0.5) * 2.0) };
            cp.rect_filled(r, 0.0, c);
        }
        Transition::Blinds => {
            let n = 6;
            let bh = r.height() / n as f32;
            for i in 0..n {
                let y = r.top() + i as f32 * bh;
                cp.rect_filled(Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), bh * prog)), 0.0, b);
            }
        }
        Transition::ClockWipe => {
            let c = r.center();
            let rad = r.width().max(r.height());
            let steps = (prog * 48.0).ceil() as usize;
            let mut pts = vec![c];
            for i in 0..=steps {
                let a_ = (i as f32 / 48.0).min(prog) * std::f32::consts::TAU;
                pts.push(c + vec2(a_.sin(), -a_.cos()) * rad);
            }
            if pts.len() > 2 {
                let mut mesh = egui::Mesh::default();
                for p_ in &pts {
                    mesh.colored_vertex(*p_, b);
                }
                for i in 1..pts.len() as u32 - 1 {
                    mesh.add_triangle(0, i, i + 1);
                }
                cp.add(egui::Shape::mesh(mesh));
            }
        }
        Transition::Flash => {
            let c = if prog < 0.5 { lerp(a, Color32::WHITE, prog * 2.0) } else { lerp(Color32::WHITE, b, (prog - 0.5) * 2.0) };
            cp.rect_filled(r, 0.0, c);
        }
    }
}

fn draw_fx_icon(p: &egui::Painter, r: Rect, fx: &VideoFx) {
    // a small sky / sun / ground scene, tinted by the effect
    let base = [
        (Rect::from_min_max(r.min, pos2(r.right(), r.top() + r.height() * 0.6)), [90u8, 150, 220]),
        (Rect::from_min_max(pos2(r.left(), r.top() + r.height() * 0.6), r.max), [70, 150, 70]),
    ];
    for (rr, c) in base {
        let (mut cr, mut cg, mut cb) = (c[0] as f32, c[1] as f32, c[2] as f32);
        let y = 0.299 * cr + 0.587 * cg + 0.114 * cb;
        match fx {
            VideoFx::BlackWhite { .. } => (cr, cg, cb) = (y, y, y),
            VideoFx::Sepia { .. } => (cr, cg, cb) = ((y * 1.1).min(255.0), y * 0.9, y * 0.7),
            VideoFx::Invert { .. } => (cr, cg, cb) = (255.0 - cr, 255.0 - cg, 255.0 - cb),
            VideoFx::Saturation { .. } => (cr, cg, cb) = ((y + (cr - y) * 1.8).clamp(0.0, 255.0), (y + (cg - y) * 1.8).clamp(0.0, 255.0), (y + (cb - y) * 1.8).clamp(0.0, 255.0)),
            VideoFx::BrightnessContrast { .. } => (cr, cg, cb) = ((cr * 1.3).min(255.0), (cg * 1.3).min(255.0), (cb * 1.3).min(255.0)),
            _ => {}
        }
        p.rect_filled(rr, 0.0, Color32::from_rgb(cr as u8, cg as u8, cb as u8));
    }
    let sun = match fx {
        VideoFx::Invert { .. } => Color32::from_rgb(20, 40, 200),
        VideoFx::BlackWhite { .. } => Color32::from_gray(230),
        _ => Color32::from_rgb(250, 220, 80),
    };
    p.circle_filled(pos2(r.left() + r.width() * 0.7, r.top() + r.height() * 0.3), 9.0, sun);
    match fx {
        VideoFx::Blur { .. } => {
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(200, 200, 200, 90));
        }
        VideoFx::Vignette { .. } => {
            for i in 0..6 {
                let s = i as f32 * 2.0;
                p.rect_stroke(r.shrink(s), 0.0, Stroke::new(2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120 - i * 18)), StrokeKind::Inside);
            }
        }
        _ => {}
    }
}
