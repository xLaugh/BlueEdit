//! What the AI features do to the project: Claude's tools, automatic
//! subtitles and automatic trimming (applied on the UI thread).

use crate::ai::{self, TrimParams, TrimResult};
use crate::app::App;
use crate::claude::Line;
use crate::media::AUDIO_RATE;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

fn curve(s: &str) -> Option<Curve> {
    Some(match s {
        "linear" => Curve::Linear,
        "fast" => Curve::Fast,
        "slow" => Curve::Slow,
        "smooth" => Curve::Smooth,
        "sharp" => Curve::Sharp,
        "hold" => Curve::Hold,
        _ => return None,
    })
}

fn transition(s: &str) -> Option<Transition> {
    Some(match s {
        "dissolve" => Transition::Dissolve,
        "wipe_left" => Transition::WipeLeft,
        "wipe_down" => Transition::WipeDown,
        "push" => Transition::Push,
        "iris" => Transition::Iris,
        "through_black" => Transition::ThroughBlack,
        "blinds" => Transition::Blinds,
        "clock_wipe" => Transition::ClockWipe,
        "flash" => Transition::Flash,
        _ => return None,
    })
}

fn scale_fx(fx: &mut VideoFx, k: f32) {
    match fx {
        VideoFx::Saturation { amount } => *amount = k * 3.0,
        VideoFx::BlackWhite { amount }
        | VideoFx::Sepia { amount }
        | VideoFx::Invert { amount }
        | VideoFx::Vignette { amount }
        | VideoFx::RgbSplit { amount }
        | VideoFx::Glitch { amount }
        | VideoFx::Vhs { amount }
        | VideoFx::Grain { amount }
        | VideoFx::Shake { amount } => *amount = k,
        VideoFx::Blur { radius } => *radius = k * 30.0,
        VideoFx::Glow { intensity, .. } => *intensity = k * 2.0,
        VideoFx::Pixelate { size } => *size = 2.0 + k * 60.0,
        VideoFx::Noir { contrast } => *contrast = k,
        VideoFx::BrightnessContrast { contrast, .. } => *contrast = k * 2.0 - 1.0,
        VideoFx::Deflicker { amount } => *amount = k,
        VideoFx::Lut { amount, .. } | VideoFx::Stabilize { amount, .. } => *amount = k,
        VideoFx::ChromaKey { tolerance, .. } => *tolerance = k,
        VideoFx::Mask { feather, .. } => *feather = k * 0.3,
        VideoFx::ColorGrade(g) => g.saturation = k * 2.0,
        VideoFx::Duotone { .. } => {}
    }
}

