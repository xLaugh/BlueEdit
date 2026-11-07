//! Application state, editing commands, menus, toolbar, shortcuts.

use crate::i18n::tr;

use crate::audio::{self, AudioEngine};
use crate::compositor::Compositor;
use crate::export;
use crate::icons::{self, Icon};
use crate::media::{self, FrameServer, MediaStore};
use crate::model::*;
use crate::timeline::{Drag, TlView};
use egui::Key;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    ProjectMedia,
    Explorer,
    Transitions,
    VideoFx,
    Generators,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Draft,
    Preview,
    Good,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Draft => tr("Ébauche (Auto)"),
            Quality::Preview => tr("Aperçu (Auto)"),
            Quality::Good => tr("Bonne (Auto)"),
        }
    }
    pub fn divisor(self) -> u32 {
        match self {
            Quality::Draft => 4,
            Quality::Preview => 2,
            Quality::Good => 1,
        }
    }
}

/// Project files: "name.blue" (files from earlier versions, ".bmproj", still open).
pub const PROJECT_EXT: &str = "blue";
pub const PROJECT_FILTER: &str = "Projet Blue Edit";

/// Product name with "major.minor", e.g. "Blue Edit 0.1".
pub fn product_name() -> String {
    format!("Blue Edit {}", crate::platform::version_folder())
}

/// Window title: file name, " *" when modified, then " - " and the product name.
pub fn window_title(file_name: &str, modified: bool) -> String {
    format!("{}{}{}{} - {}", "", "", file_name, if modified { " *" } else { "" }, product_name())
}

/// Adds the project extension unless the name already ends with it
/// (case-insensitive).
pub fn with_project_ext(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == PROJECT_EXT || ext == "bmproj" {
        name.to_string()
    } else {
        format!("{}.{}", name, PROJECT_EXT)
    }
}

pub struct RenderDialog {
    pub path: String,
    pub settings: export::RenderSettings,
    pub loop_only: bool,
}

pub struct Shot {
    pub path: PathBuf,
    pub requested: bool,
}

/// Payloads for drag and drop between panels and the timeline.
#[derive(Clone, Copy)]
pub struct MediaPayload(pub Id);
#[derive(Clone, Copy)]
pub struct TransitionPayload(pub Transition);
#[derive(Clone)]
pub struct FxPayload(pub Vec<VideoFx>);

pub struct App {
    pub p: Project,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pub path: Option<String>,
    pub dirty: bool,
    pub rev: u64,

    pub cursor: f64,
    pub playing: bool,
    pub speed: f64,
    pub play_origin: f64,
    pub sel: HashSet<Id>,
    pub focus_track: usize,
    pub clipboard: Vec<(usize, Event)>,

    pub tl: TlView,
    pub drag: Option<Drag>,

    pub snap: bool,
    pub snap_grid: bool,
    pub ripple: bool,
    pub auto_xf: bool,
    pub loop_on: bool,
    pub quantize: bool,

    pub left_tab: LeftTab,
    pub quality: Quality,
    pub show_safe: bool,
    pub show_grid: bool,
    pub preview_tex: Option<egui::TextureHandle>,
    pub preview_sig: (u64, i64, u32, u32),
    pub preview_dims: (u32, u32),
    pub thumb_tex: HashMap<(Id, usize), egui::TextureHandle>,

    pub store: Arc<MediaStore>,
    pub server: Arc<FrameServer>,
    pub comp: Compositor,
    pub audio: AudioEngine,
    pub mix_rev: u64,
    pub mix: Arc<audio::Mix>,
    pub meters: [f32; 2],
    pub peak_hold: [f32; 2],

    pub win_fx: Option<Id>,
    pub win_pan: Option<Id>,
    pub win_props: Option<Id>,
    pub win_project: bool,
    pub win_render: Option<RenderDialog>,
    pub job: Option<export::Job>,
    pub win_about: bool,
    pub win_gen: Option<Id>,
    pub rename_track: Option<usize>,

    pub explorer_dir: PathBuf,
    pub status: String,
    pub shot: Option<Shot>,
    pub frame_no: u64,
    pub last_tick: Instant,
    pub ctx: egui::Context,
    pub ai: crate::ai_ui::AiState,
    pub last_render: Option<export::RenderSettings>,

