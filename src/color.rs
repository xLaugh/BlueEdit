//! Colour grading (wheels, curves, LUTs), chroma key, masks, stabilisation
//! and deflicker, plus the colour grading editor widget.
//!
//! All pixel work is on 8-bit RGBA straight-alpha layers, like `fx_pack`.

#![allow(dead_code)] // some helpers are wired into the UI separately

use crate::compositor::Layer;
use crate::i18n::tr;
use crate::model::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Applies the grading / keying / mask / correction effects. Returns false
/// for other effects (same contract as `fx_pack::apply`).
///
/// For `Stabilize`, `t` must be the LOCAL time inside the event (seconds from
/// the start of the clip's content), since the correction path is indexed
/// by source frame: `index = round(t * fps)`.
pub fn apply(l: &mut Layer, fx: &VideoFx, t: f64) -> bool {
    let ok = l.w > 0 && l.h > 0 && l.data.len() >= l.w as usize * l.h as usize * 4;
    match fx {
        VideoFx::ColorGrade(g) => {
            if ok {
                grade(l, g)
            }
        }
        VideoFx::Lut { path, amount } => {
            if ok && !path.trim().is_empty() && *amount > 0.0 {
                if let Ok(lut) = load_cube(path) {
                    lut.apply_layer(l, *amount);
                }
            }
        }
        VideoFx::ChromaKey { color, tolerance, softness, spill } => {
            if ok {
                chroma_key(l, *color, *tolerance, *softness, *spill)
            }
        }
        VideoFx::Mask { shape, x, y, w, h, feather, invert, points } => {
            if ok {
                mask(l, *shape, [*x, *y, *w, *h], *feather, *invert, points)
            }
        }
        VideoFx::Stabilize { amount, zoom, path, fps } => {
            if ok {
                stabilize(l, *amount, *zoom, path, *fps, t)
            }
        }
        VideoFx::Deflicker { amount } => {
            if ok {
                deflicker(l, *amount)
            }
        }
        _ => return false,
    }
    true
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x >= e0 { 1.0 } else { 0.0 };
    }
    let s = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

// ---------------------------------------------------------------- grading

/// Shape-preserving (Fritsch-Carlson monotone cubic) curve through points.
struct MonoCurve {
    xs: Vec<f32>,
    ys: Vec<f32>,
    ms: Vec<f32>,
}

impl MonoCurve {
    /// None when there are no points (identity). Missing ends get implicit
    /// (0, 0) / (1, 1) points.
    fn new(pts: &[[f32; 2]]) -> Option<MonoCurve> {
        let mut p: Vec<[f32; 2]> = pts
            .iter()
            .filter(|q| q[0].is_finite() && q[1].is_finite())
            .map(|q| [q[0].clamp(0.0, 1.0), q[1].clamp(0.0, 1.0)])
            .collect();
        if p.is_empty() {
            return None;
        }
        p.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut xs: Vec<f32> = Vec::with_capacity(p.len() + 2);
        let mut ys: Vec<f32> = Vec::with_capacity(p.len() + 2);
        if p[0][0] > 1e-4 {
            xs.push(0.0);
            ys.push(0.0);
        }
        for q in &p {
            if let Some(&lx) = xs.last() {
                if q[0] - lx < 1e-4 {
                    *ys.last_mut().unwrap() = q[1];
                    continue;
                }
            }
            xs.push(q[0]);
            ys.push(q[1]);
        }
        if *xs.last().unwrap() < 1.0 - 1e-4 {
            xs.push(1.0);
            ys.push(1.0);
        }
        let n = xs.len();
        if n < 2 {
            return None;
        }
        let d: Vec<f32> = (0..n - 1).map(|k| (ys[k + 1] - ys[k]) / (xs[k + 1] - xs[k])).collect();
        let mut ms = vec![0f32; n];
        ms[0] = d[0];
        ms[n - 1] = d[n - 2];
        for k in 1..n - 1 {
            ms[k] = if d[k - 1] * d[k] <= 0.0 { 0.0 } else { (d[k - 1] + d[k]) * 0.5 };
        }
        for k in 0..n - 1 {
            if d[k].abs() < 1e-9 {
                ms[k] = 0.0;
                ms[k + 1] = 0.0;
            } else {
                let a = ms[k] / d[k];
                let b = ms[k + 1] / d[k];
                let s = a * a + b * b;
                if s > 9.0 {
                    let t = 3.0 / s.sqrt();
                    ms[k] = t * a * d[k];
                    ms[k + 1] = t * b * d[k];
                }
            }
        }
        Some(MonoCurve { xs, ys, ms })
    }

