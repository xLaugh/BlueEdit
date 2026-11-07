//! Media tools: stabilisation and motion tracking analysis (background
//! threads), sync by audio, multicam editing, slideshow, text to speech,
//! external audio editor, RAM preview.

use crate::app::App;
use crate::i18n::tr;
use crate::model::*;
use std::sync::{Arc, Mutex};

/// Result of a background analysis.
pub enum BgResult {
    Stabilize { event: Id, path: Vec<[f32; 2]>, fps: f64 },
    Track { event: Id, target: Option<Id>, points: Vec<(f64, f32, f32)> },
    Tts { path: String },
    Error(String),
}

pub struct BgTask {
    pub name: String,
    pub progress: Arc<Mutex<f32>>,
    pub result: Arc<Mutex<Option<BgResult>>>,
}

/// Multicam editing in progress: angle tracks and the recorded cuts.
pub struct Multicam {
    pub tracks: Vec<usize>,
    pub cuts: Vec<(f64, usize)>,
    pub current: usize,
}

/// Analysis frame width (the stabilisation path is in these pixels).
pub const ANALYSIS_W: u32 = 160;

fn event_video(p: &Project, id: Id) -> Option<(Event, String, Media)> {
    let e = p.event(id)?.clone();
    let m = p.media(e.media)?.clone();
    match &m.source {
        MediaSource::File(path) if m.has_video => Some((e, path.clone(), m)),
        _ => None,
    }
}

impl App {
    fn spawn_task(&mut self, name: &str, f: impl FnOnce(Arc<Mutex<f32>>) -> BgResult + Send + 'static) {
        let progress = Arc::new(Mutex::new(0.0));
        let result = Arc::new(Mutex::new(None));
        let (p2, r2) = (progress.clone(), result.clone());
        std::thread::spawn(move || {
            let r = f(p2);
            *r2.lock().unwrap() = Some(r);
        });
        self.tasks.push(BgTask { name: name.to_string(), progress, result });
    }

    /// Stabilise a video event: global motion between frames, smoothed path
    /// (analysis.rs), stored in a Stabilize effect on the event.
    pub fn stabilize_event(&mut self, id: Id) {
        let Some((e, path, m)) = event_video(&self.p, id) else {
            self.status = tr("Choisissez un événement vidéo issu d'un fichier.").into();
            return;
        };
        let fps = if m.fps > 0.0 { m.fps } else { self.p.fps };
        let h = ((ANALYSIS_W as f64 * m.height.max(1) as f64 / m.width.max(1) as f64).round() as u32).max(2) & !1;
        self.spawn_task(tr("Stabilisation"), move |prog| {
            let n = (e.length * fps).ceil() as usize;
            let Some(mut r) = crate::media::VideoReader::open(&path, e.offset, ANALYSIS_W, h, fps) else {
                return BgResult::Error(tr("Lecture de la vidéo impossible (ffmpeg).").into());
            };
            let mut prev: Option<Vec<u8>> = None;
            let mut shifts = vec![];
            for i in 0..n {
                let Some((_, f)) = r.read_frame() else { break };
                let l = crate::analysis::luma(&f);
                let s = match &prev {
                    Some(p) => crate::analysis::frame_shift(p, &l, ANALYSIS_W as usize, h as usize, 12),
                    None => (0.0, 0.0),
                };
                shifts.push(s);
                prev = Some(l);
                *prog.lock().unwrap() = i as f32 / n.max(1) as f32;
            }
            let path = crate::analysis::stabilize_path(&shifts, 15);
            BgResult::Stabilize { event: e.id, path, fps }
        });
    }

