//! Video scopes: vectorscope, histogram, waveform (luma) and RGB parade.
//!
//! `analyze` does the costly accumulation on a subsampled frame and returns a
//! `ScopeData` the caller caches; `draw` turns it into an intensity texture
//! (one stable texture per scope kind, updated in place) plus a graticule.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use egui::{pos2, vec2, Align2, Color32, ColorImage, FontId, Id, Pos2, Rect, Stroke, StrokeKind, TextureHandle, TextureId, TextureOptions};

use crate::i18n::tr;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    Vectorscope,
    Histogram,
    Waveform,
    Parade,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] = [ScopeKind::Vectorscope, ScopeKind::Histogram, ScopeKind::Waveform, ScopeKind::Parade];

    pub fn label(self) -> &'static str {
        match self {
            ScopeKind::Vectorscope => tr("Vectorscope"),
            ScopeKind::Histogram => tr("Histogramme"),
            ScopeKind::Waveform => tr("Forme d'onde"),
            ScopeKind::Parade => tr("Parade RVB"),
        }
    }

    fn tex_name(self) -> &'static str {
        match self {
            ScopeKind::Vectorscope => "scope_vectorscope",
            ScopeKind::Histogram => "scope_histogram",
            ScopeKind::Waveform => "scope_waveform",
            ScopeKind::Parade => "scope_parade",
        }
    }
}

/// Number of code values per channel (8-bit).
const LEVELS: usize = 256;
/// Vectorscope accumulation grid size (square).
const VEC_SIZE: usize = 256;
/// Horizontal resolution of the luma waveform.
const WAVE_COLS: usize = 384;
/// Horizontal resolution of each parade channel.
const PARADE_COLS: usize = 256;
/// Histogram texture height.
const HIST_H: usize = 128;
/// Approximate number of pixels sampled per frame.
const TARGET_SAMPLES: usize = 200_000;

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Analysis result cached by the caller (computing is the costly part).
#[derive(Clone)]
pub struct ScopeData {
    kind: ScopeKind,
    /// Unique id of this analysis, so `draw` re-uploads the texture only when
    /// the data actually changed.
    generation: u64,
    /// Grid width / height of each accumulation layer.
    gw: usize,
    gh: usize,
    /// Vectorscope / waveform: 1 layer; parade: 3 layers (R, G, B).
    layers: Vec<Vec<u32>>,
    /// Histogram: R, G, B, luma.
    hist: Vec<[u32; LEVELS]>,
    /// Number of pixels that were sampled.
    samples: usize,
}

impl ScopeData {
    /// Empty analysis (draws only the graticule).
    pub fn empty(kind: ScopeKind) -> Self {
        let (gw, gh, nl) = match kind {
            ScopeKind::Vectorscope => (VEC_SIZE, VEC_SIZE, 1),
            ScopeKind::Histogram => (LEVELS, HIST_H, 0),
            ScopeKind::Waveform => (WAVE_COLS, LEVELS, 1),
            ScopeKind::Parade => (PARADE_COLS, LEVELS, 3),
        };
        ScopeData {
            kind,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            gw,
            gh,
            layers: vec![vec![0u32; gw * gh]; nl],
            hist: if kind == ScopeKind::Histogram { vec![[0u32; LEVELS]; 4] } else { Vec::new() },
            samples: 0,
        }
    }

    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    /// Number of pixels accumulated.
    pub fn samples(&self) -> usize {
        self.samples
    }
}

/// Rec.709 luma of 8-bit RGB as an 8-bit code value (weights sum to 256).
#[inline]
fn luma8(r: u8, g: u8, b: u8) -> usize {
    ((54 * r as u32 + 183 * g as u32 + 19 * b as u32) >> 8) as usize
}

/// Rec.709 Cb/Cr (each in -0.5..0.5) of normalised RGB.
#[inline]
fn cbcr(r: f32, g: f32, b: f32) -> (f32, f32) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    ((b - y) / 1.8556, (r - y) / 1.5748)
}

