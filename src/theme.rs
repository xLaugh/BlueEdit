//! Interface colours: one table per "interface brightness level".
//! Level 0 is the darkest and the only one that turns on the
//! native dark window style.

#![allow(non_snake_case)]

use egui::{Color32, Stroke};
use std::sync::atomic::{AtomicU8, Ordering};

/// Number of interface brightness levels.
pub const LEVEL_COUNT: u8 = 4;
/// Default level (eInterfaceType default 0 = darkest).
pub const DEFAULT_LEVEL: u8 = 0;
/// Default interface saturation.
pub const UI_SATURATION: f32 = 1.0;

static LEVEL: AtomicU8 = AtomicU8::new(DEFAULT_LEVEL);

/// Grey entries 19..=28 then text (32) and grey text (33), per level,
/// (greys, R = G = B).
const RAMPS: [[u8; 12]; 4] = [
    [20, 30, 40, 60, 80, 20, 40, 60, 80, 80, 240, 120],
    [60, 70, 80, 100, 120, 60, 80, 100, 120, 120, 240, 160],
    [140, 150, 160, 180, 200, 140, 160, 180, 200, 200, 20, 60],
    [180, 190, 200, 220, 240, 180, 200, 220, 240, 240, 20, 100],
];

/// Colour entries common to every level: index → colour.
const fn rgb_of(colorref: u32) -> Color32 {
    Color32::from_rgb(
        (colorref & 0xff) as u8,
        ((colorref >> 8) & 0xff) as u8,
        ((colorref >> 16) & 0xff) as u8,
    )
}
const C3_BLUE: Color32 = rgb_of(0xf08c50); // index 3
const C5_ORANGE: Color32 = rgb_of(0x288cf0); // index 5
const C29_HIGHLIGHT: Color32 = rgb_of(0xdc8c3c); // index 29 (COLOR_HIGHLIGHT)
const C30_HIGHLIGHT_HOT: Color32 = rgb_of(0xe1a05a); // index 30
const C34_HIGHLIGHT_TEXT: Color32 = rgb_of(0xf0f0f0); // index 34

pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed).min(LEVEL_COUNT - 1)
}

fn level_file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("interface.txt")
}

/// Entry `idx` of the current level's table (only the indices used here).
pub fn ui_color(idx: u8) -> Color32 {
    let r = &RAMPS[level() as usize];
    let g = |v: u8| Color32::from_gray(v);
    match idx {
        0 => g(20),
        1 => g(240),
        3 => C3_BLUE,
        5 => C5_ORANGE,
        19..=28 => g(r[(idx - 19) as usize]),
        29 => saturate(C29_HIGHLIGHT, UI_SATURATION),
        30 => saturate(C30_HIGHLIGHT_HOT, UI_SATURATION),
        32 => g(r[10]),
        33 => g(r[11]),
        34 => C34_HIGHLIGHT_TEXT,
        _ => Color32::from_rgb(255, 0, 255),
    }
}