    fn eval(&self, x: f32) -> f32 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        let k = (self.xs.partition_point(|&v| v <= x).max(1) - 1).min(n - 2);
        let h = self.xs[k + 1] - self.xs[k];
        let t = (x - self.xs[k]) / h;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        (h00 * self.ys[k] + h10 * h * self.ms[k] + h01 * self.ys[k + 1] + h11 * h * self.ms[k + 1]).clamp(0.0, 1.0)
    }
}

/// Value of the master curve at `x` (identity when `points` is empty).
pub fn curve_value(points: &[[f32; 2]], x: f32) -> f32 {
    MonoCurve::new(points).map_or(x, |c| c.eval(x))
}

fn curve_lut(points: &[[f32; 2]]) -> [u8; 256] {
    let mut lut = [0u8; 256];
    match MonoCurve::new(points) {
        Some(c) => {
            for (i, v) in lut.iter_mut().enumerate() {
                *v = to_u8(c.eval(i as f32 / 255.0));
            }
        }
        None => {
            for (i, v) in lut.iter_mut().enumerate() {
                *v = i as u8;
            }
        }
    }
    lut
}

/// Per-channel part of the grade: exposure, white balance, lift/gain/gamma,
/// offset and contrast (pivot 0.5). Input and output 0..1 (output unclamped).
fn grade_channel(v: f32, c: usize, g: &ColorGrade) -> f32 {
    let wb = [
        1.0 + 0.2 * g.temperature + 0.1 * g.tint,
        1.0 - 0.2 * g.tint,
        1.0 - 0.2 * g.temperature + 0.1 * g.tint,
    ];
    let mut v = v * 2f32.powf(g.exposure) * wb[c];
    // lift raises / lowers the blacks keeping white, gain scales,
    // gamma bends the mid-tones: out = (gain * (v + lift * (1 - v)))^(1/gamma)
    v += g.lift[c] * 0.5 * (1.0 - v);
    v *= 1.0 + g.gain[c];
    if v > 0.0 {
        v = v.powf(2f32.powf(-g.gamma[c]));
    }
    v += g.offset[c] * 0.5;
    (v - 0.5) * (1.0 + g.contrast) + 0.5
}

fn grade(l: &mut Layer, g: &ColorGrade) {
    if *g == ColorGrade::default() {
        return;
    }
    let mut pre = [[0f32; 256]; 3];
    for (c, table) in pre.iter_mut().enumerate() {
        for (i, v) in table.iter_mut().enumerate() {
            *v = grade_channel(i as f32 / 255.0, c, g);
        }
    }
    let curve = curve_lut(&g.curve);
    let sat = g.saturation.max(0.0);
    let do_sat = (sat - 1.0).abs() > 1e-4;
    for p in l.data.chunks_exact_mut(4) {
        let mut c = [pre[0][p[0] as usize], pre[1][p[1] as usize], pre[2][p[2] as usize]];
        if do_sat {
            let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            for v in &mut c {
                *v = y + (*v - y) * sat;
            }
        }
        for i in 0..3 {
            p[i] = curve[to_u8(c[i]) as usize];
        }
    }
}

// ---------------------------------------------------------------- LUTs

/// A parsed Adobe / Resolve `.cube` LUT (3D or 1D).
#[derive(Clone, Debug)]
pub struct Lut {
    pub title: String,
    /// Entries per axis.
    pub size: usize,
    pub is_3d: bool,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// 3D: size^3 entries, red varying fastest. 1D: size entries.
    pub data: Vec<[f32; 3]>,
}