impl App {
    /// The whole project as JSON, for Claude.
    pub fn project_json(&self) -> Value {
        let tracks: Vec<Value> = self
            .p
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                json!({
                    "index": i,
                    "kind": if t.kind == TrackKind::Video { "video" } else { "audio" },
                    "name": t.name, "mute": t.mute, "solo": t.solo, "level": t.level,
                    "events": t.events.iter().map(|e| json!({
                        "id": e.id, "name": e.name, "start": round3(e.start), "end": round3(e.end()),
                        "media_id": e.media, "offset": round3(e.offset),
                        "fade_in": round3(e.fade_in), "fade_out": round3(e.fade_out),
                        "gain": e.gain, "muted": e.mute, "group": e.group,
                        "effects": e.fx.iter().map(|f| f.key()).collect::<Vec<_>>(),
                        "transition": format!("{:?}", e.transition),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "name": self.p.name,
            "format": {"width": self.p.width, "height": self.p.height, "fps": round3(self.p.fps)},
            "cursor": round3(self.cursor),
            "duration": round3(self.p.end_time()),
            "loop_region": self.p.loop_region,
            "selected_event_ids": self.sel.iter().collect::<Vec<_>>(),
            "markers": self.p.markers.iter().map(|m| json!({"time": round3(m.time), "name": m.name})).collect::<Vec<_>>(),
            "media": self.p.media.iter().filter(|m| !matches!(m.source, MediaSource::Generated(Generator::Subtitle{..}))).map(|m| json!({
                "id": m.id, "name": m.name, "type": m.type_label(), "duration": round3(m.duration)
            })).collect::<Vec<_>>(),
            "tracks": tracks,
        })
    }

    fn ev_id(&self, input: &Value) -> Result<Id, String> {
        let id = input["event_id"].as_u64().ok_or("event_id manquant")?;
        if self.p.event(id).is_none() {
            return Err(format!("aucun événement {id}"));
        }
        Ok(id)
    }

    /// Runs one of Claude's tools. Returns the text given back to Claude.
    pub fn run_tool(&mut self, name: &str, input: &Value) -> Result<String, String> {
        let f = |k: &str| input[k].as_f64();
        match name {
            "get_project" => Ok(self.project_json().to_string()),
            "set_cursor" => {
                self.cursor = self.click_time(f("time").ok_or("time manquant")?);
                Ok("ok".into())
            }
            "split_events" => {
                let t = f("time").ok_or("time manquant")?;
                let saved = self.sel.clone();
                self.cursor = t;
                self.sel = input["event_ids"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();
                self.split_at_cursor();
                let msg = self.status.clone();
                if self.sel.is_empty() {
                    self.sel = saved;
                }
                Ok(msg)
            }
            "delete_events" => {
                let ids: Vec<Id> = input["event_ids"].as_array().ok_or("event_ids manquant")?.iter().filter_map(|v| v.as_u64()).collect();
                let old_r = self.ripple;
                self.ripple = input["ripple"].as_bool().unwrap_or(false);
                self.sel = ids.iter().copied().collect();
                let n = self.sel.len();
                self.delete_selected();
                self.ripple = old_r;
                Ok(format!("{n} événement(s) supprimé(s)"))
            }
            "move_event" => {
                let id = self.ev_id(input)?;
                let start = f("start").ok_or("start manquant")?.max(0.0);
                self.push_undo();
                let (ti, _) = self.p.find_event(id).unwrap();
                let e = self.p.event(id).unwrap().clone();
                let delta = self.p.quantize(start) - e.start;
                let dtrack = input["track_index"].as_i64().map(|t| t - ti as i64).unwrap_or(0);
                let members: Vec<Id> = match e.group {
                    Some(g) => self.p.tracks.iter().flat_map(|t| t.events.iter()).filter(|x| x.group == Some(g)).map(|x| x.id).collect(),
                    None => vec![id],
                };
                for mid in members {
                    let (mt, mi) = self.p.find_event(mid).unwrap();
                    let mut ev = self.p.tracks[mt].events.remove(mi);
                    ev.start = (ev.start + delta).max(0.0);
                    let nt = mt as i64 + if mid == id { dtrack } else { 0 };
                    let nt = if nt >= 0 && (nt as usize) < self.p.tracks.len() && self.p.tracks[nt as usize].kind == self.p.tracks[mt].kind { nt as usize } else { mt };
                    self.p.tracks[nt].events.push(ev);
                }
                self.p.sort_events();
                Ok("ok".into())
            }
            "trim_event" => {
                let id = self.ev_id(input)?;
                self.push_undo();
                let fps = self.p.fps;
                let e = self.p.event_mut(id).unwrap();
                let end = f("end").unwrap_or(e.end());
                if let Some(s) = f("start") {
                    let s = s.max(0.0).min(end - 1.0 / fps);
                    e.offset = (e.offset + s - e.start).max(0.0);
                    e.start = s;
                }
                e.length = (end - e.start).max(1.0 / fps);
                e.fade_in = e.fade_in.min(e.length);
                e.fade_out = e.fade_out.min(e.length - e.fade_in);
                self.p.sort_events();
                Ok("ok".into())
            }
            "set_fades" => {
                let id = self.ev_id(input)?;
                self.push_undo();
                let e = self.p.event_mut(id).unwrap();
                if let Some(v) = input["fade_in"].as_f64() {
                    e.fade_in = v.clamp(0.0, e.length);
                }
                if let Some(v) = input["fade_out"].as_f64() {
                    e.fade_out = v.clamp(0.0, e.length - e.fade_in);
                }
                if let Some(c) = input["curve_in"].as_str().and_then(curve) {
                    e.curve_in = c;
                }
                if let Some(c) = input["curve_out"].as_str().and_then(curve) {
                    e.curve_out = c;
                }
                Ok("ok".into())
            }
            "set_event_gain" => {
                let id = self.ev_id(input)?;
                self.push_undo();
                self.p.event_mut(id).unwrap().gain = f("gain").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
                Ok("ok".into())
            }
            "add_effect" => {
                let id = self.ev_id(input)?;
                let (ti, _) = self.p.find_event(id).unwrap();
                if self.p.tracks[ti].kind != TrackKind::Video {
                    return Err("les effets vidéo ne s'appliquent qu'aux événements vidéo".into());
                }
                let key = input["effect"].as_str().unwrap_or("");
                let mut fx = VideoFx::from_key(key).ok_or(format!("effet inconnu : {key}"))?;
                if let Some(k) = f("intensity") {
                    scale_fx(&mut fx, k.clamp(0.0, 1.0) as f32);
                }
                self.push_undo();
                self.p.event_mut(id).unwrap().fx.push(fx);
                Ok("ok".into())
            }
            "clear_effects" => {
                let id = self.ev_id(input)?;
                self.push_undo();
                self.p.event_mut(id).unwrap().fx.clear();
                Ok("ok".into())
            }
            "set_transition" => {
                let id = self.ev_id(input)?;
                let tr = input["transition"].as_str().and_then(transition).ok_or("transition inconnue")?;
                self.push_undo();
                let e = self.p.event_mut(id).unwrap();
                e.transition = tr;
                if e.fade_in <= 0.0 {
                    e.fade_in = 1.0_f64.min(e.length / 2.0);
                }
                Ok("ok".into())
            }
            "add_text" => {
                let text = input["text"].as_str().ok_or("text manquant")?.to_string();
                let start = f("start").unwrap_or(self.cursor).max(0.0);
                let dur = f("duration").unwrap_or(DEFAULT_STILL_LEN).max(0.1);
                let subtitle = input["subtitle"].as_bool().unwrap_or(false);
                let size = f("size_pct").map(|v| v.clamp(2.0, 40.0) as u32);
                let g = if subtitle {
                    Generator::Subtitle { text, size_pct: size.unwrap_or(6) }
                } else {
                    Generator::Text { text, size_pct: size.unwrap_or(10), color: [255, 255, 255, 255], background: [0, 0, 0, 0] }
                };
                self.push_undo();
                let ctx = self.ctx.clone();
                let mid = self.add_generator(g, &ctx);
                let ti = match input["track_index"].as_u64() {
                    Some(t) if (t as usize) < self.p.tracks.len() && self.p.tracks[t as usize].kind == TrackKind::Video => t as usize,
                    _ => self.p.add_track(TrackKind::Video, Some(0)),
                };
                let m = self.p.media(mid).unwrap().clone();
                let e = self.p.new_event(&m, self.p.quantize(start), dur);
                let eid = e.id;
                self.p.tracks[ti].events.push(e);
                self.p.sort_events();
                Ok(format!("texte ajouté (événement {eid}, piste {ti})"))
            }
            "add_marker" => {
                self.push_undo();
                let t = self.p.quantize(f("time").ok_or("time manquant")?.max(0.0));
                let name = input["name"].as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{}", self.p.markers.len() + 1));
                self.p.markers.push(Marker { time: t, name, command: false });
                self.p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
                Ok("ok".into())
            }
            "set_loop_region" => {
                let (a, b) = (f("start").ok_or("start manquant")?, f("end").ok_or("end manquant")?);
                self.p.loop_region = Some((a.min(b).max(0.0), a.max(b)));
                Ok("ok".into())
            }
            "set_track" => {
                let ti = input["track_index"].as_u64().ok_or("track_index manquant")? as usize;
                if ti >= self.p.tracks.len() {
                    return Err(format!("aucune piste {ti}"));
                }
                self.push_undo();
                let t = &mut self.p.tracks[ti];
                if let Some(v) = input["mute"].as_bool() {
                    t.mute = v;
                }
                if let Some(v) = input["solo"].as_bool() {
                    t.solo = v;
                }
                if let Some(v) = input["level"].as_f64() {
                    t.level = v.clamp(0.0, if t.kind == TrackKind::Audio { 2.0 } else { 1.0 }) as f32;
                }
                if let Some(v) = input["name"].as_str() {
                    t.name = v.to_string();
                }
                Ok("ok".into())
            }
            "add_track" => {
                self.push_undo();
                let video = input["kind"].as_str() == Some("video");
                let i = if video {
                    let at = self.p.tracks.iter().position(|t| t.kind == TrackKind::Audio);
                    self.p.add_track(TrackKind::Video, at)
                } else {
                    self.p.add_track(TrackKind::Audio, None)
                };
                Ok(format!("piste ajoutée à l'index {i}"))
            }
            "insert_media" => {
                let mid = input["media_id"].as_u64().ok_or("media_id manquant")?;
                if self.p.media(mid).is_none() {
                    return Err(format!("aucun média {mid}"));
                }
                let t = f("start").unwrap_or(self.cursor);
                self.insert_media(mid, t, input["track_index"].as_u64().map(|v| v as usize), true);
                Ok(self.status.clone())
            }
            "auto_trim" => {
                let id = self.ev_id(input)?;
                let mut p = TrimParams { cut_fillers: false, ..Default::default() };
                if let Some(v) = f("min_silence") {
                    p.min_silence = v.max(0.05);
                }
                if let Some(v) = f("margin") {
                    p.margin = v.max(0.0);
                }
                p.threshold_db = f("threshold_db").map(|v| v as f32);
                let r = self.analyze_trim(id, &p, &[]).ok_or("cet événement n'a pas de son analysable (audio pas encore chargé ?)")?;
                let removed = r.duration - r.kept();
                self.apply_trim(id, &r.keep);
                Ok(format!("{:.1} s retirées sur {:.1} s ({} segments gardés)", removed, r.duration, r.keep.len()))
            }
            "generate_subtitles" => {
                let lang = input["language"].as_str().unwrap_or("fr").to_string();
                self.start_subtitles(lang)?;
                Ok("transcription lancée en arrière-plan ; les sous-titres apparaîtront sur une piste « Sous-titres » à la fin".into())
            }
            "undo" => {
                self.do_undo();
                Ok("annulé".into())
            }
            _ => Err(format!("outil inconnu : {name}")),
        }
    }

    /// Checks for Claude's answer and runs requested tools.
    pub fn claude_poll(&mut self) {
        let Some(rx) = &self.ai.chat.pending else { return };
        let Ok(res) = rx.try_recv() else {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        self.ai.chat.pending = None;
        let v = match res {
            Ok(v) => v,
            Err(e) => {
                self.ai.chat.lines.push(Line::Error(e));
                // drop the unanswered user turn so the conversation stays valid
                if self.ai.chat.messages.last().map(|m| m["role"] == "user").unwrap_or(false) {
                    let last = self.ai.chat.messages.pop();
                    if let Some(l) = last {
                        if l["content"].is_array() {
                            // a tool_result turn: keep history consistent by restarting
                            self.ai.chat.messages.clear();
                        }
                    }
                }
                return;
            }
        };
        let content = v["content"].clone();
        let stop = v["stop_reason"].as_str().unwrap_or("").to_string();
        self.ai.chat.messages.push(json!({"role": "assistant", "content": content}));
        let blocks = content.as_array().cloned().unwrap_or_default();
        for b in &blocks {
            if b["type"] == "text" {
                let t = b["text"].as_str().unwrap_or("").trim();
                if !t.is_empty() {
                    self.ai.chat.lines.push(Line::Assistant(t.to_string()));
                }
            }
        }
        match stop.as_str() {
            "tool_use" => {
                let mut results = vec![];
                for b in blocks.iter().filter(|b| b["type"] == "tool_use") {
                    let name = b["name"].as_str().unwrap_or("").to_string();
                    let id = b["id"].clone();
                    let input = if b["input"].is_object() { b["input"].clone() } else { json!({}) };
                    let r = self.run_tool(&name, &input);
                    let (text, err) = match r {
                        Ok(t) => (t, false),
                        Err(e) => (e, true),
                    };
                    if name != "get_project" {
                        self.ai.chat.lines.push(Line::Tool(format!(
                            "{} {}{}",
                            if err { "Échec :" } else { "Outil :" },
                            name,
                            if err { format!(" : {text}") } else { String::new() }
                        )));
                    }
                    results.push(json!({"type": "tool_result", "tool_use_id": id, "content": text, "is_error": err}));
                }
                self.ai.chat.messages.push(json!({"role": "user", "content": results}));
                self.ai.chat.rounds += 1;
                if self.ai.chat.rounds >= 24 {
                    self.ai.chat.lines.push(Line::Error("Trop d'étapes d'affilée : l'assistant s'arrête ici.".into()));
                    self.ai.chat.messages.clear();
                } else {
                    let ctx = self.claude_context();
                    self.ai.chat.send(ctx);
                }
            }
            "refusal" => self.ai.chat.lines.push(Line::Error("Claude a refusé cette demande.".into())),
            "max_tokens" => self.ai.chat.lines.push(Line::Error("Réponse coupée (trop longue).".into())),
            _ => {}
        }
    }

    pub fn claude_context(&self) -> String {
        format!(
            "Projet « {} » : {}x{}, {:.3} i/s, durée {:.2} s, {} pistes, curseur à {:.2} s.",
            self.p.name,
            self.p.width,
            self.p.height,
            self.p.fps,
            self.p.end_time(),
            self.p.tracks.len(),
            self.cursor
        )
    }

    pub fn claude_send_user(&mut self, text: String) {
        if text.trim().is_empty() || self.ai.chat.busy() {
            return;
        }
        self.ai.chat.lines.push(Line::User(text.clone()));
        self.ai.chat.messages.push(json!({"role": "user", "content": text}));
        self.ai.chat.rounds = 0;
        let ctx = self.claude_context();
        self.ai.chat.send(ctx);
    }

    // --------------------------------------------------------- auto trim

    /// The audio event that carries the sound of `id` (itself or a grouped partner).
    pub fn audio_partner(&self, id: Id) -> Option<Event> {
        let (ti, _) = self.p.find_event(id)?;
        let e = self.p.event(id)?;
        if self.p.tracks[ti].kind == TrackKind::Audio {
            return Some(e.clone());
        }
        let g = e.group?;
        self.p
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .flat_map(|t| t.events.iter())
            .find(|x| x.group == Some(g))
            .cloned()
    }

    pub fn analyze_trim(&self, id: Id, p: &TrimParams, words: &[ai::Segment]) -> Option<TrimResult> {
        let a = self.audio_partner(id)?;
        let pcm = self.store.audio(a.media)?;
        let levels = ai::frame_levels(&pcm.samples, AUDIO_RATE, a.offset, a.offset + a.length);
        Some(ai::compute(levels, a.length, words, p))
    }

    /// Writes the sound under an event to a WAV file for transcription.
    pub fn event_wav(&self, id: Id) -> Option<PathBuf> {
        let a = self.audio_partner(id)?;
        let pcm = self.store.audio(a.media)?;
        let n = pcm.samples.len() / 2;
        let s0 = ((a.offset * AUDIO_RATE as f64) as usize).min(n);
        let s1 = (((a.offset + a.length) * AUDIO_RATE as f64) as usize).min(n);
        let samples: Vec<f32> = pcm.samples[s0 * 2..s1 * 2].iter().map(|v| *v as f32 / 32768.0).collect();
        let path = std::env::temp_dir().join(format!("blue_trim_{}_{}.wav", std::process::id(), id));
        crate::export::write_wav(&path, &samples, AUDIO_RATE).ok()?;
        Some(path)
    }

    /// Replaces the event (and its group) by the kept pieces, end to end,
    /// and pulls the following events back (ripple).
    pub fn apply_trim(&mut self, id: Id, keep: &[(f64, f64)]) {
        let Some(e) = self.p.event(id).cloned() else { return };
        if keep.is_empty() {
            self.status = "Rien à garder : la coupe n'a pas été appliquée.".into();
            return;
        }
        self.push_undo();
        let members: Vec<Id> = match e.group {
            Some(g) => self.p.tracks.iter().flat_map(|t| t.events.iter()).filter(|x| x.group == Some(g)).map(|x| x.id).collect(),
            None => vec![id],
        };
        let kept: f64 = keep.iter().map(|(a, b)| b - a).sum();
        let mut group_ids: HashMap<usize, Id> = HashMap::new();
        if e.group.is_some() {
            for i in 0..keep.len() {
                group_ids.insert(i, self.p.new_id());
            }
        }
        let mut new_sel = vec![];
        for mid in members {
            let Some((ti, ei)) = self.p.find_event(mid) else { continue };
            let src = self.p.tracks[ti].events.remove(ei);
            let removed = src.length - kept;
            let old_end = src.end();
            let mut pos = src.start;
            let last = keep.len() - 1;
            for (i, &(a, b)) in keep.iter().enumerate() {
                let mut piece = src.clone();
                piece.id = self.p.new_id();
                piece.start = pos;
                piece.offset = src.offset + a;
                piece.length = b - a;
                piece.fade_in = if i == 0 { src.fade_in.min(piece.length) } else { 0.0 };
                piece.fade_out = if i == last { src.fade_out.min(piece.length - piece.fade_in) } else { 0.0 };
                piece.group = group_ids.get(&i).copied();
                pos += piece.length;
                new_sel.push(piece.id);
                self.p.tracks[ti].events.push(piece);
            }
            for o in &mut self.p.tracks[ti].events {
                if o.start >= old_end - 1e-6 {
                    o.start = (o.start - removed).max(0.0);
                }
            }
        }
        self.p.sort_events();
        self.sel = new_sel.into_iter().collect();
        self.status = format!("Coupe automatique : {:.1} s retirées, {} segments gardés.", e.length - kept, keep.len());
    }

    // --------------------------------------------------------- subtitles

    pub fn default_model(&self) -> Option<PathBuf> {
        self.ai.model_path.clone().or_else(|| ai::find_models().into_iter().next())
    }

    /// Mixes the project's sound to a WAV and starts transcription.
    pub fn start_subtitles(&mut self, lang: String) -> Result<(), String> {
        if self.ai.subs_job.is_some() {
            return Err("une transcription est déjà en cours".into());
        }
        let model = self.default_model().ok_or("aucun modèle de transcription : ouvrez IA > Sous-titres automatiques pour en télécharger un")?;
        let (t0, t1) = match (self.ai.subs_loop_only, self.p.loop_region) {
            (true, Some((a, b))) if b > a => (a, b),
            _ => (0.0, self.p.end_time()),
        };
        if t1 - t0 < 0.2 {
            return Err("le projet n'a pas de son à transcrire".into());
        }
        let rate = 16_000u32;
        let mix = crate::audio::build_mix(&self.p, &self.store, self.auto_xf);
        let frames = ((t1 - t0) * rate as f64) as usize;
        let mut buf = vec![0f32; frames * 2];
        crate::audio::mix_into(&mix, t0, rate as f64, &mut buf);
        let path = std::env::temp_dir().join(format!("blue_subs_{}.wav", std::process::id()));
        crate::export::write_wav(&path, &buf, rate).map_err(|e| e.to_string())?;
        self.ai.subs_offset = t0;
        self.ai.subs_job = Some(ai::transcribe_job(path, model, lang, true, false));
        self.status = "Sous-titres : transcription en cours…".into();
        Ok(())
    }

    /// Puts transcribed lines on a "Sous-titres" track.
    pub fn place_subtitles(&mut self, segs: &[ai::Segment]) {
        if segs.is_empty() {
            self.status = "Aucune parole reconnue.".into();
            return;
        }
        self.push_undo();
        let ti = match self.p.tracks.iter().position(|t| t.kind == TrackKind::Video && t.name == "Sous-titres") {
            Some(i) => i,
            None => {
                let i = self.p.add_track(TrackKind::Video, Some(0));
                self.p.tracks[i].name = "Sous-titres".into();
                i
            }
        };
        let ctx = self.ctx.clone();
        let off = self.ai.subs_offset;
        for s in segs {
            let g = Generator::Subtitle { text: wrap_line(&s.text, 42), size_pct: self.ai.subs_size };
            let mid = self.add_generator(g, &ctx);
            let m = self.p.media(mid).unwrap().clone();
            let start = self.p.quantize(off + s.start);
            let len = (s.end - s.start).max(0.4);
            let e = self.p.new_event(&m, start, len);
            self.p.tracks[ti].events.push(e);
        }
        self.p.sort_events();
        self.ai.last_subs = segs.iter().map(|s| ai::Segment { start: s.start + off, end: s.end + off, text: s.text.clone() }).collect();
        self.status = format!("{} sous-titres ajoutés sur la piste « Sous-titres ».", segs.len());
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Splits a long line in two near the middle.
fn wrap_line(t: &str, max: usize) -> String {
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mid = t.char_indices().nth(t.chars().count() / 2).map(|(i, _)| i).unwrap_or(0);
    let split = t[..mid].rfind(' ').or_else(|| t[mid..].find(' ').map(|i| i + mid));
    match split {
        Some(i) => format!("{}\n{}", t[..i].trim(), t[i..].trim()),
        None => t.to_string(),
    }
}