    /// Motion tracking: follows a region (normalised rect) of a video event
    /// and, if `target` is given, moves that event's pan/crop with it.
    pub fn track_motion(&mut self, id: Id, rect: [f32; 4], target: Option<Id>) {
        let Some((e, path, m)) = event_video(&self.p, id) else {
            self.status = tr("Choisissez un événement vidéo issu d'un fichier.").into();
            return;
        };
        let fps = if m.fps > 0.0 { m.fps } else { self.p.fps };
        let w = 320u32;
        let h = ((w as f64 * m.height.max(1) as f64 / m.width.max(1) as f64).round() as u32).max(2) & !1;
        self.spawn_task(tr("Suivi de mouvement"), move |prog| {
            let n = (e.length * fps).ceil() as usize;
            let Some(mut r) = crate::media::VideoReader::open(&path, e.offset, w, h, fps) else {
                return BgResult::Error(tr("Lecture de la vidéo impossible (ffmpeg).").into());
            };
            let (rx, ry) = ((rect[0] * w as f32) as i32, (rect[1] * h as f32) as i32);
            let tw = ((rect[2] * w as f32) as usize).clamp(8, w as usize / 2);
            let th = ((rect[3] * h as f32) as usize).clamp(8, h as usize / 2);
            let mut tpl: Option<Vec<u8>> = None;
            let mut pos = (rx, ry);
            let mut pts = vec![];
            for i in 0..n {
                let Some((_, f)) = r.read_frame() else { break };
                let l = crate::analysis::luma(&f);
                match &tpl {
                    None => {
                        let mut t = vec![0u8; tw * th];
                        for y in 0..th {
                            for x in 0..tw {
                                let sx = (rx as usize + x).min(w as usize - 1);
                                let sy = (ry as usize + y).min(h as usize - 1);
                                t[y * tw + x] = l[sy * w as usize + sx];
                            }
                        }
                        tpl = Some(t);
                    }
                    Some(t) => {
                        let (x, y, _) = crate::analysis::track_template(&l, w as usize, h as usize, t, tw, th, pos, 24);
                        pos = (x, y);
                    }
                }
                let cx = (pos.0 as f32 + tw as f32 / 2.0) / w as f32;
                let cy = (pos.1 as f32 + th as f32 / 2.0) / h as f32;
                pts.push((i as f64 / fps, cx, cy));
                *prog.lock().unwrap() = i as f32 / n.max(1) as f32;
            }
            BgResult::Track { event: e.id, target, points: pts }
        });
    }

