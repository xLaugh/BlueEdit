//! Builds the picture at a given time: tracks composited bottom to top,
//! with fades, crossfade transitions, event effects and pan/crop.

use crate::media::{self, MediaStore, Rgba};
use crate::model::*;
use std::collections::HashMap;
use std::sync::Arc;

pub trait FrameSource {
    /// A video frame of `path` at source time `t`, scaled to w x h (RGBA).
    fn video(&mut self, path: &str, event: Id, t: f64, w: u32, h: u32) -> Option<Arc<Vec<u8>>>;
    /// A still image at full size.
    fn image(&mut self, m: &Media) -> Option<Arc<Rgba>>;
    /// True when frames are decoded exactly (render), false for the preview.
    fn exact(&self) -> bool {
        false
    }
}

/// Preview source: background decoders, never blocks.
pub struct PreviewSource<'a> {
    pub server: &'a media::FrameServer,
    pub store: &'a MediaStore,
}

impl FrameSource for PreviewSource<'_> {
    fn video(&mut self, path: &str, event: Id, t: f64, w: u32, h: u32) -> Option<Arc<Vec<u8>>> {
        match media::proxy_for(path) {
            Some(px) => self.server.get(&px, event, t, w, h),
            None => self.server.get(path, event, t, w, h),
        }
    }
    fn image(&mut self, m: &Media) -> Option<Arc<Rgba>> {
        self.store.image(m.id)
    }
}

/// Render source: decodes exactly, blocking (used when exporting).
pub struct ExactSource {
    readers: HashMap<(Id, u32, u32), (media::VideoReader, Option<(f64, Arc<Vec<u8>>)>)>,
    images: HashMap<Id, Arc<Rgba>>,
    fps: f64,
}

impl ExactSource {
    pub fn new(fps: f64) -> ExactSource {
        ExactSource {
            readers: HashMap::new(),
            images: HashMap::new(),
            fps,
        }
    }
}

impl FrameSource for ExactSource {
    fn video(&mut self, path: &str, event: Id, t: f64, w: u32, h: u32) -> Option<Arc<Vec<u8>>> {
        let key = (event, w, h);
        let reopen = match self.readers.get(&key) {
            None => true,
            Some((r, last)) => {
                let lt = last.as_ref().map(|x| x.0).unwrap_or(r.next_time);
                t < lt - 1e-6 || t > r.next_time + 2.0
            }
        };
        if reopen {
            let r = media::VideoReader::open(path, t, w, h, self.fps)?;
            self.readers.insert(key, (r, None));
        }
        let (r, last) = self.readers.get_mut(&key)?;
        r.frame_at(t, last)
    }
    fn exact(&self) -> bool {
        true
    }
    fn image(&mut self, m: &Media) -> Option<Arc<Rgba>> {
        if let Some(i) = self.images.get(&m.id) {
            return Some(i.clone());
        }
        if let MediaSource::Image(p) = &m.source {
            let img = Arc::new(media::load_image(p)?);
            self.images.insert(m.id, img.clone());
            return Some(img);
        }
        None
    }
}

#[derive(Default)]
pub struct Compositor {
    gen_cache: HashMap<(String, u32, u32), Arc<Rgba>>,
    img_cache: HashMap<(Id, u32, u32), Arc<Rgba>>,
    depth: u32,
}

fn fit(mw: u32, mh: u32, w: u32, h: u32) -> (u32, u32) {
    if mw == 0 || mh == 0 {
        return (w, h);
    }
    let s = (w as f64 / mw as f64).min(h as f64 / mh as f64);
    let fw = ((mw as f64 * s).round() as u32).max(2) & !1;
    let fh = ((mh as f64 * s).round() as u32).max(2) & !1;
    (fw.min(w.max(2)), fh.min(h.max(2)))
}

