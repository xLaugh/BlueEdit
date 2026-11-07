//! Editing commands (Edit, Insert,
//! Tracks menus): heal, paste insert/repeat/attributes, trims, takes, lock,
//! nudge, regions, envelopes, track groups, ruler formats, edit tools.

use crate::app::App;
use crate::i18n::tr;
use crate::model::*;
use std::collections::HashSet;

/// Timeline edit tools (Edit > Editing Tool).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditTool {
    #[default]
    Normal,
    Envelope,
    Selection,
    Zoom,
    Slip,
    Slide,
    Shuffle,
    SplitTrim,
    Stretch,
}

impl EditTool {
    pub const ALL: [EditTool; 9] = [
        EditTool::Normal,
        EditTool::Envelope,
        EditTool::Selection,
        EditTool::Zoom,
        EditTool::Slip,
        EditTool::Slide,
        EditTool::Shuffle,
        EditTool::SplitTrim,
        EditTool::Stretch,
    ];
    pub fn label(self) -> &'static str {
        match self {
            EditTool::Normal => tr("Normal"),
            EditTool::Envelope => tr("Enveloppe"),
            EditTool::Selection => tr("Sélection"),
            EditTool::Zoom => tr("Zoom"),
            EditTool::Slip => tr("Glisser (slip)"),
            EditTool::Slide => tr("Faire coulisser (slide)"),
            EditTool::Shuffle => tr("Remanier (shuffle)"),
            EditTool::SplitTrim => tr("Rogner/diviser"),
            EditTool::Stretch => tr("Étirer/compresser le temps"),
        }
    }
    pub fn tip(self) -> &'static str {
        match self {
            EditTool::Normal => tr("Déplacer, rogner, fondus (Ctrl+D)"),
            EditTool::Envelope => tr("Cliquer pour ajouter un point d'enveloppe, glisser pour le déplacer"),
            EditTool::Selection => tr("Tracer un rectangle pour sélectionner des événements"),
            EditTool::Zoom => tr("Cliquer pour zoomer, tracer une zone pour l'agrandir (Maj : zoom arrière)"),
            EditTool::Slip => tr("Glisser le contenu dans l'événement sans le déplacer (Alt+glisser)"),
            EditTool::Slide => tr("Déplacer l'événement en ajustant ses voisins"),
            EditTool::Shuffle => tr("Changer l'ordre des événements (les autres se décalent)"),
            EditTool::SplitTrim => tr("Cliquer pour diviser, glisser un bord pour rogner"),
            EditTool::Stretch => tr("Glisser un bord pour changer la vitesse (Ctrl+glisser)"),
        }
    }
}

/// Which envelope the envelope tool edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnvKind {
    #[default]
    Volume,
    Pan,
    FadeToColor,
    Velocity,
}

impl EnvKind {
    pub const ALL: [EnvKind; 4] = [EnvKind::Volume, EnvKind::Pan, EnvKind::FadeToColor, EnvKind::Velocity];
    pub fn label(self) -> &'static str {
        match self {
            EnvKind::Volume => tr("Volume / niveau de composition"),
            EnvKind::Pan => tr("Panoramique audio"),
            EnvKind::FadeToColor => tr("Fondu vers une couleur"),
            EnvKind::Velocity => tr("Vitesse de l'événement"),
        }
    }
    /// Value range of the envelope.
    pub fn range(self, kind: TrackKind) -> (f32, f32) {
        match self {
            EnvKind::Volume => {
                if kind == TrackKind::Audio {
                    (0.0, 2.0)
                } else {
                    (0.0, 1.0)
                }
            }
            EnvKind::Pan | EnvKind::FadeToColor => (-1.0, 1.0),
            // event velocity envelope: -100 % .. 1000 %
            EnvKind::Velocity => (-100.0, 1000.0),
        }
    }
}