/// Compute from an RGBA image (w×h, straight alpha); pixels are subsampled so
/// it runs in a few ms for 1920×1080. Fully transparent pixels are ignored.
pub fn analyze(rgba: &[u8], w: usize, h: usize, kind: ScopeKind) -> ScopeData {
    let mut d = ScopeData::empty(kind);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return d;
    }
    let step = (((w * h) as f64 / TARGET_SAMPLES as f64).sqrt().ceil() as usize).max(1);
    let gw = d.gw;
    let mut n = 0usize;

    for y in (0..h).step_by(step) {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        for x in (0..w).step_by(step) {
            let p = &row[x * 4..x * 4 + 4];
            if p[3] == 0 {
                continue;
            }
            let (r, g, b) = (p[0], p[1], p[2]);
            n += 1;
            match kind {
                ScopeKind::Vectorscope => {
                    let (cb, cr) = cbcr(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
                    let s = (VEC_SIZE - 1) as f32;
                    let gx = ((cb + 0.5) * s).round().clamp(0.0, s) as usize;
                    let gy = ((0.5 - cr) * s).round().clamp(0.0, s) as usize;
                    d.layers[0][gy * VEC_SIZE + gx] += 1;
                }
                ScopeKind::Histogram => {
                    d.hist[0][r as usize] += 1;
                    d.hist[1][g as usize] += 1;
                    d.hist[2][b as usize] += 1;
                    d.hist[3][luma8(r, g, b)] += 1;
                }
                ScopeKind::Waveform => {
                    let col = x * gw / w;
                    let rowi = LEVELS - 1 - luma8(r, g, b);
                    d.layers[0][rowi * gw + col] += 1;
                }
                ScopeKind::Parade => {
                    let col = x * gw / w;
                    for (c, v) in [r, g, b].into_iter().enumerate() {
                        let rowi = LEVELS - 1 - v as usize;
                        d.layers[c][rowi * gw + col] += 1;
                    }
                }
            }
        }
    }
    d.samples = n;
    d
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn scope_bg() -> Color32 {
    Color32::from_gray(12)
}

fn grat() -> Color32 {
    Color32::from_rgba_unmultiplied(200, 200, 200, 60)
}

fn grat_text() -> Color32 {
    Color32::from_rgba_unmultiplied(210, 210, 210, 150)
}

/// Upload (or update in place) the texture of `kind`, re-using the handle kept
/// in egui temp storage. Skips the upload when `generation` didn't change.
fn upload(ctx: &egui::Context, kind: ScopeKind, generation: u64, make: impl FnOnce() -> ColorImage) -> TextureId {
    let key = Id::new(("bleu_scope_tex", kind as u8));
    let stored: Option<(TextureHandle, u64)> = ctx.data_mut(|d| d.get_temp::<(TextureHandle, u64)>(key));
    match stored {
        Some((tex, g)) if g == generation => tex.id(),
        Some((mut tex, _)) => {
            tex.set(make(), TextureOptions::LINEAR);
            let id = tex.id();
            ctx.data_mut(|d| d.insert_temp(key, (tex, generation)));
            id
        }
        None => {
            let tex = ctx.load_texture(kind.tex_name(), make(), TextureOptions::LINEAR);
            let id = tex.id();
            ctx.data_mut(|d| d.insert_temp(key, (tex, generation)));
            id
        }
    }
}

/// Log-scaled intensity of an accumulation count, 0..1.
struct Intensity {
    inv_log_ref: f32,
}

impl Intensity {
    fn new(max: u32) -> Self {
        Intensity { inv_log_ref: 1.0 / (1.0 + max.max(1) as f32).ln() }
    }

    #[inline]
    fn get(&self, c: u32) -> f32 {
        if c == 0 {
            0.0
        } else {
            ((1.0 + c as f32).ln() * self.inv_log_ref * 1.15).clamp(0.2, 1.0)
        }
    }
}

/// Premultiplied colour `tint * i` with alpha `i` (additive look over the
/// dark scope background).
#[inline]
fn glow(tint: [f32; 3], i: f32) -> Color32 {
    let a = (i * 255.0) as u8;
    Color32::from_rgba_premultiplied((tint[0] * i * 255.0) as u8, (tint[1] * i * 255.0) as u8, (tint[2] * i * 255.0) as u8, a)
}

const FULL_UV: Rect = Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 1.0, y: 1.0 } };

