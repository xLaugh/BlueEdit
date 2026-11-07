//! Menu bar and toolbars (main toolbar, timeline toolbar).

use crate::app::{App, LeftTab, TOOLBAR_ITEMS};
use crate::docks::{Preset, Tab};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::model::*;
use crate::tools::{EditTool, EnvKind, RulerFormat};

impl App {
    /// Menu entry showing its current shortcut; runs the command when clicked.
    fn mi(&mut self, ui: &mut egui::Ui, label: &str, cmd: &str, ctx: &egui::Context) {
        let keys = self.keymap.text_of(cmd);
        if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
            self.run_command(cmd, ctx);
            ui.close_menu();
        }
    }

    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button(tr("Fichier"), |ui| self.file_menu(ui, ctx));
            ui.menu_button(tr("Édition"), |ui| self.edit_menu(ui, ctx));
            ui.menu_button(tr("Affichage"), |ui| self.view_menu(ui, ctx));
            ui.menu_button(tr("Insertion"), |ui| self.insert_menu(ui, ctx));
            ui.menu_button(tr("Outils"), |ui| self.tools_menu(ui, ctx));
            ui.menu_button(tr("Options"), |ui| self.options_menu(ui, ctx));
            ui.menu_button(tr("IA"), |ui| {
                if ui.button(tr("Assistant Claude…")).clicked() {
                    self.ai.win_claude = true;
                    ui.close_menu();
                }
                if ui.button(tr("Sous-titres automatiques…")).clicked() {
                    self.ai.win_subs = true;
                    ui.close_menu();
                }
                if ui.button(tr("Coupe automatique (silences et hésitations)…")).clicked() {
                    self.ai.win_trim = true;
                    ui.close_menu();
                }
                if ui.button(tr("Synthèse vocale…")).clicked() {
                    self.win_tts = true;
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("Plugins"), |ui| {
                if ui.button(tr("Gérer les plugins…")).clicked() {
                    self.ai.win_plugins = true;
                    self.ai.plugins_tab = 0;
                    ui.close_menu();
                }
                if ui.button(tr("Installer un plugin…")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter(crate::plugins::PLUGIN_FILTER, crate::plugins::PLUGIN_EXTS).pick_file() {
                        self.ai.plugin_msg = match crate::plugins::install(&f) {
                            Ok(n) => format!("Plugin « {n} » installé."),
                            Err(e) => format!("Installation impossible : {e}"),
                        };
                        self.ai.plugins.reload();
                        self.ai.win_plugins = true;
                    }
                    ui.close_menu();
                }
                if ui.button(tr("Plugins OFX détectés…")).clicked() {
                    self.ai.win_plugins = true;
                    self.ai.plugins_tab = 1;
                    ui.close_menu();
                }
                if ui.button(tr("Gestionnaire de plugins (fenêtre ancrée)")).clicked() {
                    self.show_tab(Tab::Plugins);
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("Aide"), |ui| {
                self.mi(ui, tr("Aide de Blue Edit…"), "help", ctx);
                self.mi(ui, tr("Qu'est-ce que c'est ?"), "whats_this", ctx);
                if ui.button(tr("Raccourcis et à propos…")).clicked() {
                    self.win_about = true;
                    ui.close_menu();
                }
            });
            // brand mark: the "B" logo and the short name "Blue"
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let id = egui::Id::new("blue_logo_texture");
                let tex = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                    let t = ui.ctx().load_texture("blue_logo", crate::app_icon::color_image(64), egui::TextureOptions::LINEAR);
                    ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
                    t
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Blue").strong().size(13.0).color(egui::Color32::from_rgb(120, 175, 255))).on_hover_text("Blue Edit");
                ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(18.0, 18.0)));
                if self.recorder.is_some() {
                    ui.label(egui::RichText::new(format!("● {} {}", tr("Enregistrement"), crate::panels::fr_num(self.recorder.as_ref().map(|r| r.seconds()).unwrap_or(0.0), 1))).color(egui::Color32::from_rgb(230, 70, 60)));
                }
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.mi(ui, tr("Nouveau"), "new", ctx);
        self.mi(ui, tr("Ouvrir…"), "open", ctx);
        ui.menu_button(tr("Fichiers récents"), |ui| {
            if self.recent.is_empty() {
                ui.label(tr("(aucun)"));
            }
            for p in self.recent.clone() {
                let label = if self.prefs.recent_full_path { p.clone() } else { std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone()) };
                if ui.button(label).clicked() {
                    self.open_path(&p, ctx);
                    ui.close_menu();
                }
            }
        });
        self.mi(ui, tr("Enregistrer"), "save", ctx);
        self.mi(ui, tr("Enregistrer sous…"), "save_as", ctx);
        self.mi(ui, tr("Enregistrement incrémental"), "save_inc", ctx);
        if ui.button(tr("Archiver le projet (copier tous les médias)…")).clicked() {
            self.archive_project();
            ui.close_menu();
        }
        ui.separator();
        self.mi(ui, tr("Importer un média…"), "import", ctx);
        ui.menu_button(tr("Importer"), |ui| {
            if ui.button(tr("Média depuis un autre projet…")).clicked() {
                self.import_project(ctx);
                ui.close_menu();
            }
            if ui.button(tr("Projet comme timeline imbriquée…")).clicked() {
                self.add_nested_project(ctx);
                ui.close_menu();
            }
            if ui.button(tr("EDL, FCPXML, XML ou sous-titres SRT…")).clicked() {
                self.import_interchange(ctx);
                ui.close_menu();
            }
            if ui.button(tr("Broadcast WAV (placé à son horodatage)…")).clicked() {
                self.import_bwf(ctx);
                ui.close_menu();
            }
        });
        ui.menu_button(tr("Exporter"), |ui| {
            for (k, l) in [
                ("edl", tr("Liste de montage EDL (CMX 3600)…")),
                ("fcpxml", tr("FCPXML (Final Cut Pro, DaVinci Resolve)…")),
                ("xml", tr("XML (Premiere Pro, Final Cut 7)…")),
                ("srt", tr("Sous-titres SRT…")),
            ] {
                if ui.button(l).clicked() {
                    self.export_interchange(k);
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        self.mi(ui, tr("Rendre en tant que…"), "render", ctx);
        self.mi(ui, tr("Rendre vers une nouvelle piste"), "render_new_track", ctx);
        if ui.button(tr("File d'attente / rendu par lots…")).clicked() {
            self.win_render_queue = true;
            ui.close_menu();
        }
        ui.separator();
        self.mi(ui, tr("Propriétés du projet…"), "project_props", ctx);
        ui.separator();
        if ui.button(tr("Quitter")).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_enabled_ui(!self.undo.is_empty(), |ui| self.mi(ui, tr("Annuler"), "undo", ctx));
        ui.add_enabled_ui(!self.redo.is_empty(), |ui| self.mi(ui, tr("Rétablir"), "redo", ctx));
        if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(tr("Tout annuler"))).clicked() {
            let n = self.undo.len();
            self.undo_steps(n);
            ui.close_menu();
        }
        if ui.button(tr("Historique des modifications")).clicked() {
            self.show_tab(Tab::History);
            ui.close_menu();
        }
        if ui.button(tr("Effacer l'historique")).clicked() {
            self.clear_history();
            ui.close_menu();
        }
        ui.separator();
        let has_sel = !self.sel.is_empty();
        let has_clip = !self.clipboard.is_empty();
        ui.add_enabled_ui(has_sel, |ui| {
            self.mi(ui, tr("Couper"), "cut", ctx);
            self.mi(ui, tr("Copier"), "copy", ctx);
        });
        ui.add_enabled_ui(has_clip, |ui| {
            self.mi(ui, tr("Coller"), "paste", ctx);
            self.mi(ui, tr("Coller en insérant"), "paste_insert", ctx);
            self.mi(ui, tr("Coller plusieurs fois…"), "paste_repeat", ctx);
            self.mi(ui, tr("Coller les attributs de l'événement…"), "paste_attrs", ctx);
        });
        ui.add_enabled_ui(has_sel, |ui| self.mi(ui, tr("Supprimer"), "delete", ctx));
        ui.separator();
        self.mi(ui, tr("Diviser"), "split", ctx);
        self.mi(ui, tr("Recoller (heal)"), "heal", ctx);
        self.mi(ui, tr("Rogner à la sélection"), "trim", ctx);
        self.mi(ui, tr("Rogner le début au curseur"), "trim_start", ctx);
        self.mi(ui, tr("Rogner la fin au curseur"), "trim_end", ctx);
        ui.menu_button(tr("Prises"), |ui| {
            self.mi(ui, tr("Prise suivante"), "take_next", ctx);
            self.mi(ui, tr("Prise précédente"), "take_prev", ctx);
            if ui.button(tr("Supprimer la prise active")).clicked() {
                self.delete_take();
                ui.close_menu();
            }
            ui.menu_button(tr("Ajouter une prise depuis le média…"), |ui| {
                for m in self.p.media.clone() {
                    if ui.button(&m.name).clicked() {
                        self.add_take(m.id);
                        ui.close_menu();
                    }
                }
            });
        });
        ui.menu_button(tr("Outil de montage"), |ui| {
            for t in EditTool::ALL {
                if ui.selectable_label(self.tool == t, t.label()).on_hover_text(t.tip()).clicked() {
                    self.tool = t;
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        self.mi(ui, tr("Verrouiller / déverrouiller"), "lock", ctx);
        self.mi(ui, tr("Décaler d'une image à gauche"), "nudge_left", ctx);
        self.mi(ui, tr("Décaler d'une image à droite"), "nudge_right", ctx);
        self.mi(ui, tr("Échanger curseur et sélection"), "swap_cursor", ctx);
        self.mi(ui, tr("Événement semblable suivant"), "next_like", ctx);
        if ui.button(tr("Événement semblable précédent")).clicked() {
            self.select_similar(false);
            ui.close_menu();
        }
        ui.separator();
        self.mi(ui, tr("Sélectionner tout"), "select_all", ctx);
        self.mi(ui, tr("Grouper"), "group", ctx);
        self.mi(ui, tr("Retirer du groupe"), "ungroup", ctx);
        ui.checkbox(&mut self.ripple, tr("Montage en cascade automatique (Ctrl+L)"));
    }

    fn view_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button(tr("Fenêtres"), |ui| {
            for t in Tab::ALL {
                if ui.selectable_label(self.tab_open(t), t.label()).clicked() {
                    self.toggle_tab(t);
                    ui.close_menu();
                }
            }
        });
        ui.menu_button(tr("Dispositions"), |ui| {
            for p in Preset::ALL {
                if ui.button(p.label()).clicked() {
                    self.load_preset(p);
                    ui.close_menu();
                }
            }
            ui.separator();
            ui.menu_button(tr("Charger une disposition personnelle"), |ui| {
                for n in 0..10 {
                    if ui.add_enabled(App::layout_exists(n), egui::Button::new(format!("{} {}", tr("Disposition"), n))).clicked() {
                        self.load_layout(n);
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button(tr("Enregistrer la disposition sous"), |ui| {
                for n in 0..10 {
                    if ui.button(format!("{} {}", tr("Disposition"), n)).clicked() {
                        self.save_layout(n);
                        ui.close_menu();
                    }
                }
            });
        });
        self.mi(ui, tr("Réduire / rétablir la zone d'ancrage"), "dock_toggle", ctx);
        self.mi(ui, tr("Masquer les étiquettes d'onglets"), "labels_toggle", ctx);
        if ui.checkbox(&mut self.timeline_top, tr("Ligne de temps en haut")).changed() {
            ui.close_menu();
        }
        if ui.checkbox(&mut self.ext_monitor, tr("Moniteur externe (fenêtre d'aperçu séparée)")).changed() {
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut self.show_safe, tr("Zones de sécurité"));
        ui.checkbox(&mut self.show_grid, tr("Grille de l'aperçu"));
        if ui.checkbox(&mut self.prefs.show_envelopes, tr("Afficher les enveloppes")).changed() {
            self.prefs.save();
        }
        ui.menu_button(tr("Zoom de l'aperçu"), |ui| {
            for (z, l) in [(0.0, tr("Ajuster")), (0.5, "50 %"), (1.0, "100 %"), (2.0, "200 %")] {
                if ui.selectable_label((self.preview_zoom - z).abs() < 1e-3, l).clicked() {
                    self.preview_zoom = z;
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        self.mi(ui, tr("Zoom avant"), "zoom_in", ctx);
        self.mi(ui, tr("Zoom arrière"), "zoom_out", ctx);
        if ui.button(tr("Afficher tout le projet")).clicked() {
            self.tl.fit(self.p.end_time().max(10.0));
            ui.close_menu();
        }
        ui.menu_button(tr("Hauteur des pistes"), |ui| {
            self.mi(ui, tr("Hauteur minimale"), "min_tracks", ctx);
            self.mi(ui, tr("Hauteur par défaut"), "default_tracks", ctx);
            if ui.button(tr("Agrandir la piste active")).clicked() {
                self.track_heights(2);
                ui.close_menu();
            }
        });
        ui.separator();
        for (tab, name) in [
            (LeftTab::ProjectMedia, tr("Média du projet")),
            (LeftTab::Explorer, tr("Explorateur")),
            (LeftTab::Transitions, tr("Transitions")),
            (LeftTab::VideoFx, tr("Effets vidéo")),
            (LeftTab::Generators, tr("Générateurs de médias")),
        ] {
            if ui.selectable_label(self.left_tab == tab, name).clicked() {
                self.left_tab = tab;
                let t = match tab {
                    LeftTab::ProjectMedia => Tab::Media,
                    LeftTab::Explorer => Tab::Explorer,
                    LeftTab::Transitions => Tab::Transitions,
                    LeftTab::VideoFx => Tab::VideoFx,
                    LeftTab::Generators => Tab::Generators,
                };
                self.show_tab(t);
                ui.close_menu();
            }
        }
    }

    fn insert_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.mi(ui, tr("Piste audio"), "add_audio_track", ctx);
        self.mi(ui, tr("Piste vidéo"), "add_video_track", ctx);
        self.mi(ui, tr("Piste d'ajustement"), "add_adjust_track", ctx);
        if ui.button(tr("Bus audio")).clicked() {
            self.push_undo();
            let n = self.p.buses.len();
            self.p.buses.push(Bus { name: format!("Bus {}", (b'A' + (n % 26) as u8) as char), level: 1.0, pan: 0.0, mute: false });
            self.show_tab(Tab::Mixer);
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr("Événement vide")).clicked() {
            self.insert_empty_event(ctx);
            ui.close_menu();
        }
        if ui.button(tr("Événement d'ajustement")).clicked() {
            self.insert_adjustment_event(ctx);
            ui.close_menu();
        }
        ui.menu_button(tr("Enveloppe"), |ui| {
            for k in EnvKind::ALL {
                if ui.button(k.label()).clicked() {
                    self.insert_envelope(k);
                    ui.close_menu();
                }
            }
            if ui.button(tr("Progression de la transition")).clicked() {
                let ids: Vec<Id> = self.sel.iter().copied().collect();
                self.push_undo();
                for id in ids {
                    if let Some(e) = self.p.event_mut(id) {
                        if e.transition_env.is_empty() {
                            e.transition_env = vec![EnvPoint { t: 0.0, v: 0.0, curve: Curve::Smooth }, EnvPoint { t: 1.0, v: 1.0, curve: Curve::Linear }];
                        }
                    }
                }
                ui.close_menu();
            }
            ui.separator();
            for k in EnvKind::ALL {
                if ui.button(format!("{} {}", tr("Retirer :"), k.label())).clicked() {
                    self.clear_envelope(k);
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        self.mi(ui, tr("Marqueur"), "marker", ctx);
        self.mi(ui, tr("Région"), "region", ctx);
        self.mi(ui, tr("Marqueur de commande"), "command_marker", ctx);
        ui.separator();
        if ui.button(tr("Texte")).clicked() {
            let id = self.add_generator(crate::app::default_text(), ctx);
            self.insert_media(id, self.cursor, None, true);
            self.win_gen = Some(id);
            ui.close_menu();
        }
        if ui.button(tr("Titre animé")).clicked() {
            let id = self.add_generator(Generator::Title { text: tr("Mon titre").into(), size_pct: 12, color: [255, 255, 255, 255], anim: TitleAnim::SlideUp, anim_ms: 800 }, ctx);
            self.insert_media(id, self.cursor, None, true);
            self.win_gen = Some(id);
            ui.close_menu();
        }
        if ui.button(tr("Diaporama…")).clicked() {
            self.win_slideshow = true;
            ui.close_menu();
        }
        if ui.button(tr("Média généré…")).clicked() {
            self.left_tab = LeftTab::Generators;
            self.show_tab(Tab::Generators);
            ui.close_menu();
        }
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has_sel = !self.sel.is_empty();
        let first = self.sel.iter().next().copied();
        ui.menu_button(tr("Vidéo"), |ui| {
            if ui.add_enabled(has_sel, egui::Button::new(tr("Effets vidéo de l'événement…"))).clicked() {
                self.win_fx = first;
                ui.close_menu();
            }
            if ui.add_enabled(has_sel, egui::Button::new(tr("Panoramique/Recadrage de l'événement…"))).clicked() {
                self.win_pan = first;
                ui.close_menu();
            }
            if ui.button(tr("Effets de piste vidéo…")).clicked() {
                self.win_track_fx = Some(self.focus_track);
                ui.close_menu();
            }
            if ui.button(tr("Mouvement de piste…")).clicked() {
                self.win_track_motion = Some(self.focus_track);
                ui.close_menu();
            }
            if ui.button(tr("Effets de sortie vidéo…")).clicked() {
                self.win_output_fx = true;
                ui.close_menu();
            }
            if ui.add_enabled(has_sel, egui::Button::new(tr("Effets du média…"))).clicked() {
                self.win_media_props = first.and_then(|id| self.p.event(id)).map(|e| e.media);
                ui.close_menu();
            }
            if ui.add_enabled(has_sel, egui::Button::new(tr("Stabilisation et suivi de mouvement…"))).clicked() {
                self.win_tracking = first;
                ui.close_menu();
            }
        });
        ui.menu_button(tr("Audio"), |ui| {
            if ui.add_enabled(has_sel, egui::Button::new(tr("Audio de l'événement (effets, vitesse, hauteur)…"))).clicked() {
                self.win_event_audio = first;
                ui.close_menu();
            }
            if ui.button(tr("Effets de piste audio…")).clicked() {
                self.win_track_fx = Some(self.focus_track);
                ui.close_menu();
            }
            if ui.add_enabled(has_sel, egui::Button::new(tr("Ouvrir dans l'éditeur audio"))).clicked() {
                if let Some(id) = first {
                    self.open_in_sound_editor(id);
                }
                ui.close_menu();
            }
        });
        ui.separator();
        self.mi(ui, tr("Construire l'aperçu RAM"), "ram_preview", ctx);
        if ui.button(tr("Effacer l'aperçu RAM")).clicked() {
            self.ram_cache.clear();
            ui.close_menu();
        }
        if ui.button(tr("Créer les proxys des vidéos")).clicked() {
            self.make_proxies();
            ui.close_menu();
        }
        if ui.button(tr("Nettoyer les médias inutilisés")).clicked() {
            self.remove_unused_media();
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr("Montage multicaméra…")).clicked() {
            self.win_multicam = true;
            ui.close_menu();
        }
        if ui.button(tr("Synchroniser par l'audio")).clicked() {
            self.sync_by_audio();
            ui.close_menu();
        }
        if ui.button(tr("Synthèse vocale…")).clicked() {
            self.win_tts = true;
            ui.close_menu();
        }
        if ui.button(tr("Générer le journal de sonie")).clicked() {
            self.show_tab(Tab::Loudness);
            ui.close_menu();
        }
        ui.separator();
        ui.menu_button(tr("Scripts"), |ui| {
            if ui.button(tr("Éditeur de scripts…")).clicked() {
                self.win_scripts = true;
                ui.close_menu();
            }
            ui.separator();
            for f in App::script_files() {
                let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if ui.button(&name).clicked() {
                    let text = std::fs::read_to_string(&f).unwrap_or_default();
                    self.script_out = self.run_script(&text, ctx);
                    self.status = format!("{} {}", tr("Script exécuté :"), name);
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        if ui.button(tr("Enregistrer un instantané de l'aperçu…")).clicked() {
            self.save_snapshot();
            ui.close_menu();
        }
    }

    fn options_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.button(tr("Préférences…")).clicked() {
            self.win_prefs = true;
            ui.close_menu();
        }
        if ui.button(tr("Personnaliser le clavier…")).clicked() {
            self.win_keys = true;
            ui.close_menu();
        }
        if ui.button(tr("Personnaliser les barres d'outils…")).clicked() {
            self.win_customize = true;
            ui.close_menu();
        }
        ui.separator();
        ui.checkbox(&mut self.snap, tr("Activer l'aimantation (F8)"));
        ui.add_enabled(self.snap, egui::Checkbox::new(&mut self.snap_grid, tr("Aimanter à la grille (Ctrl+F8)")));
        ui.checkbox(&mut self.quantize, tr("Quantifier aux images (Alt+F8)"));
        if ui.checkbox(&mut self.auto_xf, tr("Fondu enchaîné automatique (Ctrl+Maj+X)")).changed() {
            self.touch();
        }
        ui.checkbox(&mut self.ripple, tr("Montage en cascade automatique (Ctrl+L)"));
        ui.checkbox(&mut self.loop_on, tr("Lecture en boucle (Q)"));
        ui.checkbox(&mut self.metronome, tr("Métronome"));
        ui.checkbox(&mut self.mute_audio, tr("Couper tout l'audio"));
        ui.checkbox(&mut self.mute_video, tr("Couper toute la vidéo"));
        ui.checkbox(&mut self.dim, tr("Atténuation (dim)"));
        if ui.checkbox(&mut self.prefs.use_proxies, tr("Utiliser les proxys dans l'aperçu")).changed() {
            self.prefs.save();
        }
        ui.menu_button(tr("Format du temps"), |ui| {
            for f in RulerFormat::ALL {
                if ui.selectable_label(self.ruler_fmt == f, f.label()).clicked() {
                    self.ruler_fmt = f;
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        ui.menu_button(tr("Langue / Language"), |ui| {
            let cur = crate::i18n::lang();
            for l in crate::i18n::Lang::ALL {
                if ui.selectable_label(cur == l, l.name()).clicked() {
                    crate::i18n::set_lang(l);
                    if l.needs_cjk() {
                        crate::i18n::install_fonts(ctx);
                    }
                    ui.close_menu();
                }
            }
        });
        // eInterfaceType: 4 brightness levels, 0 (darkest) by default
        ui.menu_button(tr("Luminosité de l'interface"), |ui| {
            let cur = crate::theme::level();
            let names = [tr("Très sombre (par défaut)"), tr("Sombre"), tr("Clair"), tr("Très clair")];
            for (i, name) in names.iter().enumerate() {
                if ui.selectable_label(cur == i as u8, *name).clicked() {
                    crate::theme::set_level(ui.ctx(), i as u8);
                    ui.close_menu();
                }
            }
        });
    }

    fn tb_on(&self, key: &str) -> bool {
        !self.prefs.toolbar_hidden.iter().any(|k| k == key)
    }

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let _ = TOOLBAR_ITEMS;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            if self.tb_on("file") {
                if icons::button(ui, Icon::New, tr("Nouveau (Ctrl+N)"), false).clicked() {
                    self.new_project();
                }
                if icons::button(ui, Icon::Open, tr("Ouvrir (Ctrl+O)"), false).clicked() {
                    self.open(ctx);
                }
                if icons::button(ui, Icon::Save, tr("Enregistrer (Ctrl+S)"), false).clicked() {
                    self.save(false);
                }
                if icons::button(ui, Icon::Render, tr("Rendre en tant que… (Ctrl+R)"), false).clicked() {
                    self.open_render_dialog();
                }
                if icons::button(ui, Icon::Properties, tr("Propriétés du projet"), false).clicked() {
                    self.win_project = true;
                }
                ui.add_space(8.0);
            }
            if self.tb_on("undo") {
                if icons::button(ui, Icon::Undo, tr("Annuler (Ctrl+Z)"), false).clicked() {
                    self.do_undo();
                }
                if icons::button(ui, Icon::Redo, tr("Rétablir (Ctrl+Y)"), false).clicked() {
                    self.do_redo();
                }
                ui.add_space(8.0);
            }
            if self.tb_on("edit") {
                if icons::button(ui, Icon::Cut, tr("Couper (Ctrl+X)"), false).clicked() {
                    self.cut_selected();
                }
                if icons::button(ui, Icon::Copy, tr("Copier (Ctrl+C)"), false).clicked() {
                    self.copy_selected();
                }
                if icons::button(ui, Icon::Paste, tr("Coller (Ctrl+V)"), false).clicked() {
                    self.paste();
                }
                if icons::button(ui, Icon::Delete, tr("Supprimer (Suppr)"), false).clicked() {
                    self.delete_selected();
                }
                if icons::button(ui, Icon::Split, tr("Diviser au curseur (S)"), false).clicked() {
                    self.split_at_cursor();
                }
                ui.add_space(8.0);
            }
            if self.tb_on("modes") {
                if icons::button(ui, Icon::Snap, tr("Activer l'aimantation (F8)"), self.snap).clicked() {
                    self.snap = !self.snap;
                }
                if icons::button(ui, Icon::AutoXfade, tr("Fondu enchaîné automatique (Ctrl+Maj+X)"), self.auto_xf).clicked() {
                    self.auto_xf = !self.auto_xf;
                    self.touch();
                }
                if icons::button(ui, Icon::Ripple, tr("Montage en cascade automatique (Ctrl+L)"), self.ripple).clicked() {
                    self.ripple = !self.ripple;
                }
                ui.add_space(8.0);
            }
            if self.tb_on("view") {
                if icons::button(ui, Icon::Marker, tr("Insérer un marqueur (M)"), false).clicked() {
                    self.add_marker();
                }
                if icons::button(ui, Icon::ZoomIn, tr("Zoom avant (Haut)"), false).clicked() {
                    self.tl.zoom_at(1.25, self.cursor);
                }
                if icons::button(ui, Icon::ZoomOut, tr("Zoom arrière (Bas)"), false).clicked() {
                    self.tl.zoom_at(0.8, self.cursor);
                }
                ui.add_space(12.0);
            }
            let btn = |ui: &mut egui::Ui, label: &str, tip: &str, on: bool| ui.add(egui::Button::new(egui::RichText::new(label).size(12.0)).selected(on)).on_hover_text(tip);
            if self.tb_on("ai") {
                if btn(ui, tr("Claude"), tr("Assistant Claude : demandez une modification du montage"), self.ai.win_claude).clicked() {
                    self.ai.win_claude = !self.ai.win_claude;
                }
                if btn(ui, tr("Sous-titres IA"), tr("Sous-titres automatiques (transcription locale)"), self.ai.win_subs).clicked() {
                    self.ai.win_subs = !self.ai.win_subs;
                }
                if btn(ui, tr("Coupe auto"), tr("Couper les silences et les hésitations de l'événement sélectionné"), self.ai.win_trim).clicked() {
                    self.ai.win_trim = !self.ai.win_trim;
                }
                if btn(ui, tr("Plugins"), tr("Gérer les plugins d'effets"), self.ai.win_plugins).clicked() {
                    self.ai.win_plugins = !self.ai.win_plugins;
                }
                ui.add_space(8.0);
            }
            if self.tb_on("windows") {
                for (t, l) in [(Tab::Trimmer, tr("Rogneuse")), (Tab::Mixer, tr("Mixage")), (Tab::Scopes, tr("Oscilloscopes")), (Tab::Grading, tr("Étalonnage"))] {
                    if btn(ui, l, t.label(), self.tab_open(t)).clicked() {
                        self.toggle_tab(t);
                    }
                }
            }
        });
    }

    /// Timeline toolbar: edit tools, envelope kind, time format.
    pub(crate) fn timeline_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for t in EditTool::ALL {
                let r = ui.add(egui::Button::new(egui::RichText::new(t.label()).size(11.0)).selected(self.tool == t)).on_hover_text(t.tip());
                if r.clicked() {
                    self.tool = t;
                }
            }
            ui.separator();
            if self.tool == EditTool::Envelope {
                egui::ComboBox::from_id_salt("envk").width(170.0).selected_text(self.env_kind.label()).show_ui(ui, |ui| {
                    for k in EnvKind::ALL {
                        ui.selectable_value(&mut self.env_kind, k, k.label());
                    }
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("rulerf").width(180.0).selected_text(self.ruler_fmt.label()).show_ui(ui, |ui| {
                    for f in RulerFormat::ALL {
                        ui.selectable_value(&mut self.ruler_fmt, f, f.label());
                    }
                });
                if ui.selectable_label(self.prefs.show_envelopes, tr("Enveloppes")).clicked() {
                    self.prefs.show_envelopes = !self.prefs.show_envelopes;
                }
            });
        });
    }
}