/// Ruler / time display formats (Options > Time Format).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RulerFormat {
    #[default]
    Smpte,
    SmpteNonDrop,
    SmpteEbu,
    SmpteFilm,
    Seconds,
    Frames,
    MeasuresBeats,
    Samples,
    FeetFrames,
    CdTime,
}

impl RulerFormat {
    pub const ALL: [RulerFormat; 10] = [
        RulerFormat::Smpte,
        RulerFormat::SmpteNonDrop,
        RulerFormat::SmpteEbu,
        RulerFormat::SmpteFilm,
        RulerFormat::Seconds,
        RulerFormat::Frames,
        RulerFormat::MeasuresBeats,
        RulerFormat::Samples,
        RulerFormat::FeetFrames,
        RulerFormat::CdTime,
    ];
    pub fn label(self) -> &'static str {
        match self {
            RulerFormat::Smpte => tr("Temps (SMPTE, selon le projet)"),
            RulerFormat::SmpteNonDrop => tr("SMPTE 30 sans saut d'images"),
            RulerFormat::SmpteEbu => tr("SMPTE EBU (25 i/s)"),
            RulerFormat::SmpteFilm => tr("SMPTE film (24 i/s)"),
            RulerFormat::Seconds => tr("Secondes"),
            RulerFormat::Frames => tr("Images absolues"),
            RulerFormat::MeasuresBeats => tr("Mesures et temps"),
            RulerFormat::Samples => tr("Échantillons"),
            RulerFormat::FeetFrames => tr("Pieds et images (35 mm)"),
            RulerFormat::CdTime => tr("Temps CD (75 i/s)"),
        }
    }
    pub fn format(self, t: f64, p: &Project) -> String {
        let t = t.max(0.0);
        let nd = |fps: f64| {
            let f = (t * fps + 1e-6).floor() as i64;
            let n = fps.round() as i64;
            format!("{:02}:{:02}:{:02}:{:02}", f / (n * 3600), (f / (n * 60)) % 60, (f / n) % 60, f % n)
        };
        match self {
            RulerFormat::Smpte => timecode(t, p.fps),
            RulerFormat::SmpteNonDrop => nd(30.0),
            RulerFormat::SmpteEbu => nd(25.0),
            RulerFormat::SmpteFilm => nd(24.0),
            RulerFormat::Seconds => format!("{:.3}", t).replace('.', ","),
            RulerFormat::Frames => format!("{}", (t * p.fps + 1e-6).floor() as i64),
            RulerFormat::MeasuresBeats => {
                // 4/4 at the project tempo: bar.beat.tick (960 ticks per beat)
                let beats = t * p.tempo / 60.0;
                let bar = (beats / 4.0).floor() as i64 + 1;
                let beat = (beats % 4.0).floor() as i64 + 1;
                let tick = ((beats.fract()) * 960.0).floor() as i64;
                format!("{}.{}.{:03}", bar, beat, tick)
            }
            RulerFormat::Samples => format!("{}", (t * p.sample_rate as f64).round() as i64),
            RulerFormat::FeetFrames => {
                // 35 mm film: 16 frames per foot at 24 fps
                let f = (t * 24.0 + 1e-6).floor() as i64;
                format!("{}+{:02}", f / 16, f % 16)
            }
            RulerFormat::CdTime => {
                let f = (t * 75.0 + 1e-6).floor() as i64;
                format!("{:02}:{:02}.{:02}", f / (75 * 60), (f / 75) % 60, f % 75)
            }
        }
    }
}

/// What "Paste Event Attributes" copies.
#[derive(Clone, Copy, Debug)]
pub struct AttrMask {
    pub fx: bool,
    pub pan: bool,
    pub fades: bool,
    pub gain: bool,
    pub rate: bool,
    pub audio_fx: bool,
}

impl Default for AttrMask {
    fn default() -> Self {
        AttrMask { fx: true, pan: true, fades: true, gain: true, rate: false, audio_fx: true }
    }
}