/// Draw into the given rect.
pub fn draw(ui: &mut egui::Ui, rect: egui::Rect, data: &ScopeData) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, scope_bg());
    match data.kind {
        ScopeKind::Vectorscope => draw_vectorscope(ui, &painter, rect, data),
        ScopeKind::Histogram => draw_histogram(ui, &painter, rect, data),
        ScopeKind::Waveform => draw_waveform(ui, &painter, rect, data),
        ScopeKind::Parade => draw_parade(ui, &painter, rect, data),
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, theme::PANEL()), StrokeKind::Inside);
    if data.samples == 0 {
        painter.text(
            rect.center_bottom() - vec2(0.0, 6.0),
            Align2::CENTER_BOTTOM,
            tr("Aucune image"),
            FontId::proportional(11.0),
            theme::TEXT_DIM(),
        );
    }
}

// --- Vectorscope -----------------------------------------------------------

fn draw_vectorscope(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, data: &ScopeData) {
    let side = (rect.width().min(rect.height()) - 12.0).max(16.0);
    let c = rect.center();
    let radius = side * 0.5;
    let img_rect = Rect::from_center_size(c, vec2(side, side));

    let tex = upload(ui.ctx(), data.kind, data.generation, || {
        let grid = &data.layers[0];
        let inten = Intensity::new(grid.iter().copied().max().unwrap_or(0));
        let n = VEC_SIZE;
        let mut pixels = vec![Color32::TRANSPARENT; n * n];
        for gy in 0..n {
            let cr = 0.5 - gy as f32 / (n - 1) as f32;
            for gx in 0..n {
                let cnt = grid[gy * n + gx];
                if cnt == 0 {
                    continue;
                }
                let cb = gx as f32 / (n - 1) as f32 - 0.5;
                // Colour each trace point by the hue it represents.
                let yv = 0.6;
                let r = (yv + 1.5748 * cr).clamp(0.0, 1.0);
                let b = (yv + 1.8556 * cb).clamp(0.0, 1.0);
                let g = ((yv - 0.2126 * r - 0.0722 * b) / 0.7152).clamp(0.0, 1.0);
                let tint = [0.45 + 0.55 * r, 0.45 + 0.55 * g, 0.45 + 0.55 * b];
                pixels[gy * n + gx] = glow(tint, inten.get(cnt));
            }
        }
        ColorImage { size: [n, n], pixels }
    });

    // Graticule under the trace: circles and axes.
    let gs = Stroke::new(1.0_f32, grat());
    painter.circle_stroke(c, radius, gs);
    painter.circle_stroke(c, radius * 0.5, Stroke::new(1.0_f32, grat().gamma_multiply(0.6)));
    painter.line_segment([c - vec2(radius, 0.0), c + vec2(radius, 0.0)], Stroke::new(1.0_f32, grat().gamma_multiply(0.6)));
    painter.line_segment([c - vec2(0.0, radius), c + vec2(0.0, radius)], Stroke::new(1.0_f32, grat().gamma_multiply(0.6)));
    for i in 0..36 {
        let a = i as f32 * std::f32::consts::TAU / 36.0;
        let d = vec2(a.cos(), a.sin());
        let len = if i % 3 == 0 { 6.0 } else { 3.0 };
        painter.line_segment([c + d * radius, c + d * (radius - len)], gs);
    }

    painter.image(tex, img_rect, FULL_UV, Color32::WHITE);

    // Skin-tone (flesh) line at ~123°.
    let a = 123f32.to_radians();
    let dir = vec2(a.cos(), -a.sin());
    painter.line_segment([c, c + dir * radius], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(235, 180, 140, 150)));

    // Rec.709 75 % colour-bar targets (boxes) and 100 % marks (dots).
    let to_screen = |cb: f32, cr: f32| -> Pos2 { c + vec2(cb, -cr) * (2.0 * radius) };
    let targets: [(&str, [f32; 3], Color32); 6] = [
        ("R", [1.0, 0.0, 0.0], Color32::from_rgb(235, 90, 90)),
        ("Mg", [1.0, 0.0, 1.0], Color32::from_rgb(225, 100, 225)),
        ("B", [0.0, 0.0, 1.0], Color32::from_rgb(110, 140, 245)),
        ("Cy", [0.0, 1.0, 1.0], Color32::from_rgb(90, 220, 225)),
        ("G", [0.0, 1.0, 0.0], Color32::from_rgb(100, 225, 110)),
        ("Yl", [1.0, 1.0, 0.0], Color32::from_rgb(230, 220, 90)),
    ];
    let font = FontId::proportional(10.0);
    let bsz = (radius * 0.07).clamp(4.0, 9.0);
    for (name, rgb, col) in targets {
        let (cb, cr) = cbcr(rgb[0] * 0.75, rgb[1] * 0.75, rgb[2] * 0.75);
        let p = to_screen(cb, cr);
        let st = Stroke::new(1.0_f32, col.gamma_multiply(0.9));
        painter.rect_stroke(Rect::from_center_size(p, vec2(bsz * 2.0, bsz * 2.0)), 0.0, st, StrokeKind::Middle);
        // Inner tolerance box.
        painter.rect_stroke(Rect::from_center_size(p, vec2(bsz * 0.7, bsz * 0.7)), 0.0, Stroke::new(1.0_f32, col.gamma_multiply(0.6)), StrokeKind::Middle);
        let (cb1, cr1) = cbcr(rgb[0], rgb[1], rgb[2]);
        let p1 = to_screen(cb1, cr1);
        painter.circle_filled(p1, 1.8, col.gamma_multiply(0.8));
        let out = (p - c).normalized();
        painter.text(p + out * (bsz + 9.0), Align2::CENTER_CENTER, name, font.clone(), col);
    }
}

