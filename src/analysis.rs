//! Picture and sound analysis used by stabilisation, tracking and audio sync.
//! Pure functions on small luma frames / audio envelopes, no UI and no FFT:
//! coarse-to-fine block matching for global motion, normalised cross
//! correlation for template tracking and envelope cross-correlation for
//! syncing two recordings.
//!
//! Conventions:
//! - Luma frames are `w * h` bytes, row-major.
//! - A shift `(dx, dy)` between `prev` and `cur` means the picture content moved
//!   by `+dx, +dy` pixels: `cur(x, y) ~= prev(x - dx, y - dy)`.
//! - `color::apply` (Stabilize) expects correction paths in pixels of a frame
//!   160 px wide, so analyse frames scaled to 160 px width (or rescale).

#![allow(dead_code)] // wired into the UI separately

/// Luma (u8) of an RGBA frame (Rec.601 weights).
pub fn luma(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29 + 128) >> 8) as u8)
        .collect()
}

/// 2x2 box downsampling.
fn half(src: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        let r0 = &src[(2 * y) * w..];
        let r1 = &src[(2 * y + 1) * w..];
        for x in 0..nw {
            let s = r0[2 * x] as u32 + r0[2 * x + 1] as u32 + r1[2 * x] as u32 + r1[2 * x + 1] as u32;
            out.push(((s + 2) / 4) as u8);
        }
    }
    (out, nw, nh)
}

/// Mean absolute difference between `cur(x, y)` and `prev(x - dx, y - dy)`
/// over the inner region `[m, w - m) x [m, h - m)`. `m` must be >= |dx|, |dy|.
fn sad(prev: &[u8], cur: &[u8], w: usize, h: usize, dx: i32, dy: i32, m: usize) -> f32 {
    if 2 * m >= w || 2 * m >= h {
        return f32::MAX;
    }
    let mut s: u64 = 0;
    let (x0, x1) = (m, w - m);
    for y in m..h - m {
        let py = (y as i32 - dy) as usize;
        let a = &cur[y * w + x0..y * w + x1];
        let ps = (py * w) as i64 + x0 as i64 - dx as i64;
        let b = &prev[ps as usize..ps as usize + (x1 - x0)];
        s += a.iter().zip(b).map(|(&p, &q)| (p as i32 - q as i32).unsigned_abs()).sum::<u32>() as u64;
    }
    s as f32 / ((x1 - x0) * (h - 2 * m)) as f32
}

