//! The Blue Edit logo, drawn in code (no image file): a white rounded "B" on
//! a blue gradient square, with an orange clip underneath like a timeline.
//! Used for the window icon, the menu bar mark, and written to PNG by the
//! packaging scripts (`--write-icon file.png`).

/// Signed distance to a rounded rectangle (negative inside), unit square coords.
fn rrect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let hx = (x1 - x0) / 2.0 - r;
    let hy = (y1 - y0) / 2.0 - r;
    let qx = (x - cx).abs() - hx;
    let qy = (y - cy).abs() - hy;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// Signed distance to the "B" (unit square coordinates).
fn letter_b(x: f32, y: f32) -> f32 {
    let stem = rrect(x, y, 0.29, 0.20, 0.44, 0.76, 0.025);
    let top = rrect(x, y, 0.29, 0.20, 0.66, 0.50, 0.15);
    let bottom = rrect(x, y, 0.29, 0.45, 0.71, 0.76, 0.155);
    let outer = stem.min(top).min(bottom);
    let top_hole = rrect(x, y, 0.44, 0.305, 0.545, 0.405, 0.05);
    let bottom_hole = rrect(x, y, 0.44, 0.555, 0.585, 0.655, 0.05);
    outer.max(-top_hole).max(-bottom_hole)
}

pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let px1 = 1.0 / s; // one pixel in unit coordinates, for anti-aliasing
    let cover = |d: f32| (0.5 - d / px1).clamp(0.0, 1.0);
    let mut px = vec![0u8; (size * size * 4) as usize];
    for yi in 0..size {
        for xi in 0..size {
            let x = (xi as f32 + 0.5) / s;
            let y = (yi as f32 + 0.5) / s;
            let i = ((yi * size + xi) * 4) as usize;
            let a = cover(rrect(x, y, 0.04, 0.04, 0.96, 0.96, 0.22));
            if a <= 0.0 {
                continue;
            }
            // diagonal blue gradient with a soft highlight at the top left
            let t = (x * 0.55 + y * 0.45).clamp(0.0, 1.0);
            let glow = (1.0 - ((x - 0.25).powi(2) + (y - 0.2).powi(2)).sqrt() * 1.6).max(0.0) * 25.0;
            let mut c = [
                22.0 + 20.0 * t + glow,
                78.0 + 70.0 * t + glow,
                200.0 + 45.0 * t + glow,
            ];
            // soft shadow under the B
            let sh = cover(letter_b(x - 0.012, y - 0.018) + 0.004) * 0.25;
            for ch in &mut c {
                *ch *= 1.0 - sh;
            }
            // the B
            let k = cover(letter_b(x, y));
            for ch in &mut c {
                *ch = *ch * (1.0 - k) + 255.0 * k;
            }
            // orange timeline clip under the letter
            let clip = cover(rrect(x, y, 0.29, 0.81, 0.71, 0.86, 0.025));
            let orange = [255.0, 168.0, 64.0];
            for ch in 0..3 {
                c[ch] = c[ch] * (1.0 - clip) + orange[ch] * clip;
            }
            px[i] = c[0].min(255.0) as u8;
            px[i + 1] = c[1].min(255.0) as u8;
            px[i + 2] = c[2].min(255.0) as u8;
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    px
}

pub fn icon_data(size: u32) -> egui::IconData {
    egui::IconData {
        rgba: rgba(size),
        width: size,
        height: size,
    }
}

pub fn color_image(size: u32) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba(size))
}

pub fn write_png(path: &str, size: u32) -> Result<(), String> {
    image::save_buffer(path, &rgba(size), size, size, image::ColorType::Rgba8).map_err(|e| e.to_string())
}