impl Lut {
    /// Parses the text of a `.cube` file.
    pub fn parse(text: &str) -> Result<Lut, String> {
        let mut title = String::new();
        let (mut size3, mut size1) = (0usize, 0usize);
        let mut dmin = [0f32; 3];
        let mut dmax = [1f32; 3];
        let mut data: Vec<[f32; 3]> = Vec::new();
        for (no, raw) in text.lines().enumerate() {
            let line = raw.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            let key = it.next().unwrap_or("");
            let first = key.as_bytes()[0];
            let floats = |s: &str| -> Result<Vec<f32>, String> {
                s.split_whitespace()
                    .take_while(|t| !t.starts_with('#'))
                    .map(|t| t.parse::<f32>().map_err(|_| format!("ligne {} : nombre invalide « {t} »", no + 1)))
                    .collect()
            };
            if first.is_ascii_digit() || matches!(first, b'-' | b'+' | b'.') {
                let v = floats(line)?;
                if v.len() < 3 {
                    return Err(format!("ligne {} : trois valeurs attendues", no + 1));
                }
                data.push([v[0], v[1], v[2]]);
                continue;
            }
            let rest = line[key.len()..].trim();
            match key.to_ascii_uppercase().as_str() {
                "TITLE" => title = rest.trim_matches('"').to_string(),
                "LUT_3D_SIZE" => {
                    size3 = rest.parse().map_err(|_| format!("ligne {} : LUT_3D_SIZE invalide", no + 1))?
                }
                "LUT_1D_SIZE" => {
                    size1 = rest.parse().map_err(|_| format!("ligne {} : LUT_1D_SIZE invalide", no + 1))?
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let v = floats(rest)?;
                    if v.len() < 3 {
                        return Err(format!("ligne {} : {key} attend trois valeurs", no + 1));
                    }
                    let d = if key.eq_ignore_ascii_case("DOMAIN_MIN") { &mut dmin } else { &mut dmax };
                    d.copy_from_slice(&v[..3]);
                }
                "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {
                    let v = floats(rest)?;
                    if v.len() >= 2 {
                        dmin = [v[0]; 3];
                        dmax = [v[1]; 3];
                    }
                }
                _ => {} // unknown keywords are ignored
            }
        }
        let (is_3d, size) = match (size3, size1) {
            (0, 0) => return Err("taille de LUT absente (LUT_3D_SIZE / LUT_1D_SIZE)".into()),
            (n, 0) => (true, n),
            (0, n) => (false, n),
            _ => return Err("LUT 1D + 3D combinées non prises en charge".into()),
        };
        if size < 2 || (is_3d && size > 256) || size > 65536 {
            return Err(format!("taille de LUT invalide : {size}"));
        }
        let want = if is_3d { size * size * size } else { size };
        if data.len() != want {
            return Err(format!("{} valeurs trouvées, {want} attendues", data.len()));
        }
        Ok(Lut { title, size, is_3d, domain_min: dmin, domain_max: dmax, data })
    }

    #[inline]
    fn coord(&self, v: f32, c: usize) -> (usize, f32) {
        let span = self.domain_max[c] - self.domain_min[c];
        let u = if span.abs() > 1e-9 { (v - self.domain_min[c]) / span } else { v };
        let x = u.clamp(0.0, 1.0) * (self.size - 1) as f32;
        let i = (x.floor() as usize).min(self.size - 2);
        (i, x - i as f32)
    }

    /// Looks up one colour (0..1 in the LUT domain) with trilinear (3D) or
    /// linear (1D) interpolation.
    pub fn lookup(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        if !self.is_3d {
            let mut out = [0f32; 3];
            for c in 0..3 {
                let (i, f) = self.coord(rgb[c], c);
                out[c] = self.data[i][c] * (1.0 - f) + self.data[i + 1][c] * f;
            }
            return out;
        }
        let (r0, fr) = self.coord(rgb[0], 0);
        let (g0, fg) = self.coord(rgb[1], 1);
        let (b0, fb) = self.coord(rgb[2], 2);
        let at = |r: usize, g: usize, b: usize| self.data[r + g * n + b * n * n];
        let mut out = [0f32; 3];
        let c000 = at(r0, g0, b0);
        let c100 = at(r0 + 1, g0, b0);
        let c010 = at(r0, g0 + 1, b0);
        let c110 = at(r0 + 1, g0 + 1, b0);
        let c001 = at(r0, g0, b0 + 1);
        let c101 = at(r0 + 1, g0, b0 + 1);
        let c011 = at(r0, g0 + 1, b0 + 1);
        let c111 = at(r0 + 1, g0 + 1, b0 + 1);
        for c in 0..3 {
            let x00 = c000[c] + (c100[c] - c000[c]) * fr;
            let x10 = c010[c] + (c110[c] - c010[c]) * fr;
            let x01 = c001[c] + (c101[c] - c001[c]) * fr;
            let x11 = c011[c] + (c111[c] - c011[c]) * fr;
            let y0 = x00 + (x10 - x00) * fg;
            let y1 = x01 + (x11 - x01) * fg;
            out[c] = y0 + (y1 - y0) * fb;
        }
        out
    }

    fn apply_layer(&self, l: &mut Layer, amount: f32) {
        let k = amount.clamp(0.0, 1.0);
        if !self.is_3d {
            // 1D: one 256-entry table per channel
            let mut t = [[0u8; 256]; 3];
            for i in 0..256 {
                let v = i as f32 / 255.0;
                let o = self.lookup([v, v, v]);
                for c in 0..3 {
                    t[c][i] = to_u8(v + (o[c] - v) * k);
                }
            }
            for p in l.data.chunks_exact_mut(4) {
                for c in 0..3 {
                    p[c] = t[c][p[c] as usize];
                }
            }
            return;
        }
        for p in l.data.chunks_exact_mut(4) {
            let src = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
            let o = self.lookup(src);
            for c in 0..3 {
                p[c] = to_u8(src[c] + (o[c] - src[c]) * k);
            }
        }
    }
}