/// Global translation between two luma frames of size w x h (`cur` relative
/// to `prev`, see module conventions), searched within +-`max_shift` px with
/// a coarse-to-fine pyramid and parabolic sub-pixel refinement.
pub fn frame_shift(prev: &[u8], cur: &[u8], w: usize, h: usize, max_shift: i32) -> (f32, f32) {
    if w < 4 || h < 4 || prev.len() < w * h || cur.len() < w * h {
        return (0.0, 0.0);
    }
    let max_shift = max_shift.max(1);
    // pyramid: level 0 = full size
    let mut pyr: Vec<(Vec<u8>, Vec<u8>, usize, usize)> = vec![(prev[..w * h].to_vec(), cur[..w * h].to_vec(), w, h)];
    loop {
        let lv = pyr.len() - 1;
        let (p, c, lw, lh) = &pyr[lv];
        if *lw < 48 || *lh < 32 || (max_shift >> lv) <= 4 {
            break;
        }
        let (hp, nw, nh) = half(p, *lw, *lh);
        let (hc, _, _) = half(c, *lw, *lh);
        pyr.push((hp, hc, nw, nh));
    }
    let top = pyr.len() - 1;

    // exhaustive search at the coarsest level
    let (p, c, lw, lh) = &pyr[top];
    let lim = ((lw.min(lh) / 2) as i32 - 2).max(0);
    let r = ((max_shift >> top) + 1).min(lim);
    let mut best = (0i32, 0i32);
    let mut best_s = f32::MAX;
    for dy in -r..=r {
        for dx in -r..=r {
            let s = sad(p, c, *lw, *lh, dx, dy, r as usize);
            if s < best_s {
                best_s = s;
                best = (dx, dy);
            }
        }
    }
    // refine on each finer level
    for lv in (0..top).rev() {
        let (p, c, lw, lh) = &pyr[lv];
        let lim = (max_shift >> lv).max(1);
        let e = ((best.0 * 2).clamp(-lim, lim), (best.1 * 2).clamp(-lim, lim));
        let m = (e.0.abs().max(e.1.abs()) + 2) as usize;
        let mut bs = f32::MAX;
        let mut b = e;
        for dy in -2..=2 {
            for dx in -2..=2 {
                let s = sad(p, c, *lw, *lh, e.0 + dx, e.1 + dy, m);
                if s < bs {
                    bs = s;
                    b = (e.0 + dx, e.1 + dy);
                }
            }
        }
        if bs < f32::MAX {
            best = b;
        }
    }
    // sub-pixel refinement (parabola through the SAD of the neighbours)
    let (p, c, _, _) = &pyr[0];
    let m = (best.0.abs().max(best.1.abs()) + 1) as usize;
    let s0 = sad(p, c, w, h, best.0, best.1, m);
    if s0 == f32::MAX {
        return (best.0 as f32, best.1 as f32);
    }
    let para = |sm: f32, sp: f32| -> f32 {
        let den = sm - 2.0 * s0 + sp;
        if den > 1e-6 && sm < f32::MAX && sp < f32::MAX {
            (0.5 * (sm - sp) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    };
    let fx = para(sad(p, c, w, h, best.0 - 1, best.1, m), sad(p, c, w, h, best.0 + 1, best.1, m));
    let fy = para(sad(p, c, w, h, best.0, best.1 - 1, m), sad(p, c, w, h, best.0, best.1 + 1, m));
    (best.0 as f32 + fx, best.1 as f32 + fy)
}

/// From per-frame shifts (`shifts[i]` = motion between frame i-1 and i, the
/// first one usually (0, 0)) compute the stabilisation correction path: the
/// cumulative camera trajectory minus its centred moving average over
/// `smooth` frames (e.g. 15). The ends are padded by point reflection so a
/// steady pan yields no correction. Output has the same length as `shifts`;
/// translating frame i by `-path[i]` stabilises it.
pub fn stabilize_path(shifts: &[(f32, f32)], smooth: usize) -> Vec<[f32; 2]> {
    let n = shifts.len();
    if n == 0 {
        return vec![];
    }
    let mut traj = Vec::with_capacity(n);
    let (mut ax, mut ay) = (0f64, 0f64);
    for &(dx, dy) in shifts {
        ax += dx as f64;
        ay += dy as f64;
        traj.push([ax, ay]);
    }
    let half = (smooth / 2) as isize;
    let last = n as isize - 1;
    let at = |j: isize| -> [f64; 2] {
        if j < 0 {
            let r = (-j).min(last) as usize;
            [2.0 * traj[0][0] - traj[r][0], 2.0 * traj[0][1] - traj[r][1]]
        } else if j > last {
            let r = (2 * last - j).max(0) as usize;
            let l = last as usize;
            [2.0 * traj[l][0] - traj[r][0], 2.0 * traj[l][1] - traj[r][1]]
        } else {
            traj[j as usize]
        }
    };
    (0..n)
        .map(|i| {
            let (mut sx, mut sy) = (0f64, 0f64);
            for j in i as isize - half..=i as isize + half {
                let p = at(j);
                sx += p[0];
                sy += p[1];
            }
            let k = (2 * half + 1) as f64;
            [(traj[i][0] - sx / k) as f32, (traj[i][1] - sy / k) as f32]
        })
        .collect()
}

/// Template tracking: find the top-left position of `tpl` (tw x th luma)
/// inside `frame` (w x h), searching +-`radius` px around `from`, by
/// normalised cross-correlation (falls back to SAD on flat areas).
/// Returns (x, y, score 0..1), 1 = perfect match.
pub fn track_template(
    frame: &[u8],
    w: usize,
    h: usize,
    tpl: &[u8],
    tw: usize,
    th: usize,
    from: (i32, i32),
    radius: i32,
) -> (i32, i32, f32) {
    if tw == 0 || th == 0 || tw > w || th > h || tpl.len() < tw * th || frame.len() < w * h {
        return (from.0, from.1, 0.0);
    }
    let n = (tw * th) as f64;
    let (mut st, mut stt) = (0f64, 0f64);
    for &v in &tpl[..tw * th] {
        st += v as f64;
        stt += (v as f64) * (v as f64);
    }
    let tvar = stt - st * st / n;
    let r = radius.max(0);
    let (xmax, ymax) = ((w - tw) as i32, (h - th) as i32);
    let (x0, x1) = ((from.0 - r).clamp(0, xmax), (from.0 + r).clamp(0, xmax));
    let (y0, y1) = ((from.1 - r).clamp(0, ymax), (from.1 + r).clamp(0, ymax));
    let mut best = (from.0.clamp(0, xmax), from.1.clamp(0, ymax), -1.0f32);
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            let (mut sf, mut sff, mut sft, mut sabs) = (0u64, 0u64, 0u64, 0u64);
            for ty in 0..th {
                let fr = &frame[(cy as usize + ty) * w + cx as usize..][..tw];
                let trow = &tpl[ty * tw..][..tw];
                for (&f, &t) in fr.iter().zip(trow) {
                    let (f, t) = (f as u64, t as u64);
                    sf += f;
                    sff += f * f;
                    sft += f * t;
                    sabs += f.abs_diff(t);
                }
            }
            let fvar = sff as f64 - (sf as f64) * (sf as f64) / n;
            let score = if tvar > 1e-3 * n && fvar > 1e-3 * n {
                let cov = sft as f64 - (sf as f64) * st / n;
                (cov / (fvar * tvar).sqrt()).max(0.0) as f32
            } else {
                1.0 - (sabs as f64 / n / 255.0) as f32
            };
            let better = score > best.2 + 1e-6
                || ((score - best.2).abs() <= 1e-6
                    && (cx - from.0).abs() + (cy - from.1).abs() < (best.0 - from.0).abs() + (best.1 - from.1).abs());
            if better {
                best = (cx, cy, score);
            }
        }
    }
    (best.0, best.1, best.2.clamp(0.0, 1.0))
}

