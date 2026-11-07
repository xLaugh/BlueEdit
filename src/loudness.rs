//! ITU-R BS.1770-4 / EBU R128 loudness measurement.
//!
//! * K-weighting (pre-filter shelf + RLB high-pass) with coefficients computed
//!   for any sample rate (same formulas as libebur128 / pyloudnorm).
//! * Momentary (400 ms), short-term (3 s), gated integrated loudness.
//! * Loudness range (EBU Tech 3342).
//! * True peak via 4x oversampling (windowed-sinc polyphase interpolation).
//!
//! All signals are interleaved stereo `f32`.
#![allow(dead_code)]

use std::sync::OnceLock;

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};

use crate::i18n::tr;
use crate::theme;

// ---------------------------------------------------------------------------
// K-weighting
// ---------------------------------------------------------------------------

/// Transposed direct-form II biquad (f64 for numerical robustness at high rates).
#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// K-weighting filter for a stereo signal: two biquads (high shelf + high-pass)
/// per channel, with their state.
#[derive(Clone, Debug)]
pub struct KWeight {
    /// `[channel][stage]`
    stages: [[Biquad; 2]; 2],
}

impl KWeight {
    pub fn new(rate: f32) -> Self {
        let fs = (rate as f64).max(1.0);

        // Stage 1: high-shelf ("pre-filter", head acoustic model).
        let f0 = 1681.974450955533;
        let g = 3.999843853973347;
        let q = 0.7071752369554196;
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b0: (vh + vb * k / q + k * k) / a0,
            b1: 2.0 * (k * k - vh) / a0,
            b2: (vh - vb * k / q + k * k) / a0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            ..Default::default()
        };

        // Stage 2: RLB high-pass.
        let f0 = 38.13547087602444;
        let q = 0.5003270373238773;
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let hp = Biquad {
            b0: 1.0,
            b1: -2.0,
            b2: 1.0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / q + k * k) / a0,
            ..Default::default()
        };

        KWeight { stages: [[shelf, hp], [shelf, hp]] }
    }

    pub fn reset(&mut self) {
        for ch in self.stages.iter_mut() {
            for s in ch.iter_mut() {
                s.reset();
            }
        }
    }

    /// Filter one sample of channel `ch` (0 or 1).
    #[inline]
    pub fn process(&mut self, ch: usize, x: f32) -> f64 {
        let st = &mut self.stages[ch & 1];
        let y = st[0].process(x as f64);
        st[1].process(y)
    }

    /// Filter one stereo frame; returns the channel-weighted sum of squares
    /// (G = 1.0 for left and right).
    #[inline]
    pub fn process_frame(&mut self, l: f32, r: f32) -> f64 {
        let yl = self.process(0, l);
        let yr = self.process(1, r);
        yl * yl + yr * yr
    }
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    /// Gated integrated loudness (LUFS).
    pub integrated: f32,
    /// Loudness range (LU).
    pub range_lu: f32,
    /// Maximum true peak over both channels (dBTP).
    pub true_peak_db: f32,
    /// Maximum momentary loudness (LUFS).
    pub max_momentary: f32,
    /// Maximum short-term loudness (LUFS).
    pub max_short_term: f32,
}

impl Default for Report {
    fn default() -> Self {
        Report {
            integrated: f32::NEG_INFINITY,
            range_lu: 0.0,
            true_peak_db: f32::NEG_INFINITY,
            max_momentary: f32::NEG_INFINITY,
            max_short_term: f32::NEG_INFINITY,
        }
    }
}

/// Mean-square (summed over channels) → LUFS.
#[inline]
fn lufs(ms: f64) -> f32 {
    if !(ms > 1e-15) {
        f32::NEG_INFINITY
    } else {
        (-0.691 + 10.0 * ms.log10()) as f32
    }
}

/// LUFS → mean-square.
#[inline]
fn energy(l: f64) -> f64 {
    10f64.powf((l + 0.691) / 10.0)
}

/// K-weighted energies of consecutive 100 ms segments, plus the segment length
/// in frames. Prefix sums let any 400 ms / 3 s window be computed in O(1).
struct Segments {
    prefix: Vec<f64>,
    hop: usize,
}