// --- Histogram -------------------------------------------------------------

fn draw_histogram(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, data: &ScopeData) {
    let plot = rect.shrink2(vec2(6.0, 14.0)).translate(vec2(0.0, 4.0));
    if plot.width() < 8.0 || plot.height() < 8.0 || data.hist.len() < 4 {
        return;
    }
    // Reference: largest bin excluding the clipped extremes (which often dominate).
    let mut reference = 1u32;
    for ch in data.hist.iter() {
        for &v in &ch[1..LEVELS - 1] {
            reference = reference.max(v);
        }
    }
    let norm = |v: u32| -> f32 { (v as f32 / reference as f32).min(1.0) };

    // Graticule.
    let gs = Stroke::new(1.0_f32, grat());
    for q in 0..=4 {
        let x = plot.left() + plot.width() * q as f32 / 4.0;
        painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], gs);
    }
    for q in 0..=2 {
        let y = plot.top() + plot.height() * q as f32 / 2.0;
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0_f32, grat().gamma_multiply(0.6)));
    }

    // Additive R/G/B fill texture.
    let tex = upload(ui.ctx(), data.kind, data.generation, || {
        let mut pixels = vec![Color32::TRANSPARENT; LEVELS * HIST_H];
        let tints = [[1.0f32, 0.25, 0.25], [0.25, 1.0, 0.3], [0.3, 0.45, 1.0]];
        for x in 0..LEVELS {
            let hs = [norm(data.hist[0][x]), norm(data.hist[1][x]), norm(data.hist[2][x])];
            for row in 0..HIST_H {
                let from_bottom = (HIST_H - 1 - row) as f32 / HIST_H as f32;
                let mut c = [0f32; 3];
                let mut any = false;
                for ch in 0..3 {
                    if hs[ch] > 0.0 && from_bottom < hs[ch] {
                        any = true;
                        for k in 0..3 {
                            c[k] += tints[ch][k];
                        }
                    }
                }
                if any {
                    let i = 0.55;
                    pixels[row * LEVELS + x] = glow([c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)], i);
                }
            }
        }
        ColorImage { size: [LEVELS, HIST_H], pixels }
    });
    painter.image(tex, plot, FULL_UV, Color32::WHITE);

    // Curves: R, G, B outlines then luma on top.
    let curve = |ch: usize| -> Vec<Pos2> {
        (0..LEVELS)
            .map(|x| {
                let px = plot.left() + plot.width() * (x as f32 + 0.5) / LEVELS as f32;
                pos2(px, plot.bottom() - norm(data.hist[ch][x]) * plot.height())
            })
            .collect()
    };
    let colours = [
        Color32::from_rgb(240, 80, 80),
        Color32::from_rgb(80, 230, 100),
        Color32::from_rgb(100, 140, 255),
        Color32::from_rgb(235, 235, 235),
    ];
    if data.samples > 0 {
        for ch in 0..4 {
            let w: f32 = if ch == 3 { 1.5 } else { 1.0 };
            painter.line(curve(ch), Stroke::new(w, colours[ch]));
        }
    }

    // Legend.
    let font = FontId::proportional(10.0);
    let names = [tr("R"), tr("V"), tr("B"), tr("Luma")];
    let mut x = rect.right() - 6.0;
    for i in (0..4).rev() {
        let r = painter.text(pos2(x, rect.top() + 3.0), Align2::RIGHT_TOP, names[i], font.clone(), colours[i]);
        x = r.left() - 6.0;
    }
    painter.text(pos2(plot.left(), rect.top() + 3.0), Align2::LEFT_TOP, "0", font.clone(), grat_text());
    painter.text(pos2(plot.center().x, rect.top() + 3.0), Align2::CENTER_TOP, "128", font, grat_text());
}

