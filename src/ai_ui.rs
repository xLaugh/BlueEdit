//! Windows for the AI and plugin features: plugin manager, automatic
//! subtitles, automatic trim (silences and hesitations), Claude assistant.

use crate::i18n::tr;

use crate::ai::{self, JobHandle, RegionKind, TrimParams, TrimResult};
use crate::app::App;
use crate::claude::{self, Chat, Line};
use crate::model::*;
use crate::plugins::{self, PluginManager};
use crate::theme;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AiState {
    pub plugins: PluginManager,
    pub win_plugins: bool,
    pub plugins_tab: u8,
    pub plugin_msg: String,

    pub win_subs: bool,
    pub subs_job: Option<JobHandle>,
    pub subs_lang: String,
    pub subs_size: u32,
    pub subs_loop_only: bool,
    pub subs_offset: f64,
    pub last_subs: Vec<ai::Segment>,
    pub models: Vec<PathBuf>,
    pub model_path: Option<PathBuf>,
    pub dl: Option<(JobHandle, Arc<AtomicBool>)>,

    pub win_trim: bool,
    pub trim_params: TrimParams,
    pub trim_words: Vec<ai::Segment>,
    pub trim_words_for: Option<Id>,
    pub trim_job: Option<(JobHandle, Id)>,
    pub trim_result: Option<(Id, TrimParams, usize, TrimResult)>,

    pub win_claude: bool,
    pub chat: Chat,
}

impl AiState {
    pub fn new() -> AiState {
        let models = ai::find_models();
        AiState {
            plugins: PluginManager::new(),
            win_plugins: false,
            plugins_tab: 0,
            plugin_msg: String::new(),
            win_subs: false,
            subs_job: None,
            subs_lang: "fr".into(),
            subs_size: 6,
            subs_loop_only: false,
            subs_offset: 0.0,
            last_subs: vec![],
            model_path: models.first().cloned(),
            models,
            dl: None,
            win_trim: false,
            trim_params: TrimParams::default(),
            trim_words: vec![],
            trim_words_for: None,
            trim_job: None,
            trim_result: None,
            win_claude: false,
            chat: Chat::new(),
        }
    }
}

fn file_label(p: &PathBuf) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