impl App {
    /// Events the next command works on: the selection, or the events under
    /// the cursor on the focused track.
    fn targets(&self) -> Vec<Id> {
        if !self.sel.is_empty() {
            return self.sel.iter().copied().collect();
        }
        self.p
            .tracks
            .get(self.focus_track)
            .map(|t| t.events.iter().filter(|e| e.start <= self.cursor && self.cursor < e.end()).map(|e| e.id).collect())
            .unwrap_or_default()
    }

    fn is_locked(&self, id: Id) -> bool {
        self.p.event(id).map(|e| e.locked).unwrap_or(false)
    }

    /// Heal: joins split events back together when they are adjacent and
    /// continuous in the same media (Edit > Heal).
    pub fn heal(&mut self) {
        self.push_undo();
        let sel = self.sel.clone();
        let mut healed = 0;
        for t in &mut self.p.tracks {
            let mut i = 0;
            while i + 1 < t.events.len() {
                let (a, b) = (&t.events[i], &t.events[i + 1]);
                let any_sel = sel.is_empty() || sel.contains(&a.id) || sel.contains(&b.id);
                let continuous = a.media == b.media
                    && (a.end() - b.start).abs() < 1e-4
                    && (a.offset + a.length * a.rate - b.offset).abs() < 2e-3
                    && (a.rate - b.rate).abs() < 1e-9
                    && a.velocity.is_empty()
                    && b.velocity.is_empty();
                if any_sel && continuous {
                    let b = t.events.remove(i + 1);
                    let a = &mut t.events[i];
                    a.length += b.length;
                    a.fade_out = b.fade_out;
                    a.curve_out = b.curve_out;
                    healed += 1;
                } else {
                    i += 1;
                }
            }
        }
        self.status = format!("{} {}", healed, tr("raccord(s) recollé(s)."));
    }

    /// Paste Insert: pastes and pushes later events on the same tracks.
    pub fn paste_insert(&mut self) {
        let r = self.ripple;
        self.ripple = true;
        self.paste();
        self.ripple = r;
    }

    /// Paste Repeat: `count` copies, `gap` seconds apart (0 = end to end).
    pub fn paste_repeat(&mut self, count: u32, gap: f64) {
        if self.clipboard.is_empty() {
            return;
        }
        let span = self.clipboard.iter().map(|(_, e)| e.end()).fold(0.0, f64::max);
        let start = self.cursor;
        for k in 0..count.clamp(1, 100) {
            self.cursor = start + k as f64 * (span + gap);
            self.paste();
        }
        self.status = format!("{} {}", count, tr("copie(s) collée(s)."));
    }

    /// Paste Event Attributes: copies effects, pan/crop, fades… of the first
    /// copied event onto the selected events.
    pub fn paste_attributes(&mut self, mask: AttrMask) {
        let Some((_, src)) = self.clipboard.first().cloned() else {
            self.status = tr("Copiez d'abord un événement.").into();
            return;
        };
        self.push_undo();
        let sel = self.sel.clone();
        for t in &mut self.p.tracks {
            for e in &mut t.events {
                if !sel.contains(&e.id) {
                    continue;
                }
                if mask.fx {
                    e.fx = src.fx.clone();
                    e.fx_keys = src.fx_keys.clone();
                }
                if mask.pan {
                    e.pan = src.pan;
                    e.pan_keys = src.pan_keys.clone();
                }
                if mask.fades {
                    e.fade_in = src.fade_in.min(e.length);
                    e.fade_out = src.fade_out.min(e.length - e.fade_in);
                    e.curve_in = src.curve_in;
                    e.curve_out = src.curve_out;
                }
                if mask.gain {
                    e.gain = src.gain;
                }
                if mask.rate {
                    e.rate = src.rate;
                    e.reverse = src.reverse;
                    e.preserve_pitch = src.preserve_pitch;
                }
                if mask.audio_fx {
                    e.afx = src.afx.clone();
                    e.pitch = src.pitch;
                    e.normalize = src.normalize;
                    e.invert_phase = src.invert_phase;
                }
            }
        }
        self.status = tr("Attributs collés.").into();
    }

