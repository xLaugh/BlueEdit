//! Audio mixing (shared by playback and export) and the playback device.

use crate::media::{AudioData, MediaStore, AUDIO_RATE};
use crate::model::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MixEvent {
    pub pcm: Arc<AudioData>,
    pub start: f64,
    pub length: f64,
    pub offset: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub curve_in: Curve,
    pub curve_out: Curve,
    pub gain: f32,
    pub looped: bool,
    /// Index into Mix::tracks.
    pub track: usize,
    pub rate: f64,
    pub reverse: bool,
    pub velocity: Vec<EnvPoint>,
}

/// Per-track values the mixer needs (envelopes, pan, routing).
#[derive(Clone, Default)]
pub struct MixTrack {
    pub level: f32,
    pub pan: f32,
    pub vol_env: Vec<EnvPoint>,
    pub pan_env: Vec<EnvPoint>,
    pub bus: Option<usize>,
    pub surround: [f32; 2],
}

#[derive(Clone, Default)]
pub struct Mix {
    pub events: Vec<MixEvent>,
    pub tracks: Vec<MixTrack>,
    /// (level, pan, mute) of each audio bus.
    pub buses: Vec<(f32, f32, bool)>,
    pub master: f32,
    /// Metronome tempo (beats per minute) when the metronome is on.
    pub metronome: Option<f64>,
}

/// Global monitoring switches (Dim, mute all audio), set from the interface.
pub static DIM: AtomicBool = AtomicBool::new(false);
pub static MUTE_ALL: AtomicBool = AtomicBool::new(false);
/// Dim amount: -20 dB.
pub const DIM_GAIN: f32 = 0.1;

fn bake_cache() -> &'static Mutex<std::collections::HashMap<(Id, String), Arc<AudioData>>> {
    static C: std::sync::OnceLock<Mutex<std::collections::HashMap<(Id, String), Arc<AudioData>>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// The event's audio with its processing applied once (effects, pitch,
/// normalise, phase), cached by settings. Rate changes keep the pitch when
/// `preserve_pitch` is on (fEventPresevePitchByDefault = 1): the source is
/// pitch-shifted by -12*log2(rate) semitones, then played faster or slower.
fn processed(e: &Event, track: &Track, pcm: Arc<AudioData>) -> Arc<AudioData> {
    let stretch = if e.preserve_pitch && e.velocity.is_empty() && (e.rate - 1.0).abs() > 1e-6 {
        -12.0 * (e.rate.max(0.01)).log2() as f32
    } else {
        0.0
    };
    let semis = e.pitch + stretch;
    if e.afx.is_empty() && track.afx.is_empty() && semis.abs() < 1e-3 && !e.normalize && !e.invert_phase {
        return pcm;
    }
    let key = (
        e.media,
        format!(
            "{}|{}|{:.3}|{}|{}",
            serde_json::to_string(&e.afx).unwrap_or_default(),
            serde_json::to_string(&track.afx).unwrap_or_default(),
            semis,
            e.normalize,
            e.invert_phase
        ),
    );
    if let Some(a) = bake_cache().lock().unwrap().get(&key) {
        return a.clone();
    }
    let rate = AUDIO_RATE as f32;
    let mut buf: Vec<f32> = pcm.samples.iter().map(|v| *v as f32 / 32768.0).collect();
    if semis.abs() >= 1e-3 {
        buf = crate::audio_fx::pitch_shift(&buf, rate, semis);
    }
    crate::audio_fx::process(&e.afx, &mut buf, rate);
    crate::audio_fx::process(&track.afx, &mut buf, rate);
    if e.invert_phase {
        buf.iter_mut().for_each(|v| *v = -*v);
    }
    if e.normalize {
        // normalise to dNormalizeTo (0.98 of full scale)
        let pk = crate::audio_fx::peak(&buf);
        if pk > 1e-6 {
            let g = NORMALIZE_TO / pk;
            buf.iter_mut().for_each(|v| *v *= g);
        }
    }
    let out = Arc::new(AudioData::from_samples(buf.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect()));
    let mut c = bake_cache().lock().unwrap();
    if c.len() > 32 {
        c.clear();
    }
    c.insert(key, out.clone());
    out
}

