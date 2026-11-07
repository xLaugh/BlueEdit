//! Dialogs: preferences, keyboard customisation, script editor, help,
//! slideshow, text to speech, render queue, paste repeat / attributes.

use crate::app::App;
use crate::i18n::tr;
use crate::theme;

impl App {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        self.prefs_window(ctx);
        self.keys_window(ctx);
        self.scripts_window(ctx);
        self.help_window(ctx);
        self.slideshow_window(ctx);
        self.tts_window(ctx);
        self.queue_window(ctx);
        self.paste_windows(ctx);
        self.dialogs2(ctx);
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.win_prefs {
            return;
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new(tr("Préférences")).open(&mut open).resizable(true).default_width(460.0).show(ctx, |ui| {
            let p = &mut self.prefs;
            egui::Grid::new("prefs").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Sauvegarde automatique"));
                save |= ui.checkbox(&mut p.autosave, "").changed();
                ui.end_row();
                ui.label(tr("Intervalle (minutes)"));
                let mut min = (p.autosave_ms / 60_000) as u32;
                if ui.add(egui::DragValue::new(&mut min).range(1..=120)).changed() {
                    p.autosave_ms = min as u64 * 60_000;
                    save = true;
                }
                ui.end_row();
                ui.label(tr("Fichiers récents dans le menu"));
                save |= ui.add(egui::DragValue::new(&mut p.recent_max).range(1..=20)).changed();
                ui.end_row();
                ui.label(tr("Niveaux d'annulation"));
                save |= ui.add(egui::DragValue::new(&mut p.undo_levels).range(10..=1000)).changed();
                ui.end_row();
                ui.label(tr("Éditeur audio externe"));
                ui.horizontal(|ui| {
                    save |= ui.add(egui::TextEdit::singleline(&mut p.sound_editor).desired_width(220.0)).changed();
                    if ui.button("…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().pick_file() {
                            p.sound_editor = f.to_string_lossy().to_string();
                            save = true;
                        }
                    }
                });
                ui.end_row();
                ui.label(tr("Dossier d'enregistrement"));
                ui.horizontal(|ui| {
                    save |= ui.add(egui::TextEdit::singleline(&mut p.record_dir).desired_width(220.0)).changed();
                    if ui.button("…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().pick_folder() {
                            p.record_dir = f.to_string_lossy().to_string();
                            save = true;
                        }
                    }
                });
                ui.end_row();
                ui.label(tr("Enregistrer en Broadcast WAV"));
                save |= ui.checkbox(&mut p.record_bwf, "").changed();
                ui.end_row();
                ui.label(tr("Utiliser les proxys dans l'aperçu"));
                save |= ui.checkbox(&mut p.use_proxies, "").changed();
                ui.end_row();
                ui.label(tr("Couper/copier les événements groupés"));
                save |= ui.checkbox(&mut p.clipboard_on_groups, "").changed();
                ui.end_row();
                ui.label(tr("Afficher les enveloppes"));
                save |= ui.checkbox(&mut p.show_envelopes, "").changed();
                ui.end_row();
                ui.label(tr("Aide « Qu'est-ce que c'est ? »"));
                save |= ui.checkbox(&mut p.whats_this, "").changed();
                ui.end_row();
                ui.label(tr("Masquer les étiquettes d'onglets"));
                save |= ui.checkbox(&mut p.hide_tab_labels, "").changed();
                ui.end_row();
                ui.label(tr("Mesures de décompte du métronome"));
                save |= ui.add(egui::DragValue::new(&mut p.metronome_count_in).range(0..=8)).changed();
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr("Ouvrir le dossier des sauvegardes automatiques")).clicked() {
                    let d = App::autosave_dir();
                    let _ = std::fs::create_dir_all(&d);
                    crate::platform::open_folder(&d);
                }
                if ui.button(tr("Valeurs par défaut")).clicked() {
                    self.prefs = crate::prefs::Prefs::default();
                    save = true;
                }
            });
        });
        if save {
            self.prefs.save();
        }
        if !open {
            self.win_prefs = false;
        }
    }

    fn keys_window(&mut self, ctx: &egui::Context) {
        if !self.win_keys {
            return;
        }
        if let Some(id) = self.key_edit.clone() {
            let pressed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some(crate::keymap::Combo::of(*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some(c) = pressed {
                if c.key != egui::Key::Escape {
                    self.keymap.binds.insert(id, vec![c]);
                    self.keymap.save();
                }
                self.key_edit = None;
            }
        }
        let mut open = true;
        egui::Window::new(tr("Personnaliser le clavier")).open(&mut open).default_size([520.0, 480.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Rechercher :"));
                ui.text_edit_singleline(&mut self.key_filter);
                if ui.button(tr("Rétablir les raccourcis d'origine")).clicked() {
                    self.keymap = crate::keymap::Keymap::defaults();
                    self.keymap.save();
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("keys").striped(true).num_columns(3).show(ui, |ui| {
                    let f = self.key_filter.to_lowercase();
                    for c in crate::keymap::COMMANDS {
                        let label = tr(c.label);
                        if !f.is_empty() && !label.to_lowercase().contains(&f) {
                            continue;
                        }
                        ui.label(label);
                        let txt = if self.key_edit.as_deref() == Some(c.id) { tr("Appuyez sur une touche…").to_string() } else { self.keymap.text_of(c.id) };
                        if ui.button(txt).clicked() {
                            self.key_edit = Some(c.id.to_string());
                        }
                        if ui.small_button(tr("Effacer")).clicked() {
                            self.keymap.binds.insert(c.id.to_string(), vec![]);
                            self.keymap.save();
                        }
                        ui.end_row();
                    }
                });
            });
        });
        if !open {
            self.win_keys = false;
            self.key_edit = None;
        }
    }

    fn scripts_window(&mut self, ctx: &egui::Context) {
        if !self.win_scripts {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new(tr("Éditeur de scripts")).open(&mut open).default_size([560.0, 460.0]).show(ctx, |ui| {
            ui.label(
                egui::RichText::new(tr("Une commande par ligne : « commande:save », « commande:split »… ou un outil de l'assistant suivi de ses paramètres JSON, par ex. add_marker {\"time\": 4, \"name\": \"Intro\"}. Les lignes commençant par # sont des commentaires."))
                    .size(11.0)
                    .color(theme::TEXT_DIM()),
            );
            ui.horizontal(|ui| {
                run = ui.button(tr("Exécuter")).clicked();
                if ui.button(tr("Ouvrir…")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter(tr("Script Blue Edit"), &["bluescript", "txt"]).pick_file() {
                        self.script_text = std::fs::read_to_string(f).unwrap_or_default();
                    }
                }
                if ui.button(tr("Enregistrer…")).clicked() {
                    let dir = crate::plugins::data_dir().join("scripts");
                    let _ = std::fs::create_dir_all(&dir);
                    if let Some(f) = rfd::FileDialog::new().set_directory(&dir).add_filter(tr("Script Blue Edit"), &["bluescript"]).set_file_name("script.bluescript").save_file() {
                        let _ = std::fs::write(f, &self.script_text);
                    }
                }
            });
            ui.add(egui::TextEdit::multiline(&mut self.script_text).code_editor().desired_rows(14).desired_width(f32::INFINITY));
            ui.separator();
            ui.label(egui::RichText::new(&self.script_out).monospace().size(11.0));
        });
        if run {
            let text = self.script_text.clone();
            self.script_out = self.run_script(&text, ctx);
        }
        if !open {
            self.win_scripts = false;
        }
    }

    /// Scripts saved in the data folder (menu Outils > Scripts).
    pub fn script_files() -> Vec<std::path::PathBuf> {
        let dir = crate::plugins::data_dir().join("scripts");
        let mut v: Vec<_> = std::fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "bluescript").unwrap_or(false)).collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// Runs a script (see the script editor for the syntax); returns a log.
    pub fn run_script(&mut self, text: &str, ctx: &egui::Context) -> String {
        let mut log = String::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(cmd) = line.strip_prefix("commande:").or_else(|| line.strip_prefix("command:")) {
                let cmd = cmd.trim();
                if crate::keymap::COMMANDS.iter().any(|c| c.id == cmd) {
                    self.run_command(cmd, ctx);
                    log.push_str(&format!("{}: {} ok\n", n + 1, cmd));
                } else {
                    log.push_str(&format!("{}: {} « {} »\n", n + 1, tr("commande inconnue"), cmd));
                }
                continue;
            }
            let (name, args) = match line.find(char::is_whitespace) {
                Some(i) => (&line[..i], line[i..].trim()),
                None => (line, "{}"),
            };
            let input: serde_json::Value = serde_json::from_str(if args.is_empty() { "{}" } else { args }).unwrap_or(serde_json::json!({}));
            match self.run_tool(name, &input) {
                Ok(r) => log.push_str(&format!("{}: {} → {}\n", n + 1, name, r.chars().take(120).collect::<String>())),
                Err(e) => log.push_str(&format!("{}: {} — {}\n", n + 1, name, e)),
            }
        }
        log
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.win_help {
            return;
        }
        let mut open = true;
        let topics: [(&str, &str); 8] = [
            (tr("Prise en main"), tr("1. Importez vos fichiers (Ctrl+I ou glisser-déposer).\n2. Glissez-les sur la ligne de temps.\n3. Coupez avec S, supprimez avec Suppr, déplacez à la souris.\n4. Ajoutez transitions et effets depuis les onglets du haut.\n5. Rendez la vidéo avec Fichier > Rendre en tant que (Ctrl+R).")),
            (tr("Ligne de temps"), tr("Molette : zoom. Maj+molette : défilement. Glisser un bord : rogner. Coins supérieurs : fondus. Bord supérieur : gain. Ctrl+glisser : copier. Alt+glisser : glisser le contenu (slip). Double-clic : sélectionner l'événement comme région.")),
            (tr("Outils de montage"), tr("D / Maj+D changent d'outil, Ctrl+D revient à l'outil normal. Enveloppe : cliquez pour ajouter un point. Sélection : tracez un rectangle. Zoom : cliquez ou tracez une zone. Étirer : glissez un bord pour changer la vitesse.")),
            (tr("Enveloppes et images clés"), tr("Insertion > Enveloppe (Maj+V volume, Maj+P panoramique). Les images clés de panoramique/recadrage et d'effets se créent dans leurs fenêtres avec « Ajouter une image clé ».")),
            (tr("Audio"), tr("Table de mixage (Alt+3) : volumes, bus, effets d'insertion, automatisation. Effets audio d'un événement : clic droit > Audio de l'événement. Enregistrement : armez une piste puis le bouton rouge.")),
            (tr("Couleur"), tr("Étalonnage (Alt+G) : roues, courbes, LUT .cube. Oscilloscopes (Ctrl+Alt+2) : vectorscope, histogramme, forme d'onde, parade RVB.")),
            (tr("Fenêtres et dispositions"), tr("Glissez les onglets pour réorganiser, les détacher ou les regrouper. Affichage > Dispositions : modèles et 10 dispositions personnelles. F11 réduit la zone d'ancrage.")),
            (tr("Échanges"), tr("Fichier > Importer/Exporter : EDL, FCPXML (Final Cut, DaVinci Resolve), XML (Premiere Pro), SRT. Archiver le projet copie tous les médias dans un dossier.")),
        ];
        egui::Window::new(tr("Aide de Blue Edit")).open(&mut open).default_size([560.0, 380.0]).show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    for (i, (t, _)) in topics.iter().enumerate() {
                        ui.selectable_value(&mut self.help_topic, i, *t);
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    let (t, body) = topics[self.help_topic.min(topics.len() - 1)];
                    ui.heading(t);
                    ui.label(body);
                });
            });
        });
        if !open {
            self.win_help = false;
        }
    }

    fn slideshow_window(&mut self, ctx: &egui::Context) {
        if !self.win_slideshow {
            return;
        }
        let mut open = true;
        let mut go = false;
        egui::Window::new(tr("Diaporama")).open(&mut open).resizable(false).show(ctx, |ui| {
            egui::Grid::new("ss").num_columns(2).show(ui, |ui| {
                ui.label(tr("Durée par image (s)"));
                ui.add(egui::DragValue::new(&mut self.ss_secs).range(0.5..=60.0).speed(0.1));
                ui.end_row();
                ui.label(tr("Chevauchement (fondu, s)"));
                ui.add(egui::DragValue::new(&mut self.ss_overlap).range(0.0..=10.0).speed(0.05));
                ui.end_row();
                ui.label(tr("Zoom lent (effet Ken Burns)"));
                ui.checkbox(&mut self.ss_kb, "");
                ui.end_row();
            });
            go = ui.button(tr("Choisir les images et créer…")).clicked();
        });
        if go {
            if let Some(files) = rfd::FileDialog::new().add_filter("Images", crate::media::IMAGE_EXT).pick_files() {
                let paths = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
                self.slideshow(paths, self.ss_secs, self.ss_overlap, self.ss_kb, ctx);
                open = false;
            }
        }
        if !open {
            self.win_slideshow = false;
        }
    }

    fn tts_window(&mut self, ctx: &egui::Context) {
        if !self.win_tts {
            return;
        }
        let mut open = true;
        let mut go = false;
        egui::Window::new(tr("Synthèse vocale")).open(&mut open).default_width(420.0).show(ctx, |ui| {
            ui.label(tr("Texte à prononcer :"));
            ui.add(egui::TextEdit::multiline(&mut self.tts_text).desired_rows(5).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.label(tr("Voix (vide = voix du système) :"));
                ui.text_edit_singleline(&mut self.prefs.tts_voice);
            });
            go = ui.add_enabled(!self.tts_text.trim().is_empty(), egui::Button::new(tr("Générer et placer au curseur"))).clicked();
            for t in &self.tasks {
                ui.add(egui::ProgressBar::new(*t.progress.lock().unwrap()).text(&t.name));
            }
        });
        if go {
            self.prefs.save();
            self.text_to_speech(self.tts_text.clone(), self.prefs.tts_voice.clone());
            open = false;
        }
        if !open {
            self.win_tts = false;
        }
    }

    fn queue_window(&mut self, ctx: &egui::Context) {
        if !self.win_render_queue {
            return;
        }
        let mut open = true;
        let mut regions = false;
        let mut new_track = false;
        egui::Window::new(tr("File d'attente de rendu")).open(&mut open).default_width(480.0).show(ctx, |ui| {
            if let Some(job) = &self.job {
                let pr = job.progress.lock().unwrap();
                ui.add(egui::ProgressBar::new(pr.fraction).text(format!("{} — {}", job.path, pr.message)));
            }
            ui.separator();
            let mut del = None;
            for (i, q) in self.render_queue.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}. {} → {}", i + 1, q.name, q.path));
                    if ui.small_button("✕").clicked() {
                        del = Some(i);
                    }
                });
            }
            if let Some(i) = del {
                self.render_queue.remove(i);
            }
            if self.render_queue.is_empty() {
                ui.label(egui::RichText::new(tr("Aucun rendu en attente.")).color(theme::TEXT_DIM()));
            }
            ui.separator();
            ui.horizontal(|ui| {
                regions = ui.add_enabled(!self.p.regions.is_empty(), egui::Button::new(tr("Une sortie par région (rendu par lots)…"))).clicked();
                new_track = ui.button(tr("Rendre la sélection vers une nouvelle piste")).clicked();
            });
        });
        if regions {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                let s = self.last_render.clone().unwrap_or_else(|| crate::export::RenderSettings::for_project(&self.p));
                self.queue_regions(&s, &dir);
            }
        }
        if new_track {
            self.render_to_new_track();
        }
        if !open {
            self.win_render_queue = false;
        }
    }

    fn paste_windows(&mut self, ctx: &egui::Context) {
        if self.win_paste_repeat {
            let mut open = true;
            let mut go = false;
            egui::Window::new(tr("Coller plusieurs fois")).open(&mut open).resizable(false).show(ctx, |ui| {
                egui::Grid::new("pr").num_columns(2).show(ui, |ui| {
                    ui.label(tr("Nombre de fois"));
                    ui.add(egui::DragValue::new(&mut self.prefs.paste_repeat_count).range(1..=100));
                    ui.end_row();
                    ui.label(tr("Espace entre les copies (s)"));
                    ui.add(egui::DragValue::new(&mut self.paste_gap).range(0.0..=600.0).speed(0.05));
                    ui.end_row();
                });
                go = ui.button(tr("Coller")).clicked();
            });
            if go {
                let n = self.prefs.paste_repeat_count;
                self.paste_repeat(n, self.paste_gap);
                open = false;
            }
            if !open {
                self.win_paste_repeat = false;
            }
        }
        if self.win_paste_attrs {
            let mut open = true;
            let mut go = false;
            egui::Window::new(tr("Coller les attributs de l'événement")).open(&mut open).resizable(false).show(ctx, |ui| {
                let m = &mut self.attr_mask;
                ui.checkbox(&mut m.fx, tr("Effets vidéo et leurs images clés"));
                ui.checkbox(&mut m.pan, tr("Panoramique/recadrage"));
                ui.checkbox(&mut m.fades, tr("Fondus et courbes"));
                ui.checkbox(&mut m.gain, tr("Gain / opacité"));
                ui.checkbox(&mut m.rate, tr("Vitesse et sens"));
                ui.checkbox(&mut m.audio_fx, tr("Effets audio, hauteur, normalisation"));
                go = ui.button(tr("Coller sur la sélection")).clicked();
            });
            if go {
                self.paste_attributes(self.attr_mask);
                open = false;
            }
            if !open {
                self.win_paste_attrs = false;
            }
        }
    }
}