pub struct Layer {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Layer {
    pub fn empty(w: u32, h: u32) -> Layer {
        Layer { w, h, data: vec![0u8; (w * h * 4) as usize] }
    }
}

/// Is this event an adjustment event (its effects change what is below)?
fn is_adjustment(p: &Project, e: &Event) -> bool {
    matches!(p.media(e.media).map(|m| &m.source), Some(MediaSource::Generated(Generator::Adjustment)))
}

impl Compositor {
    pub fn clear_cache(&mut self) {
        if self.gen_cache.len() > 64 {
            self.gen_cache.clear();
        }
        if self.img_cache.len() > 64 {
            self.img_cache.clear();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn source_image(
        &mut self,
        m: &Media,
        ev: &Event,
        t: f64,
        w: u32,
        h: u32,
        src: &mut dyn FrameSource,
    ) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        let lt = t - ev.start;
        match &m.source {
            MediaSource::Generated(g) => {
                if g.is_audio() || matches!(g, Generator::Adjustment) {
                    return None;
                }
                if let Generator::Title { anim, anim_ms, .. } = g {
                    if *anim != TitleAnim::None && lt * 1000.0 < *anim_ms as f64 {
                        let img = media::render_title(g, w, h, lt);
                        return Some((img.w, img.h, Arc::new(img.data)));
                    }
                }
                let key = (format!("{:?}", g), w, h);
                let img = self
                    .gen_cache
                    .entry(key)
                    .or_insert_with(|| Arc::new(media::render_generator(g, w, h)))
                    .clone();
                Some((img.w, img.h, Arc::new(img.data.clone())))
            }
            MediaSource::Image(_) => {
                let (fw, fh) = if ev.keep_aspect { fit(m.width, m.height, w, h) } else { (w, h) };
                if let Some(c) = self.img_cache.get(&(m.id, fw, fh)) {
                    return Some((fw, fh, Arc::new(c.data.clone())));
                }
                let full = src.image(m)?;
                let small = Arc::new(media::resize_nearest(&full, fw, fh));
                self.img_cache.insert((m.id, fw, fh), small.clone());
                Some((fw, fh, Arc::new(small.data.clone())))
            }
            MediaSource::Nested(path) => {
                if self.depth > 4 {
                    return None;
                }
                let sub = media::load_nested(path)?;
                let (fw, fh) = if ev.keep_aspect { fit(sub.width, sub.height, w, h) } else { (w, h) };
                let mut st = ev.source_time(lt);
                let d = sub.end_time();
                if d > 0.0 {
                    st = if ev.looped { st.rem_euclid(d) } else { st.clamp(0.0, d) };
                }
                self.depth += 1;
                let img = self.compose(&sub, st, fw, fh, true, src);
                self.depth -= 1;
                Some((fw, fh, Arc::new(img)))
            }
            MediaSource::File(path) => {
                if !m.has_video {
                    return None;
                }
                let (fw, fh) = if ev.keep_aspect { fit(m.width, m.height, w, h) } else { (w, h) };
                let wrap = |mut st: f64| {
                    if m.duration > 0.0 {
                        if ev.looped {
                            st = st.rem_euclid(m.duration);
                        } else {
                            st = st.clamp(0.0, m.duration - 0.001);
                        }
                    }
                    st.max(0.0)
                };
                let st = wrap(ev.source_time(lt));
                let f = src.video(path, ev.id, st, fw, fh)?;
                if f.len() != (fw * fh * 4) as usize {
                    return None;
                }
                // Frame blending (resample) when the rate is changed, or forced.
                let blend = match ev.resample {
                    Resample::Force => true,
                    Resample::Smart => (ev.rate - 1.0).abs() > 1e-6 || !ev.velocity.is_empty(),
                    Resample::Disable => false,
                };
                if blend && src.exact() && m.fps > 0.0 {
                    let pos = st * m.fps;
                    let frac = (pos - pos.floor()) as f32;
                    if frac > 0.01 {
                        let a = src.video(path, ev.id ^ 0x8000_0000, wrap(pos.floor() / m.fps), fw, fh);
                        let b = src.video(path, ev.id ^ 0x4000_0000, wrap((pos.floor() + 1.0) / m.fps), fw, fh);
                        if let (Some(a), Some(b)) = (a, b) {
                            if a.len() == f.len() && b.len() == f.len() {
                                let mixed: Vec<u8> = a.iter().zip(b.iter()).map(|(x, y)| (*x as f32 * (1.0 - frac) + *y as f32 * frac) as u8).collect();
                                return Some((fw, fh, Arc::new(mixed)));
                            }
                        }
                    }
                }
                Some((fw, fh, f))
            }
        }
    }

    /// The event's picture placed in a w x h layer (straight alpha), effects applied.
    fn event_layer(&mut self, p: &Project, ev: &Event, t: f64, w: u32, h: u32, src: &mut dyn FrameSource) -> Option<Layer> {
        let m = p.media(ev.media)?;
        let (sw, sh, data) = self.source_image(m, ev, t, w, h, src)?;
        let lt = t - ev.start;
        let pan = ev.pan_at(lt);
        let mut layer = Layer { w: sw, h: sh, data: data.to_vec() };
        // media effects first (they belong to the source)
        for fx in &m.fx {
            apply_fx(&mut layer, fx, lt);
        }
        let mut layer = place(&layer, w, h, &pan);
        for fx in ev.fx_at(lt).iter() {
            apply_fx(&mut layer, fx, lt);
        }
        Some(layer)
    }

    /// The picture of one track at time t (straight alpha), before its motion.
    fn track_layer(&mut self, p: &Project, ti: usize, t: f64, w: u32, h: u32, auto_crossfade: bool, src: &mut dyn FrameSource) -> Option<Layer> {
        let track = &p.tracks[ti];
        let active: Vec<usize> = track
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.mute && e.start <= t && t < e.end() && !is_adjustment(p, e))
            .map(|(i, _)| i)
            .collect();
        if active.is_empty() {
            return None;
        }
        let mut out = if active.len() >= 2 && auto_crossfade {
            let ai = active[active.len() - 2];
            let bi = active[active.len() - 1];
            let a = &track.events[ai];
            let b = &track.events[bi];
            let ov = (a.end() - b.start).max(1e-6);
            let u = (t - b.start) / ov;
            // progress envelope (Transition Progress) when present, else the fade curve
            let prog = if b.transition_env.is_empty() { b.curve_in.gain_in(u) } else { env_value(&b.transition_env, u, 0.0) as f64 };
            let la = self.event_layer(p, a, t, w, h, src).unwrap_or_else(|| Layer::empty(w, h));
            let lb = self.event_layer(p, b, t, w, h, src).unwrap_or_else(|| Layer::empty(w, h));
            let ga = a.gain * fade_gain(a, t - a.start, a.fade_in, 0.0) as f32;
            let gb = b.gain * fade_gain(b, t - b.start, 0.0, b.fade_out) as f32;
            let (mut la, mut lb) = (la, lb);
            scale_alpha(&mut la, ga);
            scale_alpha(&mut lb, gb);
            if b.transition == Transition::Dissolve && b.transition_env.is_empty() {
                // Weighted crossfade: each picture gets its own curve weight, in Q14
                // fixed point (x16384), summed on all four
                // channels with rounding (+0x2000) and clamped to 255.
                let wa = (a.curve_out.gain_out(u) * 16384.0) as u32;
                let wb = (b.curve_in.gain_in(u) * 16384.0) as u32;
                weighted_crossfade(&la, &lb, wa, wb)
            } else {
                transition(b.transition, &la, &lb, prog as f32)
            }
        } else {
            let ei = *active.last().unwrap();
            let e = &track.events[ei];
            let (fin, fout) = effective_fades(track, ei, auto_crossfade);
            let g = e.gain * fade_gain(e, t - e.start, fin, fout) as f32;
            let mut l = self.event_layer(p, e, t, w, h, src)?;
            scale_alpha(&mut l, g);
            l
        };
        for fx in &track.fx {
            apply_fx(&mut out, fx, t);
        }
        // fade to colour: >0 toward the top colour, <0 toward the bottom colour
        let fade = env_value(&track.fade_env, t, 0.0);
        if fade.abs() > 1e-4 {
            let c = if fade > 0.0 { track.fade_colors[0] } else { track.fade_colors[1] };
            let k = fade.abs().min(1.0);
            for px in out.data.chunks_exact_mut(4) {
                for i in 0..3 {
                    px[i] = (px[i] as f32 * (1.0 - k) + c[i] as f32 * k) as u8;
                }
            }
        }
        Some(out)
    }