/// Snapshot of everything audible, for the audio thread.
pub fn build_mix(p: &Project, store: &MediaStore, auto_crossfade: bool) -> Mix {
    let mut events = vec![];
    let mut tracks = vec![];
    for (ti, t) in p.tracks.iter().enumerate() {
        if t.kind != TrackKind::Audio || !p.track_audible(ti) {
            continue;
        }
        let mt = tracks.len();
        tracks.push(MixTrack {
            level: t.level,
            pan: t.pan,
            vol_env: if t.auto_mode == AutoMode::Off { vec![] } else { t.vol_env.clone() },
            pan_env: if t.auto_mode == AutoMode::Off { vec![] } else { t.pan_env.clone() },
            bus: t.bus.filter(|b| *b < p.buses.len()),
            surround: t.surround,
        });
        for (ei, e) in t.events.iter().enumerate() {
            if e.mute {
                continue;
            }
            let Some(pcm) = store.audio(e.media) else { continue };
            let pcm = processed(e, t, pcm);
            let (fin, fout) = effective_fades(t, ei, auto_crossfade);
            events.push(MixEvent {
                pcm,
                start: e.start,
                length: e.length,
                offset: e.offset,
                fade_in: fin,
                fade_out: fout,
                curve_in: e.curve_in,
                curve_out: e.curve_out,
                gain: e.gain,
                looped: e.looped,
                track: mt,
                rate: e.rate,
                reverse: e.reverse,
                velocity: e.velocity.clone(),
            });
        }
    }
    Mix {
        events,
        tracks,
        buses: p.buses.iter().map(|b| (b.level, b.pan, b.mute)).collect(),
        master: p.master_volume,
        metronome: None,
    }
}

/// Mix stereo frames starting at project time `t0`, at `rate` Hz.
pub fn mix_into(mix: &Mix, t0: f64, rate: f64, out: &mut [f32]) {
    mix_channels(mix, t0, rate, out, 2);
}

/// Speaker gains of a source (left, right) for 5.1: L, R, C, LFE, Ls, Rs
/// (equal-power position from the track's surround pan x, y).
fn surround_gains(x: f32, y: f32) -> [[f32; 6]; 2] {
    let front = ((1.0 - y) / 2.0).clamp(0.0, 1.0);
    let back = 1.0 - front;
    let left = ((1.0 - x) / 2.0).clamp(0.0, 1.0);
    let right = 1.0 - left;
    let centre = front * (1.0 - x.abs()) * 0.5;
    [
        [(front * left).sqrt(), 0.0, centre, 0.1, (back * left).sqrt(), 0.0],
        [0.0, (front * right).sqrt(), centre, 0.1, 0.0, (back * right).sqrt()],
    ]
}