static LUT_CACHE: OnceLock<Mutex<HashMap<String, Arc<Lut>>>> = OnceLock::new();

/// Loads (and caches) a `.cube` LUT file.
pub fn load_cube(path: &str) -> Result<Arc<Lut>, String> {
    let cache = LUT_CACHE.get_or_init(Default::default);
    if let Some(l) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        return Ok(l.clone());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path} : {e}"))?;
    let lut = Arc::new(Lut::parse(&String::from_utf8_lossy(&bytes)).map_err(|e| format!("{path} : {e}"))?);
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_string(), lut.clone());
    Ok(lut)
}

// ---------------------------------------------------------------- keying

#[inline]
fn chroma(r: f32, g: f32, b: f32) -> (f32, f32) {
    // BT.601 Cb / Cr, inputs 0..1
    (-0.168736 * r - 0.331264 * g + 0.5 * b, 0.5 * r - 0.418688 * g - 0.081312 * b)
}

fn chroma_key(l: &mut Layer, color: [u8; 3], tolerance: f32, softness: f32, spill: f32) {
    let kc = [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0];
    let (kb, kr) = chroma(kc[0], kc[1], kc[2]);
    let dom = if kc[1] >= kc[0] && kc[1] >= kc[2] {
        1
    } else if kc[2] >= kc[0] {
        2
    } else {
        0
    };
    let (o1, o2) = ((dom + 1) % 3, (dom + 2) % 3);
    let tol = tolerance.max(0.0);
    let soft = softness.max(0.0);
    let spill = spill.clamp(0.0, 1.0);
    for p in l.data.chunks_exact_mut(4) {
        let (cb, cr) = chroma(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let d = ((cb - kb).powi(2) + (cr - kr).powi(2)).sqrt();
        let k = smoothstep(tol, tol + soft, d);
        if spill > 0.0 {
            let avg = (p[o1] as f32 + p[o2] as f32) * 0.5;
            let v = p[dom] as f32;
            if v > avg {
                p[dom] = (v - (v - avg) * spill).round() as u8;
            }
        }
        p[3] = (p[3] as f32 * k + 0.5) as u8;
    }
}

// ---------------------------------------------------------------- masks

#[inline]
fn seg_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let (wx, wy) = (px - a.0, py - a.1);
    let l2 = vx * vx + vy * vy;
    let t = if l2 > 1e-12 { ((wx * vx + wy * vy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (dx, dy) = (wx - t * vx, wy - t * vy);
    (dx * dx + dy * dy).sqrt()
}

fn mask(l: &mut Layer, shape: MaskShape, r: [f32; 4], feather: f32, invert: bool, points: &[[f32; 2]]) {
    let (w, h) = (l.w as usize, l.h as usize);
    let (wf, hf) = (w as f32, h as f32);
    // soft edge width in pixels, centred on the shape outline (min 1 px of anti-aliasing)
    let f = (feather.max(0.0) * wf.min(hf)).max(1.0);
    let cov = |sd: f32| smoothstep(0.0, 1.0, 0.5 - sd / f);
    let put = |data: &mut [u8], i: usize, m: f32| {
        let m = if invert { 1.0 - m } else { m };
        if m < 1.0 {
            data[i * 4 + 3] = (data[i * 4 + 3] as f32 * m + 0.5) as u8;
        }
    };
    match shape {
        MaskShape::Rectangle | MaskShape::Ellipse => {
            let (x0, x1) = ((r[0] * wf).min((r[0] + r[2]) * wf), (r[0] * wf).max((r[0] + r[2]) * wf));
            let (y0, y1) = ((r[1] * hf).min((r[1] + r[3]) * hf), (r[1] * hf).max((r[1] + r[3]) * hf));
            let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
            let (ax, ay) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5);
            for y in 0..h {
                let py = y as f32 + 0.5 - cy;
                for x in 0..w {
                    let px = x as f32 + 0.5 - cx;
                    let sd = if shape == MaskShape::Rectangle {
                        let (qx, qy) = (px.abs() - ax, py.abs() - ay);
                        let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                        out + qx.max(qy).min(0.0)
                    } else if ax < 1e-3 || ay < 1e-3 {
                        f32::MAX
                    } else {
                        // first-order distance to the ellipse: (k - 1) / |grad k|
                        let k = ((px / ax).powi(2) + (py / ay).powi(2)).sqrt();
                        if k < 1e-6 {
                            -ax.min(ay)
                        } else {
                            let g = ((px / (ax * ax)).powi(2) + (py / (ay * ay)).powi(2)).sqrt() / k;
                            (k - 1.0) / g.max(1e-9)
                        }
                    };
                    put(&mut l.data, y * w + x, cov(sd));
                }
            }
        }
        MaskShape::Polygon => {
            if points.len() < 3 {
                return;
            }
            let pts: Vec<(f32, f32)> = points.iter().map(|p| (p[0] * wf, p[1] * hf)).collect();
            let edges: Vec<((f32, f32), (f32, f32))> =
                (0..pts.len()).map(|i| (pts[i], pts[(i + 1) % pts.len()])).collect();
            let band = f * 0.5;
            let mut xs: Vec<f32> = Vec::new();
            let mut near: Vec<usize> = Vec::new();
            for y in 0..h {
                let py = y as f32 + 0.5;
                // even-odd crossings of this scanline
                xs.clear();
                for &(a, b) in &edges {
                    if (a.1 > py) != (b.1 > py) {
                        xs.push(a.0 + (py - a.1) * (b.0 - a.0) / (b.1 - a.1));
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                // edges close enough to matter for the soft edge
                near.clear();
                for (i, &(a, b)) in edges.iter().enumerate() {
                    if py >= a.1.min(b.1) - band && py <= a.1.max(b.1) + band {
                        near.push(i);
                    }
                }
                let mut ci = 0;
                let mut inside = false;
                for x in 0..w {
                    let px = x as f32 + 0.5;
                    while ci < xs.len() && xs[ci] <= px {
                        inside = !inside;
                        ci += 1;
                    }
                    let mut dmin = band;
                    for &i in &near {
                        let (a, b) = edges[i];
                        if px < a.0.min(b.0) - band || px > a.0.max(b.0) + band {
                            continue;
                        }
                        dmin = dmin.min(seg_dist(px, py, a, b));
                    }
                    let m = if dmin >= band {
                        if inside {
                            1.0
                        } else {
                            0.0
                        }
                    } else {
                        cov(if inside { -dmin } else { dmin })
                    };
                    put(&mut l.data, y * w + x, m);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- correction

fn bilinear(src: &[u8], w: usize, h: usize, x: f32, y: f32) -> [u8; 4] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let p = |xx: usize, yy: usize| src[(yy * w + xx) * 4 + c] as f32;
        let a = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * fx;
        let b = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * fx;
        out[c] = (a + (b - a) * fy + 0.5) as u8;
    }
    out
}

/// `t` = local event time (see [`apply`]). `path` values are in pixels of a
/// 160 px wide frame; the picture is moved by `-path[i] * amount` then scaled
/// by `zoom` around the centre to hide the borders.
fn stabilize(l: &mut Layer, amount: f32, zoom: f32, path: &[[f32; 2]], fps: f64, t: f64) {
    if path.is_empty() || !(fps > 0.0) || !t.is_finite() {
        return;
    }
    let i = ((t * fps).round().max(0.0) as usize).min(path.len() - 1);
    let (w, h) = (l.w as usize, l.h as usize);
    let s = w as f32 / 160.0;
    let (ox, oy) = (path[i][0] * amount * s, path[i][1] * amount * s);
    let z = if zoom > 0.01 { zoom } else { 1.0 };
    if ox.abs() < 1e-3 && oy.abs() < 1e-3 && (z - 1.0).abs() < 1e-4 {
        return;
    }
    let src = l.data.clone();
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
    for y in 0..h {
        let sy = cy + (y as f32 + 0.5 - cy) / z + oy - 0.5;
        for x in 0..w {
            let sx = cx + (x as f32 + 0.5 - cx) / z + ox - 0.5;
            let i = (y * w + x) * 4;
            l.data[i..i + 4].copy_from_slice(&bilinear(&src, w, h, sx, sy));
        }
    }
}

/// Vertical [0.25, 0.5, 0.25] blend mixed with the original by `amount`.
fn deflicker(l: &mut Layer, amount: f32) {
    let k = amount.clamp(0.0, 1.0);
    if k <= 0.0 {
        return;
    }
    let (w, h) = (l.w as usize, l.h as usize);
    let src = l.data.clone();
    let row = w * 4;
    for y in 0..h {
        let (ym, yp) = (y.saturating_sub(1), (y + 1).min(h - 1));
        for x in 0..w {
            for c in 0..3 {
                let i = y * row + x * 4 + c;
                let o = src[i] as f32;
                let v = (src[ym * row + x * 4 + c] as f32 + 2.0 * o + src[yp * row + x * 4 + c] as f32) * 0.25;
                l.data[i] = (o + (v - o) * k + 0.5) as u8;
            }
        }
    }
}

// ---------------------------------------------------------------- editor UI

/// Hue directions of R, G, B on the wheels (degrees, counter-clockwise from +x).
const HUE_DEG: [f32; 3] = [90.0, 210.0, 330.0];
/// Chroma amplitude reached at the rim of a wheel.
const WHEEL_SCALE: f32 = 0.5;

/// Splits RGB offsets into master (mean) and a 2D chroma vector.
fn wheel_decompose(v: &[f32; 3]) -> (f32, f32, f32) {
    let m = (v[0] + v[1] + v[2]) / 3.0;
    let (mut x, mut y) = (0.0, 0.0);
    for c in 0..3 {
        let a = HUE_DEG[c].to_radians();
        x += (v[c] - m) * a.cos();
        y += (v[c] - m) * a.sin();
    }
    (m, x * 2.0 / 3.0, y * 2.0 / 3.0)
}

fn wheel_compose(m: f32, x: f32, y: f32) -> [f32; 3] {
    let mut v = [0f32; 3];
    for c in 0..3 {
        let a = HUE_DEG[c].to_radians();
        v[c] = (m + x * a.cos() + y * a.sin()).clamp(-1.0, 1.0);
    }
    v
}

fn hue_color(angle: f32) -> egui::Color32 {
    let mut c = [0u8; 3];
    for i in 0..3 {
        let v = 0.5 + 0.5 * (angle - HUE_DEG[i].to_radians()).cos();
        c[i] = ((0.25 + 0.6 * v) * 255.0) as u8;
    }
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

fn wheel_ui(ui: &mut egui::Ui, label: &'static str, v: &mut [f32; 3]) -> bool {
    use egui::{vec2, Color32, Stroke};
    let mut changed = false;
    ui.vertical(|ui| {
        ui.label(tr(label));
        ui.horizontal(|ui| {
            let size = 96.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click_and_drag());
            let c = rect.center();
            let r = size * 0.5 - 3.0;
            let (m, mut x, mut y) = wheel_decompose(v);
            if resp.double_clicked() {
                *v = [m; 3];
                x = 0.0;
                y = 0.0;
                changed = true;
            } else if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let mut d = vec2(p.x - c.x, c.y - p.y) / r;
                    if d.length() > 1.0 {
                        d = d.normalized();
                    }
                    x = d.x * WHEEL_SCALE;
                    y = d.y * WHEEL_SCALE;
                    *v = wheel_compose(m, x, y);
                    changed = true;
                }
            }
            let painter = ui.painter();
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(c, Color32::from_gray(110));
            let n = 48u32;
            for i in 0..=n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                mesh.colored_vertex(c + vec2(a.cos(), -a.sin()) * r, hue_color(a));
            }
            for i in 0..n {
                mesh.add_triangle(0, 1 + i, 2 + i);
            }
            painter.add(egui::Shape::mesh(mesh));
            let line = Stroke::new(1.0_f32,Color32::from_black_alpha(70));
            painter.circle_stroke(c, r, Stroke::new(1.0_f32,Color32::from_gray(40)));
            painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], line);
            painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], line);
            let dot = c + vec2(x, -y) / WHEEL_SCALE * r;
            painter.circle_filled(dot, 4.5, Color32::WHITE);
            painter.circle_stroke(dot, 4.5, Stroke::new(1.0_f32,Color32::BLACK));
            // master: offsets all three channels
            let mut mm = m;
            ui.spacing_mut().slider_width = size;
            let s = ui.add(egui::Slider::new(&mut mm, -1.0..=1.0).vertical().show_value(false));
            if s.changed() {
                let (_, x, y) = wheel_decompose(v);
                *v = wheel_compose(mm, x, y);
                changed = true;
            }
        });
    });
    changed
}