    /// Applies adjustment events/tracks: effects on what is already composited.
    fn adjust(&mut self, p: &Project, ti: usize, t: f64, canvas: &mut Vec<u8>, w: u32, h: u32) {
        let track = &p.tracks[ti];
        let level = env_value(&track.vol_env, t, track.level);
        for (ei, e) in track.events.iter().enumerate() {
            if e.mute || !(e.start <= t && t < e.end()) {
                continue;
            }
            if !(track.adjustment || is_adjustment(p, e)) {
                continue;
            }
            let (fin, fout) = effective_fades(track, ei, false);
            let k = (e.gain * fade_gain(e, t - e.start, fin, fout) as f32 * level).clamp(0.0, 1.0);
            if k <= 0.0 {
                continue;
            }
            let mut l = Layer { w, h, data: canvas.clone() };
            let lt = t - e.start;
            if let Some(m) = p.media(e.media) {
                for fx in &m.fx {
                    apply_fx(&mut l, fx, lt);
                }
            }
            for fx in e.fx_at(lt).iter().chain(track.fx.iter()) {
                apply_fx(&mut l, fx, lt);
            }
            for (d, s) in canvas.chunks_exact_mut(4).zip(l.data.chunks_exact(4)) {
                for i in 0..3 {
                    d[i] = (d[i] as f32 * (1.0 - k) + s[i] as f32 * k) as u8;
                }
            }
        }
    }