/// Mix `out.len() / channels` frames (channels = 2, or 6 for 5.1).
pub fn mix_channels(mix: &Mix, t0: f64, rate: f64, out: &mut [f32], channels: usize) {
    let ch = channels.max(1);
    out.iter_mut().for_each(|s| *s = 0.0);
    if MUTE_ALL.load(Ordering::Relaxed) {
        return;
    }
    let frames = out.len() / ch;
    let t1 = t0 + frames as f64 / rate;
    // one buffer per bus (same layout as out)
    let mut bus_bufs: Vec<Vec<f32>> = mix.buses.iter().map(|_| vec![0f32; out.len()]).collect();
    for ev in &mix.events {
        if ev.start >= t1 || ev.start + ev.length <= t0 {
            continue;
        }
        let n_src = ev.pcm.frames();
        if n_src == 0 {
            continue;
        }
        let tr = mix.tracks.get(ev.track).cloned().unwrap_or_default();
        let tmp_event = Event {
            start: ev.start,
            length: ev.length,
            offset: ev.offset,
            fade_in: ev.fade_in,
            fade_out: ev.fade_out,
            curve_in: ev.curve_in,
            curve_out: ev.curve_out,
            gain: ev.gain,
            looped: ev.looped,
            rate: ev.rate,
            reverse: ev.reverse,
            velocity: ev.velocity.clone(),
            ..Event::blank()
        };
        let dest: &mut [f32] = match tr.bus {
            Some(b) if b < bus_bufs.len() => &mut bus_bufs[b],
            _ => &mut *out,
        };
        let simple = ev.velocity.is_empty() && !ev.reverse;
        for i in 0..frames {
            let t = t0 + i as f64 / rate;
            let lt = t - ev.start;
            if lt < 0.0 || lt >= ev.length {
                continue;
            }
            let src_t = if simple { ev.offset + lt * ev.rate } else { tmp_event.source_time(lt) };
            if src_t < 0.0 {
                continue;
            }
            let mut idx = (src_t * AUDIO_RATE as f64) as usize;
            if idx >= n_src {
                if ev.looped {
                    idx %= n_src;
                } else {
                    continue;
                }
            }
            let mut g = fade_gain(&tmp_event, lt, ev.fade_in, ev.fade_out) * ev.gain as f64;
            // quickfade on event edges (fQuickfadeAudio)
            let edge = lt.min(ev.length - lt);
            if edge < QUICKFADE_SECS {
                g *= edge / QUICKFADE_SECS;
            }
            let level = env_value(&tr.vol_env, t, tr.level);
            let pan = env_value(&tr.pan_env, t, tr.pan);
            let g = g as f32 * level;
            let l = ev.pcm.samples[idx * 2] as f32 / 32768.0 * g;
            let r = ev.pcm.samples[idx * 2 + 1] as f32 / 32768.0 * g;
            if ch == 6 {
                let sg = surround_gains(tr.surround[0], tr.surround[1]);
                for k in 0..6 {
                    dest[i * 6 + k] += l * sg[0][k] + r * sg[1][k];
                }
            } else {
                // simple pan law: centre = full on both sides
                let gl = (1.0 - pan).min(1.0);
                let gr = (1.0 + pan).min(1.0);
                dest[i * ch] += l * gl;
                if ch > 1 {
                    dest[i * ch + 1] += r * gr;
                }
            }
        }
    }
    for (b, buf) in bus_bufs.iter().enumerate() {
        let (level, pan, mute) = mix.buses[b];
        if mute {
            continue;
        }
        for i in 0..frames {
            for k in 0..ch {
                let pg = if ch == 2 {
                    if k == 0 {
                        (1.0 - pan).min(1.0)
                    } else {
                        (1.0 + pan).min(1.0)
                    }
                } else {
                    1.0
                };
                out[i * ch + k] += buf[i * ch + k] * level * pg;
            }
        }
    }
    if let Some(bpm) = mix.metronome {
        let beat = 60.0 / bpm.max(1.0);
        for i in 0..frames {
            let t = t0 + i as f64 / rate;
            let n = (t / beat).floor();
            let dt = t - n * beat;
            if dt < 0.03 {
                // accent on the first beat of each 4/4 bar
                let f = if (n as i64).rem_euclid(4) == 0 { 1500.0 } else { 1000.0 };
                let v = ((2.0 * std::f64::consts::PI * f * dt).sin() * (-dt * 150.0).exp() * 0.5) as f32;
                for k in 0..ch.min(2) {
                    out[i * ch + k] += v;
                }
            }
        }
    }
    let dim = if DIM.load(Ordering::Relaxed) { DIM_GAIN } else { 1.0 };
    for s in out.iter_mut() {
        *s = (*s * mix.master * dim).clamp(-1.0, 1.0);
    }
}