    /// Trim the selected events to the time selection (loop region).
    pub fn trim_to_selection(&mut self) {
        let Some((a, b)) = self.p.loop_region else {
            self.status = tr("Faites d'abord une sélection temporelle (région de boucle).").into();
            return;
        };
        self.push_undo();
        let ids: HashSet<Id> = self.targets().into_iter().collect();
        for t in &mut self.p.tracks {
            t.events.retain(|e| !(ids.contains(&e.id) && (e.end() <= a || e.start >= b)));
            for e in &mut t.events {
                if !ids.contains(&e.id) || e.locked {
                    continue;
                }
                if e.start < a {
                    let d = a - e.start;
                    e.offset += d * e.rate;
                    e.start = a;
                    e.length -= d;
                }
                if e.end() > b {
                    e.length = b - e.start;
                }
                e.fade_in = e.fade_in.min(e.length);
                e.fade_out = e.fade_out.min(e.length - e.fade_in);
            }
        }
        self.status = tr("Rogné à la sélection.").into();
    }

    /// Trim start (Alt+[) or end (Alt+]) of events under the cursor.
    pub fn trim_at_cursor(&mut self, start: bool) {
        let c = self.cursor;
        let ids = self.targets();
        if ids.is_empty() {
            return;
        }
        self.push_undo();
        for id in ids {
            let Some(e) = self.p.event_mut(id) else { continue };
            if e.locked || !(e.start < c && c < e.end()) {
                continue;
            }
            if start {
                let d = c - e.start;
                e.offset += d * e.rate;
                e.start = c;
                e.length -= d;
            } else {
                e.length = c - e.start;
            }
            e.fade_in = e.fade_in.min(e.length);
            e.fade_out = e.fade_out.min(e.length - e.fade_in);
        }
    }

    /// Next / previous take (T / Shift+T): swaps the active media with the
    /// event's next alternative take.
    pub fn cycle_take(&mut self, forward: bool) {
        let ids = self.targets();
        self.push_undo();
        let mut n = 0;
        for id in ids {
            let Some(e) = self.p.event_mut(id) else { continue };
            if e.takes.is_empty() {
                continue;
            }
            let cur = Take { media: e.media, offset: e.offset, name: e.name.clone() };
            let next = if forward { e.takes.remove(0) } else { e.takes.pop().unwrap() };
            if forward {
                e.takes.push(cur);
            } else {
                e.takes.insert(0, cur);
            }
            e.media = next.media;
            e.offset = next.offset;
            e.name = next.name;
            n += 1;
        }
        self.touch();
        self.status = if n > 0 { tr("Prise suivante active.").into() } else { tr("Cet événement n'a qu'une prise.").into() };
    }

    /// Adds a media as a new take of the selected event (and makes it active).
    pub fn add_take(&mut self, media: Id) {
        let Some(id) = self.sel.iter().next().copied() else { return };
        let Some(m) = self.p.media(media).cloned() else { return };
        self.push_undo();
        if let Some(e) = self.p.event_mut(id) {
            e.takes.push(Take { media: e.media, offset: e.offset, name: e.name.clone() });
            e.media = m.id;
            e.offset = 0.0;
            e.name = m.name.clone();
        }
    }

    pub fn delete_take(&mut self) {
        let ids = self.targets();
        self.push_undo();
        for id in ids {
            if let Some(e) = self.p.event_mut(id) {
                if let Some(t) = e.takes.pop() {
                    e.media = t.media;
                    e.offset = t.offset;
                    e.name = t.name;
                }
            }
        }
    }

    pub fn toggle_lock(&mut self) {
        let ids = self.targets();
        if ids.is_empty() {
            return;
        }
        self.push_undo();
        let lock = !ids.iter().all(|id| self.is_locked(*id));
        for id in ids {
            if let Some(e) = self.p.event_mut(id) {
                e.locked = lock;
            }
        }
        self.status = if lock { tr("Événement verrouillé.").into() } else { tr("Événement déverrouillé.").into() };
    }