    /// Composite the whole project at time t into an opaque RGBA image.
    pub fn compose(&mut self, p: &Project, t: f64, w: u32, h: u32, auto_crossfade: bool, src: &mut dyn FrameSource) -> Vec<u8> {
        let mut canvas = vec![0u8; (w * h * 4) as usize];
        for px in canvas.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let n = p.tracks.len();
        let mut ti = n;
        while ti > 0 {
            ti -= 1;
            let track = &p.tracks[ti];
            if track.kind != TrackKind::Video || track.child || !p.track_audible(ti) {
                continue;
            }
            // children of this track: the video tracks just below marked "child"
            let mut children = vec![];
            let mut j = ti + 1;
            while j < n && p.tracks[j].kind == TrackKind::Video && p.tracks[j].child {
                children.push(j);
                j += 1;
            }
            let level = env_value(&track.vol_env, t, track.level).clamp(0.0, 1.0);
            if level <= 0.0 {
                continue;
            }
            let layer = if children.is_empty() {
                if track.adjustment || track.events.iter().any(|e| e.start <= t && t < e.end() && is_adjustment(p, e)) {
                    self.adjust(p, ti, t, &mut canvas, w, h);
                }
                self.track_layer(p, ti, t, w, h, auto_crossfade, src)
            } else {
                // compositing group: children bottom to top, then the parent's own picture
                let mut group = Layer::empty(w, h);
                for &c in children.iter().rev() {
                    if !p.track_audible(c) {
                        continue;
                    }
                    let ct = &p.tracks[c];
                    if let Some(mut l) = self.track_layer(p, c, t, w, h, auto_crossfade, src) {
                        let m = ct.motion_at(t);
                        if !m.is_identity() {
                            l = place(&l, w, h, &m);
                        }
                        let lv = env_value(&ct.vol_env, t, ct.level).clamp(0.0, 1.0);
                        blend_layer(&mut group.data, &l, lv, ct.blend);
                    }
                }
                if let Some(l) = self.track_layer(p, ti, t, w, h, auto_crossfade, src) {
                    blend_layer(&mut group.data, &l, 1.0, BlendMode::Normal);
                }
                Some(group)
            };
            if let Some(mut l) = layer {
                let m = track.motion_at(t);
                if !m.is_identity() {
                    l = place(&l, w, h, &m);
                }
                blend_layer(&mut canvas, &l, level, track.blend);
            }
        }
        if !p.output_fx.is_empty() {
            let mut l = Layer { w, h, data: canvas };
            for fx in &p.output_fx {
                apply_fx(&mut l, fx, t);
            }
            canvas = l.data;
            for px in canvas.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        canvas
    }
}

/// Places a source picture (sw x sh) centred in a w x h layer with pan/crop
/// (zoom, offset, rotation), nearest sampling.
pub fn place(src: &Layer, w: u32, h: u32, pan: &PanCrop) -> Layer {
    let (sw, sh) = (src.w, src.h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    if pan.is_identity() && sw <= w && sh <= h {
        let ox = (w - sw) / 2;
        let oy = (h - sh) / 2;
        let cw = sw as usize * 4;
        for y in 0..sh {
            let si = (y * sw) as usize * 4;
            let di = (((y + oy) * w + ox) * 4) as usize;
            out[di..di + cw].copy_from_slice(&src.data[si..si + cw]);
        }
        return Layer { w, h, data: out };
    }
    let zoom = pan.zoom.max(0.01);
    let (sin, cos) = (-pan.rotation.to_radians()).sin_cos();
    let cx = w as f32 / 2.0 + pan.x * w as f32;
    let cy = h as f32 / 2.0 + pan.y * h as f32;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let ux = (dx * cos - dy * sin) / zoom + sw as f32 / 2.0;
            let uy = (dx * sin + dy * cos) / zoom + sh as f32 / 2.0;
            if ux >= 0.0 && uy >= 0.0 && (ux as u32) < sw && (uy as u32) < sh {
                let si = ((uy as u32 * sw + ux as u32) * 4) as usize;
                let di = ((y * w + x) * 4) as usize;
                out[di..di + 4].copy_from_slice(&src.data[si..si + 4]);
            }
        }
    }
    Layer { w, h, data: out }
}

/// Composites a layer onto `dst` with a blend mode (straight alpha on both).
pub fn blend_layer(dst: &mut [u8], l: &Layer, opacity: f32, mode: BlendMode) {
    if mode == BlendMode::Normal {
        blend_over_alpha(dst, l, opacity);
        return;
    }
    let op = opacity.clamp(0.0, 1.0);
    for (d, s) in dst.chunks_exact_mut(4).zip(l.data.chunks_exact(4)) {
        let a = s[3] as f32 / 255.0 * op;
        if a <= 0.0 {
            continue;
        }
        for k in 0..3 {
            let (b, f) = (d[k] as f32 / 255.0, s[k] as f32 / 255.0);
            let v = match mode {
                BlendMode::Normal => f,
                BlendMode::Add => (b + f).min(1.0),
                BlendMode::Subtract => (b - f).max(0.0),
                BlendMode::Multiply => b * f,
                BlendMode::Screen => 1.0 - (1.0 - b) * (1.0 - f),
                BlendMode::Overlay => {
                    if b < 0.5 {
                        2.0 * b * f
                    } else {
                        1.0 - 2.0 * (1.0 - b) * (1.0 - f)
                    }
                }
                BlendMode::Darken => b.min(f),
                BlendMode::Lighten => b.max(f),
                BlendMode::Difference => (b - f).abs(),
            };
            d[k] = ((b * (1.0 - a) + v * a) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (d[3] as f32 + (255.0 - d[3] as f32) * a) as u8;
    }
}

/// "Over" compositing that keeps the destination alpha meaningful (groups).
fn blend_over_alpha(dst: &mut [u8], l: &Layer, opacity: f32) {
    let op = opacity.clamp(0.0, 1.0);
    if op <= 0.0 {
        return;
    }
    for (d, s) in dst.chunks_exact_mut(4).zip(l.data.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0 * op;
        if sa <= 0.0 {
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        if oa <= 0.0 {
            continue;
        }
        for k in 0..3 {
            d[k] = ((s[k] as f32 * sa + d[k] as f32 * da * (1.0 - sa)) / oa).round().min(255.0) as u8;
        }
        d[3] = (oa * 255.0).round() as u8;
    }
}

/// out = (A*wa + B*wb + 0x2000) >> 14, clamped to 255 (weights in Q14).
fn weighted_crossfade(a: &Layer, b: &Layer, wa: u32, wb: u32) -> Layer {
    let clamp = wa + wb > 0x4000;
    let data = a
        .data
        .iter()
        .zip(b.data.iter())
        .map(|(&pa, &pb)| {
            let v = (pa as u32 * wa + pb as u32 * wb + 0x2000) >> 14;
            if clamp {
                v.min(255) as u8
            } else {
                v as u8
            }
        })
        .collect();
    Layer { w: a.w, h: a.h, data }
}

fn scale_alpha(l: &mut Layer, g: f32) {
    if (g - 1.0).abs() < 1e-4 {
        return;
    }
    for px in l.data.chunks_exact_mut(4) {
        px[3] = (px[3] as f32 * g.clamp(0.0, 1.0)) as u8;
    }
}

pub fn blend_over(canvas: &mut [u8], l: &Layer, opacity: f32) {
    let op = opacity.clamp(0.0, 1.0);
    if op <= 0.0 {
        return;
    }
    for (d, s) in canvas.chunks_exact_mut(4).zip(l.data.chunks_exact(4)) {
        let a = s[3] as f32 / 255.0 * op;
        if a <= 0.0 {
            continue;
        }
        if a >= 0.999 {
            d[0] = s[0];
            d[1] = s[1];
            d[2] = s[2];
            continue;
        }
        for k in 0..3 {
            d[k] = (s[k] as f32 * a + d[k] as f32 * (1.0 - a)) as u8;
        }
    }
}

fn transition(kind: Transition, a: &Layer, b: &Layer, p: f32) -> Layer {
    let (w, h) = (a.w, a.h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let p = p.clamp(0.0, 1.0);
    let lerp = |x: &[u8], y: &[u8], t: f32, o: &mut [u8]| {
        // mix with premultiplied alpha so transparent areas fade correctly
        let xa = x[3] as f32 / 255.0;
        let ya = y[3] as f32 / 255.0;
        let oa = xa * (1.0 - t) + ya * t;
        for k in 0..3 {
            let v = x[k] as f32 * xa * (1.0 - t) + y[k] as f32 * ya * t;
            o[k] = if oa > 0.0 { (v / oa).min(255.0) as u8 } else { 0 };
        }
        o[3] = (oa * 255.0) as u8;
    };
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let pa = &a.data[i..i + 4];
            let pb = &b.data[i..i + 4];
            let o = &mut out[i..i + 4];
            match kind {
                Transition::Dissolve => lerp(pa, pb, p, o),
                Transition::WipeLeft => {
                    let edge = p * w as f32;
                    let soft = (w as f32 * 0.02).max(1.0);
                    let t = ((edge - x as f32) / soft + 0.5).clamp(0.0, 1.0);
                    lerp(pa, pb, t, o)
                }
                Transition::WipeDown => {
                    let edge = p * h as f32;
                    let soft = (h as f32 * 0.02).max(1.0);
                    let t = ((edge - y as f32) / soft + 0.5).clamp(0.0, 1.0);
                    lerp(pa, pb, t, o)
                }
                Transition::Push => {
                    let shift = ((1.0 - p) * w as f32) as i64;
                    let xb = x as i64 - shift;
                    if xb >= 0 {
                        let j = ((y as i64 * w as i64 + xb) * 4) as usize;
                        o.copy_from_slice(&b.data[j..j + 4]);
                    } else {
                        let xa = x as i64 + (p * w as f32) as i64;
                        if xa < w as i64 {
                            let j = ((y as i64 * w as i64 + xa) * 4) as usize;
                            o.copy_from_slice(&a.data[j..j + 4]);
                        }
                    }
                }
                Transition::Iris => {
                    let dx = x as f32 - w as f32 / 2.0;
                    let dy = y as f32 - h as f32 / 2.0;
                    let r = (dx * dx + dy * dy).sqrt();
                    let maxr = ((w * w + h * h) as f32).sqrt() / 2.0;
                    let soft = maxr * 0.02;
                    let t = ((p * maxr - r) / soft + 0.5).clamp(0.0, 1.0);
                    lerp(pa, pb, t, o)
                }
                Transition::ThroughBlack => {
                    let black = [0u8, 0, 0, 255];
                    if p < 0.5 {
                        lerp(pa, &black, p * 2.0, o);
                    } else {
                        lerp(&black, pb, (p - 0.5) * 2.0, o);
                    }
                }
                Transition::Blinds => {
                    let n = 8.0;
                    let band = h as f32 / n;
                    let local = (y as f32 % band) / band;
                    let tt = ((p * 1.1 - local * 0.1) * 1.0).clamp(0.0, 1.0);
                    lerp(pa, pb, if local < tt { 1.0 } else { 0.0 }, o)
                }
                Transition::ClockWipe => {
                    let dx = x as f32 - w as f32 / 2.0;
                    let dy = y as f32 - h as f32 / 2.0;
                    // angle from 12 o'clock, clockwise, 0..1
                    let ang = (dx.atan2(-dy) / std::f32::consts::TAU + 1.0) % 1.0;
                    lerp(pa, pb, if ang < p { 1.0 } else { 0.0 }, o)
                }
                Transition::Flash => {
                    let white = [255u8, 255, 255, 255];
                    if p < 0.5 {
                        lerp(pa, &white, (p * 2.0).powf(2.0), o);
                    } else {
                        lerp(&white, pb, ((p - 0.5) * 2.0).sqrt(), o);
                    }
                }
            }
        }
    }
    Layer { w, h, data: out }
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

pub fn apply_fx(l: &mut Layer, fx: &VideoFx, t: f64) {
    if crate::color::apply(l, fx, t) || crate::fx_pack::apply(l, fx, t) {
        return;
    }
    match *fx {
        VideoFx::Blur { radius } => {
            let r = (radius * l.w as f32 / 960.0).round() as i32;
            if r >= 1 {
                box_blur(l, r);
                box_blur(l, r);
            }
            return;
        }
        VideoFx::Vignette { amount } => {
            let (w, h) = (l.w as f32, l.h as f32);
            for y in 0..l.h {
                for x in 0..l.w {
                    let dx = (x as f32 / w - 0.5) * 2.0;
                    let dy = (y as f32 / h - 0.5) * 2.0;
                    let d = ((dx * dx + dy * dy) / 2.0).min(1.0);
                    let f = 1.0 - amount * d * d * 1.5;
                    let i = ((y * l.w + x) * 4) as usize;
                    for k in 0..3 {
                        l.data[i + k] = (l.data[i + k] as f32 * f.clamp(0.0, 1.0)) as u8;
                    }
                }
            }
            return;
        }
        _ => {}
    }
    for px in l.data.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
        let (nr, ng, nb) = match *fx {
            VideoFx::BrightnessContrast { brightness, contrast } => {
                let f = |c: f32| ((c / 255.0 - 0.5) * (1.0 + contrast) + 0.5 + brightness) * 255.0;
                (f(r), f(g), f(b))
            }
            VideoFx::Saturation { amount } => {
                let y = luma(r, g, b);
                (y + (r - y) * amount, y + (g - y) * amount, y + (b - y) * amount)
            }
            VideoFx::BlackWhite { amount } => {
                let y = luma(r, g, b);
                (r + (y - r) * amount, g + (y - g) * amount, b + (y - b) * amount)
            }
            VideoFx::Sepia { amount } => {
                let sr = 0.393 * r + 0.769 * g + 0.189 * b;
                let sg = 0.349 * r + 0.686 * g + 0.168 * b;
                let sb = 0.272 * r + 0.534 * g + 0.131 * b;
                (r + (sr - r) * amount, g + (sg - g) * amount, b + (sb - b) * amount)
            }
            VideoFx::Invert { amount } => (
                r + (255.0 - 2.0 * r) * amount,
                g + (255.0 - 2.0 * g) * amount,
                b + (255.0 - 2.0 * b) * amount,
            ),
            _ => (r, g, b),
        };
        px[0] = nr.clamp(0.0, 255.0) as u8;
        px[1] = ng.clamp(0.0, 255.0) as u8;
        px[2] = nb.clamp(0.0, 255.0) as u8;
    }
}

pub fn blur_layer(l: &mut Layer, r: i32) {
    box_blur(l, r);
}

fn box_blur(l: &mut Layer, r: i32) {
    let (w, h) = (l.w as i32, l.h as i32);
    let mut tmp = l.data.clone();
    let n = (2 * r + 1) as u32;
    for y in 0..h {
        let mut acc = [0u32; 4];
        for k in -r..=r {
            let xi = k.clamp(0, w - 1);
            let i = ((y * w + xi) * 4) as usize;
            for c in 0..4 {
                acc[c] += l.data[i + c] as u32;
            }
        }
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                tmp[i + c] = (acc[c] / n) as u8;
            }
            let add = (x + r + 1).clamp(0, w - 1);
            let sub = (x - r).clamp(0, w - 1);
            let ia = ((y * w + add) * 4) as usize;
            let is = ((y * w + sub) * 4) as usize;
            for c in 0..4 {
                acc[c] = acc[c] + l.data[ia + c] as u32 - l.data[is + c] as u32;
            }
        }
    }
    for x in 0..w {
        let mut acc = [0u32; 4];
        for k in -r..=r {
            let yi = k.clamp(0, h - 1);
            let i = ((yi * w + x) * 4) as usize;
            for c in 0..4 {
                acc[c] += tmp[i + c] as u32;
            }
        }
        for y in 0..h {
            let i = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                l.data[i + c] = (acc[c] / n) as u8;
            }
            let add = (y + r + 1).clamp(0, h - 1);
            let sub = (y - r).clamp(0, h - 1);
            let ia = ((add * w + x) * 4) as usize;
            let is = ((sub * w + x) * 4) as usize;
            for c in 0..4 {
                acc[c] = acc[c] + tmp[ia + c] as u32 - tmp[is + c] as u32;
            }
        }
    }
}