// --- Waveform / parade -----------------------------------------------------

/// Plot area of an IRE scope (room on the left for the labels).
fn ire_plot(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.left() + 28.0, rect.top() + 8.0), pos2(rect.right() - 6.0, rect.bottom() - 8.0))
}

/// IRE 0–100 graticule with labelled lines at 0, 25, 50, 75, 100.
fn draw_ire_graticule(painter: &egui::Painter, rect: Rect, plot: Rect, labels: bool) {
    let font = FontId::proportional(10.0);
    for ire in [0, 25, 50, 75, 100] {
        let y = plot.bottom() - plot.height() * ire as f32 / 100.0;
        let strong = ire == 0 || ire == 100;
        let st = Stroke::new(1.0_f32, if strong { grat() } else { grat().gamma_multiply(0.6) });
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], st);
        if !labels {
            continue;
        }
        painter.text(pos2(rect.left() + 24.0, y), Align2::RIGHT_CENTER, format!("{}", ire), font.clone(), grat_text());
    }
}

/// Image of one accumulation layer (rows = levels, top = 255).
fn layer_pixels(layer: &[u32], inten: &Intensity, tint: [f32; 3], out: &mut [Color32], out_w: usize, x0: usize, gw: usize) {
    for row in 0..LEVELS {
        for col in 0..gw {
            let c = layer[row * gw + col];
            if c != 0 {
                out[row * out_w + x0 + col] = glow(tint, inten.get(c));
            }
        }
    }
}

fn draw_waveform(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, data: &ScopeData) {
    let plot = ire_plot(rect);
    if plot.width() < 8.0 || plot.height() < 8.0 {
        return;
    }
    draw_ire_graticule(painter, rect, plot, true);
    let tex = upload(ui.ctx(), data.kind, data.generation, || {
        let gw = data.gw;
        let layer = &data.layers[0];
        let inten = Intensity::new(layer.iter().copied().max().unwrap_or(0));
        let mut pixels = vec![Color32::TRANSPARENT; gw * LEVELS];
        layer_pixels(layer, &inten, [0.6, 1.0, 0.68], &mut pixels, gw, 0, gw);
        ColorImage { size: [gw, LEVELS], pixels }
    });
    painter.image(tex, plot, FULL_UV, Color32::WHITE);
}

