//! Audio effects applied to clips: EQ, compressor, reverb, delay, noise gate,
//! noise reduction, gain, plus an offline pitch shifter.
//!
//! All processing works on interleaved stereo `f32` buffers (L, R, L, R, ...)
//! and is fully deterministic (no randomness, no global state).
#![allow(dead_code)]

use crate::i18n::tr;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::ops::RangeInclusive;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AudioFx {
    /// Low shelf at 120 Hz, peaking band at `mid_hz` (Q 1.0), high shelf at 8 kHz.
    Eq { low_db: f32, mid_db: f32, mid_hz: f32, high_db: f32 },
    /// Stereo-linked RMS-detector compressor with a 6 dB soft knee.
    Compressor { threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32, makeup_db: f32 },
    /// Freeverb-style reverb (8 parallel combs + 4 series allpasses per channel).
    Reverb { room: f32, damping: f32, mix: f32 },
    /// Feedback delay (echo).
    Delay { ms: f32, feedback: f32, mix: f32 },
    /// Noise gate with hysteresis and hold.
    NoiseGate { threshold_db: f32, release_ms: f32 },
    /// STFT spectral subtraction, noise profile estimated from the quietest 10% of frames.
    NoiseReduction { amount: f32 },
    /// Plain gain.
    Gain { db: f32 },
}

impl AudioFx {
    /// One default instance of each effect.
    pub fn presets() -> Vec<AudioFx> {
        vec![
            AudioFx::Eq { low_db: 0.0, mid_db: 0.0, mid_hz: 1000.0, high_db: 0.0 },
            AudioFx::Compressor {
                threshold_db: -18.0,
                ratio: 4.0,
                attack_ms: 10.0,
                release_ms: 120.0,
                makeup_db: 0.0,
            },
            AudioFx::Reverb { room: 0.5, damping: 0.5, mix: 0.25 },
            AudioFx::Delay { ms: 350.0, feedback: 0.35, mix: 0.3 },
            AudioFx::NoiseGate { threshold_db: -45.0, release_ms: 120.0 },
            AudioFx::NoiseReduction { amount: 0.5 },
            AudioFx::Gain { db: 0.0 },
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AudioFx::Eq { .. } => tr("Égaliseur"),
            AudioFx::Compressor { .. } => tr("Compresseur"),
            AudioFx::Reverb { .. } => tr("Réverbération"),
            AudioFx::Delay { .. } => tr("Écho (délai)"),
            AudioFx::NoiseGate { .. } => tr("Porte de bruit"),
            AudioFx::NoiseReduction { .. } => tr("Réduction de bruit"),
            AudioFx::Gain { .. } => tr("Gain"),
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            AudioFx::Eq { .. } => "eq",
            AudioFx::Compressor { .. } => "compressor",
            AudioFx::Reverb { .. } => "reverb",
            AudioFx::Delay { .. } => "delay",
            AudioFx::NoiseGate { .. } => "gate",
            AudioFx::NoiseReduction { .. } => "noise_reduction",
            AudioFx::Gain { .. } => "gain",
        }
    }

    pub fn from_key(k: &str) -> Option<AudioFx> {
        AudioFx::presets().into_iter().find(|f| f.key() == k)
    }