impl App {
    /// Called every frame: background jobs, then the windows.
    pub fn ai_frame(&mut self, ctx: &egui::Context) {
        self.claude_poll();
        if let Some(job) = self.ai.subs_job.clone() {
            let (done, err, segs) = {
                let j = job.lock().unwrap();
                (j.done, j.error.clone(), j.segments.clone())
            };
            if done {
                self.ai.subs_job = None;
                match err {
                    Some(e) => self.status = format!("Sous-titres : {e}"),
                    None => self.place_subtitles(&segs),
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if let Some((job, id)) = self.ai.trim_job.clone() {
            let (done, err, words) = {
                let j = job.lock().unwrap();
                (j.done, j.error.clone(), j.words.clone())
            };
            if done {
                self.ai.trim_job = None;
                match err {
                    Some(e) => self.status = format!("Transcription : {e}"),
                    None => {
                        self.ai.trim_words = words;
                        self.ai.trim_words_for = Some(id);
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if let Some((job, _)) = self.ai.dl.clone() {
            let (done, path) = {
                let j = job.lock().unwrap();
                (j.done, j.path.clone())
            };
            if done {
                if path.is_some() {
                    self.ai.models = ai::find_models();
                    self.ai.model_path = path;
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        self.plugins_window(ctx);
        self.subs_window(ctx);
        self.trim_window(ctx);
        self.claude_window(ctx);
    }

    // ------------------------------------------------------------ plugins

    fn plugins_window(&mut self, ctx: &egui::Context) {
        if !self.ai.win_plugins {
            return;
        }
        if !self.ai.plugins.scanned {
            self.ai.plugins.rescan_ofx();
        }
        let mut open = true;
        egui::Window::new(tr("Plugins"))
            .open(&mut open)
            .default_size([560.0, 460.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.ai.plugins_tab, 0, tr("Packs d'effets"));
                    ui.selectable_value(&mut self.ai.plugins_tab, 1, format!("Plugins OFX détectés ({})", self.ai.plugins.ofx.len()));
                });
                ui.separator();
                if !self.ai.plugin_msg.is_empty() {
                    ui.label(egui::RichText::new(&self.ai.plugin_msg).color(theme::ACCENT()));
                }
                if self.ai.plugins_tab == 0 {
                    self.packs_tab(ui);
                } else {
                    self.ofx_tab(ui);
                }
            });
        if !open {
            self.ai.win_plugins = false;
        }
    }

    fn packs_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Installer un plugin… (.bluefx)")).clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter(plugins::PLUGIN_FILTER, plugins::PLUGIN_EXTS).pick_file() {
                    self.ai.plugin_msg = match plugins::install(&f) {
                        Ok(n) => format!("Plugin « {n} » installé."),
                        Err(e) => format!("Installation impossible : {e}"),
                    };
                    self.ai.plugins.reload();
                }
            }
            if ui.button(tr("Ouvrir le dossier des plugins")).clicked() {
                let d = plugins::plugins_dir();
                let _ = std::fs::create_dir_all(&d);
                crate::platform::open_folder(&d);
            }
            let sel = self.sel.iter().next().copied();
            let chain = sel.and_then(|id| self.p.event(id)).map(|e| e.fx.clone()).unwrap_or_default();
            if ui
                .add_enabled(!chain.is_empty(), egui::Button::new(tr("Créer un plugin avec les effets de l'événement…")))
                .on_hover_text(tr("Enregistre la chaîne d'effets de l'événement sélectionné dans un fichier .bluefx à partager ou à installer."))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().add_filter(plugins::PLUGIN_FILTER, &[plugins::PLUGIN_EXT]).set_file_name(format!("mon_effet.{}", plugins::PLUGIN_EXT)).save_file() {
                    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Mon effet".into());
                    self.ai.plugin_msg = match plugins::save_pack(&path, &name, &name, chain) {
                        Ok(()) => format!("Plugin enregistré : {}", path.display()),
                        Err(e) => format!("Échec : {e}"),
                    };
                }
            }
        });
        ui.add_space(4.0);
        let mut remove = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (i, pack) in self.ai.plugins.packs.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut on = !pack.disabled;
                        if ui.checkbox(&mut on, "").on_hover_text(tr("Afficher ce pack dans l'onglet Effets vidéo")).changed() {
                            pack.disabled = !on;
                        }
                        ui.strong(&pack.name);
                        ui.label(egui::RichText::new(format!("{} effets", pack.effects.len())).color(theme::TEXT_DIM()));
                        if !pack.author.is_empty() {
                            ui.label(egui::RichText::new(format!("— {} {}", pack.author, pack.version)).color(theme::TEXT_DIM()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !pack.builtin && ui.small_button(tr("Désinstaller")).clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    let names: Vec<&str> = pack.effects.iter().map(|e| e.name.as_str()).collect();
                    ui.label(egui::RichText::new(names.join(" · ")).size(11.0).color(theme::TEXT_DIM()));
                });
            }
        });
        if let Some(i) = remove {
            let pack = self.ai.plugins.packs[i].clone();
            self.ai.plugin_msg = match plugins::uninstall(&pack) {
                Ok(()) => format!("« {} » désinstallé.", pack.name),
                Err(e) => format!("Échec : {e}"),
            };
            self.ai.plugins.reload();
        }
    }

    fn ofx_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Plugins OpenFX installés pour d'autres logiciels de montage. Leur code n'est pas encore chargé par Blue Edit ; \
                 quand un équivalent intégré existe, vous pouvez l'appliquer à la sélection.",
            )
            .size(11.0)
            .color(theme::TEXT_DIM()),
        );
        if ui.button(tr("Rechercher à nouveau")).clicked() {
            self.ai.plugins.rescan_ofx();
        }
        let ofx = self.ai.plugins.ofx.clone();
        let mut apply: Option<Vec<VideoFx>> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if ofx.is_empty() {
                ui.label(tr("Aucun plugin OFX trouvé dans les dossiers OpenFX standard."));
            }
            let mut last_cat = String::new();
            for p in &ofx {
                if p.category != last_cat {
                    ui.add_space(4.0);
                    ui.strong(format!("{} — {}", p.vendor, p.category));
                    last_cat = p.category.clone();
                }
                ui.horizontal(|ui| {
                    ui.label(&p.name);
                    if let Some(eq) = &p.equivalent {
                        let label = format!("Équivalent : {}", eq.iter().map(|f| f.label()).collect::<Vec<_>>().join(" + "));
                        if ui
                            .add_enabled(!self.sel.is_empty(), egui::Button::new(label).small())
                            .on_hover_text(tr("Ajoute l'effet intégré le plus proche aux événements vidéo sélectionnés"))
                            .clicked()
                        {
                            apply = Some(eq.clone());
                        }
                    } else {
                        ui.label(egui::RichText::new(tr("pas d'équivalent")).size(10.5).color(theme::TEXT_DIM()));
                    }
                });
            }
        });
        if let Some(chain) = apply {
            self.add_fx_to_selection(chain);
        }
    }

    pub fn add_fx_to_selection(&mut self, chain: Vec<VideoFx>) {
        self.push_undo();
        let sel = self.sel.clone();
        let mut n = 0;
        let mut first = None;
        for t in &mut self.p.tracks {
            if t.kind != TrackKind::Video {
                continue;
            }
            for e in &mut t.events {
                if sel.contains(&e.id) {
                    e.fx.extend(chain.iter().cloned());
                    first.get_or_insert(e.id);
                    n += 1;
                }
            }
        }
        self.status = format!("Effet ajouté à {n} événement(s) vidéo.");
        if first.is_some() {
            self.win_fx = first;
        }
    }

    // ------------------------------------------------------- model picker

    fn model_picker(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("Modèle :"));
            let cur = self.ai.model_path.as_ref().map(file_label).unwrap_or_else(|| "aucun".into());
            egui::ComboBox::from_id_salt("whisper_model").width(260.0).selected_text(cur).show_ui(ui, |ui| {
                for m in self.ai.models.clone() {
                    let l = file_label(&m);
                    if ui.selectable_label(self.ai.model_path.as_ref() == Some(&m), l).clicked() {
                        self.ai.model_path = Some(m);
                    }
                }
            });
            if ui.button(tr("Parcourir…")).clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Modèle whisper.cpp", &["bin"]).pick_file() {
                    if !self.ai.models.contains(&f) {
                        self.ai.models.push(f.clone());
                    }
                    self.ai.model_path = Some(f);
                }
            }
        });
        if let Some((job, cancel)) = self.ai.dl.clone() {
            let j = job.lock().unwrap();
            if j.done {
                match &j.error {
                    Some(e) => ui.colored_label(Color32::from_rgb(240, 110, 90), e),
                    None => ui.label(&j.message),
                };
            } else {
                ui.horizontal(|ui| {
                    ui.add(egui::ProgressBar::new(j.progress).desired_width(260.0).text(j.message.clone()));
                    if ui.button(tr("Annuler")).clicked() {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            }
        }
        let downloading = self.ai.dl.as_ref().map(|(j, _)| !j.lock().unwrap().done).unwrap_or(false);
        ui.collapsing(tr("Télécharger un modèle de transcription"), |ui| {
            ui.label(egui::RichText::new(tr("Modèles whisper.cpp officiels (Hugging Face). Ils s'installent dans le dossier de Blue Edit.")).size(11.0).color(theme::TEXT_DIM()));
            for (label, file, url) in ai::DOWNLOADS {
                if ui.add_enabled(!downloading, egui::Button::new(format!("Télécharger {label}"))).clicked() {
                    let cancel = Arc::new(AtomicBool::new(false));
                    self.ai.dl = Some((ai::download_model(file, url, cancel.clone()), cancel));
                }
            }
        });
    }

    // ---------------------------------------------------------- subtitles

    fn subs_window(&mut self, ctx: &egui::Context) {
        if !self.ai.win_subs {
            return;
        }
        let mut open = true;
        let mut start = false;
        egui::Window::new(tr("Sous-titres automatiques"))
            .open(&mut open)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(tr("La parole du projet est transcrite sur cet ordinateur (whisper.cpp via ffmpeg) : rien n'est envoyé sur Internet.")).size(11.0).color(theme::TEXT_DIM()));
                ui.add_space(4.0);
                self.model_picker(ui);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr("Langue :"));
                    egui::ComboBox::from_id_salt("subs_lang").selected_text(self.ai.subs_lang.clone()).show_ui(ui, |ui| {
                        for (code, name) in [("fr", "Français"), ("en", "Anglais"), ("es", "Espagnol"), ("de", "Allemand"), ("it", "Italien"), ("pt", "Portugais"), ("auto", "Détection automatique")] {
                            ui.selectable_value(&mut self.ai.subs_lang, code.to_string(), name);
                        }
                    });
                });
                ui.add(egui::Slider::new(&mut self.ai.subs_size, 3..=12).text(tr("Taille du texte (% de la hauteur)")));
                ui.add_enabled(self.p.loop_region.is_some(), egui::Checkbox::new(&mut self.ai.subs_loop_only, tr("Seulement la région de boucle")));
                ui.separator();
                let running = self.ai.subs_job.is_some();
                ui.horizontal(|ui| {
                    if ui.add_enabled(!running && self.ai.model_path.is_some(), egui::Button::new(tr("Générer les sous-titres"))).clicked() {
                        start = true;
                    }
                    if running {
                        ui.spinner();
                        ui.label(tr("Transcription en cours…"));
                    }
                    if !self.ai.last_subs.is_empty() && ui.button(tr("Exporter en .srt…")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres SubRip", &["srt"]).set_file_name(format!("{}.srt", self.p.name)).save_file() {
                            self.status = match std::fs::write(&p, ai::to_srt(&self.ai.last_subs)) {
                                Ok(()) => format!("Sous-titres exportés : {}", p.display()),
                                Err(e) => format!("Échec de l'export : {e}"),
                            };
                        }
                    }
                });
                if self.ai.model_path.is_none() {
                    ui.colored_label(Color32::from_rgb(240, 190, 90), tr("Choisissez ou téléchargez d'abord un modèle."));
                }
            });
        if start {
            let lang = self.ai.subs_lang.clone();
            if let Err(e) = self.start_subtitles(lang) {
                self.status = format!("Sous-titres : {e}");
            }
        }
        if !open {
            self.ai.win_subs = false;
        }
    }

    // ----------------------------------------------------------- auto trim

    fn trim_target(&self) -> Option<Id> {
        let mut ids: Vec<Id> = self.sel.iter().copied().collect();
        ids.sort();
        ids.into_iter().find(|id| self.audio_partner(*id).is_some())
    }

    fn trim_window(&mut self, ctx: &egui::Context) {
        if !self.ai.win_trim {
            return;
        }
        let mut open = true;
        let target = self.trim_target();
        let mut apply = false;
        let mut transcribe = false;
        egui::Window::new(tr("Coupe automatique (silences et hésitations)"))
            .open(&mut open)
            .default_width(620.0)
            .resizable(false)
            .show(ctx, |ui| {
                let Some(id) = target else {
                    ui.label(tr("Sélectionnez un événement qui contient du son (une prise filmée, une voix…)."));
                    return;
                };
                let name = self.p.event(id).map(|e| e.name.clone()).unwrap_or_default();
                ui.label(format!("Événement : {name}"));
                let p = &mut self.ai.trim_params;
                let mut auto = p.threshold_db.is_none();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut auto, tr("Seuil automatique"));
                    let mut v = p.threshold_db.unwrap_or(-40.0);
                    if ui.add_enabled(!auto, egui::Slider::new(&mut v, -70.0..=-10.0).text(tr("dB"))).changed() || !auto {
                        p.threshold_db = Some(v);
                    }
                    if auto {
                        p.threshold_db = None;
                    }
                });
                ui.add(egui::Slider::new(&mut p.min_silence, 0.1..=3.0).text(tr("Silence minimum (s)")));
                ui.add(egui::Slider::new(&mut p.margin, 0.0..=0.5).text(tr("Marge autour de la parole (s)")));
                ui.add(egui::Slider::new(&mut p.min_keep, 0.05..=1.0).text(tr("Segment gardé minimum (s)")));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut p.cut_fillers, tr("Couper les hésitations (euh, heu, hum, mmh)"));
                    ui.label(tr("Autres mots :"));
                    ui.add(egui::TextEdit::singleline(&mut p.extra_fillers).hint_text(tr("ben, bah…")).desired_width(120.0));
                });
                let have_words = self.ai.trim_words_for == Some(id);
                ui.horizontal(|ui| {
                    if self.ai.trim_job.is_some() {
                        ui.spinner();
                        ui.label(tr("Recherche des hésitations (transcription)…"));
                    } else if have_words {
                        ui.label(format!("{} mots transcrits.", self.ai.trim_words.len()));
                    } else if ui
                        .add_enabled(self.ai.model_path.is_some(), egui::Button::new(tr("Trouver les hésitations (transcription locale)")))
                        .clicked()
                    {
                        transcribe = true;
                    }
                    if self.ai.model_path.is_none() {
                        ui.label(egui::RichText::new(tr("modèle requis : voir IA > Sous-titres automatiques")).size(11.0).color(theme::TEXT_DIM()));
                    }
                });
                // analysis (recomputed when settings change)
                let words = if have_words { self.ai.trim_words.clone() } else { vec![] };
                let params = self.ai.trim_params.clone();
                let fresh = match &self.ai.trim_result {
                    Some((rid, rp, n, _)) => *rid == id && *rp == params && *n == words.len(),
                    None => false,
                };
                if !fresh {
                    self.ai.trim_result = self.analyze_trim(id, &params, &words).map(|r| (id, params.clone(), words.len(), r));
                }
                let Some((_, _, _, r)) = &self.ai.trim_result else {
                    ui.label(tr("Le son de cet événement n'est pas encore chargé."));
                    return;
                };
                draw_trim(ui, r);
                let removed = r.duration - r.kept();
                ui.label(format!(
                    "Durée {:.1} s — gardé {:.1} s — retiré {:.1} s ({:.0} %) — {} segments — {} hésitation(s) — seuil {:.1} dB",
                    r.duration,
                    r.kept(),
                    removed,
                    if r.duration > 0.0 { removed / r.duration * 100.0 } else { 0.0 },
                    r.keep.len(),
                    r.fillers,
                    r.threshold_db
                ));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr("■ gardé")).color(Color32::from_rgb(90, 190, 110)));
                    ui.label(egui::RichText::new(tr("■ silence")).color(Color32::from_rgb(200, 80, 70)));
                    ui.label(egui::RichText::new(tr("■ hésitation")).color(Color32::from_rgb(235, 150, 40)));
                });
                ui.separator();
                if ui.button(tr("Appliquer la coupe")).clicked() {
                    apply = true;
                }
            });
        if transcribe {
            if let (Some(id), Some(model)) = (target, self.ai.model_path.clone()) {
                if let Some(wav) = self.event_wav(id) {
                    self.ai.trim_job = Some((ai::transcribe_job(wav, model, self.ai.subs_lang.clone(), false, true), id));
                }
            }
        }
        if apply {
            if let (Some(id), Some((_, _, _, r))) = (target, self.ai.trim_result.clone()) {
                self.apply_trim(id, &r.keep);
                self.ai.trim_result = None;
                self.ai.trim_words_for = None;
                self.ai.win_trim = false;
            }
        }
        if !open {
            self.ai.win_trim = false;
        }
    }

    // -------------------------------------------------------------- Claude

    fn claude_window(&mut self, ctx: &egui::Context) {
        if !self.ai.win_claude {
            return;
        }
        let mut open = true;
        let mut send: Option<String> = None;
        egui::Window::new(tr("Assistant Claude"))
            .open(&mut open)
            .default_size([400.0, 560.0])
            .default_pos(pos2(ctx.screen_rect().right() - 430.0, 80.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} {}", tr("Modèle :"), claude::MODEL)).size(11.0).color(theme::TEXT_DIM()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(tr("Nouvelle conversation")).clicked() {
                            self.ai.chat.reset();
                        }
                        if ui.small_button(tr("Clé d'API")).clicked() {
                            self.ai.chat.show_key = !self.ai.chat.show_key;
                        }
                    });
                });
                if self.ai.chat.key.trim().is_empty() || self.ai.chat.show_key {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(tr("Clé d'API Claude (créée sur console.anthropic.com) :"));
                        if self.ai.chat.key_from_env {
                            ui.label(egui::RichText::new(tr("Clé lue dans la variable ANTHROPIC_API_KEY.")).size(11.0).color(theme::TEXT_DIM()));
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.ai.chat.key).password(true).desired_width(250.0));
                            if ui.button(tr("Enregistrer")).clicked() {
                                self.status = match claude::save_key(&self.ai.chat.key) {
                                    Ok(()) => "Clé d'API enregistrée sur cet ordinateur.".into(),
                                    Err(e) => format!("Impossible d'enregistrer la clé : {e}"),
                                };
                                self.ai.chat.show_key = false;
                            }
                        });
                    });
                }
                let input_h = 70.0;
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink(false)
                    .max_height(ui.available_height() - input_h)
                    .show(ui, |ui| {
                        if self.ai.chat.lines.is_empty() {
                            ui.label(egui::RichText::new(
                                "Demandez par exemple :\n• « Coupe les silences de ma prise »\n• « Ajoute un titre \"Chapitre 2\" à 12 s »\n• « Mets un fondu enchaîné entre tous les plans »\n• « Génère les sous-titres en français »",
                            ).color(theme::TEXT_DIM()));
                        }
                        for l in &self.ai.chat.lines {
                            match l {
                                Line::User(t) => {
                                    egui::Frame::new().fill(Color32::from_rgb(52, 76, 108)).corner_radius(6).inner_margin(6).show(ui, |ui| {
                                        ui.label(t);
                                    });
                                }
                                Line::Assistant(t) => {
                                    egui::Frame::new().fill(Color32::from_rgb(60, 60, 64)).corner_radius(6).inner_margin(6).show(ui, |ui| {
                                        ui.label(t);
                                    });
                                }
                                Line::Tool(t) => {
                                    ui.label(egui::RichText::new(t).size(11.0).monospace().color(theme::TEXT_DIM()));
                                }
                                Line::Error(t) => {
                                    ui.colored_label(Color32::from_rgb(240, 110, 90), t);
                                }
                            }
                            ui.add_space(3.0);
                        }
                        if self.ai.chat.busy() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new(tr("Claude réfléchit…")).color(theme::TEXT_DIM()));
                            });
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let te = ui.add(
                        egui::TextEdit::multiline(&mut self.ai.chat.input)
                            .desired_rows(2)
                            .desired_width(ui.available_width() - 80.0)
                            .hint_text(tr("Votre demande… (Entrée pour envoyer)")),
                    );
                    let enter = te.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                    let can = !self.ai.chat.busy() && !self.ai.chat.input.trim().is_empty();
                    if (ui.add_enabled(can, egui::Button::new(tr("Envoyer"))).clicked() || (enter && can)) && can {
                        send = Some(std::mem::take(&mut self.ai.chat.input).trim().to_string());
                    }
                });
            });
        if let Some(t) = send {
            self.claude_send_user(t);
        }
        if !open {
            self.ai.win_claude = false;
        }
    }
}