    // ---- added: tools, prefs, docking, new windows
    pub undo_labels: Vec<String>,
    pub redo_labels: Vec<String>,
    pub tool: crate::tools::EditTool,
    pub env_kind: crate::tools::EnvKind,
    pub ruler_fmt: crate::tools::RulerFormat,
    pub prefs: crate::prefs::Prefs,
    pub recent: Vec<String>,
    pub keymap: crate::keymap::Keymap,
    pub key_edit: Option<String>,
    pub key_filter: String,
    pub dock: egui_dock::DockState<crate::docks::Tab>,
    pub timeline_top: bool,
    pub dock_hidden: bool,
    pub dock_height: f32,
    pub trimmer: crate::tabs::Trimmer,
    pub scope_kind: crate::scopes::ScopeKind,
    pub scope_cache: Option<((u64, u8), crate::scopes::ScopeData)>,
    pub preview_rgba: Option<(u32, u32, Arc<Vec<u8>>)>,
    pub preview_serial: u64,
    pub preview_zoom: f32,
    pub loud_report: Option<crate::loudness::Report>,
    pub loud_live: (f32, f32),
    pub loud_live_at: f64,
    pub loud_log: Vec<(f32, f32)>,
    pub details_view: u8,
    pub details_dirty: bool,
    pub grading_dirty: bool,
    pub win_prefs: bool,
    pub win_keys: bool,
    pub win_scripts: bool,
    pub win_help: bool,
    pub help_topic: usize,
    pub win_slideshow: bool,
    pub win_tts: bool,
    pub win_render_queue: bool,
    pub win_paste_repeat: bool,
    pub win_paste_attrs: bool,
    pub win_track_fx: Option<usize>,
    pub win_track_motion: Option<usize>,
    pub win_event_audio: Option<Id>,
    pub win_tracking: Option<Id>,
    pub win_multicam: bool,
    pub win_media_props: Option<Id>,
    pub win_output_fx: bool,
    pub win_customize: bool,
    pub ext_monitor: bool,
    pub ss_secs: f64,
    pub ss_overlap: f64,
    pub ss_kb: bool,
    pub tts_text: String,
    pub paste_gap: f64,
    pub attr_mask: crate::tools::AttrMask,
    pub script_text: String,
    pub script_out: String,
    pub media_search: String,
    pub media_bin: String,
    pub tasks: Vec<crate::media_tools::BgTask>,
    pub tracking_rect: [f32; 4],
    pub tracking_target: Option<Id>,
    pub tracking_result: Option<(Id, Vec<(f64, f32, f32)>)>,
    pub multicam: Option<crate::media_tools::Multicam>,
    pub ram_cache: HashMap<i64, Arc<Vec<u8>>>,
    pub ram_sig: (u64, u32, u32),
    pub metronome: bool,
    pub dim: bool,
    pub mute_audio: bool,
    pub mute_video: bool,
    pub recorder: Option<crate::record::Recorder>,
    pub render_queue: Vec<crate::files::QueuedRender>,
    pub pending_track_render: Option<(String, f64)>,
    pub proxy_results: Arc<std::sync::Mutex<Vec<(Id, Result<String, String>)>>>,
    pub proxy_map: HashMap<String, String>,
    pub last_autosave: Instant,
    pub automation_writes: Vec<(usize, bool, f32)>,
    pub sel_rect: Option<(egui::Pos2, egui::Pos2)>,
}