fn draw_parade(ui: &egui::Ui, painter: &egui::Painter, rect: Rect, data: &ScopeData) {
    let plot = ire_plot(rect);
    if plot.width() < 24.0 || plot.height() < 8.0 {
        return;
    }
    let gap = 6.0;
    let sub_w = (plot.width() - 2.0 * gap) / 3.0;
    let subs: Vec<Rect> = (0..3)
        .map(|i| Rect::from_min_size(pos2(plot.left() + i as f32 * (sub_w + gap), plot.top()), vec2(sub_w, plot.height())))
        .collect();
    // Only the left scale is labelled.
    for (i, s) in subs.iter().enumerate() {
        draw_ire_graticule(painter, rect, *s, i == 0);
    }

    // One texture with the three channels side by side (a 2-px gap between
    // them avoids bleeding with linear filtering).
    let pad = 2usize;
    let gw = data.gw;
    let tw = gw * 3 + pad * 2;
    let tex = upload(ui.ctx(), data.kind, data.generation, || {
        let max = data.layers.iter().flat_map(|l| l.iter().copied()).max().unwrap_or(0);
        let inten = Intensity::new(max);
        let tints = [[1.0f32, 0.35, 0.35], [0.4, 1.0, 0.45], [0.45, 0.6, 1.0]];
        let mut pixels = vec![Color32::TRANSPARENT; tw * LEVELS];
        for c in 0..3 {
            layer_pixels(&data.layers[c], &inten, tints[c], &mut pixels, tw, c * (gw + pad), gw);
        }
        ColorImage { size: [tw, LEVELS], pixels }
    });
    let labels = [tr("R"), tr("V"), tr("B")];
    let label_cols = [Color32::from_rgb(240, 90, 90), Color32::from_rgb(90, 230, 110), Color32::from_rgb(110, 150, 255)];
    for (i, s) in subs.iter().enumerate() {
        let u0 = (i * (gw + pad)) as f32 / tw as f32;
        let u1 = (i * (gw + pad) + gw) as f32 / tw as f32;
        painter.image(tex, *s, Rect::from_min_max(pos2(u0, 0.0), pos2(u1, 1.0)), Color32::WHITE);
        painter.text(s.right_top() + vec2(-3.0, 2.0), Align2::RIGHT_TOP, labels[i], FontId::proportional(10.0), label_cols[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, rgb: [u8; 3]) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            v.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        v
    }

    #[test]
    fn grey_frame_is_centred_on_vectorscope() {
        let img = solid(1920, 1080, [128, 128, 128]);
        let d = analyze(&img, 1920, 1080, ScopeKind::Vectorscope);
        assert!(d.samples > 50_000 && d.samples < 400_000);
        let (idx, _) = d.layers[0].iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
        let (gx, gy) = (idx % VEC_SIZE, idx / VEC_SIZE);
        assert!((gx as i32 - 128).abs() <= 1 && (gy as i32 - 128).abs() <= 1);
    }

    #[test]
    fn red_frame_hits_red_quadrant_and_parade() {
        let img = solid(640, 360, [255, 0, 0]);
        let v = analyze(&img, 640, 360, ScopeKind::Vectorscope);
        let (idx, _) = v.layers[0].iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
        let (gx, gy) = (idx % VEC_SIZE, idx / VEC_SIZE);
        assert!(gx < 128 && gy < 10, "{gx} {gy}");

        let p = analyze(&img, 640, 360, ScopeKind::Parade);
        let total: u32 = p.layers[0][0..PARADE_COLS].iter().sum(); // R at level 255 (top row)
        assert_eq!(total as usize, p.samples);
        let total_g: u32 = p.layers[1][(LEVELS - 1) * PARADE_COLS..].iter().sum(); // G at 0 (bottom row)
        assert_eq!(total_g as usize, p.samples);

        let hgram = analyze(&img, 640, 360, ScopeKind::Histogram);
        assert_eq!(hgram.hist[0][255] as usize, hgram.samples);
        assert_eq!(hgram.hist[3][luma8(255, 0, 0)] as usize, hgram.samples);
    }

    #[test]
    fn bad_input_is_empty() {
        let d = analyze(&[0u8; 10], 100, 100, ScopeKind::Waveform);
        assert_eq!(d.samples, 0);
    }
}