/// Level curve with kept / silence / hesitation regions (like CrispTrim's timeline).
fn draw_trim(ui: &mut egui::Ui, r: &TrimResult) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Color32::from_rgb(22, 22, 25));
    if r.duration <= 0.0 {
        return;
    }
    let x = |t: f64| rect.left() + (t / r.duration) as f32 * rect.width();
    for &(a, b, k) in &r.regions {
        let rr = Rect::from_min_max(pos2(x(a), rect.top()), pos2(x(b).max(x(a) + 1.0), rect.bottom()));
        let c = match k {
            RegionKind::Keep => Color32::from_rgba_unmultiplied(90, 190, 110, 50),
            RegionKind::Silence => Color32::from_rgba_unmultiplied(200, 80, 70, 70),
            RegionKind::Filler => Color32::from_rgba_unmultiplied(235, 150, 40, 120),
        };
        p.rect_filled(rr, 0.0, c);
        if k == RegionKind::Silence {
            // hatching
            let mut xx = rr.left();
            while xx < rr.right() {
                p.line_segment([pos2(xx, rr.bottom()), pos2((xx + 8.0).min(rr.right()), rr.top())], Stroke::new(1.0, Color32::from_rgba_unmultiplied(220, 90, 80, 90)));
                xx += 6.0;
            }
        }
    }
    let n = r.levels.len();
    if n > 1 {
        let y = |db: f32| rect.bottom() - ((db + 70.0) / 70.0).clamp(0.0, 1.0) * rect.height();
        let cols = rect.width() as usize;
        let mut pts = Vec::with_capacity(cols);
        for c in 0..cols {
            let i0 = c * n / cols;
            let i1 = ((c + 1) * n / cols).max(i0 + 1).min(n);
            let m = r.levels[i0..i1].iter().cloned().fold(-120.0f32, f32::max);
            pts.push(pos2(rect.left() + c as f32, y(m)));
        }
        p.add(egui::Shape::line(pts, Stroke::new(1.0, theme::WAVE())));
        let ty = y(r.threshold_db);
        p.line_segment([pos2(rect.left(), ty), pos2(rect.right(), ty)], Stroke::new(1.0, Color32::from_rgb(240, 200, 90)));
    }
}