    /// Edit the parameters with sliders; returns true when something changed.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        match self {
            AudioFx::Eq { low_db, mid_db, mid_hz, high_db } => {
                changed |= slider(ui, low_db, -24.0..=24.0, tr("Graves (120 Hz)"), " dB");
                changed |= slider(ui, mid_db, -24.0..=24.0, tr("Médiums"), " dB");
                changed |= ui
                    .add(
                        egui::Slider::new(mid_hz, 100.0..=8000.0)
                            .logarithmic(true)
                            .text(tr("Fréquence des médiums"))
                            .suffix(" Hz"),
                    )
                    .changed();
                changed |= slider(ui, high_db, -24.0..=24.0, tr("Aigus (8 kHz)"), " dB");
            }
            AudioFx::Compressor { threshold_db, ratio, attack_ms, release_ms, makeup_db } => {
                changed |= slider(ui, threshold_db, -60.0..=0.0, tr("Seuil"), " dB");
                changed |= ui
                    .add(
                        egui::Slider::new(ratio, 1.0..=20.0)
                            .logarithmic(true)
                            .text(tr("Taux"))
                            .suffix(":1"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(attack_ms, 0.1..=200.0)
                            .logarithmic(true)
                            .text(tr("Attaque"))
                            .suffix(" ms"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(release_ms, 5.0..=2000.0)
                            .logarithmic(true)
                            .text(tr("Relâchement"))
                            .suffix(" ms"),
                    )
                    .changed();
                changed |= slider(ui, makeup_db, 0.0..=24.0, tr("Gain de compensation"), " dB");
            }
            AudioFx::Reverb { room, damping, mix } => {
                changed |= slider(ui, room, 0.0..=1.0, tr("Taille de la pièce"), "");
                changed |= slider(ui, damping, 0.0..=1.0, tr("Amortissement"), "");
                changed |= slider(ui, mix, 0.0..=1.0, tr("Mélange"), "");
            }
            AudioFx::Delay { ms, feedback, mix } => {
                changed |= ui
                    .add(
                        egui::Slider::new(ms, 1.0..=2000.0)
                            .logarithmic(true)
                            .text(tr("Délai"))
                            .suffix(" ms"),
                    )
                    .changed();
                changed |= slider(ui, feedback, 0.0..=0.95, tr("Réinjection"), "");
                changed |= slider(ui, mix, 0.0..=1.0, tr("Mélange"), "");
            }
            AudioFx::NoiseGate { threshold_db, release_ms } => {
                changed |= slider(ui, threshold_db, -90.0..=0.0, tr("Seuil"), " dB");
                changed |= ui
                    .add(
                        egui::Slider::new(release_ms, 5.0..=2000.0)
                            .logarithmic(true)
                            .text(tr("Relâchement"))
                            .suffix(" ms"),
                    )
                    .changed();
            }
            AudioFx::NoiseReduction { amount } => {
                changed |= slider(ui, amount, 0.0..=1.0, tr("Intensité"), "");
            }
            AudioFx::Gain { db } => {
                changed |= slider(ui, db, -48.0..=24.0, tr("Gain"), " dB");
            }
        }
        changed
    }
}

fn slider(
    ui: &mut egui::Ui,
    v: &mut f32,
    range: RangeInclusive<f32>,
    label: &'static str,
    suffix: &str,
) -> bool {
    ui.add(egui::Slider::new(v, range).text(label).suffix(suffix)).changed()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Replace non-finite parameters with a default.
fn fin(v: f32, default: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        default
    }
}

fn sane_rate(rate: f32) -> f64 {
    if rate.is_finite() && rate >= 1000.0 {
        rate as f64
    } else {
        48000.0
    }
}

fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

fn lin_to_db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

/// One-pole smoothing coefficient for a time constant in milliseconds.
fn time_coef(ms: f64, rate: f64) -> f64 {
    let n = ms.max(0.01) * 0.001 * rate;
    (-1.0 / n.max(1e-6)).exp()
}

/// Peak of interleaved samples (abs max).
pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0f32, |m, &s| if s.abs() > m { s.abs() } else { m })
}

// ---------------------------------------------------------------------------
// Chain
// ---------------------------------------------------------------------------