/// Saturation slider: 1.0 keeps the colour, 0.0 gives grey.
pub fn saturate(c: Color32, s: f32) -> Color32 {
    if (s - 1.0).abs() < f32::EPSILON {
        return c;
    }
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    let m = |v: u8| (l + (v as f32 - l) * s).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

fn is_light() -> bool {
    level() >= 2
}

// Roles:
// COLOR_MENU/APPWORKSPACE → 19, COLOR_WINDOW/BTNFACE → 21, COLOR_SCROLLBAR → 22,
// COLOR_BTNHIGHLIGHT/WINDOWFRAME → 23, COLOR_BTNTEXT/WINDOWTEXT/MENUTEXT → 32,
// COLOR_GRAYTEXT → 33, COLOR_HIGHLIGHT → 29, COLOR_HIGHLIGHTTEXT → 34.
pub fn BG() -> Color32 {
    ui_color(20)
}
pub fn PANEL() -> Color32 {
    ui_color(21)
}
pub fn BAR() -> Color32 {
    ui_color(19)
}
pub fn HEADER() -> Color32 {
    ui_color(22)
}
pub fn HEADER_SEL() -> Color32 {
    ui_color(23)
}
pub fn TRACK_BG() -> Color32 {
    ui_color(21)
}
pub fn TRACK_BG_ALT() -> Color32 {
    ui_color(20)
}
pub fn RULER() -> Color32 {
    ui_color(19)
}
pub fn TEXT() -> Color32 {
    ui_color(32)
}
pub fn TEXT_DIM() -> Color32 {
    ui_color(33)
}
pub fn ACCENT() -> Color32 {
    ui_color(29)
}
pub fn CURSOR() -> Color32 {
    ui_color(32)
}
pub fn MARKER() -> Color32 {
    ui_color(5)
}
pub fn LOOP() -> Color32 {
    ui_color(3)
}
pub fn EVENT_VIDEO() -> Color32 {
    ui_color(23)
}
pub fn EVENT_AUDIO() -> Color32 {
    ui_color(22)
}
pub fn WAVE() -> Color32 {
    if is_light() {
        Color32::from_rgb(30, 70, 130)
    } else {
        Color32::from_rgb(196, 220, 240)
    }
}

/// Changes the brightness level, saves it and restyles the interface.
pub fn set_level(ctx: &egui::Context, l: u8) {
    LEVEL.store(l.min(LEVEL_COUNT - 1), Ordering::Relaxed);
    let _ = std::fs::create_dir_all(crate::plugins::data_dir());
    let _ = std::fs::write(level_file(), level().to_string());
    apply(ctx);
}

/// Saved level (BLUE_UI_LEVEL overrides it, used for checks).
fn load_level() {
    let saved = std::env::var("BLUE_UI_LEVEL")
        .ok()
        .or_else(|| std::fs::read_to_string(level_file()).ok())
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(DEFAULT_LEVEL);
    LEVEL.store(saved.min(LEVEL_COUNT - 1), Ordering::Relaxed);
}

pub fn init(ctx: &egui::Context) {
    load_level();
    apply(ctx);
}

pub fn apply(ctx: &egui::Context) {
    let light = is_light();
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    let text = TEXT();
    let strong = if light { Color32::BLACK } else { Color32::WHITE };
    v.panel_fill = PANEL();
    v.window_fill = PANEL();
    v.extreme_bg_color = ui_color(19);
    v.faint_bg_color = ui_color(22);
    v.override_text_color = Some(text);
    v.selection.bg_fill = ACCENT();
    v.selection.stroke = Stroke::new(1.0, C34_HIGHLIGHT_TEXT);
    v.hyperlink_color = ui_color(30);
    v.widgets.noninteractive.bg_fill = PANEL();
    v.widgets.noninteractive.weak_bg_fill = PANEL();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ui_color(23));
    v.widgets.inactive.bg_fill = ui_color(22);
    v.widgets.inactive.weak_bg_fill = ui_color(22);
    v.widgets.hovered.bg_fill = ui_color(23);
    v.widgets.hovered.weak_bg_fill = ui_color(23);
    v.widgets.active.bg_fill = ACCENT();
    v.widgets.active.weak_bg_fill = ACCENT();
    v.window_corner_radius = 4.into();
    // readable text on every widget state (menus, buttons, labels)
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, strong);
    v.widgets.active.fg_stroke = Stroke::new(1.5, C34_HIGHLIGHT_TEXT);
    v.widgets.open.fg_stroke = Stroke::new(1.0, C34_HIGHLIGHT_TEXT);
    v.widgets.open.weak_bg_fill = ACCENT();
    v.widgets.open.bg_fill = ACCENT();
    v.window_stroke = Stroke::new(1.0, ui_color(23));
    v.popup_shadow = egui::epaint::Shadow {
        offset: [2, 4],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(if light { 60 } else { 140 }),
    };

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    // the base font sizes are scaled once (apply can run several times)
    let base = egui::Style::default();
    for (k, f) in style.text_styles.iter_mut() {
        if let Some(b) = base.text_styles.get(k) {
            f.size = (b.size * 0.93).round();
        }
    }
    // The panels are painted by us, so the same style is used whatever the
    // system theme is: set it for both themes and force one.
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(if light { egui::ThemePreference::Light } else { egui::ThemePreference::Dark });
    // native title bar: dark only at level 0, system default otherwise
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if level() == 0 {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::SystemDefault
    }));
    // BLUE_TEST_LIGHT=1 simulates a system in light mode (used to check the menus)
    if std::env::var("BLUE_TEST_LIGHT").is_ok() {
        ctx.set_theme(egui::ThemePreference::Light);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_converted_to_rgb() {
        // 0xDC8C3C (COLORREF) is R=60 G=140 B=220
        assert_eq!(C29_HIGHLIGHT, Color32::from_rgb(60, 140, 220));
        assert_eq!(C5_ORANGE, Color32::from_rgb(240, 140, 40));
    }

    #[test]
    fn levels_keep_text_readable() {
        for (i, r) in RAMPS.iter().enumerate() {
            let bg = r[2] as i32; // window / button face
            let text = r[10] as i32;
            assert!((text - bg).abs() >= 140, "level {i}");
        }
    }
}