impl Segments {
    fn new(samples: &[f32], rate: f32) -> Self {
        let hop = ((rate as f64) * 0.1).round().max(1.0) as usize;
        let frames = samples.len() / 2;
        let mut kw = KWeight::new(rate);
        let mut prefix = Vec::with_capacity(frames / hop + 2);
        prefix.push(0.0);
        let mut total = 0.0f64;
        let mut acc = 0.0f64;
        let mut n = 0usize;
        for fr in samples.chunks_exact(2) {
            acc += kw.process_frame(fr[0], fr[1]);
            n += 1;
            if n == hop {
                total += acc;
                prefix.push(total);
                acc = 0.0;
                n = 0;
            }
        }
        Segments { prefix, hop }
    }

    fn len(&self) -> usize {
        self.prefix.len() - 1
    }

    /// Mean square over the window of `nseg` segments ending at segment `end`
    /// (exclusive). Missing segments before the start count as silence.
    fn window(&self, end: usize, nseg: usize) -> f64 {
        let end = end.min(self.len());
        let start = end.saturating_sub(nseg);
        (self.prefix[end] - self.prefix[start]) / (nseg * self.hop) as f64
    }

    /// Mean squares of all complete windows of `nseg` segments, sliding by one
    /// segment (100 ms). If the signal is shorter than one window, a single
    /// zero-padded window is returned (empty signal → none).
    fn blocks(&self, nseg: usize) -> Vec<f64> {
        let n = self.len();
        if n == 0 {
            return Vec::new();
        }
        if n < nseg {
            return vec![self.window(n, nseg)];
        }
        (nseg..=n).map(|e| self.window(e, nseg)).collect()
    }
}

/// BS.1770-4 gated loudness of 400 ms blocks: absolute gate -70 LUFS,
/// relative gate -10 LU.
fn gated_integrated(blocks: &[f64]) -> f32 {
    let abs_thr = energy(-70.0);
    let mut sum = 0.0;
    let mut n = 0usize;
    for &b in blocks {
        if b > abs_thr {
            sum += b;
            n += 1;
        }
    }
    if n == 0 {
        return f32::NEG_INFINITY;
    }
    let rel_thr = (sum / n as f64) * 0.1; // -10 LU
    let mut sum2 = 0.0;
    let mut n2 = 0usize;
    for &b in blocks {
        if b > abs_thr && b > rel_thr {
            sum2 += b;
            n2 += 1;
        }
    }
    if n2 == 0 {
        return f32::NEG_INFINITY;
    }
    lufs(sum2 / n2 as f64)
}

/// EBU Tech 3342 loudness range from 3 s short-term blocks.
fn loudness_range(short_blocks: &[f64]) -> f32 {
    let abs_thr = energy(-70.0);
    let gated: Vec<f64> = short_blocks.iter().copied().filter(|&b| b > abs_thr).collect();
    if gated.is_empty() {
        return 0.0;
    }
    let mean = gated.iter().sum::<f64>() / gated.len() as f64;
    let rel_thr = mean * 0.01; // -20 LU
    let mut vals: Vec<f64> = gated
        .iter()
        .copied()
        .filter(|&b| b > rel_thr)
        .map(|b| -0.691 + 10.0 * b.log10())
        .collect();
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pct = |p: f64| -> f64 {
        let pos = p * (vals.len() - 1) as f64;
        let i = pos.floor() as usize;
        let frac = pos - i as f64;
        let j = (i + 1).min(vals.len() - 1);
        vals[i] + (vals[j] - vals[i]) * frac
    };
    (pct(0.95) - pct(0.10)).max(0.0) as f32
}

fn max_lufs(blocks: &[f64]) -> f32 {
    blocks.iter().fold(f32::NEG_INFINITY, |m, &b| m.max(lufs(b)))
}

// ---------------------------------------------------------------------------
// True peak
// ---------------------------------------------------------------------------

const TP_TAPS: usize = 12;
/// Index offset of the first tap relative to the left sample `m`.
const TP_FIRST: isize = -5;