fn centered(v: &[f32]) -> Vec<f32> {
    let m = v.iter().map(|&x| x as f64).sum::<f64>() / v.len().max(1) as f64;
    v.iter().map(|&x| x - m as f32).collect()
}

fn decimate(v: &[f32], f: usize) -> Vec<f32> {
    v.chunks(f).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect()
}

/// Normalised correlation of `a[i + lag]` with `b[i]` over their overlap.
fn corr_at(a: &[f32], b: &[f32], lag: i64, min_overlap: usize) -> Option<f64> {
    let lo = 0i64.max(-lag);
    let hi = (b.len() as i64).min(a.len() as i64 - lag);
    if hi - lo < min_overlap.max(1) as i64 {
        return None;
    }
    let (mut s, mut ea, mut eb) = (0f64, 0f64, 0f64);
    let aa = &a[(lo + lag) as usize..(hi + lag) as usize];
    let bb = &b[lo as usize..hi as usize];
    for (&x, &y) in aa.iter().zip(bb) {
        s += (x * y) as f64;
        ea += (x * x) as f64;
        eb += (y * y) as f64;
    }
    Some(s / (ea * eb).sqrt().max(1e-12))
}

fn best_lag(a: &[f32], b: &[f32], lo: i64, hi: i64, min_overlap: usize) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;
    for lag in lo..=hi {
        if let Some(c) = corr_at(a, b, lag, min_overlap) {
            if best.map_or(true, |(_, bc)| c > bc) {
                best = Some((lag, c));
            }
        }
    }
    best
}