    /// Nudge the selection by dt seconds (numeric keypad 4/6: one frame;
    /// 1/3: one pixel at the current zoom).
    pub fn nudge(&mut self, dt: f64) {
        if self.sel.is_empty() {
            return;
        }
        self.push_undo();
        let sel = self.sel.clone();
        for t in &mut self.p.tracks {
            for e in &mut t.events {
                if sel.contains(&e.id) && !e.locked {
                    e.start = (e.start + dt).max(0.0);
                }
            }
        }
        self.p.sort_events();
    }

    /// Moves the cursor to the other edge of the time selection.
    pub fn swap_cursor_selection(&mut self) {
        if let Some((a, b)) = self.p.loop_region {
            self.cursor = if (self.cursor - a).abs() < (self.cursor - b).abs() { b } else { a };
        }
    }

    /// Selects the next / previous event using the same media.
    pub fn select_similar(&mut self, next: bool) {
        let Some(id) = self.sel.iter().next().copied() else { return };
        let Some(e) = self.p.event(id).cloned() else { return };
        let mut all: Vec<&Event> = self.p.tracks.iter().flat_map(|t| t.events.iter()).filter(|o| o.media == e.media && o.id != e.id).collect();
        all.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        let found = if next { all.into_iter().find(|o| o.start > e.start) } else { all.into_iter().rev().find(|o| o.start < e.start) };
        if let Some(o) = found {
            let (oid, st) = (o.id, o.start);
            self.sel.clear();
            self.sel.insert(oid);
            self.cursor = st;
        }
    }

    /// Inserts an empty event (transparent) on the focused track.
    pub fn insert_empty_event(&mut self, ctx: &egui::Context) {
        let id = self.add_generator(Generator::SolidColor([0, 0, 0, 0]), ctx);
        if let Some(m) = self.p.media.iter_mut().find(|m| m.id == id) {
            m.name = tr("Événement vide").into();
        }
        self.insert_media(id, self.cursor, Some(self.focus_track), true);
    }

    pub fn insert_adjustment_event(&mut self, ctx: &egui::Context) {
        let id = self.add_generator(Generator::Adjustment, ctx);
        let ti = self
            .p
            .tracks
            .get(self.focus_track)
            .filter(|t| t.kind == TrackKind::Video)
            .map(|_| self.focus_track)
            .unwrap_or_else(|| {
                let i = self.p.add_track(TrackKind::Video, Some(0));
                self.p.tracks[i].name = tr("Ajustement").into();
                i
            });
        self.insert_media(id, self.cursor, Some(ti), true);
        self.status = tr("Événement d'ajustement inséré : ajoutez-lui des effets, ils s'appliquent aux pistes du dessous.").into();
    }

    /// Region from the time selection (R).
    pub fn add_region(&mut self) {
        let (a, b) = self.p.loop_region.unwrap_or((self.cursor, self.cursor + 5.0));
        self.push_undo();
        let n = self.p.regions.len() + 1;
        self.p.regions.push(Region { start: a, end: b, name: format!("{} {}", tr("Région"), n) });
        self.p.regions.sort_by(|x, y| x.start.partial_cmp(&y.start).unwrap());
    }

    pub fn add_command_marker(&mut self, text: &str) {
        self.push_undo();
        let t = self.p.quantize(self.cursor);
        self.p.markers.push(Marker { time: t, name: text.to_string(), command: true });
        self.p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }

    /// Converts the transition at the start of an event into a cut.
    pub fn transition_to_cut(&mut self, id: Id) {
        let Some((ti, ei)) = self.p.find_event(id) else { return };
        self.push_undo();
        let start = self.p.tracks[ti].events[ei].start;
        let mut cut = None;
        for o in &mut self.p.tracks[ti].events {
            if o.id != id && o.start < start && o.end() > start {
                let mid = start + (o.end() - start) / 2.0;
                o.length = mid - o.start;
                o.fade_out = 0.0;
                cut = Some(mid);
            }
        }
        if let Some(mid) = cut {
            let e = &mut self.p.tracks[ti].events[ei];
            let d = mid - e.start;
            e.offset += d * e.rate;
            e.start = mid;
            e.length -= d;
        }
        let e = &mut self.p.tracks[ti].events[ei];
        e.fade_in = 0.0;
        e.transition_env.clear();
    }