/// Interpolation kernels for the 3 intermediate phases (¼, ½, ¾) of a 4×
/// oversampler: Blackman-windowed sinc, 12 taps per phase, DC-normalised.
fn tp_coefs() -> &'static [[f32; TP_TAPS]; 3] {
    static C: OnceLock<[[f32; TP_TAPS]; 3]> = OnceLock::new();
    C.get_or_init(|| {
        let half = 6.5f64;
        let mut c = [[0f32; TP_TAPS]; 3];
        for (p, frac) in [0.25f64, 0.5, 0.75].iter().enumerate() {
            let mut taps = [0f64; TP_TAPS];
            let mut sum = 0.0;
            for (j, tap) in taps.iter_mut().enumerate() {
                let t = (j as isize + TP_FIRST) as f64 - frac;
                let x = std::f64::consts::PI * t;
                let sinc = if t.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                let a = std::f64::consts::PI * t / half;
                let w = 0.42 + 0.5 * a.cos() + 0.08 * (2.0 * a).cos();
                *tap = sinc * w;
                sum += *tap;
            }
            for j in 0..TP_TAPS {
                c[p][j] = (taps[j] / sum) as f32;
            }
        }
        c
    })
}

/// Linear true peak of one channel of an interleaved buffer.
fn true_peak_channel(samples: &[f32], ch: usize, nch: usize) -> f32 {
    let frames = samples.len() / nch;
    if frames == 0 {
        return 0.0;
    }
    let x = |i: isize| -> f32 {
        if i < 0 || i as usize >= frames {
            0.0
        } else {
            samples[i as usize * nch + ch]
        }
    };
    let mut peak = 0f32;
    for i in 0..frames {
        peak = peak.max(samples[i * nch + ch].abs());
    }
    if peak == 0.0 {
        return 0.0;
    }
    // Only interpolate where the neighbourhood is loud enough to possibly
    // produce the maximum (intersample overs on real material stay within a
    // few dB of the surrounding samples).
    let thr = peak * 0.35;
    let coefs = tp_coefs();
    let mut win = [0f32; TP_TAPS];
    for m in 0..frames {
        let a = samples[m * nch + ch].abs();
        let b = if m + 1 < frames { samples[(m + 1) * nch + ch].abs() } else { 0.0 };
        if a.max(b) < thr {
            continue;
        }
        let base = m as isize + TP_FIRST;
        if base >= 0 && (base as usize + TP_TAPS) <= frames {
            let b0 = base as usize;
            for (j, w) in win.iter_mut().enumerate() {
                *w = samples[(b0 + j) * nch + ch];
            }
        } else {
            for (j, w) in win.iter_mut().enumerate() {
                *w = x(base + j as isize);
            }
        }
        for phase in coefs.iter() {
            let mut y = 0f32;
            for j in 0..TP_TAPS {
                y += win[j] * phase[j];
            }
            peak = peak.max(y.abs());
        }
    }
    peak
}

