//! Keyboard dispatch through the editable key map, and the command runner
//! shared by shortcuts, menus and scripts.

use crate::app::{App, LeftTab};
use crate::docks::Tab;
use crate::i18n::tr;
use crate::model::*;
use crate::tools::EditTool;
use egui::Key;

impl App {
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.key_edit.is_some() {
            return;
        }
        let keys: Vec<(Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (k, m) in keys {
            // multicam: 1..9 switch angles while editing
            if self.multicam.is_some() && !m.command && !m.alt && !m.shift {
                let n = match k {
                    Key::Num1 => Some(0),
                    Key::Num2 => Some(1),
                    Key::Num3 => Some(2),
                    Key::Num4 => Some(3),
                    Key::Num5 => Some(4),
                    Key::Num6 => Some(5),
                    Key::Num7 => Some(6),
                    Key::Num8 => Some(7),
                    Key::Num9 => Some(8),
                    _ => None,
                };
                if let Some(a) = n {
                    self.multicam_cut(a);
                    continue;
                }
            }
            let combo = crate::keymap::Combo::of(k, m);
            if let Some(id) = self.keymap.lookup(combo).map(|s| s.to_string()) {
                self.run_command(&id, ctx);
            }
        }
    }

    fn cycle_tool(&mut self, dir: i32) {
        let all = EditTool::ALL;
        let i = all.iter().position(|t| *t == self.tool).unwrap_or(0) as i32;
        let n = all.len() as i32;
        self.tool = all[((i + dir).rem_euclid(n)) as usize];
        self.status = format!("{} {}", tr("Outil :"), self.tool.label());
    }

    fn pitch_by(&mut self, d: f32) {
        let ids: Vec<Id> = self.sel.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        self.push_undo();
        for id in ids {
            if let Some(e) = self.p.event_mut(id) {
                e.pitch = if d == 0.0 { 0.0 } else { (e.pitch + d).clamp(-48.0, 48.0) };
                self.status = format!("{} {} {}", tr("Hauteur :"), crate::panels::fr_num(e.pitch as f64, 2), tr("demi-tons"));
            }
        }
    }