    /// Exclusive solo: only this track is soloed.
    pub fn exclusive_solo(&mut self, ti: usize) {
        self.push_undo();
        let kind = self.p.tracks[ti].kind;
        for (i, t) in self.p.tracks.iter_mut().enumerate() {
            if t.kind == kind {
                t.solo = i == ti;
            }
        }
    }

    pub fn exclusive_mute(&mut self, ti: usize) {
        self.push_undo();
        for (i, t) in self.p.tracks.iter_mut().enumerate() {
            t.mute = i != ti;
        }
    }

    /// Track height presets: minimise all, default (68 px), maximise focused.
    pub fn track_heights(&mut self, preset: u8) {
        for (i, t) in self.p.tracks.iter_mut().enumerate() {
            t.height = match preset {
                0 => MIN_TRACK_HEIGHT,
                1 => DEFAULT_TRACK_HEIGHT,
                _ => {
                    if i == self.focus_track {
                        220.0
                    } else {
                        MIN_TRACK_HEIGHT
                    }
                }
            };
        }
    }

    /// Groups the focused track with the next `n` tracks.
    pub fn group_tracks(&mut self, first: usize, count: usize) {
        if count == 0 {
            return;
        }
        self.push_undo();
        let gid = self.p.new_id();
        let n = self.p.track_groups.len() + 1;
        self.p.track_groups.push(TrackGroup { id: gid, name: format!("{} {}", tr("Groupe"), n), collapsed: false });
        for t in self.p.tracks.iter_mut().skip(first).take(count) {
            t.group = Some(gid);
        }
    }

    pub fn toggle_group_collapse(&mut self, gid: Id) {
        if let Some(g) = self.p.track_groups.iter_mut().find(|g| g.id == gid) {
            g.collapsed = !g.collapsed;
        }
        self.touch();
    }

    pub fn ungroup_tracks(&mut self, gid: Id) {
        self.push_undo();
        self.p.track_groups.retain(|g| g.id != gid);
        for t in &mut self.p.tracks {
            if t.group == Some(gid) {
                t.group = None;
            }
        }
    }

    /// Displayed height of a track (collapsed groups are drawn thin:
    /// nTrackGroupEventDrawHeight = 5 px per event row, 12 px with borders).
    pub fn shown_height(&self, ti: usize) -> f32 {
        let t = &self.p.tracks[ti];
        if let Some(g) = t.group {
            if self.p.track_groups.iter().any(|x| x.id == g && x.collapsed) {
                return 12.0;
            }
        }
        t.height
    }

    /// Adds an envelope point to the focused track (or the selected event for
    /// velocity) at the cursor, with the current value.
    pub fn insert_envelope(&mut self, kind: EnvKind) {
        self.push_undo();
        let c = self.cursor;
        match kind {
            EnvKind::Velocity => {
                for id in self.targets() {
                    if let Some(e) = self.p.event_mut(id) {
                        if e.velocity.is_empty() {
                            e.velocity.push(EnvPoint { t: 0.0, v: 100.0, curve: Curve::Linear });
                            e.velocity.push(EnvPoint { t: e.length, v: 100.0, curve: Curve::Linear });
                        }
                    }
                }
            }
            _ => {
                let Some(t) = self.p.tracks.get_mut(self.focus_track) else { return };
                let (env, def) = match kind {
                    EnvKind::Volume => (&mut t.vol_env, t.level),
                    EnvKind::Pan => (&mut t.pan_env, t.pan),
                    _ => (&mut t.fade_env, 0.0),
                };
                if env.is_empty() {
                    env.push(EnvPoint { t: 0.0, v: def, curve: Curve::Linear });
                }
                let v = env_value(env, c, def);
                env_set(env, c, v);
            }
        }
        self.prefs.show_envelopes = true;
    }