    /// Text to speech with the system voice (Windows SAPI, macOS `say`,
    /// Linux `espeak`), saved as WAV and placed at the cursor.
    pub fn text_to_speech(&mut self, text: String, voice: String) {
        let dir = crate::plugins::data_dir().join("tts");
        let _ = std::fs::create_dir_all(&dir);
        let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) + 1;
        let out = dir.join(format!("voix_{n:03}.wav"));
        self.spawn_task(tr("Synthèse vocale"), move |_| {
            let o = out.to_string_lossy().to_string();
            let txt_file = out.with_extension("txt");
            if std::fs::write(&txt_file, &text).is_err() {
                return BgResult::Error(tr("Écriture impossible.").into());
            }
            #[cfg(windows)]
            let st = {
                let script = format!(
                    "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; {} $s.SetOutputToWaveFile('{}'); $s.Speak([IO.File]::ReadAllText('{}', [Text.Encoding]::UTF8)); $s.Dispose()",
                    if voice.is_empty() { String::new() } else { format!("$s.SelectVoice('{}');", voice.replace('\'', "''")) },
                    o.replace('\'', "''"),
                    txt_file.to_string_lossy().replace('\'', "''")
                );
                crate::platform::command("powershell").args(["-NoProfile", "-Command", &script]).status()
            };
            #[cfg(target_os = "macos")]
            let st = {
                let mut c = std::process::Command::new("say");
                if !voice.is_empty() {
                    c.args(["-v", &voice]);
                }
                c.args(["--data-format=LEI16@48000", "-o", &o, "-f"]).arg(&txt_file).status()
            };
            #[cfg(all(unix, not(target_os = "macos")))]
            let st = {
                let mut c = std::process::Command::new("espeak");
                if !voice.is_empty() {
                    c.args(["-v", &voice]);
                }
                c.args(["-w", &o, "-f"]).arg(&txt_file).status()
            };
            let _ = std::fs::remove_file(&txt_file);
            match st {
                Ok(s) if s.success() && out.exists() => BgResult::Tts { path: o },
                _ => BgResult::Error(tr("La synthèse vocale du système n'a pas fonctionné.").into()),
            }
        });
    }

    /// Collects finished background tasks.
    pub fn poll_tasks(&mut self, ctx: &egui::Context) {
        let mut done = vec![];
        self.tasks.retain(|t| {
            let r = t.result.lock().unwrap().take();
            match r {
                Some(r) => {
                    done.push(r);
                    false
                }
                None => true,
            }
        });
        for r in done {
            match r {
                BgResult::Stabilize { event, path, fps } => {
                    self.push_undo();
                    if let Some(e) = self.p.event_mut(event) {
                        e.fx.retain(|f| !matches!(f, VideoFx::Stabilize { .. }));
                        e.fx.insert(0, VideoFx::Stabilize { amount: 1.0, zoom: 1.08, path, fps });
                    }
                    self.status = tr("Stabilisation appliquée.").into();
                }
                BgResult::Track { event, target, points } => {
                    self.push_undo();
                    self.tracking_result = Some((event, points.clone()));
                    if let Some(tid) = target {
                        let start = self.p.event(event).map(|e| e.start).unwrap_or(0.0);
                        let tstart = self.p.event(tid).map(|e| e.start).unwrap_or(0.0);
                        if let Some(te) = self.p.event_mut(tid) {
                            let base = te.pan;
                            let (x0, y0) = points.first().map(|p| (p.1, p.2)).unwrap_or((0.5, 0.5));
                            te.pan_keys = points
                                .iter()
                                .step_by(2)
                                .map(|(t, x, y)| PanKey {
                                    t: (start + t - tstart).max(0.0),
                                    pan: PanCrop { x: base.x + (x - x0), y: base.y + (y - y0), ..base },
                                })
                                .collect();
                        }
                        self.status = tr("Suivi appliqué au mouvement de l'événement choisi.").into();
                    } else {
                        self.status = format!("{} {} {}", tr("Suivi terminé :"), points.len(), tr("positions."));
                    }
                }
                BgResult::Tts { path } => {
                    let ids = self.import_files(vec![path], ctx);
                    if let Some(id) = ids.first() {
                        self.insert_media(*id, self.cursor, None, true);
                    }
                    self.status = tr("Voix de synthèse ajoutée.").into();
                }
                BgResult::Error(e) => self.status = e,
            }
        }
        if !self.tasks.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Aligns the selected events on the first one by their audio
    /// (cross-correlation of the envelopes, ±30 s).
    pub fn sync_by_audio(&mut self) {
        let ids: Vec<Id> = {
            let mut v: Vec<(Id, f64)> = self.sel.iter().filter_map(|id| self.p.event(*id).map(|e| (*id, e.start))).collect();
            v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            v.into_iter().map(|x| x.0).collect()
        };
        if ids.len() < 2 {
            self.status = tr("Sélectionnez au moins deux événements avec du son.").into();
            return;
        }
        let env_of = |app: &App, id: Id| -> Option<(Vec<f32>, f64, f64)> {
            let e = app.p.event(id)?;
            let a = app.store.audio(e.media)?;
            let hop = (crate::media::AUDIO_RATE / 100) as usize;
            Some((crate::analysis::envelope_i16(&a.samples, hop), e.offset, e.start))
        };
        let Some((ref_env, ref_off, ref_start)) = env_of(self, ids[0]) else {
            self.status = tr("Le son du premier événement n'est pas encore chargé.").into();
            return;
        };
        self.push_undo();
        let mut moved = 0;
        for id in &ids[1..] {
            let Some((env, off, _)) = env_of(self, *id) else { continue };
            // where does b's content sit in a's content (seconds)?
            let d = crate::analysis::audio_offset(&ref_env, &env, 100.0, 30.0);
            // b's content time 0 = a's content time d → place b so the same moment lines up
            let new_start = ref_start - ref_off + d + off;
            let group = self.p.event(*id).and_then(|e| e.group);
            let old_start = self.p.event(*id).map(|e| e.start).unwrap_or(0.0);
            let delta = new_start - old_start;
            for t in &mut self.p.tracks {
                for e in &mut t.events {
                    if e.id == *id || (group.is_some() && e.group == group) {
                        e.start = (e.start + delta).max(0.0);
                    }
                }
            }
            moved += 1;
        }
        self.p.sort_events();
        self.status = format!("{} {}", moved, tr("événement(s) synchronisé(s) par l'audio."));
    }

    /// Starts multicam editing with the video tracks that have events.
    pub fn multicam_begin(&mut self) {
        let tracks: Vec<usize> = self.p.tracks.iter().enumerate().filter(|(_, t)| t.kind == TrackKind::Video && !t.events.is_empty()).map(|(i, _)| i).collect();
        if tracks.len() < 2 {
            self.status = tr("Il faut au moins deux pistes vidéo (une par caméra).").into();
            return;
        }
        self.multicam = Some(Multicam { tracks, cuts: vec![(self.cursor, 0)], current: 0 });
        self.status = tr("Multicaméra : lancez la lecture et appuyez sur 1, 2, 3… pour changer d'angle, puis « Terminer ».").into();
    }

    pub fn multicam_cut(&mut self, angle: usize) {
        let c = self.cursor;
        if let Some(mc) = &mut self.multicam {
            if angle < mc.tracks.len() {
                mc.current = angle;
                mc.cuts.retain(|(t, _)| *t < c - 1e-6);
                mc.cuts.push((c, angle));
            }
        }
    }

    /// Builds the edit on a new track from the recorded angle cuts.
    pub fn multicam_finish(&mut self) {
        let Some(mc) = self.multicam.take() else { return };
        self.push_undo();
        let end = self.p.end_time();
        let ti = self.p.add_track(TrackKind::Video, Some(0));
        self.p.tracks[ti].name = tr("Multicaméra").into();
        let shift = |i: usize| if i >= ti { i + 1 } else { i };
        let mut new_events = vec![];
        for (k, (t0, angle)) in mc.cuts.iter().enumerate() {
            let t1 = mc.cuts.get(k + 1).map(|c| c.0).unwrap_or(end);
            if t1 <= *t0 {
                continue;
            }
            let src = shift(mc.tracks[*angle]);
            for e in &self.p.tracks[src].events {
                let a = e.start.max(*t0);
                let b = e.end().min(t1);
                if b > a {
                    let mut ne = e.clone();
                    ne.start = a;
                    ne.length = b - a;
                    ne.offset = e.source_time(a - e.start);
                    ne.fade_in = 0.0;
                    ne.fade_out = 0.0;
                    ne.group = None;
                    new_events.push(ne);
                }
            }
        }
        for mut e in new_events {
            e.id = self.p.new_id();
            self.p.tracks[ti].events.push(e);
        }
        // mute the angle tracks so only the edit is seen
        for i in &mc.tracks {
            let i = shift(*i);
            self.p.tracks[i].mute = true;
        }
        self.p.sort_events();
        self.status = tr("Montage multicaméra créé sur une nouvelle piste.").into();
    }

    /// Slideshow: images one after another with overlap (crossfade) and an
    /// optional slow zoom (pan/crop keyframes).
    pub fn slideshow(&mut self, paths: Vec<String>, secs: f64, overlap: f64, ken_burns: bool, ctx: &egui::Context) {
        let ids = self.import_files(paths, ctx);
        if ids.is_empty() {
            return;
        }
        self.push_undo();
        let ti = self
            .p
            .tracks
            .get(self.focus_track)
            .filter(|t| t.kind == TrackKind::Video)
            .map(|_| self.focus_track)
            .unwrap_or_else(|| self.p.add_track(TrackKind::Video, Some(0)));
        let mut t = self.cursor;
        let overlap = overlap.clamp(0.0, secs / 2.0);
        for (k, id) in ids.iter().enumerate() {
            let Some(m) = self.p.media(*id).cloned() else { continue };
            let mut e = self.p.new_event(&m, t, secs);
            if k > 0 {
                e.fade_in = overlap;
            }
            if ken_burns {
                let z0 = if k % 2 == 0 { 1.0 } else { 1.15 };
                let z1 = if k % 2 == 0 { 1.15 } else { 1.0 };
                e.pan_keys = vec![
                    PanKey { t: 0.0, pan: PanCrop { zoom: z0, ..PanCrop::default() } },
                    PanKey { t: secs, pan: PanCrop { zoom: z1, x: if k % 3 == 0 { 0.03 } else { -0.03 }, ..PanCrop::default() } },
                ];
            }
            self.p.tracks[ti].events.push(e);
            t += secs - overlap;
        }
        self.p.sort_events();
        self.status = format!("{} {} {}", tr("Diaporama de"), ids.len(), tr("images créé."));
    }

    /// Opens the event's audio file in the external sound editor (szSoundEditorExe).
    pub fn open_in_sound_editor(&mut self, id: Id) {
        let editor = self.prefs.sound_editor.clone();
        if editor.trim().is_empty() {
            self.status = tr("Choisissez d'abord un éditeur audio dans les Préférences.").into();
            self.win_prefs = true;
            return;
        }
        let Some(path) = self.p.event(id).and_then(|e| self.p.media(e.media)).and_then(|m| match &m.source {
            MediaSource::File(p) => Some(p.clone()),
            _ => None,
        }) else {
            self.status = tr("Cet événement n'a pas de fichier audio.").into();
            return;
        };
        match std::process::Command::new(&editor).arg(&path).spawn() {
            Ok(_) => self.status = format!("{} {}", tr("Ouvert dans"), editor),
            Err(e) => self.status = format!("{} {e}", tr("Impossible de lancer l'éditeur :")),
        }
    }

    /// RAM preview: renders the time selection at preview size into memory,
    /// then plays it from there (Tools > Build Dynamic RAM Preview).
    pub fn build_ram_preview(&mut self) {
        let (a, b) = self.p.loop_region.filter(|(a, b)| b > a).unwrap_or((self.cursor, self.cursor + 5.0));
        let (w, h) = self.preview_dims;
        if w == 0 {
            return;
        }
        let n = ((b - a) * self.p.fps).ceil() as i64;
        if n > 3000 {
            self.status = tr("Sélection trop longue pour l'aperçu RAM (100 s au plus).").into();
            return;
        }
        let mut src = crate::compositor::ExactSource::new(self.p.fps);
        let mut comp = crate::compositor::Compositor::default();
        self.ram_cache.clear();
        let f0 = self.p.frame_of(a);
        for k in 0..n {
            let t = (f0 + k) as f64 / self.p.fps;
            let img = comp.compose(&self.p, t, w, h, self.auto_xf, &mut src);
            self.ram_cache.insert(f0 + k, Arc::new(img));
        }
        self.ram_sig = (self.rev, w, h);
        self.loop_on = true;
        self.p.loop_region = Some((a, b));
        self.status = format!("{} {} {}", tr("Aperçu RAM prêt :"), n, tr("images en mémoire."));
    }
}