pub struct Shared {
    pub playing: AtomicBool,
    pos_bits: AtomicU64,
    pub mix: Mutex<Arc<Mix>>,
    pub loop_region: Mutex<Option<(f64, f64)>>,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
}

impl Shared {
    pub fn pos(&self) -> f64 {
        f64::from_bits(self.pos_bits.load(Ordering::Relaxed))
    }
    pub fn set_pos(&self, t: f64) {
        self.pos_bits.store(t.to_bits(), Ordering::Relaxed);
    }
    /// Peaks since last call (linear 0..1).
    pub fn take_peaks(&self) -> (f32, f32) {
        let l = f32::from_bits(self.peak_l.swap(0f32.to_bits(), Ordering::Relaxed));
        let r = f32::from_bits(self.peak_r.swap(0f32.to_bits(), Ordering::Relaxed));
        (l, r)
    }
    fn add_peaks(&self, l: f32, r: f32) {
        let pl = f32::from_bits(self.peak_l.load(Ordering::Relaxed));
        let pr = f32::from_bits(self.peak_r.load(Ordering::Relaxed));
        self.peak_l.store(pl.max(l).to_bits(), Ordering::Relaxed);
        self.peak_r.store(pr.max(r).to_bits(), Ordering::Relaxed);
    }
}

pub struct AudioEngine {
    pub shared: Arc<Shared>,
    _stream: Option<cpal::Stream>,
    pub device_name: String,
}

impl AudioEngine {
    pub fn new(enable_device: bool) -> AudioEngine {
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            pos_bits: AtomicU64::new(0f64.to_bits()),
            mix: Mutex::new(Arc::new(Mix::default())),
            loop_region: Mutex::new(None),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
        });
        let (stream, device_name) = if enable_device {
            match open_stream(shared.clone()) {
                Some((s, n)) => (Some(s), n),
                None => (None, String::new()),
            }
        } else {
            (None, String::new())
        };
        AudioEngine {
            shared,
            _stream: stream,
            device_name,
        }
    }
    pub fn has_device(&self) -> bool {
        self._stream.is_some()
    }
}

fn open_stream(shared: Arc<Shared>) -> Option<(cpal::Stream, String)> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let name = device.name().unwrap_or_default();
    let supported = device.default_output_config().ok()?;
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f64;
    let mut buf: Vec<f32> = vec![];
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels.max(1);
                if !shared.playing.load(Ordering::Relaxed) {
                    data.iter_mut().for_each(|s| *s = 0.0);
                    return;
                }
                buf.resize(frames * 2, 0.0);
                let t0 = shared.pos();
                let mix = shared.mix.lock().unwrap().clone();
                mix_into(&mix, t0, rate, &mut buf);
                let (mut pl, mut pr) = (0f32, 0f32);
                for f in 0..frames {
                    let l = buf[f * 2];
                    let r = buf[f * 2 + 1];
                    pl = pl.max(l.abs());
                    pr = pr.max(r.abs());
                    for c in 0..channels {
                        data[f * channels + c] = match c {
                            0 => l,
                            1 => r,
                            _ => 0.0,
                        };
                    }
                }
                shared.add_peaks(pl, pr);
                let mut t = t0 + frames as f64 / rate;
                if let Some((a, b)) = *shared.loop_region.lock().unwrap() {
                    if b > a && t >= b {
                        t = a + (t - b);
                    }
                }
                shared.set_pos(t);
            },
            |_err| {},
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((stream, name))
}

/// Peak level of a mix around time t (used for meters when no device plays).
pub fn peak_at(mix: &Mix, t: f64, window: f64) -> (f32, f32) {
    let rate = 8000.0;
    let n = ((window * rate) as usize).max(16);
    let mut buf = vec![0f32; n * 2];
    mix_into(mix, t, rate, &mut buf);
    let mut l = 0f32;
    let mut r = 0f32;
    for f in 0..n {
        l = l.max(buf[f * 2].abs());
        r = r.max(buf[f * 2 + 1].abs());
    }
    (l, r)
}