fn curve_ui(ui: &mut egui::Ui, pts: &mut Vec<[f32; 2]>) -> bool {
    use egui::{pos2, vec2, Color32, Pos2, Stroke};
    let mut changed = false;
    let width = ui.available_width().clamp(160.0, 260.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, width * 0.7), egui::Sense::click_and_drag());
    let to_screen = |p: [f32; 2]| pos2(rect.left() + p[0] * rect.width(), rect.bottom() - p[1] * rect.height());
    let from_screen = |p: Pos2| {
        [
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0),
        ]
    };
    let nearest = |pts: &[[f32; 2]], p: Pos2| -> Option<usize> {
        pts.iter()
            .enumerate()
            .map(|(i, q)| (i, to_screen(*q).distance(p)))
            .filter(|(_, d)| *d < 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let insert = |pts: &mut Vec<[f32; 2]>, q: [f32; 2]| -> usize {
        if pts.is_empty() {
            pts.push([0.0, 0.0]);
            pts.push([1.0, 1.0]);
        }
        let i = pts.partition_point(|p| p[0] < q[0]);
        pts.insert(i, q);
        i
    };
    let drag_id = resp.id.with("curve_drag");
    if resp.secondary_clicked() || resp.double_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            if let Some(i) = nearest(pts, p) {
                pts.remove(i);
                changed = true;
            }
        }
    } else if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            if nearest(pts, p).is_none() {
                insert(pts, from_screen(p));
                changed = true;
            }
        }
    }
    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        if let Some(o) = origin {
            let i = match nearest(pts, o) {
                Some(i) => i,
                None => {
                    changed = true;
                    insert(pts, from_screen(o))
                }
            };
            ui.data_mut(|d| d.insert_temp(drag_id, i));
        }
    }
    if resp.dragged() {
        let idx = ui.data(|d| d.get_temp::<usize>(drag_id));
        if let (Some(i), Some(p)) = (idx, resp.interact_pointer_pos()) {
            if i < pts.len() {
                let q = from_screen(p);
                let lo = if i > 0 { pts[i - 1][0] + 0.005 } else { 0.0 };
                let hi = if i + 1 < pts.len() { pts[i + 1][0] - 0.005 } else { 1.0 };
                pts[i] = [q[0].clamp(lo.min(hi), hi.max(lo)), q[1]];
                changed = true;
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<usize>(drag_id));
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, Color32::from_gray(28));
    let grid = Stroke::new(1.0_f32,Color32::from_gray(50));
    for k in 1..4 {
        let f = k as f32 / 4.0;
        painter.line_segment([to_screen([f, 0.0]), to_screen([f, 1.0])], grid);
        painter.line_segment([to_screen([0.0, f]), to_screen([1.0, f])], grid);
    }
    painter.line_segment([to_screen([0.0, 0.0]), to_screen([1.0, 1.0])], Stroke::new(1.0_f32,Color32::from_gray(70)));
    let curve = MonoCurve::new(pts);
    let line: Vec<Pos2> = (0..=64)
        .map(|i| {
            let x = i as f32 / 64.0;
            to_screen([x, curve.as_ref().map_or(x, |c| c.eval(x))])
        })
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.5_f32,Color32::from_gray(230))));
    for q in pts.iter() {
        painter.circle_filled(to_screen(*q), 4.0, Color32::WHITE);
        painter.circle_stroke(to_screen(*q), 4.0, Stroke::new(1.0_f32,Color32::BLACK));
    }
    let border = Stroke::new(1.0_f32,Color32::from_gray(60));
    let (a, b, c, d) = (rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom());
    painter.add(egui::Shape::closed_line(vec![a, b, c, d], border));
    changed
}