/// Toolbar buttons that can be hidden (Options > Personnaliser la barre d'outils).
pub const TOOLBAR_ITEMS: &[(&str, &str)] = &[
    ("file", "Fichier (nouveau, ouvrir, enregistrer, rendre)"),
    ("undo", "Annuler / rétablir"),
    ("edit", "Couper, copier, coller, supprimer, diviser"),
    ("modes", "Aimantation, fondu automatique, cascade"),
    ("view", "Marqueur et zoom"),
    ("ai", "Claude, sous-titres IA, coupe auto, plugins"),
    ("windows", "Rogneuse, mixage, oscilloscopes, étalonnage"),
];

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>, media_files: Vec<String>) -> App {
        crate::theme::init(&cc.egui_ctx);
        if crate::i18n::lang().needs_cjk() {
            crate::i18n::install_fonts(&cc.egui_ctx);
        }
        let ctx = cc.egui_ctx.clone();
        let store = Arc::new(MediaStore::default());
        let server = FrameServer::new(ctx.clone());
        let audio = AudioEngine::new(shot.is_none());
        let p = demo_project();
        for m in &p.media {
            store.ensure(m, &ctx);
        }
        let explorer_dir = crate::platform::videos_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        let status = if media::ffmpeg_available() {
            tr("Prêt.").to_string()
        } else {
            "Prêt. (ffmpeg introuvable : seules les images et les générateurs sont disponibles)".to_string()
        };
        let mut app = App {
            p,
            undo: vec![],
            redo: vec![],
            path: None,
            dirty: false,
            rev: 1,
            cursor: 5.5,
            playing: false,
            speed: 1.0,
            play_origin: 0.0,
            sel: HashSet::new(),
            focus_track: 1,
            clipboard: vec![],
            tl: TlView::default(),
            drag: None,
            snap: true,
            snap_grid: false,
            ripple: false,
            auto_xf: true,
            loop_on: false,
            quantize: true,
            left_tab: LeftTab::ProjectMedia,
            quality: Quality::Preview,
            show_safe: false,
            show_grid: false,
            preview_tex: None,
            preview_sig: (0, -1, 0, 0),
            preview_dims: (0, 0),
            thumb_tex: HashMap::new(),
            store,
            server,
            comp: Compositor::default(),
            audio,
            mix_rev: 0,
            mix: Arc::new(audio::Mix::default()),
            meters: [0.0; 2],
            peak_hold: [0.0; 2],
            win_fx: None,
            win_pan: None,
            win_props: None,
            win_project: false,
            win_render: None,
            job: None,
            win_about: false,
            win_gen: None,
            rename_track: None,
            explorer_dir,
            status,
            shot: shot.map(|path| Shot {
                path,
                requested: false,
            }),
            frame_no: 0,
            last_tick: Instant::now(),
            ctx: ctx.clone(),
            ai: crate::ai_ui::AiState::new(),
            last_render: None,
            undo_labels: vec![],
            redo_labels: vec![],
            tool: Default::default(),
            env_kind: Default::default(),
            ruler_fmt: Default::default(),
            prefs: crate::prefs::Prefs::load(),
            recent: crate::prefs::load_recent(),
            keymap: crate::keymap::Keymap::load(),
            key_edit: None,
            key_filter: String::new(),
            dock: crate::docks::preset(crate::docks::Preset::Default).0,
            timeline_top: false,
            dock_hidden: false,
            dock_height: TOP_DOCK_HEIGHT,
            trimmer: Default::default(),
            scope_kind: crate::scopes::ScopeKind::Waveform,
            scope_cache: None,
            preview_rgba: None,
            preview_serial: 0,
            preview_zoom: 0.0,
            loud_report: None,
            loud_live: (f32::NEG_INFINITY, f32::NEG_INFINITY),
            loud_live_at: -1.0,
            loud_log: vec![],
            details_view: 0,
            details_dirty: false,
            grading_dirty: false,
            win_prefs: false,
            win_keys: false,
            win_scripts: false,
            win_help: false,
            help_topic: 0,
            win_slideshow: false,
            win_tts: false,
            win_render_queue: false,
            win_paste_repeat: false,
            win_paste_attrs: false,
            win_track_fx: None,
            win_track_motion: None,
            win_event_audio: None,
            win_tracking: None,
            win_multicam: false,
            win_media_props: None,
            win_output_fx: false,
            win_customize: false,
            ext_monitor: false,
            ss_secs: DEFAULT_STILL_LEN,
            ss_overlap: 1.0,
            ss_kb: true,
            tts_text: String::new(),
            paste_gap: 0.0,
            attr_mask: Default::default(),
            script_text: "# Exemple : marqueurs toutes les 5 secondes\nadd_marker {\"time\": 0, \"name\": \"Début\"}\nadd_marker {\"time\": 5, \"name\": \"5 s\"}\n".into(),
            script_out: String::new(),
            media_search: String::new(),
            media_bin: String::new(),
            tasks: vec![],
            tracking_rect: [0.4, 0.4, 0.2, 0.2],
            tracking_target: None,
            tracking_result: None,
            multicam: None,
            ram_cache: HashMap::new(),
            ram_sig: (0, 0, 0),
            metronome: false,
            dim: false,
            mute_audio: false,
            mute_video: false,
            recorder: None,
            render_queue: vec![],
            pending_track_render: None,
            proxy_results: Arc::new(std::sync::Mutex::new(vec![])),
            proxy_map: HashMap::new(),
            last_autosave: Instant::now(),
            automation_writes: vec![],
            sel_rect: None,
        };
        // BLUE_LAYOUT=n loads a preset layout at start (used to check them).
        if let Some(n) = std::env::var("BLUE_LAYOUT").ok().and_then(|s| s.parse::<usize>().ok()) {
            if let Some(p) = crate::docks::Preset::ALL.get(n) {
                app.load_preset(*p);
            }
        }
        if let Some(e) = app.p.tracks.get(1).and_then(|t| t.events.get(1)) {
            app.sel.insert(e.id);
        }
        if !media_files.is_empty() {
            let ids = app.import_files(media_files, &cc.egui_ctx);
            let mut t = app.p.end_time();
            app.cursor = t + 1.0;
            for id in ids {
                app.insert_media(id, t, Some(1), false);
                t += app.p.media(id).map(|m| if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN }).unwrap_or(0.0);
            }
            app.tl.fit(app.p.end_time());
        }
        // BLUE_SHOW_RENDER=1 opens the render dialog at start (used to check it).
        if std::env::var("BLUE_SHOW_RENDER").is_ok() {
            app.open_render_dialog();
        }
        // BLUE_SHOW_AI=1 opens the AI windows at start (used to check them).
        if std::env::var("BLUE_SHOW_AI").is_ok() {
            app.ai.win_trim = true;
            app.ai.win_claude = true;
            app.ai.chat.lines.push(crate::claude::Line::User("Coupe les silences de ma voix off".into()));
            app.ai.chat.lines.push(crate::claude::Line::Tool("Outil : auto_trim".into()));
            app.ai.chat.lines.push(crate::claude::Line::Assistant("C'est fait : 2,2 s de silences et une hésitation retirées.".into()));
        }
        app
    }

    // ------------------------------------------------------------------ edits

    pub fn push_undo(&mut self) {
        self.undo.push(self.p.clone());
        self.undo_labels.push(String::new());
        while self.undo.len() > self.prefs.undo_levels.max(10) {
            self.undo.remove(0);
            self.undo_labels.remove(0);
        }
        self.redo.clear();
        self.redo_labels.clear();
        self.dirty = true;
        self.rev += 1;
    }

    pub fn touch(&mut self) {
        self.rev += 1;
        self.dirty = true;
    }

    pub fn do_undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.p, prev));
            let l = self.undo_labels.pop().unwrap_or_default();
            self.redo_labels.push(l);
            self.rev += 1;
            self.status = "Annulé.".into();
        }
    }

    pub fn do_redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.p, next));
            let l = self.redo_labels.pop().unwrap_or_default();
            self.undo_labels.push(l);
            self.rev += 1;
            self.status = "Rétabli.".into();
        }
    }

    pub fn select_event(&mut self, id: Id, additive: bool) {
        if !additive {
            self.sel.clear();
        }
        let group = self.p.event(id).and_then(|e| e.group);
        if additive && self.sel.contains(&id) {
            self.sel.remove(&id);
            return;
        }
        self.sel.insert(id);
        if let Some(g) = group {
            for t in &self.p.tracks {
                for e in &t.events {
                    if e.group == Some(g) {
                        self.sel.insert(e.id);
                    }
                }
            }
        }
    }

    pub fn delete_selected(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.push_undo();
        let ripple = self.ripple;
        for t in &mut self.p.tracks {
            let mut removed: Vec<(f64, f64)> = vec![];
            t.events.retain(|e| {
                if self.sel.contains(&e.id) {
                    removed.push((e.start, e.length));
                    false
                } else {
                    true
                }
            });
            if ripple {
                removed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                for (s, len) in removed {
                    for e in &mut t.events {
                        if e.start >= s + len - 1e-6 {
                            e.start -= len;
                        }
                    }
                }
            }
        }
        self.sel.clear();
        self.status = "Événements supprimés.".into();
    }

    /// Split selected events (or all events under the cursor) at the cursor.
    pub fn split_at_cursor(&mut self) {
        let t = self.cursor;
        let targets: Vec<Id> = self
            .p
            .tracks
            .iter()
            .flat_map(|tr| tr.events.iter())
            .filter(|e| e.start + 1e-6 < t && t < e.end() - 1e-6)
            .filter(|e| self.sel.is_empty() || self.sel.contains(&e.id))
            .map(|e| e.id)
            .collect();
        if targets.is_empty() {
            self.status = "Aucun événement sous le curseur à diviser.".into();
            return;
        }
        self.push_undo();
        let mut new_groups: HashMap<Id, Id> = HashMap::new();
        for id in targets {
            let Some((ti, ei)) = self.p.find_event(id) else { continue };
            let mut right = self.p.tracks[ti].events[ei].clone();
            right.id = self.p.new_id();
            if let Some(g) = right.group {
                let ng = *new_groups.entry(g).or_insert_with(|| {
                    self.p.next_id += 1;
                    self.p.next_id - 1
                });
                right.group = Some(ng);
            }
            let left = &mut self.p.tracks[ti].events[ei];
            let cut = t - left.start;
            right.start = t;
            right.offset = left.offset + cut;
            right.length = left.length - cut;
            right.fade_in = 0.0;
            left.length = cut;
            left.fade_out = 0.0;
            self.sel.insert(right.id);
            self.p.tracks[ti].events.insert(ei + 1, right);
        }
        self.p.sort_events();
        self.status = format!("Divisé à {}.", timecode(t, self.p.fps));
    }

    pub fn copy_selected(&mut self) {
        let mut items = vec![];
        let mut min_start = f64::MAX;
        for (ti, t) in self.p.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel.contains(&e.id) {
                    min_start = min_start.min(e.start);
                    items.push((ti, e.clone()));
                }
            }
        }
        for (_, e) in &mut items {
            e.start -= min_start;
        }
        if !items.is_empty() {
            self.status = format!("{} événement(s) copié(s).", items.len());
            self.clipboard = items;
        }
    }

    pub fn cut_selected(&mut self) {
        self.copy_selected();
        self.delete_selected();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.push_undo();
        self.sel.clear();
        let at = self.cursor;
        let span = self
            .clipboard
            .iter()
            .map(|(_, e)| e.end())
            .fold(0.0, f64::max);
        let mut groups: HashMap<Id, Id> = HashMap::new();
        let items = self.clipboard.clone();
        for (ti, mut e) in items {
            if ti >= self.p.tracks.len() {
                continue;
            }
            if self.ripple {
                for o in &mut self.p.tracks[ti].events {
                    if o.start >= at - 1e-6 {
                        o.start += span;
                    }
                }
            }
            e.id = self.p.new_id();
            e.start += at;
            if let Some(g) = e.group {
                let ng = *groups.entry(g).or_insert_with(|| {
                    self.p.next_id += 1;
                    self.p.next_id - 1
                });
                e.group = Some(ng);
            }
            self.sel.insert(e.id);
            self.p.tracks[ti].events.push(e);
        }
        self.p.sort_events();
        self.cursor = at + span;
        self.status = "Collé.".into();
    }

    pub fn select_all(&mut self) {
        self.sel = self
            .p
            .tracks
            .iter()
            .flat_map(|t| t.events.iter().map(|e| e.id))
            .collect();
    }

    pub fn add_marker(&mut self) {
        self.push_undo();
        let n = self.p.markers.len() + 1;
        let t = self.p.quantize(self.cursor);
        self.p.markers.push(Marker {
            time: t,
            name: format!("{}", n),
            command: false,
        });
        self.p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    fn track_of_kind(&mut self, kind: TrackKind, prefer: usize) -> usize {
        if let Some(t) = self.p.tracks.get(prefer) {
            if t.kind == kind {
                return prefer;
            }
        }
        if let Some(i) = self.p.tracks.iter().position(|t| t.kind == kind) {
            return i;
        }
        let at = match kind {
            TrackKind::Video => Some(0),
            TrackKind::Audio => None,
        };
        self.p.add_track(kind, at)
    }

    /// Put a media on the timeline at `time` (video and audio parts grouped).
    pub fn insert_media(&mut self, media_id: Id, time: f64, prefer_track: Option<usize>, undo: bool) {
        let Some(m) = self.p.media(media_id).cloned() else { return };
        if undo {
            self.push_undo();
        }
        let time = if self.quantize { self.p.quantize(time.max(0.0)) } else { time.max(0.0) };
        let len = if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN };
        let group = if m.has_video && m.has_audio {
            Some(self.p.new_id())
        } else {
            None
        };
        self.sel.clear();
        let pref = prefer_track.unwrap_or(self.focus_track);
        if m.has_video {
            let ti = self.track_of_kind(TrackKind::Video, pref);
            let mut e = self.p.new_event(&m, time, len);
            e.group = group;
            self.sel.insert(e.id);
            self.p.tracks[ti].events.push(e);
        }
        if m.has_audio {
            let pref_a = if m.has_video { usize::MAX } else { pref };
            let ti = self.track_of_kind(TrackKind::Audio, pref_a);
            let mut e = self.p.new_event(&m, time, len);
            e.group = group;
            self.sel.insert(e.id);
            self.p.tracks[ti].events.push(e);
        }
        self.p.sort_events();
        self.store.ensure(&m, &self.server_ctx());
        self.status = format!("« {} » ajouté à la ligne de temps.", m.name);
    }

    pub fn server_ctx(&self) -> egui::Context {
        self.ctx.clone()
    }

    pub fn import_files(&mut self, paths: Vec<String>, ctx: &egui::Context) -> Vec<Id> {
        let mut ids = vec![];
        let mut errors = vec![];
        for path in paths {
            if let Some(m) = self.p.media.iter().find(|m| match &m.source {
                MediaSource::File(p) | MediaSource::Image(p) => *p == path,
                _ => false,
            }) {
                ids.push(m.id);
                continue;
            }
            match media::probe(&path) {
                Ok(m) => {
                    let id = self.p.add_media(m);
                    let m = self.p.media(id).unwrap().clone();
                    self.store.ensure(&m, ctx);
                    ids.push(id);
                }
                Err(e) => errors.push(e),
            }
        }
        self.touch();
        if !errors.is_empty() {
            self.status = errors.join(" ");
        } else if !ids.is_empty() {
            self.status = format!("{} média(s) importé(s).", ids.len());
        }
        ids
    }

    pub fn add_generator(&mut self, g: Generator, ctx: &egui::Context) -> Id {
        let audio = g.is_audio();
        let m = Media {
            id: 0,
            name: g.label(),
            source: MediaSource::Generated(g),
            has_video: !audio,
            has_audio: audio,
            duration: if audio { 30.0 } else { 0.0 },
            width: self.p.width,
            height: self.p.height,
            fps: 0.0,
            bin: String::new(),
            tags: vec![],
            proxy: None,
            fx: vec![],
        };
        let id = self.p.add_media(m);
        let m = self.p.media(id).unwrap().clone();
        self.store.ensure(&m, ctx);
        id
    }

    pub fn new_project(&mut self) {
        self.push_undo();
        let mut p = Project::empty();
        p.add_track(TrackKind::Video, None);
        p.add_track(TrackKind::Audio, None);
        self.p = p;
        self.path = None;
        self.sel.clear();
        self.cursor = 0.0;
        self.tl = TlView::default();
        self.status = "Nouveau projet : choisissez son format (horizontal, vertical, 4K…).".into();
        self.win_project = true;
    }

    pub fn save(&mut self, save_as: bool) {
        // The dialog opens in the project's own
        // folder when it exists, otherwise in a default folder; an empty name
        // becomes "Untitled"; the extension is added when the
        // typed name does not already end with it (case-insensitive).
        let path = if save_as || self.path.is_none() {
            let name = if self.p.name.trim().is_empty() { tr("Sans titre").to_string() } else { self.p.name.clone() };
            let dir = self
                .path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()))
                .filter(|d| d.is_dir())
                .or_else(crate::platform::videos_dir);
            let mut dlg = rfd::FileDialog::new()
                .add_filter(PROJECT_FILTER, &[PROJECT_EXT])
                .set_file_name(with_project_ext(&name));
            if let Some(d) = dir {
                dlg = dlg.set_directory(d);
            }
            dlg.save_file().map(|p| with_project_ext(&p.to_string_lossy()))
        } else {
            self.path.clone()
        };
        let Some(path) = path else { return };
        match serde_json::to_string_pretty(&self.p) {
            Ok(s) => match std::fs::write(&path, s) {
                Ok(()) => {
                    self.path = Some(path.clone());
                    self.dirty = false;
                    self.add_recent(&path);
                    self.status = format!("Projet enregistré : {}", path);
                }
                Err(e) => self.status = format!("Échec de l'enregistrement : {e}"),
            },
            Err(e) => self.status = format!("Échec de l'enregistrement : {e}"),
        }
    }

    pub fn open(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(PROJECT_FILTER, &[PROJECT_EXT, "bmproj"])
            .pick_file()
        else {
            return;
        };
        self.open_path(&path.to_string_lossy(), ctx);
    }

    pub fn import_dialog(&mut self, ctx: &egui::Context) {
        let mut all: Vec<&str> = vec![];
        all.extend_from_slice(media::VIDEO_EXT);
        all.extend_from_slice(media::AUDIO_EXT);
        all.extend_from_slice(media::IMAGE_EXT);
        if let Some(files) = rfd::FileDialog::new()
            .add_filter("Tous les médias", &all)
            .add_filter("Vidéo", media::VIDEO_EXT)
            .add_filter("Audio", media::AUDIO_EXT)
            .add_filter("Images", media::IMAGE_EXT)
            .pick_files()
        {
            let paths = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
            self.import_files(paths, ctx);
            self.left_tab = LeftTab::ProjectMedia;
        }
    }

    // --------------------------------------------------------------- playback

    pub fn play(&mut self) {
        if !self.playing {
            self.playing = true;
            self.speed = 1.0;
            self.play_origin = self.cursor;
            if let Some((a, b)) = self.active_loop() {
                if self.cursor < a || self.cursor >= b {
                    self.cursor = a;
                }
            }
            self.audio.shared.set_pos(self.cursor);
        }
    }

    pub fn stop(&mut self) {
        if self.playing {
            self.playing = false;
            // Classic NLE behaviour: stop returns the cursor to where playback started
            self.cursor = self.play_origin;
        }
        self.speed = 1.0;
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.speed = 1.0;
    }

    pub fn toggle_play(&mut self) {
        if self.playing {
            self.stop();
        } else {
            self.play();
        }
    }

    pub fn active_loop(&self) -> Option<(f64, f64)> {
        if self.loop_on {
            self.p.loop_region.filter(|(a, b)| b > a)
        } else {
            None
        }
    }

    fn tick_playback(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        let mix_key = self.rev ^ ((self.metronome as u64) << 62);
        if self.mix_rev != mix_key {
            let mut mix = audio::build_mix(&self.p, &self.store, self.auto_xf);
            mix.metronome = if self.metronome { Some(self.p.tempo) } else { None };
            self.mix = Arc::new(mix);
            *self.audio.shared.mix.lock().unwrap() = self.mix.clone();
            self.mix_rev = mix_key;
        }
        *self.audio.shared.loop_region.lock().unwrap() = self.active_loop();
        let device_plays = self.playing && self.audio.has_device() && (self.speed - 1.0).abs() < 1e-9;
        self.audio.shared.playing.store(device_plays, std::sync::atomic::Ordering::Relaxed);
        if self.playing {
            if device_plays {
                self.cursor = self.audio.shared.pos();
            } else {
                self.cursor += dt * self.speed;
                if let Some((a, b)) = self.active_loop() {
                    if self.cursor >= b {
                        self.cursor = a + (self.cursor - b);
                    }
                }
                if self.cursor < 0.0 {
                    self.cursor = 0.0;
                    self.playing = false;
                }
                self.audio.shared.set_pos(self.cursor);
            }
            let end = self.p.end_time();
            if self.active_loop().is_none() && self.cursor > end + 0.5 && self.speed > 0.0 {
                self.playing = false;
                self.cursor = end;
            }
            // keep the cursor visible
            let vis = self.tl.visible_secs();
            if self.cursor > self.tl.scroll + vis * 0.95 || self.cursor < self.tl.scroll {
                self.tl.scroll = (self.cursor - vis * 0.05).max(0.0);
            }
            ctx.request_repaint();
        } else {
            self.audio.shared.set_pos(self.cursor);
        }
        // meters (33 ms refresh, -60 dB floor)
        let (l, r) = if device_plays {
            self.audio.shared.take_peaks()
        } else if self.playing {
            audio::peak_at(&self.mix, self.cursor, 0.033)
        } else {
            (0.0, 0.0)
        };
        for (i, v) in [l, r].into_iter().enumerate() {
            let decay = (self.meters[i] - dt as f32 * 1.5).max(0.0);
            self.meters[i] = v.max(decay);
            self.peak_hold[i] = self.peak_hold[i].max(v);
        }
        if self.playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        } else if self.meters.iter().any(|m| *m > 0.0) {
            ctx.request_repaint();
        }
    }

    // -------------------------------------------------------------- shortcuts

    pub fn mark_in(&mut self) {
        let t = self.p.quantize(self.cursor);
        let end = self.p.loop_region.map(|(_, b)| b).filter(|b| *b > t).unwrap_or(t + 1.0);
        self.p.loop_region = Some((t, end));
    }

    pub fn mark_out(&mut self) {
        let t = self.p.quantize(self.cursor);
        let start = self
            .p
            .loop_region
            .map(|(a, _)| a)
            .filter(|a| *a < t)
            .unwrap_or((t - 1.0).max(0.0));
        self.p.loop_region = Some((start, t));
    }

    /// G groups the selection, U removes it from its group.
    pub fn group_selected(&mut self, group: bool) {
        if self.sel.is_empty() {
            return;
        }
        self.push_undo();
        let g = if group { Some(self.p.new_id()) } else { None };
        let sel = self.sel.clone();
        for t in &mut self.p.tracks {
            for e in &mut t.events {
                if sel.contains(&e.id) {
                    e.group = g;
                }
            }
        }
        self.status = if group {
            "Événements groupés.".into()
        } else {
            "Événements retirés du groupe.".into()
        };
    }

    pub fn open_render_dialog(&mut self) {
        let base = crate::platform::videos_dir().unwrap_or_default();
        let settings = self
            .last_render
            .clone()
            .unwrap_or_else(|| export::RenderSettings::for_project(&self.p));
        self.win_render = Some(RenderDialog {
            path: base
                .join(format!("{}.{}", self.p.name, settings.format.ext()))
                .to_string_lossy()
                .to_string(),
            settings,
            loop_only: self.p.loop_region.is_some() && self.loop_on,
        });
    }

    // ------------------------------------------------------------------- menus

    pub fn save_snapshot(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Image PNG", &["png"])
            .set_file_name("instantané.png")
            .save_file()
        else {
            return;
        };
        let (w, h) = (self.p.width, self.p.height);
        let mut src = crate::compositor::ExactSource::new(self.p.fps);
        let img = self.comp.compose(&self.p, self.cursor, w, h, self.auto_xf, &mut src);
        match image::save_buffer(&path, &img, w, h, image::ColorType::Rgba8) {
            Ok(()) => self.status = format!("Instantané enregistré : {}", path.display()),
            Err(e) => self.status = format!("Échec de l'instantané : {e}"),
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.as_ref().map(|p| p.to_string_lossy().to_string()))
                .collect()
        });
        if files.is_empty() {
            return;
        }
        let ids = self.import_files(files, ctx);
        if !ids.is_empty() {
            self.push_undo();
            let mut t = self.cursor;
            for id in ids {
                self.insert_media(id, t, None, false);
                let len = self.p.media(id).map(|m| if m.duration > 0.0 { m.duration } else { DEFAULT_STILL_LEN }).unwrap_or(0.0);
                t += len;
            }
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else { return };
        ctx.request_repaint();
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut raw = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                raw.extend_from_slice(&p.to_srgba_unmultiplied());
            }
            if let Some(dir) = shot.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = image::save_buffer(&shot.path, &raw, w as u32, h as u32, image::ColorType::Rgba8) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frame_no >= 120 && !shot.requested {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_no += 1;
        self.server.set_fps(self.p.fps);
        self.screenshot_step(ctx);
        self.handle_dropped_files(ctx);
        self.shortcuts(ctx);
        self.tick_playback(ctx);
        self.server.collect();
        self.comp.clear_cache();
        // global monitoring switches, background work, autosave
        crate::audio::DIM.store(self.dim, std::sync::atomic::Ordering::Relaxed);
        crate::audio::MUTE_ALL.store(self.mute_audio, std::sync::atomic::Ordering::Relaxed);
        self.poll_tasks(ctx);
        self.poll_proxies();
        self.process_render_queue(ctx);
        self.autosave_tick();
        for (ti, pan, v) in std::mem::take(&mut self.automation_writes) {
            self.record_automation(ti, pan, v);
        }
        if self.recorder.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // "What's this?" help (Help.WhatsThis, Maj+F1): help bubbles appear at once
        let ws = self.prefs.whats_this;
        if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("whats_this_applied"))) != Some(ws) {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("whats_this_applied"), ws));
            for th in [egui::Theme::Dark, egui::Theme::Light] {
                ctx.style_mut_of(th, |s| {
                    s.interaction.tooltip_delay = if ws { 0.0 } else { 0.5 };
                    s.interaction.show_tooltips_only_when_still = !ws;
                });
            }
        }
        // label the last undo step with the message of the action
        if let Some(l) = self.undo_labels.last_mut() {
            if l.is_empty() {
                *l = self.status.trim_end_matches('.').chars().take(60).collect();
            }
        }

        let title = window_title(
            &self
                .path
                .as_ref()
                .map(|p| std::path::Path::new(p).file_name().unwrap().to_string_lossy().to_string())
                .unwrap_or_else(|| with_project_ext(&self.p.name)),
            self.dirty,
        );
        if self.frame_no % 30 == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        egui::TopBottomPanel::top("menu")
            .frame(egui::Frame::new().fill(crate::theme::BAR()).inner_margin(egui::Margin::symmetric(4, 2)))
            .show(ctx, |ui| self.menu_bar(ui, ctx));
        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::new().fill(crate::theme::BAR()).inner_margin(egui::Margin::symmetric(4, 2)))
            .show(ctx, |ui| self.toolbar(ui, ctx));
        egui::TopBottomPanel::bottom("status")
            .exact_height(22.0)
            .frame(egui::Frame::new().fill(crate::theme::BAR()).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ctx, |ui| self.status_bar(ui));
        egui::TopBottomPanel::bottom("transport")
            .exact_height(32.0)
            .frame(egui::Frame::new().fill(crate::theme::PANEL()).inner_margin(egui::Margin::symmetric(6, 3)))
            .show(ctx, |ui| self.transport(ui));
        egui::TopBottomPanel::bottom("tl_toolbar")
            .exact_height(24.0)
            .frame(egui::Frame::new().fill(crate::theme::BAR()).inner_margin(egui::Margin::symmetric(6, 1)))
            .show(ctx, |ui| self.timeline_toolbar(ui));
        if !self.dock_hidden {
            let panel = if self.timeline_top { egui::TopBottomPanel::bottom("dock") } else { egui::TopBottomPanel::top("dock") };
            let r = panel
                .resizable(true)
                .default_height(self.dock_height)
                .height_range(120.0..=900.0)
                .frame(egui::Frame::new().fill(crate::theme::BG()))
                .show(ctx, |ui| self.dock_area(ui, ctx));
            self.dock_height = r.response.rect.height();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::theme::BG()))
            .show(ctx, |ui| self.timeline(ui, ctx));

        self.windows(ctx);
        self.dialogs(ctx);
        self.ai_frame(ctx);
    }
}