/// Offset in seconds of audio `b` relative to audio `a` (mono envelopes sampled
/// at `rate` Hz, e.g. 100 Hz from [`envelope_i16`]), maximising the
/// cross-correlation within +-`max_secs`.
///
/// Sign: if the same sound happens at time `ta` in `a` and `tb` in `b`, the
/// result is `ta - tb`. Positive = `b` started recording later than `a`; to
/// sync, place `b` so that it starts `offset` seconds after `a` starts
/// (`b.start = a.start + offset`). Returns 0 when nothing can be compared.
pub fn audio_offset(a: &[f32], b: &[f32], rate: f32, max_secs: f32) -> f64 {
    if a.len() < 4 || b.len() < 4 || !(rate > 0.0) {
        return 0.0;
    }
    let a = centered(a);
    let b = centered(b);
    let total = (a.len() + b.len()) as i64;
    let max_lag = ((max_secs.max(0.0) * rate).round() as i64).min(total);
    let shortest = a.len().min(b.len());
    let min_overlap = (shortest / 4).max(8).min(shortest);
    let found = if shortest > 2048 && max_lag > 64 {
        const F: usize = 4;
        let ad = decimate(&a, F);
        let bd = decimate(&b, F);
        let fl = F as i64;
        match best_lag(&ad, &bd, -max_lag / fl - 1, max_lag / fl + 1, min_overlap / F) {
            Some((cl, _)) => {
                best_lag(&a, &b, (cl * fl - 2 * fl).max(-max_lag), (cl * fl + 2 * fl).min(max_lag), min_overlap)
            }
            None => None,
        }
    } else {
        best_lag(&a, &b, -max_lag, max_lag, min_overlap)
    };
    let Some((lag, c0)) = found else {
        return 0.0;
    };
    // parabolic refinement around the peak
    let mut frac = 0.0;
    if let (Some(cm), Some(cp)) = (corr_at(&a, &b, lag - 1, min_overlap), corr_at(&a, &b, lag + 1, min_overlap)) {
        let den = cm - 2.0 * c0 + cp;
        if den < -1e-12 {
            frac = (0.5 * (cm - cp) / den).clamp(-0.5, 0.5);
        }
    }
    (lag as f64 + frac) / rate as f64
}