    pub fn clear_envelope(&mut self, kind: EnvKind) {
        self.push_undo();
        match kind {
            EnvKind::Velocity => {
                for id in self.targets() {
                    if let Some(e) = self.p.event_mut(id) {
                        e.velocity.clear();
                    }
                }
            }
            _ => {
                if let Some(t) = self.p.tracks.get_mut(self.focus_track) {
                    match kind {
                        EnvKind::Volume => t.vol_env.clear(),
                        EnvKind::Pan => t.pan_env.clear(),
                        _ => t.fade_env.clear(),
                    }
                }
            }
        }
    }

    /// Writes automation while playing (Touch / Latch modes).
    pub fn record_automation(&mut self, ti: usize, pan: bool, v: f32) {
        let playing = self.playing;
        let c = self.cursor;
        let Some(t) = self.p.tracks.get_mut(ti) else { return };
        if !playing || !matches!(t.auto_mode, AutoMode::Touch | AutoMode::Latch) {
            return;
        }
        let env = if pan { &mut t.pan_env } else { &mut t.vol_env };
        // thin recorded automation: one point per 50 ms (fAutomationSmoothAndThin)
        if env.last().map(|p| c - p.t >= 0.05 || c < p.t).unwrap_or(true) {
            env_set(env, c, v);
        }
    }

    /// Selects the events intersecting a time range on a range of tracks.
    pub fn select_rect(&mut self, t0: f64, t1: f64, tr0: usize, tr1: usize, additive: bool) {
        if !additive {
            self.sel.clear();
        }
        let (a, b) = (t0.min(t1), t0.max(t1));
        let (ta, tb) = (tr0.min(tr1), tr0.max(tr1));
        for ti in ta..=tb.min(self.p.tracks.len().saturating_sub(1)) {
            for e in &self.p.tracks[ti].events {
                if e.end() > a && e.start < b {
                    self.sel.insert(e.id);
                }
            }
        }
    }

    /// Shuffle: puts the event at the position of the drop time among the
    /// contiguous events of its track; the others close up.
    pub fn shuffle_to(&mut self, id: Id, t: f64) {
        let Some((ti, _)) = self.p.find_event(id) else { return };
        let track = &mut self.p.tracks[ti];
        let base = track.events.iter().map(|e| e.start).fold(f64::MAX, f64::min);
        let Some(pos) = track.events.iter().position(|e| e.id == id) else { return };
        let ev = track.events.remove(pos);
        let mut idx = track.events.iter().filter(|e| e.start + e.length / 2.0 < t).count();
        idx = idx.min(track.events.len());
        track.events.insert(idx, ev);
        let mut x = base;
        for e in &mut track.events {
            e.start = x;
            x += e.length;
        }
        self.touch();
    }

    /// Formatted time in the chosen ruler format.
    pub fn fmt_time(&self, t: f64) -> String {
        self.ruler_fmt.format(t, &self.p)
    }

    /// Label for the last undo step (shown in the history window).
    pub fn label_last_undo(&mut self, label: &str) {
        if let Some(l) = self.undo_labels.last_mut() {
            if l.is_empty() {
                *l = label.to_string();
            }
        }
    }

    /// Undo several steps at once (history window).
    pub fn undo_steps(&mut self, n: usize) {
        for _ in 0..n {
            if self.undo.is_empty() {
                break;
            }
            self.do_undo();
        }
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.undo_labels.clear();
        self.redo_labels.clear();
        self.status = tr("Historique effacé.").into();
    }
}

