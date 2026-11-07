//! The built-in "Univers" effect pack: glow, RGB split, glitch, VHS, grain,
//! camera shake, duotone, mosaic and modern noir. Written from scratch in the
//! spirit of the Red Giant Universe OFX plugins installed on this machine.
//! Animated effects use the time `t` so they move from frame to frame.

use crate::compositor::Layer;
use crate::model::VideoFx;

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^ (x >> 33)
}

fn rnd(seed: u64) -> f32 {
    (hash(seed) % 10_000) as f32 / 10_000.0
}

fn luma(p: &[u8]) -> f32 {
    (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0
}

/// Applies a pack effect. Returns false if `fx` is not part of the pack.
pub fn apply(l: &mut Layer, fx: &VideoFx, t: f64) -> bool {
    let frame = (t * 30.0).floor() as u64;
    let (w, h) = (l.w as usize, l.h as usize);
    let scale = l.w as f32 / 960.0;
    match *fx {
        VideoFx::Glow { radius, intensity, threshold } => {
            // bright-pass, blur, add back
            let mut bright = Layer { w: l.w, h: l.h, data: l.data.clone() };
            for p in bright.data.chunks_exact_mut(4) {
                let k = ((luma(p) - threshold) / (1.0 - threshold).max(0.01)).clamp(0.0, 1.0);
                for c in 0..3 {
                    p[c] = (p[c] as f32 * k) as u8;
                }
            }
            let r = ((radius * scale).round() as i32).max(1);
            crate::compositor::blur_layer(&mut bright, r);
            crate::compositor::blur_layer(&mut bright, r);
            for (d, s) in l.data.chunks_exact_mut(4).zip(bright.data.chunks_exact(4)) {
                for c in 0..3 {
                    d[c] = (d[c] as f32 + s[c] as f32 * intensity * 1.5).min(255.0) as u8;
                }
            }
        }
        VideoFx::RgbSplit { amount } => {
            let off = (amount * 12.0 * scale).round() as i64;
            let src = l.data.clone();
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    let xr = (x as i64 - off).clamp(0, w as i64 - 1) as usize;
                    let xb = (x as i64 + off).clamp(0, w as i64 - 1) as usize;
                    l.data[i] = src[(y * w + xr) * 4];
                    l.data[i + 2] = src[(y * w + xb) * 4 + 2];
                }
            }
        }
        VideoFx::Glitch { amount } => {
            // horizontal slices shifted at random, changing a few times per second
            let step = frame / 3;
            let src = l.data.clone();
            let bands = 6 + (rnd(step) * 10.0) as u64;
            for b in 0..bands {
                if rnd(step * 131 + b) > amount {
                    continue;
                }
                let y0 = (rnd(step * 7 + b * 13) * h as f32) as usize;
                let bh = ((rnd(step * 3 + b * 5) * 0.08 + 0.01) * h as f32) as usize;
                let shift = ((rnd(step * 11 + b) - 0.5) * 0.25 * w as f32 * amount) as i64;
                let tint = (rnd(step + b * 17) * 3.0) as usize;
                for y in y0..(y0 + bh).min(h) {
                    for x in 0..w {
                        let sx = (x as i64 - shift).rem_euclid(w as i64) as usize;
                        let si = (y * w + sx) * 4;
                        let di = (y * w + x) * 4;
                        l.data[di..di + 4].copy_from_slice(&src[si..si + 4]);
                        l.data[di + tint] = l.data[di + tint].saturating_add(60);
                    }
                }
            }
        }
        VideoFx::Vhs { amount } => {
            let src = l.data.clone();
            let off = (amount * 4.0 * scale).round().max(1.0) as i64;
            for y in 0..h {
                // slow wobble + tracking noise near the bottom
                let wob = ((y as f64 * 0.05 + t * 3.0).sin() * amount as f64 * 2.0 * scale as f64) as i64;
                let track = if (y as f32) > h as f32 * 0.92 { (rnd(frame * 977 + y as u64) * 30.0 * scale) as i64 } else { 0 };
                let scan = if y % 2 == 0 { 1.0 - 0.12 * amount } else { 1.0 };
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    let sx = |d: i64| (x as i64 + wob + track + d).clamp(0, w as i64 - 1) as usize;
                    let r = src[(y * w + sx(-off)) * 4] as f32;
                    let g = src[(y * w + sx(0)) * 4 + 1] as f32;
                    let b = src[(y * w + sx(off)) * 4 + 2] as f32;
                    let n = (rnd(frame * 31 + (y * w + x) as u64) - 0.5) * 30.0 * amount;
                    // warm, slightly washed colours
                    l.data[i] = ((r * 1.05 + 10.0 + n) * scan).clamp(0.0, 255.0) as u8;
                    l.data[i + 1] = ((g * 0.97 + 6.0 + n) * scan).clamp(0.0, 255.0) as u8;
                    l.data[i + 2] = ((b * 0.85 + n) * scan).clamp(0.0, 255.0) as u8;
                }
            }
        }
        VideoFx::Grain { amount } => {
            for (k, p) in l.data.chunks_exact_mut(4).enumerate() {
                let n = (rnd(frame * 7919 + k as u64) - 0.5) * 90.0 * amount;
                for c in 0..3 {
                    p[c] = (p[c] as f32 + n).clamp(0.0, 255.0) as u8;
                }
            }
        }
        VideoFx::Shake { amount } => {
            let tt = t * 9.0;
            let dx = (((tt).sin() * 0.6 + (tt * 2.3).sin() * 0.4) * amount as f64 * 0.04 * w as f64) as i64;
            let dy = (((tt * 1.7).cos() * 0.6 + (tt * 3.1).sin() * 0.4) * amount as f64 * 0.04 * h as f64) as i64;
            let src = l.data.clone();
            // zoom in a little so the edges never show
            let z = 1.0 + amount as f64 * 0.1;
            for y in 0..h {
                for x in 0..w {
                    let sx = (((x as f64 - w as f64 / 2.0) / z + w as f64 / 2.0) as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let sy = (((y as f64 - h as f64 / 2.0) / z + h as f64 / 2.0) as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    let di = (y * w + x) * 4;
                    let si = (sy * w + sx) * 4;
                    l.data[di..di + 4].copy_from_slice(&src[si..si + 4]);
                }
            }
        }
        VideoFx::Duotone { dark, light } => {
            for p in l.data.chunks_exact_mut(4) {
                let k = luma(p);
                for c in 0..3 {
                    p[c] = (dark[c] as f32 * (1.0 - k) + light[c] as f32 * k) as u8;
                }
            }
        }
        VideoFx::Pixelate { size } => {
            let s = ((size * scale).round() as usize).max(2);
            for by in (0..h).step_by(s) {
                for bx in (0..w).step_by(s) {
                    let ci = ((by + s / 2).min(h - 1) * w + (bx + s / 2).min(w - 1)) * 4;
                    let c = [l.data[ci], l.data[ci + 1], l.data[ci + 2], l.data[ci + 3]];
                    for y in by..(by + s).min(h) {
                        for x in bx..(bx + s).min(w) {
                            let i = (y * w + x) * 4;
                            l.data[i..i + 4].copy_from_slice(&c);
                        }
                    }
                }
            }
        }
        VideoFx::Noir { contrast } => {
            for p in l.data.chunks_exact_mut(4) {
                let y = luma(p);
                // S-curve on luminance
                let c = 1.0 + contrast * 2.0;
                let v = (1.0 / (1.0 + (-(y - 0.5) * 6.0 * c).exp())).clamp(0.0, 1.0);
                let g = (v * 255.0) as u8;
                p[0] = g;
                p[1] = g;
                p[2] = (g as f32 * 0.96 + 6.0).min(255.0) as u8;
            }
        }
        _ => return false,
    }
    true
}
