//! File commands: recent files, incremental save, autosave (fEnableAutoSave,
//! msAutoSaveInterval = 5 min), archive, import of another project,
//! interchange formats (EDL, FCPXML, XML, SRT), BWF placement, render queue,
//! render to a new track, proxies, unused media cleanup.

use crate::app::{App, LeftTab};
use crate::export;
use crate::i18n::tr;
use crate::model::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A render waiting in the queue (batch render).
#[derive(Clone)]
pub struct QueuedRender {
    pub path: String,
    pub settings: export::RenderSettings,
    pub range: (f64, f64),
    pub name: String,
    /// Put the result on a new track when done ("render to new track").
    pub to_track: bool,
}

impl App {
    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(self.prefs.recent_max.max(1));
        crate::prefs::save_recent(&self.recent);
    }

    /// Opens a project file by path (recent files, command line).
    pub fn open_path(&mut self, path: &str, ctx: &egui::Context) {
        match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Project>(&s).map_err(|e| e.to_string()))
        {
            Ok(p) => {
                self.push_undo();
                self.p = p;
                for m in &self.p.media {
                    self.store.ensure(m, ctx);
                }
                self.path = Some(path.to_string());
                self.dirty = false;
                self.sel.clear();
                self.cursor = 0.0;
                self.add_recent(path);
                self.status = tr("Projet ouvert.").into();
            }
            Err(e) => self.status = format!("{} {e}", tr("Impossible d'ouvrir le projet :")),
        }
    }

    fn write_project(&self, path: &Path) -> Result<(), String> {
        let s = serde_json::to_string_pretty(&self.p).map_err(|e| e.to_string())?;
        std::fs::write(path, s).map_err(|e| e.to_string())
    }

    /// Incremental save (Ctrl+Alt+S): name_001.blue, name_002.blue…
    pub fn incremental_save(&mut self) {
        let Some(cur) = self.path.clone() else {
            self.save(true);
            return;
        };
        let p = PathBuf::from(&cur);
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // strip an existing _NNN suffix
        let base = match stem.rsplit_once('_') {
            Some((b, n)) if n.len() == 3 && n.chars().all(|c| c.is_ascii_digit()) => b.to_string(),
            _ => stem.clone(),
        };
        let mut k = 1;
        let next = loop {
            let cand = dir.join(format!("{}_{:03}.{}", base, k, crate::app::PROJECT_EXT));
            if !cand.exists() {
                break cand;
            }
            k += 1;
        };
        match self.write_project(&next) {
            Ok(()) => {
                let s = next.to_string_lossy().to_string();
                self.path = Some(s.clone());
                self.dirty = false;
                self.add_recent(&s);
                self.status = format!("{} {}", tr("Enregistré :"), s);
            }
            Err(e) => self.status = format!("{} {e}", tr("Échec de l'enregistrement :")),
        }
    }

    /// Autosave every msAutoSaveInterval while the project is modified, into
    /// the data folder (the project file itself is not touched).
    pub fn autosave_tick(&mut self) {
        if !self.prefs.autosave || !self.dirty {
            return;
        }
        if self.last_autosave.elapsed().as_millis() < self.prefs.autosave_ms.max(10_000) as u128 {
            return;
        }
        self.last_autosave = Instant::now();
        let dir = crate::plugins::data_dir().join("autosave");
        let _ = std::fs::create_dir_all(&dir);
        let name = format!("{}.autosave.{}", if self.p.name.is_empty() { "projet" } else { &self.p.name }, crate::app::PROJECT_EXT);
        let path = dir.join(name.replace(['/', '\\', ':'], "_"));
        if self.write_project(&path).is_ok() {
            self.status = format!("{} {}", tr("Sauvegarde automatique :"), path.display());
        }
    }

    pub fn autosave_dir() -> PathBuf {
        crate::plugins::data_dir().join("autosave")
    }

    /// Archive: copies the project and every media file into a folder, with
    /// the paths rewritten to the copies.
    pub fn archive_project(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title(tr("Dossier d'archive")).pick_folder() else { return };
        let media_dir = dir.join("Media");
        let _ = std::fs::create_dir_all(&media_dir);
        let mut p = self.p.clone();
        let mut copied = 0;
        let mut errors = vec![];
        for m in &mut p.media {
            let src = match &m.source {
                MediaSource::File(s) | MediaSource::Image(s) | MediaSource::Nested(s) => s.clone(),
                _ => continue,
            };
            let name = Path::new(&src).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let dst = media_dir.join(&name);
            match std::fs::copy(&src, &dst) {
                Ok(_) => {
                    copied += 1;
                    let d = dst.to_string_lossy().to_string();
                    m.source = match &m.source {
                        MediaSource::File(_) => MediaSource::File(d),
                        MediaSource::Image(_) => MediaSource::Image(d),
                        _ => MediaSource::Nested(d),
                    };
                    m.proxy = None;
                }
                Err(e) => errors.push(format!("{name} : {e}")),
            }
        }
        let file = dir.join(crate::app::with_project_ext(if p.name.is_empty() { "archive" } else { &p.name }));
        let r = serde_json::to_string_pretty(&p).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&file, s).map_err(|e| e.to_string()));
        self.status = match r {
            Ok(()) if errors.is_empty() => format!("{} {} ({} {})", tr("Projet archivé :"), file.display(), copied, tr("médias copiés")),
            Ok(()) => format!("{} {}", tr("Archivé avec des erreurs :"), errors.join(", ")),
            Err(e) => format!("{} {e}", tr("Échec de l'archivage :")),
        };
    }

    /// Imports the media, tracks and markers of another project (appended).
    pub fn import_project(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter(crate::app::PROJECT_FILTER, &[crate::app::PROJECT_EXT, "bmproj"]).pick_file() else { return };
        let other: Project = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("{} {e}", tr("Impossible d'ouvrir le projet :"));
                return;
            }
        };
        self.push_undo();
        let offset = self.cursor;
        let mut map = std::collections::HashMap::new();
        for m in &other.media {
            let mut nm = m.clone();
            let old = nm.id;
            let id = self.p.add_media(nm.clone());
            nm.id = id;
            map.insert(old, id);
            self.store.ensure(&nm, ctx);
        }
        for t in &other.tracks {
            let mut nt = t.clone();
            nt.id = self.p.new_id();
            nt.group = None;
            nt.bus = None;
            for e in &mut nt.events {
                e.id = self.p.new_id();
                e.media = *map.get(&e.media).unwrap_or(&e.media);
                e.start += offset;
                e.group = None;
                for tk in &mut e.takes {
                    tk.media = *map.get(&tk.media).unwrap_or(&tk.media);
                }
            }
            let at = if nt.kind == TrackKind::Video { self.p.tracks.iter().position(|x| x.kind == TrackKind::Audio) } else { None };
            let idx = at.unwrap_or(self.p.tracks.len());
            self.p.tracks.insert(idx, nt);
        }
        for mk in &other.markers {
            self.p.markers.push(Marker { time: mk.time + offset, ..mk.clone() });
        }
        self.p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
        self.status = format!("{} {}", tr("Projet importé :"), path.display());
    }

    /// Adds a project file as a media (nested timeline).
    pub fn add_nested_project(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter(crate::app::PROJECT_FILTER, &[crate::app::PROJECT_EXT, "bmproj"]).pick_file() else { return };
        match crate::media::probe_nested(&path.to_string_lossy()) {
            Ok(m) => {
                let id = self.p.add_media(m);
                let m = self.p.media(id).unwrap().clone();
                self.store.ensure(&m, ctx);
                self.insert_media(id, self.cursor, None, true);
                self.left_tab = LeftTab::ProjectMedia;
            }
            Err(e) => self.status = e,
        }
    }

    /// Opens a nested project media as its own project (sub-timeline).
    pub fn open_nested(&mut self, media: Id, ctx: &egui::Context) {
        if let Some(MediaSource::Nested(p)) = self.p.media(media).map(|m| m.source.clone()) {
            if self.dirty {
                self.save(false);
            }
            self.open_path(&p, ctx);
        }
    }

    /// Import EDL / FCPXML / XML / SRT.
    pub fn import_interchange(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr("Listes de montage et sous-titres"), &["edl", "fcpxml", "xml", "srt"])
            .pick_file()
        else {
            return;
        };
        self.push_undo();
        let probe = |p: &str| crate::media::probe(p).ok();
        match crate::interchange::import(&path.to_string_lossy(), &mut self.p, &probe) {
            Ok(msg) => {
                for m in self.p.media.clone() {
                    self.store.ensure(&m, ctx);
                }
                self.status = msg;
            }
            Err(e) => {
                self.do_undo();
                self.status = e;
            }
        }
    }

    /// Export EDL / FCPXML / XML (Premiere, Resolve) / SRT.
    pub fn export_interchange(&mut self, kind: &str) {
        let (filter, ext, text) = match kind {
            "edl" => ("EDL CMX 3600", "edl", crate::interchange::export_edl(&self.p)),
            "fcpxml" => ("Final Cut Pro X / DaVinci Resolve (FCPXML)", "fcpxml", crate::interchange::export_fcpxml(&self.p)),
            "xml" => ("Premiere Pro / Final Cut 7 (XML)", "xml", crate::interchange::export_xmeml(&self.p)),
            _ => ("SubRip (SRT)", "srt", crate::interchange::export_srt(&self.p)),
        };
        let Some(path) = rfd::FileDialog::new().add_filter(filter, &[ext]).set_file_name(format!("{}.{}", self.p.name, ext)).save_file() else { return };
        self.status = match std::fs::write(&path, text) {
            Ok(()) => format!("{} {}", tr("Exporté :"), path.display()),
            Err(e) => format!("{} {e}", tr("Échec de l'export :")),
        };
    }

    /// Imports Broadcast WAV files and places them at their time stamp.
    pub fn import_bwf(&mut self, ctx: &egui::Context) {
        let Some(files) = rfd::FileDialog::new().add_filter("Broadcast WAV", &["wav", "bwf"]).pick_files() else { return };
        let mut placed = 0;
        for f in files {
            let s = f.to_string_lossy().to_string();
            let at = crate::interchange::bwf_time_reference(&s);
            let ids = self.import_files(vec![s], ctx);
            for id in ids {
                self.insert_media(id, at.unwrap_or(self.cursor), None, true);
                placed += 1;
            }
        }
        self.status = format!("{} {}", placed, tr("fichier(s) BWF placé(s) à leur horodatage."));
    }

    /// Removes media not used by any event or take.
    pub fn remove_unused_media(&mut self) {
        let used: std::collections::HashSet<Id> = self
            .p
            .tracks
            .iter()
            .flat_map(|t| t.events.iter().flat_map(|e| std::iter::once(e.media).chain(e.takes.iter().map(|k| k.media))))
            .collect();
        let before = self.p.media.len();
        self.push_undo();
        self.p.media.retain(|m| used.contains(&m.id));
        self.status = format!("{} {}", before - self.p.media.len(), tr("média(s) inutilisé(s) retiré(s)."));
    }

    /// Adds the current render dialog's job to the queue.
    pub fn queue_render(&mut self, q: QueuedRender) {
        self.render_queue.push(q);
        self.status = format!("{} {}", self.render_queue.len(), tr("rendu(s) en file d'attente."));
    }

    /// Queues one render per region (batch render).
    pub fn queue_regions(&mut self, settings: &export::RenderSettings, folder: &Path) {
        for r in self.p.regions.clone() {
            let path = folder.join(format!("{}.{}", r.name.replace(['/', '\\', ':'], "_"), settings.format.ext()));
            self.render_queue.push(QueuedRender {
                path: path.to_string_lossy().to_string(),
                settings: settings.clone(),
                range: (r.start, r.end),
                name: r.name.clone(),
                to_track: false,
            });
        }
    }

    /// Starts the next queued render when nothing is rendering; puts finished
    /// "render to new track" results on a new track.
    pub fn process_render_queue(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.job {
            let done = job.progress.lock().map(|p| p.done && p.error.is_none()).unwrap_or(false);
            let finished = job.progress.lock().map(|p| p.done).unwrap_or(false);
            if finished {
                if done {
                    if let Some((path, at)) = self.pending_track_render.take() {
                        let ids = self.import_files(vec![path], ctx);
                        if let Some(id) = ids.first().copied() {
                            self.push_undo();
                            let has_v = self.p.media(id).map(|m| m.has_video).unwrap_or(false);
                            let kind = if has_v { TrackKind::Video } else { TrackKind::Audio };
                            let ti = self.p.add_track(kind, if has_v { Some(0) } else { None });
                            self.p.tracks[ti].name = tr("Rendu").into();
                            if let Some(m) = self.p.media(id).cloned() {
                                let len = if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN };
                                let e = self.p.new_event(&m, at, len);
                                self.p.tracks[ti].events.push(e);
                            }
                            self.status = tr("Rendu placé sur une nouvelle piste.").into();
                        }
                    }
                }
                if !self.render_queue.is_empty() {
                    self.job = None;
                }
            } else {
                return;
            }
        }
        if self.job.is_some() || self.render_queue.is_empty() {
            return;
        }
        let q = self.render_queue.remove(0);
        if q.to_track {
            self.pending_track_render = Some((q.path.clone(), q.range.0));
        }
        self.job = Some(export::start(self.p.clone(), self.store.clone(), q.path.clone(), q.settings.clone(), q.range, self.auto_xf));
        self.status = format!("{} {}", tr("Rendu :"), q.name);
    }

    /// Render the time selection (or the whole project) to a file and put it
    /// on a new track.
    pub fn render_to_new_track(&mut self) {
        let range = self.p.loop_region.filter(|(a, b)| b > a).unwrap_or((0.0, self.p.end_time()));
        let dir = self
            .path
            .as_ref()
            .and_then(|p| Path::new(p).parent().map(|d| d.to_path_buf()))
            .or_else(crate::platform::videos_dir)
            .unwrap_or_else(std::env::temp_dir);
        let mut k = 1;
        let path = loop {
            let c = dir.join(format!("{} - {} {}.mp4", self.p.name, tr("rendu"), k));
            if !c.exists() {
                break c;
            }
            k += 1;
        };
        let settings = self.last_render.clone().unwrap_or_else(|| export::RenderSettings::for_project(&self.p));
        let mut s = settings;
        if !s.format.has_video() || s.format == export::Format::Dvd {
            s.format = export::Format::Mp4;
            s.codec = export::Codec::H264;
        }
        self.queue_render(QueuedRender { path: path.to_string_lossy().to_string(), settings: s, range, name: tr("Rendre vers une nouvelle piste").into(), to_track: true });
    }

    /// Creates proxies for every video file (in a background thread).
    pub fn make_proxies(&mut self) {
        let files: Vec<(Id, String)> = self
            .p
            .media
            .iter()
            .filter(|m| m.has_video && m.proxy.is_none())
            .filter_map(|m| match &m.source {
                MediaSource::File(p) => Some((m.id, p.clone())),
                _ => None,
            })
            .collect();
        if files.is_empty() {
            self.status = tr("Aucune vidéo sans proxy.").into();
            return;
        }
        let out = crate::plugins::data_dir().join("proxies");
        let res = self.proxy_results.clone();
        let n = files.len();
        std::thread::spawn(move || {
            for (id, p) in files {
                let r = crate::media::make_proxy(&p, &out);
                res.lock().unwrap().push((id, r));
            }
        });
        self.status = format!("{} {} {}", tr("Création de"), n, tr("proxy(s) en cours…"));
    }

    /// Collects finished proxies and refreshes the preview mapping.
    pub fn poll_proxies(&mut self) {
        let done: Vec<(Id, Result<String, String>)> = std::mem::take(&mut *self.proxy_results.lock().unwrap());
        for (id, r) in done {
            match r {
                Ok(path) => {
                    if let Some(m) = self.p.media.iter_mut().find(|m| m.id == id) {
                        m.proxy = Some(path);
                    }
                    self.status = tr("Proxy créé.").into();
                    self.touch();
                }
                Err(e) => self.status = e,
            }
        }
        let map: std::collections::HashMap<String, String> = if self.prefs.use_proxies {
            self.p
                .media
                .iter()
                .filter_map(|m| match (&m.source, &m.proxy) {
                    (MediaSource::File(p), Some(x)) => Some((p.clone(), x.clone())),
                    _ => None,
                })
                .collect()
        } else {
            Default::default()
        };
        if map != self.proxy_map {
            crate::media::set_proxies(map.clone());
            self.proxy_map = map;
        }
    }
}