fn dirs_home() -> Option<PathBuf> {
    crate::platform::home()
}

pub fn default_text() -> Generator {
    Generator::Text {
        text: "Texte d'exemple".into(),
        size_pct: 10,
        color: [255, 255, 255, 255],
        background: [0, 0, 0, 0],
    }
}

/// The project shown at start: generated media only, so it works without files.
pub fn demo_project() -> Project {
    let mut p = Project::empty();
    p.name = "Mon premier montage".into();
    let v1 = p.add_track(TrackKind::Video, None);
    let v2 = p.add_track(TrackKind::Video, None);
    let a1 = p.add_track(TrackKind::Audio, None);
    let a2 = p.add_track(TrackKind::Audio, None);
    p.tracks[v1].name = "Titres".into();
    p.tracks[v2].name = "Images".into();
    p.tracks[a1].name = "Musique".into();
    p.tracks[a2].name = "Ambiance".into();
    p.tracks[a2].level = 0.5;
    p.tracks[a2].pan = -0.3;

    let mk = |p: &mut Project, g: Generator, dur: f64| -> Media {
        let audio = g.is_audio();
        let m = Media {
            id: 0,
            name: g.label(),
            source: MediaSource::Generated(g),
            has_video: !audio,
            has_audio: audio,
            duration: if audio { dur } else { 0.0 },
            width: 1920,
            height: 1080,
            fps: 0.0,
            bin: String::new(),
            tags: vec![],
            proxy: None,
            fx: vec![],
        };
        let id = p.add_media(m);
        p.media(id).unwrap().clone()
    };
    let title = mk(
        &mut p,
        Generator::Text {
            text: "Mon premier montage".into(),
            size_pct: 11,
            color: [255, 255, 255, 255],
            background: [0, 0, 0, 0],
        },
        0.0,
    );
    let sub = mk(
        &mut p,
        Generator::Text {
            text: "Chapitre 1".into(),
            size_pct: 8,
            color: [255, 236, 170, 255],
            background: [0, 0, 0, 0],
        },
        0.0,
    );
    let bars = mk(&mut p, Generator::ColorBars, 0.0);
    let grad = mk(&mut p, Generator::Gradient([24, 64, 140, 255], [220, 120, 60, 255]), 0.0);
    let grad2 = mk(&mut p, Generator::Gradient([30, 120, 90, 255], [20, 30, 60, 255]), 0.0);
    let solid = mk(&mut p, Generator::SolidColor([120, 40, 90, 255]), 0.0);
    let music = mk(&mut p, Generator::DemoMusic { seed: 0 }, 24.0);
    let amb = mk(&mut p, Generator::DemoMusic { seed: 2 }, 24.0);

    let fps = p.fps;
    let q = |t: f64| (t * fps).round() / fps;
    let mut e = p.new_event(&title, q(0.5), q(6.0));
    e.fade_in = 1.0;
    e.fade_out = 1.5;
    p.tracks[v1].events.push(e);
    let mut e = p.new_event(&sub, q(11.0), q(5.0));
    e.fade_in = 0.7;
    e.fade_out = 0.7;
    p.tracks[v1].events.push(e);

    let e = p.new_event(&bars, 0.0, q(5.0));
    p.tracks[v2].events.push(e);
    let mut e = p.new_event(&grad, q(4.0), q(7.0));
    e.transition = Transition::Dissolve;
    p.tracks[v2].events.push(e);
    let mut e = p.new_event(&grad2, q(10.0), q(6.0));
    e.transition = Transition::WipeLeft;
    e.fx.push(VideoFx::Vignette { amount: 0.6 });
    p.tracks[v2].events.push(e);
    let mut e = p.new_event(&solid, q(15.0), q(5.0));
    e.fade_out = 1.0;
    p.tracks[v2].events.push(e);

    let mut e = p.new_event(&music, 0.0, q(20.0));
    e.fade_in = 0.5;
    e.fade_out = 2.5;
    p.tracks[a1].events.push(e);
    let mut e = p.new_event(&amb, q(8.0), q(10.0));
    e.fade_in = 1.5;
    e.fade_out = 1.5;
    e.curve_in = Curve::Slow;
    e.curve_out = Curve::Fast;
    p.tracks[a2].events.push(e);

    p.markers.push(Marker {
        time: q(4.0),
        name: "1".into(),
        command: false,
    });
    p.markers.push(Marker {
        time: q(11.0),
        name: "2".into(),
        command: false,
    });
    p.loop_region = Some((q(4.0), q(11.0)));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_title_format() {
        let v = crate::platform::version_folder();
        assert_eq!(window_title("Film.blue", true), format!("Film.blue * - Blue Edit {v}"));
        assert_eq!(window_title("Film.blue", false), format!("Film.blue - Blue Edit {v}"));
    }
    #[test]
    fn extension_added_once() {
        assert_eq!(with_project_ext("Film"), "Film.blue");
        assert_eq!(with_project_ext("Film.BLUE"), "Film.BLUE");
        assert_eq!(with_project_ext("Film.mp4"), "Film.mp4.blue");
        assert_eq!(with_project_ext("Ancien.bmproj"), "Ancien.bmproj");
    }
}