fn to_db(lin: f32) -> f32 {
    if lin > 0.0 {
        20.0 * lin.log10()
    } else {
        f32::NEG_INFINITY
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Full analysis of interleaved stereo f32 at `rate`.
pub fn analyze(samples: &[f32], rate: f32) -> Report {
    let segs = Segments::new(samples, rate);
    let momentary = segs.blocks(4);
    let short = segs.blocks(30);
    // LRA only makes sense on complete 3 s blocks.
    let lra = if segs.len() >= 30 { loudness_range(&short) } else { 0.0 };
    let tp = true_peak_channel(samples, 0, 2).max(true_peak_channel(samples, 1, 2));
    Report {
        integrated: if segs.len() >= 4 { gated_integrated(&momentary) } else { f32::NEG_INFINITY },
        range_lu: lra,
        true_peak_db: to_db(tp),
        max_momentary: max_lufs(&momentary),
        max_short_term: max_lufs(&short),
    }
}

/// Momentary (last 400 ms) and short-term (last 3 s) loudness of a window that
/// ends at the end of `samples` (interleaved stereo). Missing history counts as
/// silence.
pub fn momentary_short(samples: &[f32], rate: f32) -> (f32, f32) {
    let frames = samples.len() / 2;
    if frames == 0 || rate <= 0.0 {
        return (f32::NEG_INFINITY, f32::NEG_INFINITY);
    }
    let m_len = ((rate as f64) * 0.4).round().max(1.0) as usize;
    let s_len = ((rate as f64) * 3.0).round().max(1.0) as usize;
    // Extra lead-in so the filters (38 Hz high-pass) settle.
    let warm = ((rate as f64) * 0.25).round() as usize;
    let start = frames.saturating_sub(s_len + warm);
    let mut kw = KWeight::new(rate);
    let (mut sm, mut ss) = (0f64, 0f64);
    for f in start..frames {
        let e = kw.process_frame(samples[2 * f], samples[2 * f + 1]);
        let from_end = frames - f;
        if from_end <= s_len {
            ss += e;
            if from_end <= m_len {
                sm += e;
            }
        }
    }
    (lufs(sm / m_len as f64), lufs(ss / s_len as f64))
}

/// Short-term loudness every `step_secs` over the whole signal (for the
/// loudness log), returns (time, LUFS). Times are window end times.
pub fn short_term_series(samples: &[f32], rate: f32, step_secs: f32) -> Vec<(f32, f32)> {
    if !(step_secs > 0.0) || rate <= 0.0 {
        return Vec::new();
    }
    let segs = Segments::new(samples, rate);
    let n = segs.len();
    if n == 0 {
        return Vec::new();
    }
    let dur = (samples.len() / 2) as f64 / rate as f64;
    let seg_secs = segs.hop as f64 / rate as f64;
    let mut out = Vec::new();
    let mut i = 1usize;
    loop {
        let t = i as f64 * step_secs as f64;
        if t > dur + 1e-6 {
            break;
        }
        let end = ((t / seg_secs).round() as usize).min(n);
        out.push((t as f32, lufs(segs.window(end, 30))));
        i += 1;
    }
    out
}

/// Format a value with one decimal and a French decimal comma.
fn fmt_num(v: f32) -> String {
    format!("{:.1}", v).replace('.', ",")
}

/// Format LUFS for display, French decimal comma, e.g. "-23,0 LUFS"; -inf → "-inf".
pub fn fmt_lufs(v: f32) -> String {
    if !v.is_finite() || v < -200.0 {
        "-inf".to_string()
    } else {
        format!("{} LUFS", fmt_num(v))
    }
}

fn fmt_db(v: f32, unit: &str) -> String {
    if !v.is_finite() || v < -200.0 {
        "-inf".to_string()
    } else if unit.is_empty() {
        fmt_num(v)
    } else {
        format!("{} {}", fmt_num(v), unit)
    }
}

// ---------------------------------------------------------------------------
// Meter widget
// ---------------------------------------------------------------------------

const METER_MIN: f32 = -60.0;
const METER_MAX: f32 = 0.0;
const TARGET_EBU: f32 = -23.0;
const TARGET_STREAM: f32 = -14.0;

fn zone_colors() -> [(f32, f32, Color32); 3] {
    [
        (METER_MIN, -18.0, Color32::from_rgb(70, 190, 100)),
        (-18.0, -9.0, Color32::from_rgb(230, 190, 60)),
        (-9.0, METER_MAX, Color32::from_rgb(225, 70, 60)),
    ]
}

/// Small meter widget: vertical bars for momentary and short-term loudness
/// (scale -60..0 LUFS, marks at the -23 LUFS EBU R128 and -14 LUFS streaming
/// targets), with integrated / LRA / true peak below.
pub fn meter_ui(ui: &mut egui::Ui, momentary: f32, short: f32, report: Option<&Report>) {
    let scale_w = 28.0;
    let col_w = 70.0;
    let bar_w = 18.0;
    let bar_h = 160.0;
    let text_h = 30.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(scale_w + 2.0 * col_w, bar_h + text_h + 6.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let font = FontId::proportional(10.0);
    let top = rect.top() + 6.0;
    let bottom = top + bar_h;
    let y_of = |v: f32| -> f32 {
        let t = (v.clamp(METER_MIN, METER_MAX) - METER_MIN) / (METER_MAX - METER_MIN);
        bottom - t * bar_h
    };

    // Scale.
    let mut db = METER_MAX;
    while db >= METER_MIN - 0.01 {
        let y = y_of(db);
        painter.line_segment(
            [pos2(rect.left() + scale_w - 4.0, y), pos2(rect.right(), y)],
            Stroke::new(1.0_f32, theme::TEXT_DIM().gamma_multiply(0.25)),
        );
        painter.text(pos2(rect.left() + scale_w - 6.0, y), Align2::RIGHT_CENTER, format!("{}", db as i32), font.clone(), theme::TEXT_DIM());
        db -= 10.0;
    }

    let values = [(momentary, tr("Momentanée")), (short, tr("Court terme"))];
    for (i, (v, label)) in values.iter().enumerate() {
        let cx = rect.left() + scale_w + col_w * (i as f32 + 0.5);
        let bar = Rect::from_min_max(pos2(cx - bar_w * 0.5, top), pos2(cx + bar_w * 0.5, bottom));
        painter.rect_filled(bar, 2.0, theme::BG());
        if v.is_finite() && *v > METER_MIN {
            for (lo, hi, c) in zone_colors() {
                if *v <= lo {
                    continue;
                }
                let seg_top = y_of(v.min(hi));
                let seg = Rect::from_min_max(pos2(bar.left() + 1.0, seg_top), pos2(bar.right() - 1.0, y_of(lo)));
                painter.rect_filled(seg, 0.0, c);
            }
        }
        painter.rect_stroke(bar, 2.0, Stroke::new(1.0_f32, theme::TEXT_DIM().gamma_multiply(0.5)), StrokeKind::Inside);
        painter.text(pos2(cx, bottom + 4.0), Align2::CENTER_TOP, fmt_db(*v, ""), font.clone(), theme::TEXT());
        painter.text(pos2(cx, bottom + 17.0), Align2::CENTER_TOP, *label, font.clone(), theme::TEXT_DIM());
    }

    // Target marks.
    let bars_l = rect.left() + scale_w + col_w * 0.5 - bar_w * 0.5 - 3.0;
    let bars_r = rect.left() + scale_w + col_w * 1.5 + bar_w * 0.5 + 3.0;
    for (t, c) in [(TARGET_EBU, theme::ACCENT()), (TARGET_STREAM, theme::TEXT_DIM())] {
        let y = y_of(t);
        painter.line_segment([pos2(bars_l, y), pos2(bars_r, y)], Stroke::new(1.5_f32, c));
        painter.text(pos2(bars_r + 2.0, y), Align2::LEFT_CENTER, format!("{}", t as i32), FontId::proportional(9.0), c);
    }
    resp.on_hover_text(tr("Repères : -23 LUFS (EBU R128), -14 LUFS (plateformes de streaming)"));

    ui.add_space(4.0);
    egui::Grid::new(ui.id().with("loudness_meter_grid"))
        .num_columns(2)
        .spacing(vec2(12.0, 2.0))
        .show(ui, |ui| {
            let dash = || "—".to_string();
            let (i, lra, tp, tp_db) = match report {
                Some(r) => (fmt_lufs(r.integrated), format!("{} LU", fmt_num(r.range_lu)), fmt_db(r.true_peak_db, "dBTP"), r.true_peak_db),
                None => (dash(), dash(), dash(), f32::NEG_INFINITY),
            };
            ui.colored_label(theme::TEXT_DIM(), tr("Intégrée"));
            ui.colored_label(theme::TEXT(), i);
            ui.end_row();
            ui.colored_label(theme::TEXT_DIM(), tr("Plage (LRA)"));
            ui.colored_label(theme::TEXT(), lra);
            ui.end_row();
            ui.colored_label(theme::TEXT_DIM(), tr("Crête vraie"));
            let c = if tp_db > -1.0 { Color32::from_rgb(235, 80, 70) } else { theme::TEXT() };
            ui.colored_label(c, tp);
            ui.end_row();
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp_l: f32, amp_r: f32, rate: f32, secs: f32) -> Vec<f32> {
        let n = (rate * secs) as usize;
        let mut v = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = (2.0 * std::f64::consts::PI * freq as f64 * i as f64 / rate as f64).sin() as f32;
            v.push(s * amp_l);
            v.push(s * amp_r);
        }
        v
    }

    #[test]
    fn stereo_sine_minus_20_dbfs() {
        // EBU Tech 3341 test 1 scaled: -20 dBFS sine on both channels → -20 LUFS.
        let a = 10f32.powf(-20.0 / 20.0);
        let s = sine(997.0, a, a, 48000.0, 10.0);
        let r = analyze(&s, 48000.0);
        assert!((r.integrated + 20.0).abs() < 0.1, "integrated {}", r.integrated);
        assert!(r.range_lu < 0.1, "lra {}", r.range_lu);
        assert!((r.max_momentary + 20.0).abs() < 0.1);
        assert!((r.max_short_term + 20.0).abs() < 0.1);
        let (m, st) = momentary_short(&s, 48000.0);
        assert!((m + 20.0).abs() < 0.1 && (st + 20.0).abs() < 0.1, "{m} {st}");
    }

    #[test]
    fn other_rate_44k1() {
        let a = 10f32.powf(-23.0 / 20.0);
        let s = sine(997.0, a, a, 44100.0, 6.0);
        let r = analyze(&s, 44100.0);
        assert!((r.integrated + 23.0).abs() < 0.1, "integrated {}", r.integrated);
    }

    #[test]
    fn single_channel_full_scale() {
        // BS.1770: 0 dBFS 1 kHz sine on one channel → -3.01 LKFS.
        let s = sine(997.0, 1.0, 0.0, 48000.0, 5.0);
        let r = analyze(&s, 48000.0);
        assert!((r.integrated + 3.01).abs() < 0.1, "integrated {}", r.integrated);
    }

    #[test]
    fn silence_is_neg_infinity() {
        let s = vec![0f32; 48000 * 2 * 4];
        let r = analyze(&s, 48000.0);
        assert_eq!(r.integrated, f32::NEG_INFINITY);
        assert_eq!(r.true_peak_db, f32::NEG_INFINITY);
        assert_eq!(r.max_momentary, f32::NEG_INFINITY);
        let (m, st) = momentary_short(&s, 48000.0);
        assert_eq!(m, f32::NEG_INFINITY);
        assert_eq!(st, f32::NEG_INFINITY);
        assert_eq!(analyze(&[], 48000.0).integrated, f32::NEG_INFINITY);
    }

    #[test]
    fn true_peak_half_amplitude() {
        let s = sine(997.0, 0.5, 0.5, 48000.0, 2.0);
        let r = analyze(&s, 48000.0);
        assert!((r.true_peak_db + 6.02).abs() < 0.05, "tp {}", r.true_peak_db);
    }

    #[test]
    fn true_peak_intersample() {
        // fs/4 sine sampled at 45° phase: samples peak at 0.707, true peak is 1.0.
        let rate = 48000.0f32;
        let n = 4800;
        let mut s = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin();
            s.push(v);
            s.push(v);
        }
        let r = analyze(&s, rate);
        assert!(r.true_peak_db > -0.3 && r.true_peak_db < 0.3, "tp {}", r.true_peak_db);
    }

    #[test]
    fn loudness_range_two_levels() {
        // 10 s at -20 then 10 s at -30 → LRA close to 10 LU.
        let rate = 48000.0;
        let a1 = 10f32.powf(-20.0 / 20.0);
        let a2 = 10f32.powf(-30.0 / 20.0);
        let mut s = sine(997.0, a1, a1, rate, 10.0);
        s.extend(sine(997.0, a2, a2, rate, 10.0));
        let r = analyze(&s, rate);
        assert!((r.range_lu - 10.0).abs() < 0.5, "lra {}", r.range_lu);
    }

    #[test]
    fn series_and_format() {
        let a = 10f32.powf(-20.0 / 20.0);
        let s = sine(997.0, a, a, 48000.0, 6.0);
        let ser = short_term_series(&s, 48000.0, 1.0);
        assert_eq!(ser.len(), 6);
        assert!((ser[5].1 + 20.0).abs() < 0.1, "{:?}", ser[5]);
        assert!(ser[0].1 < ser[5].1);
        assert_eq!(fmt_lufs(-23.0), "-23,0 LUFS");
        assert_eq!(fmt_lufs(f32::NEG_INFINITY), "-inf");
    }
}