    /// Runs a command by id (see keymap::COMMANDS).
    pub fn run_command(&mut self, id: &str, ctx: &egui::Context) {
        let before = self.undo.len();
        match id {
            "play_toggle" => self.toggle_play(),
            "play_start" => {
                self.playing = false;
                self.cursor = 0.0;
                self.play();
                self.play_origin = 0.0;
            }
            "play_pause" => {
                if self.playing {
                    self.pause()
                } else {
                    self.play()
                }
            }
            "stop" => {
                if self.playing {
                    self.stop()
                }
                self.sel_rect = None;
            }
            "project_props" => self.win_project = true,
            "new" => self.new_project(),
            "open" => self.open(ctx),
            "save" => self.save(false),
            "save_as" => self.save(true),
            "save_inc" => self.incremental_save(),
            "import" => self.import_dialog(ctx),
            "render" => self.open_render_dialog(),
            "render_new_track" => self.render_to_new_track(),
            "split" => self.split_at_cursor(),
            "delete" => self.delete_selected(),
            "undo" => self.do_undo(),
            "redo" => self.do_redo(),
            "copy" => self.copy_selected(),
            "cut" => self.cut_selected(),
            "paste" => self.paste(),
            "paste_insert" => self.paste_insert(),
            "paste_repeat" => self.win_paste_repeat = true,
            "paste_attrs" => self.win_paste_attrs = true,
            "select_all" => self.select_all(),
            "heal" => self.heal(),
            "trim" => self.trim_to_selection(),
            "trim_start" => self.trim_at_cursor(true),
            "trim_end" => self.trim_at_cursor(false),
            "take_next" => self.cycle_take(true),
            "take_prev" => self.cycle_take(false),
            "group" => self.group_selected(true),
            "ungroup" => self.group_selected(false),
            "auto_xf" => {
                self.auto_xf = !self.auto_xf;
                self.touch();
            }
            "ripple" => self.ripple = !self.ripple,
            "loop" => self.loop_on = !self.loop_on,
            "add_audio_track" => {
                self.push_undo();
                self.focus_track = self.p.add_track(TrackKind::Audio, None);
            }
            "add_video_track" => {
                self.push_undo();
                let at = self.p.tracks.iter().position(|t| t.kind == TrackKind::Audio);
                self.focus_track = self.p.add_track(TrackKind::Video, at);
            }
            "add_adjust_track" => {
                self.push_undo();
                let i = self.p.add_track(TrackKind::Video, Some(0));
                self.p.tracks[i].adjustment = true;
                self.p.tracks[i].name = tr("Ajustement").into();
                self.focus_track = i;
                self.win_track_fx = Some(i);
            }
            "marker" => self.add_marker(),
            "region" => self.add_region(),
            "command_marker" => self.add_command_marker(tr("Commande")),
            "mark_in" => self.mark_in(),
            "mark_out" => self.mark_out(),
            "mute_track" | "solo_track" => {
                if let Some(t) = self.p.tracks.get_mut(self.focus_track) {
                    if id == "mute_track" {
                        t.mute = !t.mute;
                    } else {
                        t.solo = !t.solo;
                    }
                    self.touch();
                }
            }
            "mute_excl" => {
                let ti = self.focus_track;
                if ti < self.p.tracks.len() {
                    self.exclusive_mute(ti);
                }
            }
            "solo_excl" => {
                let ti = self.focus_track;
                if ti < self.p.tracks.len() {
                    self.exclusive_solo(ti);
                }
            }
            "scrub_left" => {
                self.speed = if self.playing && self.speed < 0.0 { (self.speed * 2.0).max(-MAX_SCRUB_SPEED) } else { -1.0 };
                if !self.playing {
                    self.playing = true;
                    self.play_origin = self.cursor;
                }
            }
            "scrub_pause" => self.pause(),
            "scrub_right" => {
                if self.playing && self.speed > 0.0 {
                    self.speed = (self.speed * 2.0).min(MAX_SCRUB_SPEED);
                } else {
                    self.play();
                }
            }
            "snap" => self.snap = !self.snap,
            "snap_grid" => self.snap_grid = !self.snap_grid,
            "quantize" => self.quantize = !self.quantize,
            "tool_normal" => {
                self.tool = EditTool::Normal;
                self.status = format!("{} {}", tr("Outil :"), self.tool.label());
            }
            "tool_next" => self.cycle_tool(1),
            "tool_prev" => self.cycle_tool(-1),
            "show_env" => {
                self.prefs.show_envelopes = !self.prefs.show_envelopes;
                self.prefs.save();
            }
            "env_volume" => self.insert_envelope(crate::tools::EnvKind::Volume),
            "env_pan" => self.insert_envelope(crate::tools::EnvKind::Pan),
            "pitch_up" => self.pitch_by(1.0),
            "pitch_down" => self.pitch_by(-1.0),
            "pitch_cent_up" => self.pitch_by(0.01),
            "pitch_cent_down" => self.pitch_by(-0.01),
            "pitch_oct_up" => self.pitch_by(12.0),
            "pitch_oct_down" => self.pitch_by(-12.0),
            "pitch_reset" => self.pitch_by(0.0),
            "nudge_left" => self.nudge(-1.0 / self.p.fps),
            "nudge_right" => self.nudge(1.0 / self.p.fps),
            "nudge_left_px" => self.nudge(-1.0 / self.tl.pps),
            "nudge_right_px" => self.nudge(1.0 / self.tl.pps),
            "swap_cursor" => self.swap_cursor_selection(),
            "next_like" => self.select_similar(true),
            "lock" => self.toggle_lock(),
            "ram_preview" => self.build_ram_preview(),
            "min_tracks" => self.track_heights(0),
            "default_tracks" => self.track_heights(1),
            "more_height" | "less_height" => {
                let d = if id == "more_height" { 12.0 } else { -12.0 };
                for t in &mut self.p.tracks {
                    t.height = (t.height + d).clamp(MIN_TRACK_HEIGHT, 300.0);
                }
            }
            "dock_toggle" => self.dock_hidden = !self.dock_hidden,
            "labels_toggle" => {
                self.prefs.hide_tab_labels = !self.prefs.hide_tab_labels;
                self.prefs.save();
            }
            "dim" => self.dim = !self.dim,
            "trimmer" => self.show_tab(Tab::Trimmer),
            "mixer" => self.show_tab(Tab::Mixer),
            "scopes" => self.show_tab(Tab::Scopes),
            "edit_details" => self.show_tab(Tab::EditDetails),
            "loudness" => self.show_tab(Tab::Loudness),
            "color_grading" => self.show_tab(Tab::Grading),
            "notes" => self.show_tab(Tab::Notes),
            "explorer" => {
                self.left_tab = LeftTab::Explorer;
                self.show_tab(Tab::Explorer);
            }
            "project_media" => {
                self.left_tab = LeftTab::ProjectMedia;
                self.show_tab(Tab::Media);
            }
            "transitions" => {
                self.left_tab = LeftTab::Transitions;
                self.show_tab(Tab::Transitions);
            }
            "video_fx" => {
                self.left_tab = LeftTab::VideoFx;
                self.show_tab(Tab::VideoFx);
            }
            "generators" => {
                self.left_tab = LeftTab::Generators;
                self.show_tab(Tab::Generators);
            }
            "surround" => self.show_tab(Tab::Surround),
            "scripts" => self.win_scripts = true,
            "help" => self.win_help = true,
            "whats_this" => {
                self.prefs.whats_this = !self.prefs.whats_this;
                self.status = if self.prefs.whats_this { tr("Aide « Qu'est-ce que c'est ? » activée : survolez un élément.").into() } else { tr("Aide « Qu'est-ce que c'est ? » désactivée.").into() };
            }
            "open_nested" => {
                if let Some(mid) = self.sel.iter().next().and_then(|id| self.p.event(*id)).map(|e| e.media) {
                    self.open_nested(mid, ctx);
                }
            }
            "home" => {
                self.cursor = 0.0;
                self.tl.scroll = 0.0;
            }
            "end" => self.cursor = self.p.end_time(),
            "frame_prev" => self.cursor = (self.p.quantize(self.cursor) - 1.0 / self.p.fps).max(0.0),
            "frame_next" => self.cursor = self.p.quantize(self.cursor) + 1.0 / self.p.fps,
            "zoom_in" => self.tl.zoom_at(1.25, self.cursor),
            "zoom_out" => self.tl.zoom_at(0.8, self.cursor),
            "layout_default" => self.load_preset(crate::docks::Preset::Default),
            _ => {}
        }
        if self.undo.len() > before {
            let l = crate::keymap::label(id);
            self.label_last_undo(l);
        }
    }
}