/// Colour grading editor: lift / gamma / gain wheels, basic sliders, master
/// curve and reset. Returns true when `g` changed.
pub fn grade_ui(ui: &mut egui::Ui, g: &mut ColorGrade) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= wheel_ui(ui, "Ombres (lift)", &mut g.lift);
        changed |= wheel_ui(ui, "Tons moyens (gamma)", &mut g.gamma);
        changed |= wheel_ui(ui, "Hautes lumières (gain)", &mut g.gain);
    });
    ui.add_space(4.0);
    changed |= ui.add(egui::Slider::new(&mut g.exposure, -4.0..=4.0).text(tr("Exposition"))).changed();
    changed |= ui.add(egui::Slider::new(&mut g.contrast, -1.0..=1.0).text(tr("Contraste"))).changed();
    changed |= ui.add(egui::Slider::new(&mut g.saturation, 0.0..=2.0).text(tr("Saturation"))).changed();
    changed |= ui.add(egui::Slider::new(&mut g.temperature, -1.0..=1.0).text(tr("Température"))).changed();
    changed |= ui.add(egui::Slider::new(&mut g.tint, -1.0..=1.0).text(tr("Teinte"))).changed();
    ui.add_space(4.0);
    ui.label(tr("Courbe"));
    changed |= curve_ui(ui, &mut g.curve);
    if ui.button(tr("Réinitialiser")).clicked() {
        *g = ColorGrade::default();
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(px: &[[u8; 4]], w: u32) -> Layer {
        Layer { w, h: px.len() as u32 / w, data: px.iter().flatten().copied().collect() }
    }

    #[test]
    fn chroma_key_removes_green() {
        let mut l = layer(&[[0, 255, 0, 255], [200, 50, 60, 255]], 2);
        let fx = VideoFx::ChromaKey { color: [0, 255, 0], tolerance: 0.25, softness: 0.1, spill: 0.5 };
        assert!(apply(&mut l, &fx, 0.0));
        assert_eq!(l.data[3], 0);
        assert_eq!(l.data[7], 255);
        // default preset key colour also keys pure green
        let mut l = layer(&[[0, 255, 0, 255]], 1);
        apply(&mut l, &VideoFx::ChromaKey { color: [0, 200, 60], tolerance: 0.25, softness: 0.1, spill: 0.5 }, 0.0);
        assert_eq!(l.data[3], 0);
    }

    #[test]
    fn identity_cube_keeps_pixels() {
        let n = 5;
        let mut s = String::from("# identity\nTITLE \"id\"\nLUT_3D_SIZE 5\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let f = |v: usize| v as f32 / (n - 1) as f32;
                    s += &format!("{} {} {}\n", f(r), f(g), f(b));
                }
            }
        }
        let path = std::env::temp_dir().join(format!("blue_identity_{}.cube", std::process::id()));
        std::fs::write(&path, s).unwrap();
        let p = path.to_string_lossy().to_string();
        let px: Vec<[u8; 4]> = (0..64u32).map(|i| [(i * 4) as u8, (255 - i * 3) as u8, (i * 7 % 256) as u8, 200]).collect();
        let mut l = layer(&px, 8);
        let orig = l.data.clone();
        assert!(apply(&mut l, &VideoFx::Lut { path: p.clone(), amount: 1.0 }, 0.0));
        for (a, b) in l.data.iter().zip(&orig) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} vs {b}");
        }
        assert!(load_cube(&p).unwrap().is_3d);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cube_1d_parses() {
        let lut = Lut::parse("LUT_1D_SIZE 2\n0 0 0\n0.5 0.5 0.5\n").unwrap();
        let o = lut.lookup([1.0, 0.5, 0.0]);
        assert!((o[0] - 0.5).abs() < 1e-5 && (o[1] - 0.25).abs() < 1e-5 && o[2].abs() < 1e-5);
    }

    #[test]
    fn neutral_grade_is_identity() {
        let px: Vec<[u8; 4]> = (0..=255u32).map(|i| [i as u8, (255 - i) as u8, (i / 2) as u8, 255]).collect();
        let mut l = layer(&px, 16);
        let orig = l.data.clone();
        let g = ColorGrade { saturation: 1.0001, curve: vec![[0.0, 0.0], [1.0, 1.0]], ..ColorGrade::default() };
        apply(&mut l, &VideoFx::ColorGrade(g), 0.0);
        for (a, b) in l.data.iter().zip(&orig) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
    }

    #[test]
    fn curve_is_monotone() {
        let pts = [[0.0, 0.0], [0.3, 0.5], [0.6, 0.55], [1.0, 1.0]];
        let mut last = -1.0;
        for i in 0..=100 {
            let v = curve_value(&pts, i as f32 / 100.0);
            assert!(v >= last - 1e-6);
            last = v;
        }
        assert!((curve_value(&pts, 0.3) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn mask_rectangle_cuts_outside() {
        let mut l = layer(&vec![[255, 255, 255, 255]; 100], 10);
        let fx = VideoFx::Mask {
            shape: MaskShape::Rectangle,
            x: 0.2,
            y: 0.2,
            w: 0.6,
            h: 0.6,
            feather: 0.0,
            invert: false,
            points: vec![],
        };
        apply(&mut l, &fx, 0.0);
        assert_eq!(l.data[(5 * 10 + 5) * 4 + 3], 255);
        assert_eq!(l.data[3], 0);
        let mut l = layer(&vec![[255, 255, 255, 255]; 100], 10);
        let fx = VideoFx::Mask {
            shape: MaskShape::Polygon,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            feather: 0.0,
            invert: true,
            points: vec![[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]],
        };
        apply(&mut l, &fx, 0.0);
        assert_eq!(l.data[(5 * 10 + 5) * 4 + 3], 0);
        assert_eq!(l.data[3], 255);
    }
}