impl App {
    /// Extra entries of the track header context menu.
    pub(crate) fn track_menu_extra(&mut self, ui: &mut egui::Ui, ti: usize) {
        let t = self.p.tracks[ti].clone();
        if ui.button(tr("Effets de piste…")).clicked() {
            self.win_track_fx = Some(ti);
            ui.close_menu();
        }
        if t.kind == TrackKind::Video {
            if ui.button(tr("Mouvement de piste…")).clicked() {
                self.win_track_motion = Some(ti);
                ui.close_menu();
            }
            ui.menu_button(tr("Mode de composition"), |ui| {
                for b in BlendMode::ALL {
                    if ui.selectable_label(t.blend == b, b.label()).clicked() {
                        self.push_undo();
                        self.p.tracks[ti].blend = b;
                        ui.close_menu();
                    }
                }
            });
            if ui.selectable_label(t.child, tr("Piste enfant (composée avec la piste au-dessus)")).clicked() {
                self.push_undo();
                self.p.tracks[ti].child = !t.child;
                ui.close_menu();
            }
            if ui.selectable_label(t.adjustment, tr("Piste d'ajustement")).clicked() {
                self.push_undo();
                self.p.tracks[ti].adjustment = !t.adjustment;
                ui.close_menu();
            }
        } else {
            if ui.selectable_label(t.armed, tr("Armer pour l'enregistrement")).clicked() {
                self.p.tracks[ti].armed = !t.armed;
                ui.close_menu();
            }
            ui.menu_button(tr("Sortie"), |ui| {
                if ui.selectable_label(t.bus.is_none(), tr("Maître")).clicked() {
                    self.push_undo();
                    self.p.tracks[ti].bus = None;
                    ui.close_menu();
                }
                for (i, b) in self.p.buses.clone().iter().enumerate() {
                    if ui.selectable_label(t.bus == Some(i), &b.name).clicked() {
                        self.push_undo();
                        self.p.tracks[ti].bus = Some(i);
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button(tr("Automatisation"), |ui| {
                for m in AutoMode::ALL {
                    if ui.selectable_label(t.auto_mode == m, m.label()).clicked() {
                        self.p.tracks[ti].auto_mode = m;
                        ui.close_menu();
                    }
                }
            });
        }
        ui.menu_button(tr("Enveloppes"), |ui| {
            for k in [EnvKind::Volume, EnvKind::Pan, EnvKind::FadeToColor] {
                if k == EnvKind::Pan && t.kind == TrackKind::Video {
                    continue;
                }
                if k == EnvKind::FadeToColor && t.kind == TrackKind::Audio {
                    continue;
                }
                if ui.button(k.label()).clicked() {
                    self.focus_track = ti;
                    self.insert_envelope(k);
                    self.env_kind = k;
                    ui.close_menu();
                }
            }
            if ui.button(tr("Retirer les enveloppes")).clicked() {
                self.push_undo();
                let tr = &mut self.p.tracks[ti];
                tr.vol_env.clear();
                tr.pan_env.clear();
                tr.fade_env.clear();
                ui.close_menu();
            }
        });
        if ui.button(tr("Solo exclusif")).clicked() {
            self.exclusive_solo(ti);
            ui.close_menu();
        }
        if ui.button(tr("Muet exclusif")).clicked() {
            self.exclusive_mute(ti);
            ui.close_menu();
        }
        ui.menu_button(tr("Groupe de pistes"), |ui| {
            match t.group {
                Some(gid) => {
                    let collapsed = self.p.track_groups.iter().any(|g| g.id == gid && g.collapsed);
                    if ui.button(if collapsed { tr("Déplier le groupe") } else { tr("Replier le groupe") }).clicked() {
                        self.toggle_group_collapse(gid);
                        ui.close_menu();
                    }
                    if ui.button(tr("Dissocier le groupe")).clicked() {
                        self.ungroup_tracks(gid);
                        ui.close_menu();
                    }
                }
                None => {
                    for n in [2usize, 3, 4] {
                        if ti + n <= self.p.tracks.len() && ui.button(format!("{} {} {}", tr("Grouper avec les"), n - 1, tr("piste(s) suivante(s)"))).clicked() {
                            self.group_tracks(ti, n);
                            ui.close_menu();
                        }
                    }
                }
            }
        });
    }
}