/// Downsampled absolute envelope of interleaved stereo i16 samples: mean of
/// |L| and |R| (0..1) over each block of `hop` frames.
pub fn envelope_i16(samples: &[i16], hop: usize) -> Vec<f32> {
    let hop = hop.max(1);
    let frames = samples.len() / 2;
    samples[..frames * 2]
        .chunks(hop * 2)
        .map(|c| {
            let s: u64 = c.iter().map(|&v| (v as i32).unsigned_abs() as u64).sum();
            s as f32 / c.len() as f32 / 32768.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(mut x: u64) -> u64 {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
        x ^ (x >> 33)
    }

    fn lattice(ix: i64, iy: i64) -> f32 {
        (hash((ix as u64).wrapping_mul(73856093) ^ (iy as u64).wrapping_mul(19349663)) % 1000) as f32 / 1000.0
    }

    /// Aperiodic smooth texture (value noise, two octaves).
    fn tex(x: f32, y: f32) -> u8 {
        let noise = |x: f32, y: f32, s: f32| {
            let (fx, fy) = (x / s, y / s);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - ix, fy - iy);
            let (ix, iy) = (ix as i64, iy as i64);
            let a = lattice(ix, iy) * (1.0 - tx) + lattice(ix + 1, iy) * tx;
            let b = lattice(ix, iy + 1) * (1.0 - tx) + lattice(ix + 1, iy + 1) * tx;
            a * (1.0 - ty) + b * ty
        };
        (noise(x, y, 9.0) * 170.0 + noise(x + 500.0, y, 3.0) * 80.0).clamp(0.0, 255.0) as u8
    }

    #[test]
    fn shift_is_recovered() {
        let (w, h) = (160usize, 90usize);
        for &(dx, dy) in &[(5i32, -3i32), (-11, 7), (0, 0), (14, 2)] {
            let prev: Vec<u8> = (0..w * h).map(|i| tex((i % w) as f32, (i / w) as f32)).collect();
            let cur: Vec<u8> =
                (0..w * h).map(|i| tex((i % w) as f32 - dx as f32, (i / w) as f32 - dy as f32)).collect();
            let (ex, ey) = frame_shift(&prev, &cur, w, h, 16);
            assert!((ex - dx as f32).abs() < 0.5 && (ey - dy as f32).abs() < 0.5, "want {dx},{dy} got {ex},{ey}");
        }
    }

    #[test]
    fn luma_of_white_and_black() {
        assert_eq!(luma(&[255, 255, 255, 255, 0, 0, 0, 255]), vec![255, 0]);
    }

    #[test]
    fn constant_motion_needs_no_correction() {
        let shifts = vec![(2.0f32, -1.0f32); 60];
        for p in stabilize_path(&shifts, 15) {
            assert!(p[0].abs() < 1e-3 && p[1].abs() < 1e-3, "{p:?}");
        }
    }

    #[test]
    fn jitter_is_corrected() {
        let shifts: Vec<(f32, f32)> = (0..40).map(|i| if i == 20 { (6.0, 0.0) } else { (0.0, 0.0) }).collect();
        let p = stabilize_path(&shifts, 15);
        // just before the jump the picture is pulled forward, just after it back
        assert!(p[18][0] < -1.0 && p[22][0] > 1.0, "{:?} {:?}", p[18], p[22]);
        // far from the jump, nothing to correct
        assert!(p[5][0].abs() < 1e-4 && p[35][0].abs() < 1e-4);
    }

    #[test]
    fn template_is_found() {
        let (w, h) = (120usize, 80usize);
        let frame: Vec<u8> = (0..w * h).map(|i| tex((i % w) as f32, (i / w) as f32)).collect();
        let (tw, th) = (16usize, 16usize);
        let tpl: Vec<u8> = (0..tw * th).map(|i| frame[(30 + i / tw) * w + 50 + i % tw]).collect();
        let (x, y, s) = track_template(&frame, w, h, &tpl, tw, th, (44, 25), 12);
        assert_eq!((x, y), (50, 30));
        assert!(s > 0.99);
    }

    #[test]
    fn audio_delay_is_recovered() {
        // bursty envelope at 100 Hz
        let rate = 100.0;
        let a: Vec<f32> = (0..3000u64)
            .map(|i| {
                let burst = if hash(i / 7) % 5 == 0 { 0.8 } else { 0.05 };
                burst + (hash(i + 99) % 100) as f32 / 2000.0
            })
            .collect();
        // b started recording 1.5 s after a: b[i] = a[i + 150]
        let b: Vec<f32> = a[150..].iter().enumerate().map(|(i, &v)| v + (hash(i as u64 + 7) % 100) as f32 / 4000.0).collect();
        let off = audio_offset(&a, &b, rate, 5.0);
        assert!((off - 1.5).abs() < 0.02, "got {off}");
        let off2 = audio_offset(&b, &a, rate, 5.0);
        assert!((off2 + 1.5).abs() < 0.02, "got {off2}");
    }

    #[test]
    fn envelope_blocks() {
        let s = [16384i16, -16384, 0, 0, 32767, 32767];
        let e = envelope_i16(&s, 2);
        assert_eq!(e.len(), 2);
        assert!((e[0] - 0.25).abs() < 1e-3 && (e[1] - 1.0).abs() < 1e-3);
    }
}