/// Process interleaved stereo f32 samples in place (len = frames*2), at
/// `rate` Hz, applying the chain in order.
pub fn process(chain: &[AudioFx], samples: &mut [f32], rate: f32) {
    if samples.is_empty() {
        return;
    }
    let rate = sane_rate(rate);
    for fx in chain {
        match *fx {
            AudioFx::Eq { low_db, mid_db, mid_hz, high_db } => eq(
                samples,
                rate,
                fin(low_db, 0.0),
                fin(mid_db, 0.0),
                fin(mid_hz, 1000.0),
                fin(high_db, 0.0),
            ),
            AudioFx::Compressor { threshold_db, ratio, attack_ms, release_ms, makeup_db } => {
                compressor(
                    samples,
                    rate,
                    fin(threshold_db, -18.0) as f64,
                    fin(ratio, 4.0) as f64,
                    fin(attack_ms, 10.0) as f64,
                    fin(release_ms, 120.0) as f64,
                    fin(makeup_db, 0.0) as f64,
                )
            }
            AudioFx::Reverb { room, damping, mix } => reverb(
                samples,
                rate,
                fin(room, 0.5).clamp(0.0, 1.0),
                fin(damping, 0.5).clamp(0.0, 1.0),
                fin(mix, 0.25).clamp(0.0, 1.0),
            ),
            AudioFx::Delay { ms, feedback, mix } => delay(
                samples,
                rate,
                fin(ms, 350.0).clamp(0.0, 10_000.0),
                fin(feedback, 0.35).clamp(0.0, 0.98),
                fin(mix, 0.3).clamp(0.0, 1.0),
            ),
            AudioFx::NoiseGate { threshold_db, release_ms } => noise_gate(
                samples,
                rate,
                fin(threshold_db, -45.0) as f64,
                fin(release_ms, 120.0) as f64,
            ),
            AudioFx::NoiseReduction { amount } => {
                noise_reduction(samples, rate, fin(amount, 0.0).clamp(0.0, 1.0))
            }
            AudioFx::Gain { db } => {
                let g = db_to_lin(fin(db, 0.0).clamp(-120.0, 60.0) as f64) as f32;
                if g != 1.0 {
                    for s in samples.iter_mut() {
                        *s *= g;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Biquads (RBJ cookbook), transposed direct form II in f64
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn clamp_freq(f: f64, rate: f64) -> f64 {
        f.clamp(10.0, rate * 0.45)
    }

    fn peaking(rate: f64, f: f64, q: f64, db: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w0 = 2.0 * PI * Self::clamp_freq(f, rate) / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        Self::norm(
            1.0 + alpha * a,
            -2.0 * cs,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cs,
            1.0 - alpha / a,
        )
    }

    /// Shelf slope S = 1.
    fn low_shelf(rate: f64, f: f64, db: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w0 = 2.0 * PI * Self::clamp_freq(f, rate) / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / 2.0 * std::f64::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * cs + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
            a * ((a + 1.0) - (a - 1.0) * cs - sa),
            (a + 1.0) + (a - 1.0) * cs + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cs),
            (a + 1.0) + (a - 1.0) * cs - sa,
        )
    }

    /// Shelf slope S = 1.
    fn high_shelf(rate: f64, f: f64, db: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w0 = 2.0 * PI * Self::clamp_freq(f, rate) / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / 2.0 * std::f64::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * cs + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
            a * ((a + 1.0) + (a - 1.0) * cs - sa),
            (a + 1.0) - (a - 1.0) * cs + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cs),
            (a + 1.0) - (a - 1.0) * cs - sa,
        )
    }

    fn lowpass(rate: f64, f: f64, q: f64) -> Self {
        let w0 = 2.0 * PI * Self::clamp_freq(f, rate) / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        Self::norm(
            (1.0 - cs) / 2.0,
            1.0 - cs,
            (1.0 - cs) / 2.0,
            1.0 + alpha,
            -2.0 * cs,
            1.0 - alpha,
        )
    }

    #[inline]
    fn run(&self, st: &mut BiquadState, x: f64) -> f64 {
        let y = self.b0 * x + st.z1;
        st.z1 = self.b1 * x - self.a1 * y + st.z2;
        st.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

fn eq(samples: &mut [f32], rate: f64, low_db: f32, mid_db: f32, mid_hz: f32, high_db: f32) {
    let mut filters = Vec::new();
    if low_db.abs() > 1e-4 {
        filters.push(Biquad::low_shelf(rate, 120.0, low_db as f64));
    }
    if mid_db.abs() > 1e-4 {
        filters.push(Biquad::peaking(rate, mid_hz as f64, 1.0, mid_db as f64));
    }
    if high_db.abs() > 1e-4 {
        filters.push(Biquad::high_shelf(rate, 8000.0, high_db as f64));
    }
    if filters.is_empty() {
        return;
    }
    let mut states = vec![[BiquadState::default(); 2]; filters.len()];
    for frame in samples.chunks_exact_mut(2) {
        for ch in 0..2 {
            let mut x = frame[ch] as f64;
            for (f, st) in filters.iter().zip(states.iter_mut()) {
                x = f.run(&mut st[ch], x);
            }
            frame[ch] = x as f32;
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamics
// ---------------------------------------------------------------------------

/// Gain reduction (positive dB) for a level in dB, with a soft knee.
fn comp_gain_reduction(level_db: f64, thr: f64, ratio: f64, knee: f64) -> f64 {
    let over = level_db - thr;
    let slope = 1.0 - 1.0 / ratio;
    if 2.0 * over <= -knee {
        0.0
    } else if 2.0 * over.abs() < knee {
        let t = over + knee / 2.0;
        slope * t * t / (2.0 * knee)
    } else {
        slope * over
    }
}

fn compressor(
    samples: &mut [f32],
    rate: f64,
    threshold_db: f64,
    ratio: f64,
    attack_ms: f64,
    release_ms: f64,
    makeup_db: f64,
) {
    let ratio = ratio.max(1.0);
    let att = time_coef(attack_ms.max(0.05), rate);
    let rel = time_coef(release_ms.max(1.0), rate);
    // Short RMS window for the detector (5 ms), stereo-linked (max of channels).
    let rms_c = time_coef(5.0, rate);
    let mut ms = 0.0f64;
    let mut gr = 0.0f64;
    let knee = 6.0;
    for frame in samples.chunks_exact_mut(2) {
        let l = frame[0] as f64;
        let r = frame[1] as f64;
        let sq = (l * l).max(r * r);
        ms = rms_c * ms + (1.0 - rms_c) * sq;
        // Blend RMS and peak: RMS mostly, but don't ignore fast transients.
        let level = ms.sqrt().max(sq.sqrt() * 0.5);
        let target = comp_gain_reduction(lin_to_db(level), threshold_db, ratio, knee);
        let c = if target > gr { att } else { rel };
        gr = c * gr + (1.0 - c) * target;
        let g = db_to_lin(makeup_db - gr) as f32;
        frame[0] *= g;
        frame[1] *= g;
    }
    // A trailing unpaired sample (odd length) gets the last gain.
    if samples.len() % 2 == 1 {
        let g = db_to_lin(makeup_db - gr) as f32;
        let n = samples.len();
        samples[n - 1] *= g;
    }
}

fn noise_gate(samples: &mut [f32], rate: f64, threshold_db: f64, release_ms: f64) {
    let open_thr = db_to_lin(threshold_db);
    let close_thr = open_thr * 0.5; // 6 dB hysteresis
    let env_rel = time_coef(10.0, rate);
    let att = time_coef(1.0, rate);
    let rel = time_coef(release_ms.max(1.0), rate);
    let hold_len = (0.02 * rate) as usize;
    let mut env = 0.0f64;
    let mut open = false;
    let mut hold = 0usize;
    let mut g = 0.0f64;
    for frame in samples.chunks_exact_mut(2) {
        let x = (frame[0].abs().max(frame[1].abs())) as f64;
        env = if x > env { x } else { env * env_rel };
        if env >= open_thr {
            open = true;
            hold = hold_len;
        } else if env < close_thr {
            if hold > 0 {
                hold -= 1;
            } else {
                open = false;
            }
        }
        let target = if open { 1.0 } else { 0.0 };
        let c = if target > g { att } else { rel };
        g = target + (g - target) * c;
        frame[0] *= g as f32;
        frame[1] *= g as f32;
    }
}

// ---------------------------------------------------------------------------
// Reverb (Freeverb)
// ---------------------------------------------------------------------------

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Comb { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0 }
    }

    #[inline]
    fn run(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * damp2 + self.store * damp1;
        if self.store.abs() < 1e-20 {
            self.store = 0.0;
        }
        self.buf[self.idx] = input + self.store * feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Allpass { buf: vec![0.0; len.max(1)], idx: 0 }
    }

    #[inline]
    fn run(&mut self, input: f32) -> f32 {
        let bufout = self.buf[self.idx];
        let out = -input + bufout;
        let mut v = input + bufout * 0.5;
        if v.abs() < 1e-20 {
            v = 0.0;
        }
        self.buf[self.idx] = v;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

fn reverb(samples: &mut [f32], rate: f64, room: f32, damping: f32, mix: f32) {
    if mix <= 0.0 {
        return;
    }
    const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
    const SPREAD: usize = 23;
    let scale = rate / 44100.0;
    let sz = |n: usize| ((n as f64) * scale).round().max(1.0) as usize;

    let mut combs_l: Vec<Comb> = COMBS.iter().map(|&n| Comb::new(sz(n))).collect();
    let mut combs_r: Vec<Comb> = COMBS.iter().map(|&n| Comb::new(sz(n + SPREAD))).collect();
    let mut aps_l: Vec<Allpass> = ALLPASSES.iter().map(|&n| Allpass::new(sz(n))).collect();
    let mut aps_r: Vec<Allpass> = ALLPASSES.iter().map(|&n| Allpass::new(sz(n + SPREAD))).collect();

    let feedback = room * 0.28 + 0.7;
    let damp1 = damping * 0.4;
    let damp2 = 1.0 - damp1;
    let wet = mix * 3.0;
    let dry = 1.0 - mix;
    const FIXED_GAIN: f32 = 0.015;

    for frame in samples.chunks_exact_mut(2) {
        let in_l = frame[0];
        let in_r = frame[1];
        let input = (in_l + in_r) * FIXED_GAIN;
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for c in combs_l.iter_mut() {
            out_l += c.run(input, feedback, damp1, damp2);
        }
        for c in combs_r.iter_mut() {
            out_r += c.run(input, feedback, damp1, damp2);
        }
        for a in aps_l.iter_mut() {
            out_l = a.run(out_l);
        }
        for a in aps_r.iter_mut() {
            out_r = a.run(out_r);
        }
        frame[0] = in_l * dry + out_l * wet;
        frame[1] = in_r * dry + out_r * wet;
    }
}

// ---------------------------------------------------------------------------
// Delay
// ---------------------------------------------------------------------------

fn delay(samples: &mut [f32], rate: f64, ms: f32, feedback: f32, mix: f32) {
    if mix <= 0.0 {
        return;
    }
    let d = ((ms as f64) * 0.001 * rate).round().max(1.0) as usize;
    let mut buf_l = vec![0.0f32; d];
    let mut buf_r = vec![0.0f32; d];
    let mut idx = 0usize;
    for frame in samples.chunks_exact_mut(2) {
        let dl = buf_l[idx];
        let dr = buf_r[idx];
        let xl = frame[0];
        let xr = frame[1];
        buf_l[idx] = xl + dl * feedback;
        buf_r[idx] = xr + dr * feedback;
        idx += 1;
        if idx >= d {
            idx = 0;
        }
        frame[0] = xl * (1.0 - mix) + dl * mix;
        frame[1] = xr * (1.0 - mix) + dr * mix;
    }
}

// ---------------------------------------------------------------------------
// Noise reduction (STFT spectral subtraction)
// ---------------------------------------------------------------------------

/// In-place iterative radix-2 complex FFT. `re.len()` must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    if n <= 1 {
        return;
    }
    // Bit reversal.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * PI / len as f64;
        let (ws, wc) = ang.sin_cos();
        let half = len / 2;
        let mut start = 0;
        while start < n {
            let mut cr = 1.0f64;
            let mut ci = 0.0f64;
            for k in 0..half {
                let a = start + k;
                let b = a + half;
                let tr_ = re[b] as f64 * cr - im[b] as f64 * ci;
                let ti = re[b] as f64 * ci + im[b] as f64 * cr;
                re[b] = (re[a] as f64 - tr_) as f32;
                im[b] = (im[a] as f64 - ti) as f32;
                re[a] = (re[a] as f64 + tr_) as f32;
                im[a] = (im[a] as f64 + ti) as f32;
                let ncr = cr * wc - ci * ws;
                ci = cr * ws + ci * wc;
                cr = ncr;
            }
            start += len;
        }
        len <<= 1;
    }
    if inverse {
        let inv = 1.0 / n as f32;
        for v in re.iter_mut() {
            *v *= inv;
        }
        for v in im.iter_mut() {
            *v *= inv;
        }
    }
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()) as f32)
        .collect()
}

fn noise_reduction(samples: &mut [f32], rate: f64, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let frames = samples.len() / 2;
    if frames == 0 {
        return;
    }
    let n = if rate > 64000.0 { 2048usize } else { 1024usize };
    let hop = n / 4;
    let bins = n / 2 + 1;
    let win = hann(n);

    // Frame starts cover every sample with n/hop frames.
    let first = -((n - hop) as isize);
    let starts: Vec<isize> = {
        let mut v = Vec::new();
        let mut s = first;
        while s < frames as isize {
            v.push(s);
            s += hop as isize;
        }
        v
    };

    let over_sub = 1.0 + amount; // over-subtraction factor
    let floor = 1.0 - 0.9 * amount; // spectral floor (gain)

    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];
    let mut chan = vec![0.0f32; frames];
    let mut out = vec![0.0f32; frames];
    let mut wsum = vec![0.0f32; frames];
    let mut noise = vec![0.0f32; bins];
    let mut prev_gain = vec![1.0f32; bins];

    for ch in 0..2 {
        for (i, c) in chan.iter_mut().enumerate() {
            *c = samples[i * 2 + ch];
        }
        let load = |start: isize, re: &mut [f32], im: &mut [f32], chan: &[f32]| {
            for k in 0..n {
                let p = start + k as isize;
                let x = if p >= 0 && (p as usize) < frames { chan[p as usize] } else { 0.0 };
                re[k] = x * win[k];
                im[k] = 0.0;
            }
        };

        // Pass 1: frame energies, pick the quietest 10% of (preferably full) frames.
        let full: Vec<isize> = starts
            .iter()
            .copied()
            .filter(|&s| s >= 0 && s as usize + n <= frames)
            .collect();
        let candidates: &[isize] = if full.is_empty() { &starts } else { &full };
        let mut energies: Vec<(f64, isize)> = candidates
            .iter()
            .map(|&s| {
                let mut e = 0.0f64;
                for k in 0..n {
                    let p = s + k as isize;
                    if p >= 0 && (p as usize) < frames {
                        let x = (chan[p as usize] * win[k]) as f64;
                        e += x * x;
                    }
                }
                (e, s)
            })
            .collect();
        energies.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
        let count = ((energies.len() as f64 * 0.1).ceil() as usize).clamp(1, energies.len().max(1));

        // Pass 2: average noise power spectrum of those frames.
        noise.iter_mut().for_each(|v| *v = 0.0);
        for &(_, s) in energies.iter().take(count) {
            load(s, &mut re, &mut im, &chan);
            fft(&mut re, &mut im, false);
            for b in 0..bins {
                noise[b] += re[b] * re[b] + im[b] * im[b];
            }
        }
        let inv = 1.0 / count as f32;
        noise.iter_mut().for_each(|v| *v *= inv);

        // Pass 3: subtract and resynthesize with weighted overlap-add.
        out.iter_mut().for_each(|v| *v = 0.0);
        wsum.iter_mut().for_each(|v| *v = 0.0);
        prev_gain.iter_mut().for_each(|v| *v = 1.0);
        for &s in &starts {
            load(s, &mut re, &mut im, &chan);
            fft(&mut re, &mut im, false);
            for b in 0..bins {
                let p = re[b] * re[b] + im[b] * im[b];
                let mut g = if p > 1e-30 {
                    ((p - over_sub * noise[b]) / p).max(0.0).sqrt()
                } else {
                    0.0
                };
                g = g.max(floor);
                // Fast attack, smoothed decay: limits "musical noise".
                if g < prev_gain[b] {
                    g = 0.5 * g + 0.5 * prev_gain[b];
                }
                prev_gain[b] = g;
                re[b] *= g;
                im[b] *= g;
                // Mirror for a real-valued inverse.
                if b > 0 && b < n / 2 {
                    re[n - b] = re[b];
                    im[n - b] = -im[b];
                }
            }
            fft(&mut re, &mut im, true);
            for k in 0..n {
                let p = s + k as isize;
                if p >= 0 && (p as usize) < frames {
                    out[p as usize] += re[k] * win[k];
                    wsum[p as usize] += win[k] * win[k];
                }
            }
        }
        for i in 0..frames {
            samples[i * 2 + ch] = if wsum[i] > 1e-6 { out[i] / wsum[i] } else { 0.0 };
        }
    }
}

// ---------------------------------------------------------------------------
// Pitch shift (WSOLA time-stretch + band-limited resampling)
// ---------------------------------------------------------------------------

/// Pitch-shift interleaved stereo by `semitones`, keeping the same length.
pub fn pitch_shift(samples: &[f32], rate: f32, semitones: f32) -> Vec<f32> {
    if !semitones.is_finite() || semitones.abs() < 1e-4 || samples.len() < 4 {
        return samples.to_vec();
    }
    let rate = sane_rate(rate);
    let frames = samples.len() / 2;
    let r = 2f64.powf(semitones.clamp(-36.0, 36.0) as f64 / 12.0);

    // 1. Time-stretch by r (length * r) without changing pitch.
    let st_frames = (frames as f64 * r).ceil() as usize + 4;
    let mut stretched = wsola(samples, frames, rate, r, st_frames);

    // 2. Anti-alias when reading faster than real time (pitch up).
    if r > 1.0 {
        let cutoff = 0.5 * rate / r * 0.9;
        if cutoff < rate * 0.45 {
            let lp = Biquad::lowpass(rate, cutoff, 0.7071);
            for _ in 0..2 {
                let mut st = [BiquadState::default(); 2];
                for f in stretched.chunks_exact_mut(2) {
                    for ch in 0..2 {
                        f[ch] = lp.run(&mut st[ch], f[ch] as f64) as f32;
                    }
                }
            }
        }
    }

    // 3. Resample by r back to the original length (Catmull-Rom).
    let mut out = vec![0.0f32; samples.len()];
    let at = |i: isize, ch: usize| -> f32 {
        if i >= 0 && (i as usize) < st_frames {
            stretched[i as usize * 2 + ch]
        } else {
            0.0
        }
    };
    for i in 0..frames {
        let pos = i as f64 * r;
        let i1 = pos.floor() as isize;
        let t = (pos - i1 as f64) as f32;
        for ch in 0..2 {
            let p0 = at(i1 - 1, ch);
            let p1 = at(i1, ch);
            let p2 = at(i1 + 1, ch);
            let p3 = at(i1 + 2, ch);
            out[i * 2 + ch] = 0.5
                * (2.0 * p1
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
        }
    }
    if samples.len() % 2 == 1 {
        let n = samples.len();
        out[n - 1] = samples[n - 1];
    }
    out
}

/// WSOLA time-stretch of interleaved stereo by factor `r` (output ~ frames * r).
fn wsola(src: &[f32], frames: usize, rate: f64, r: f64, out_frames: usize) -> Vec<f32> {
    let w = (((rate * 0.046) as usize).max(64)) & !1; // ~46 ms window, even
    let hs = w / 2; // synthesis hop (50% overlap)
    let ha = hs as f64 / r; // analysis hop
    let tol = ((rate * 0.008) as isize).max(1); // +-8 ms search
    // Half-sample shifted Hann: sums to 1 at 50% overlap and is never 0 at j = 0.
    let win: Vec<f32> = (0..w)
        .map(|j| (0.5 - 0.5 * (2.0 * PI * (j as f64 + 0.5) / w as f64).cos()) as f32)
        .collect();
    let mono: Vec<f32> = (0..frames).map(|i| 0.5 * (src[i * 2] + src[i * 2 + 1])).collect();
    let get_m = |i: isize| -> f32 {
        if i >= 0 && (i as usize) < frames {
            mono[i as usize]
        } else {
            0.0
        }
    };
    let get = |i: isize, ch: usize| -> f32 {
        if i >= 0 && (i as usize) < frames {
            src[i as usize * 2 + ch]
        } else {
            0.0
        }
    };

    let mut out = vec![0.0f32; out_frames * 2];
    let mut wsum = vec![0.0f32; out_frames];
    let mut prev_pos: isize = 0;
    let mut reference = vec![0.0f32; hs];
    const STRIDE: usize = 4;

    let mut k = 0usize;
    loop {
        let out_start = k * hs;
        if out_start >= out_frames {
            break;
        }
        let ideal = (k as f64 * ha).round() as isize;
        let pos = if k == 0 {
            0
        } else {
            // Natural continuation of the previous segment.
            let nat = prev_pos + hs as isize;
            for (j, v) in reference.iter_mut().enumerate() {
                *v = get_m(nat + j as isize);
            }
            let score = |cand: isize| -> f64 {
                let mut num = 0.0f64;
                let mut en = 0.0f64;
                let mut j = 0;
                while j < hs {
                    let c = get_m(cand + j as isize) as f64;
                    num += c * reference[j] as f64;
                    en += c * c;
                    j += STRIDE;
                }
                num / (en + 1e-9).sqrt()
            };
            // Coarse search (step 2), then refine +-1.
            let mut best = ideal;
            let mut best_s = f64::NEG_INFINITY;
            let mut d = -tol;
            while d <= tol {
                let s = score(ideal + d);
                if s > best_s {
                    best_s = s;
                    best = ideal + d;
                }
                d += 2;
            }
            let center = best;
            for c in [center - 1, center + 1] {
                let s = score(c);
                if s > best_s {
                    best_s = s;
                    best = c;
                }
            }
            best
        };
        for j in 0..w {
            let o = out_start + j;
            if o >= out_frames {
                break;
            }
            let wj = win[j];
            let p = pos + j as isize;
            out[o * 2] += wj * get(p, 0);
            out[o * 2 + 1] += wj * get(p, 1);
            wsum[o] += wj;
        }
        prev_pos = pos;
        k += 1;
    }
    for (i, &ws) in wsum.iter().enumerate() {
        if ws > 1e-4 {
            out[i * 2] /= ws;
            out[i * 2 + 1] /= ws;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48000.0;

    fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (RATE * secs) as usize;
        let mut v = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE).sin();
            v.push(s);
            v.push(s);
        }
        v
    }

    /// Deterministic pseudo-noise (LCG).
    fn noise(n_frames: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n_frames * 2)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                ((s >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    fn zero_crossings(v: &[f32], ch: usize) -> usize {
        let mut c = 0;
        let mut prev = v[ch];
        for f in v.chunks_exact(2).skip(1) {
            let x = f[ch];
            if (prev < 0.0) != (x < 0.0) {
                c += 1;
            }
            prev = x;
        }
        c
    }

    #[test]
    fn gain_doubles_amplitude() {
        let src = sine(440.0, 0.25, 0.1);
        let mut v = src.clone();
        process(&[AudioFx::Gain { db: 6.0206 }], &mut v, RATE);
        for (a, b) in src.iter().zip(v.iter()) {
            assert!((a * 2.0 - b).abs() < 1e-4, "{a} -> {b}");
        }
        assert!((peak(&v) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn flat_eq_is_identity() {
        let src = noise(4800, 0.5, 7);
        let mut v = src.clone();
        process(
            &[AudioFx::Eq { low_db: 0.0, mid_db: 0.0, mid_hz: 1000.0, high_db: 0.0 }],
            &mut v,
            RATE,
        );
        for (a, b) in src.iter().zip(v.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn eq_boosts_low_end() {
        let src = sine(50.0, 0.1, 0.5);
        let mut v = src.clone();
        process(
            &[AudioFx::Eq { low_db: 12.0, mid_db: 0.0, mid_hz: 1000.0, high_db: 0.0 }],
            &mut v,
            RATE,
        );
        let tail = &v[v.len() / 2..];
        // ~ +11-12 dB at 50 Hz on a 120 Hz shelf.
        assert!(peak(tail) > 0.3, "peak {}", peak(tail));
    }

    #[test]
    fn pitch_shift_keeps_length_and_shifts() {
        let src = sine(440.0, 0.5, 1.0);
        let up = pitch_shift(&src, RATE, 12.0);
        assert_eq!(up.len(), src.len());
        let down = pitch_shift(&src, RATE, -5.0);
        assert_eq!(down.len(), src.len());
        assert_eq!(pitch_shift(&src, RATE, 0.0), src);
        // One octave up should roughly double the zero crossing rate.
        let mid = |v: &[f32]| v[v.len() / 4..v.len() * 3 / 4].to_vec();
        let zs = zero_crossings(&mid(&src), 0) as f32;
        let zu = zero_crossings(&mid(&up), 0) as f32;
        assert!((zu / zs - 2.0).abs() < 0.15, "ratio {}", zu / zs);
        // Odd / tiny inputs must not panic.
        assert_eq!(pitch_shift(&[0.1, 0.2, 0.3], RATE, 3.0).len(), 3);
        assert!(pitch_shift(&[], RATE, 3.0).is_empty());
    }

    #[test]
    fn compressor_reduces_loud_sine() {
        let mut v = sine(1000.0, 1.0, 1.0);
        process(
            &[AudioFx::Compressor {
                threshold_db: -20.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
                makeup_db: 0.0,
            }],
            &mut v,
            RATE,
        );
        let tail = &v[v.len() / 2..];
        let p = peak(tail);
        assert!(p < 0.5 && p > 0.05, "peak {p}");
    }

    #[test]
    fn gate_silences_quiet_signal() {
        let mut v = sine(200.0, 0.001, 0.5); // -60 dBFS
        process(&[AudioFx::NoiseGate { threshold_db: -40.0, release_ms: 50.0 }], &mut v, RATE);
        assert!(peak(&v[v.len() / 2..]) < 1e-5);
        let mut loud = sine(200.0, 0.5, 0.5);
        process(&[AudioFx::NoiseGate { threshold_db: -40.0, release_ms: 50.0 }], &mut loud, RATE);
        assert!(peak(&loud[loud.len() / 2..]) > 0.45);
    }

    #[test]
    fn noise_reduction_lowers_noise_floor() {
        // Noise everywhere, a tone in the second half only.
        let frames = 48000;
        let mut v = noise(frames, 0.05, 3);
        let tone = sine(1000.0, 0.5, 1.0);
        for i in frames / 2..frames {
            v[i * 2] += tone[i * 2];
            v[i * 2 + 1] += tone[i * 2 + 1];
        }
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let before = rms(&v[2048..frames / 2]);
        process(&[AudioFx::NoiseReduction { amount: 1.0 }], &mut v, RATE);
        let after = rms(&v[2048..frames / 2]);
        assert!(after < before * 0.5, "before {before} after {after}");
        // The tone survives.
        assert!(peak(&v[frames + 4096..]) > 0.3);
        assert!(v.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn empty_and_odd_buffers_do_not_panic() {
        let chain = AudioFx::presets();
        let mut empty: [f32; 0] = [];
        process(&chain, &mut empty, RATE);
        let mut one = [0.5f32, -0.5];
        process(&chain, &mut one, RATE);
        let mut odd = [0.5f32, -0.5, 0.25];
        process(&chain, &mut odd, RATE);
        let mut v = noise(3000, 0.3, 11);
        process(&chain, &mut v, 0.0); // bogus rate falls back to 48 kHz
        assert!(v.iter().all(|x| x.is_finite()));
        assert_eq!(peak(&[]), 0.0);
    }

    #[test]
    fn keys_roundtrip_and_serde() {
        for fx in AudioFx::presets() {
            assert_eq!(AudioFx::from_key(fx.key()), Some(fx.clone()));
            let s = serde_json::to_string(&fx).unwrap();
            let back: AudioFx = serde_json::from_str(&s).unwrap();
            assert_eq!(back, fx);
        }
        assert_eq!(AudioFx::from_key("nope"), None);
    }

    #[test]
    fn delay_and_reverb_add_tail() {
        // An impulse followed by silence: the echo must appear after `ms`.
        let mut v = vec![0.0f32; 48000 * 2];
        v[0] = 1.0;
        v[1] = 1.0;
        process(&[AudioFx::Delay { ms: 100.0, feedback: 0.5, mix: 0.5 }], &mut v, RATE);
        assert!((v[4800 * 2] - 0.5).abs() < 1e-6);
        assert!((v[9600 * 2] - 0.25).abs() < 1e-6);

        let mut r = vec![0.0f32; 48000 * 2];
        r[0] = 1.0;
        r[1] = 1.0;
        process(&[AudioFx::Reverb { room: 0.8, damping: 0.3, mix: 0.5 }], &mut r, RATE);
        assert!(peak(&r[4800 * 2..]) > 1e-4);
        assert!(r.iter().all(|x| x.is_finite()));
    }
}
