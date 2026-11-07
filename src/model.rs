//! Project model: tracks, events, media, markers. Values from MECHANICS.md.

use crate::i18n::tr;

use serde::{Deserialize, Serialize};

pub type Id = u64;

// ---- values from the mechanics sheet ----
pub const DEFAULT_FPS: f64 = 30000.0 / 1001.0; // 29.97003 (dFrameRateDefault)
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000; // dwSampleRateDefault
pub const DEFAULT_TEMPO: f64 = 120.0; // dTempoDefault
pub const MIN_PROJECT_DISPLAY_LEN: f64 = 120.0; // ctProjectMinLength
pub const DEFAULT_STILL_LEN: f64 = 5.0; // ntDefVideoStillLength
pub const SNAP_WIDTH_PX: f32 = 7.0; // cxSnapWidth
pub const DEFAULT_TRACK_HEIGHT: f32 = 68.0; // cyDefaultTrackHeight
pub const MIN_TRACK_HEIGHT: f32 = 25.0; // min video track height
pub const TOP_DOCK_HEIGHT: f32 = 350.0; // cyTopDock
pub const QUICKFADE_SECS: f64 = 0.010; // ntQuickfadeAudioSize
pub const PREROLL_SECS: f64 = 2.0; // ntPrerollForPreview
pub const TITLE_SAFE: f32 = 0.2; // dVideoPreviewTitleSafe
pub const ACTION_SAFE: f32 = 0.1; // dVideoPreviewActionSafe
pub const PREVIEW_GRID: u32 = 10; // cxVideoPreviewGrid
pub const MAX_SCRUB_SPEED: f64 = 20.0; // flScrubSpeedMax
pub const METER_FLOOR_DB: f32 = -60.0; // flPeakFloor
pub const NORMALIZE_TO: f32 = 0.98; // dNormalizeTo
pub const LIGHT_SELECTION_ALPHA: u8 = 90; // bLightSelectionAlpha
pub const HEAVY_SELECTION_ALPHA: u8 = 110; // bHeavySelectionAlpha
pub const HEADER_SELECTION_ALPHA: u8 = 64; // bHeaderSelectionAlpha
pub const SELECTION_PULSE_RATE: f32 = 3.0; // fSelectionPulseRate
pub const MIN_VISIBLE_GRID_PX: f32 = 9.0; // cxMinVisibleGrid
pub const DRAW_NORMALIZED_WAVES: bool = true; // fDrawNormalizedWaves

/// Fade curve (linear=1 ... hold=6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Curve {
    Linear,
    Fast,
    Slow,
    Smooth,
    Sharp,
    Hold,
}

impl Curve {
    pub const ALL: [Curve; 6] = [
        Curve::Linear,
        Curve::Fast,
        Curve::Slow,
        Curve::Smooth,
        Curve::Sharp,
        Curve::Hold,
    ];
    pub fn code(self) -> u8 {
        match self {
            Curve::Linear => 1,
            Curve::Fast => 2,
            Curve::Slow => 3,
            Curve::Smooth => 4,
            Curve::Sharp => 5,
            Curve::Hold => 6,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Curve::Linear => tr("Linéaire"),
            Curve::Fast => tr("Rapide"),
            Curve::Slow => tr("Lente"),
            Curve::Smooth => tr("Douce"),
            Curve::Sharp => tr("Nette"),
            Curve::Hold => tr("Maintien"),
        }
    }
    /// Gain while fading in, t in 0..1 (0 = silent, 1 = full).
    /// Classic fade shapes (internal codes hold 0, linear 1, fast 2, slow -2,
    /// smooth 4, sharp -4).
    pub fn gain_in(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            // code 2: v1 + (v0 - v1)(1 - u)^2
            Curve::Fast => 1.0 - (1.0 - t) * (1.0 - t),
            // code -2: u^2
            Curve::Slow => t * t,
            // code 4: 3u^2 - 2u^3
            Curve::Smooth => 3.0 * t * t - 2.0 * t * t * t,
            // code -4: 3(2u^3 - 3u^2 + u) + (3u^2 - 2u^3) = 4u^3 - 6u^2 + 3u
            Curve::Sharp => 4.0 * t * t * t - 6.0 * t * t + 3.0 * t,
            Curve::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
    /// Gain while fading out, u in 0..1 (0 = start of fade, full; 1 = silent).
    pub fn gain_out(self, u: f64) -> f64 {
        match self {
            Curve::Hold => {
                if u >= 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
            _ => 1.0 - self.gain_in(u),
        }
    }
}

impl Default for Curve {
    fn default() -> Self {
        Curve::Smooth // eDefaultVideoEnvelopeFadeType = 4
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Transition {
    Dissolve,
    WipeLeft,
    WipeDown,
    Push,
    Iris,
    ThroughBlack,
    Blinds,
    ClockWipe,
    Flash,
}

impl Transition {
    pub const ALL: [Transition; 9] = [
        Transition::Dissolve,
        Transition::WipeLeft,
        Transition::WipeDown,
        Transition::Push,
        Transition::Iris,
        Transition::ThroughBlack,
        Transition::Blinds,
        Transition::ClockWipe,
        Transition::Flash,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Transition::Dissolve => tr("Fondu enchaîné"),
            Transition::WipeLeft => tr("Volet horizontal"),
            Transition::WipeDown => tr("Volet vertical"),
            Transition::Push => tr("Poussée"),
            Transition::Iris => tr("Iris"),
            Transition::ThroughBlack => tr("Fondu par le noir"),
            Transition::Blinds => tr("Stores"),
            Transition::ClockWipe => tr("Volet horloge"),
            Transition::Flash => tr("Flash blanc"),
        }
    }
}

impl Default for Transition {
    fn default() -> Self {
        Transition::Dissolve
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VideoFx {
    BrightnessContrast { brightness: f32, contrast: f32 },
    Saturation { amount: f32 },
    BlackWhite { amount: f32 },
    Sepia { amount: f32 },
    Invert { amount: f32 },
    Blur { radius: f32 },
    Vignette { amount: f32 },
    // "Univers" pack: effects in the spirit of the Red Giant Universe OFX
    // plugins found on this machine (own implementations, same idea).
    Glow { radius: f32, intensity: f32, threshold: f32 },
    RgbSplit { amount: f32 },
    Glitch { amount: f32 },
    Vhs { amount: f32 },
    Grain { amount: f32 },
    Shake { amount: f32 },
    Duotone { dark: [u8; 4], light: [u8; 4] },
    Pixelate { size: f32 },
    Noir { contrast: f32 },
    // colour grading, keying, masks, stabilisation (color.rs)
    ColorGrade(ColorGrade),
    Lut { path: String, amount: f32 },
    ChromaKey { color: [u8; 3], tolerance: f32, softness: f32, spill: f32 },
    Mask { shape: MaskShape, x: f32, y: f32, w: f32, h: f32, feather: f32, invert: bool, points: Vec<[f32; 2]> },
    Stabilize { amount: f32, zoom: f32, path: Vec<[f32; 2]>, fps: f64 },
    Deflicker { amount: f32 },
}

/// Colour grading (Color Grading panel): wheels, basic controls and curves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorGrade {
    /// Lift / gamma / gain / offset wheels, RGB offsets around 0 (-1..1).
    pub lift: [f32; 3],
    pub gamma: [f32; 3],
    pub gain: [f32; 3],
    pub offset: [f32; 3],
    /// Exposure in stops (-4..4), contrast (-1..1), saturation (0..2),
    /// temperature and tint (-1..1).
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    /// Master curve points (x, y) in 0..1, sorted by x; empty = identity.
    pub curve: Vec<[f32; 2]>,
}

impl Default for ColorGrade {
    fn default() -> Self {
        ColorGrade {
            lift: [0.0; 3],
            gamma: [0.0; 3],
            gain: [0.0; 3],
            offset: [0.0; 3],
            exposure: 0.0,
            contrast: 0.0,
            saturation: 1.0,
            temperature: 0.0,
            tint: 0.0,
            curve: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskShape {
    Rectangle,
    Ellipse,
    /// Free shape from `points` (normalised 0..1 coordinates).
    Polygon,
}

impl VideoFx {
    pub fn presets() -> Vec<VideoFx> {
        vec![
            VideoFx::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            VideoFx::Saturation { amount: 1.0 },
            VideoFx::BlackWhite { amount: 1.0 },
            VideoFx::Sepia { amount: 1.0 },
            VideoFx::Invert { amount: 1.0 },
            VideoFx::Blur { radius: 4.0 },
            VideoFx::Vignette { amount: 0.6 },
        ]
    }
    /// The built-in "Univers" pack.
    pub fn pack() -> Vec<VideoFx> {
        vec![
            VideoFx::Glow { radius: 12.0, intensity: 0.8, threshold: 0.6 },
            VideoFx::RgbSplit { amount: 0.4 },
            VideoFx::Glitch { amount: 0.5 },
            VideoFx::Vhs { amount: 0.6 },
            VideoFx::Grain { amount: 0.35 },
            VideoFx::Shake { amount: 0.3 },
            VideoFx::Duotone { dark: [30, 20, 90, 255], light: [255, 190, 120, 255] },
            VideoFx::Pixelate { size: 16.0 },
            VideoFx::Noir { contrast: 0.6 },
        ]
    }
    /// Grading, keying, masks and correction effects.
    pub fn pro() -> Vec<VideoFx> {
        vec![
            VideoFx::ColorGrade(ColorGrade::default()),
            VideoFx::Lut { path: String::new(), amount: 1.0 },
            VideoFx::ChromaKey { color: [0, 200, 60], tolerance: 0.25, softness: 0.1, spill: 0.5 },
            VideoFx::Mask { shape: MaskShape::Ellipse, x: 0.25, y: 0.2, w: 0.5, h: 0.6, feather: 0.05, invert: false, points: vec![] },
            VideoFx::Stabilize { amount: 1.0, zoom: 1.08, path: vec![], fps: 0.0 },
            VideoFx::Deflicker { amount: 0.5 },
        ]
    }
    pub fn label(&self) -> &'static str {
        match self {
            VideoFx::BrightnessContrast { .. } => tr("Luminosité et contraste"),
            VideoFx::Saturation { .. } => tr("Saturation"),
            VideoFx::BlackWhite { .. } => tr("Noir et blanc"),
            VideoFx::Sepia { .. } => tr("Sépia"),
            VideoFx::Invert { .. } => tr("Inverser les couleurs"),
            VideoFx::Blur { .. } => tr("Flou"),
            VideoFx::Vignette { .. } => tr("Vignettage"),
            VideoFx::Glow { .. } => tr("Lueur"),
            VideoFx::RgbSplit { .. } => tr("Séparation RVB"),
            VideoFx::Glitch { .. } => tr("Glitch"),
            VideoFx::Vhs { .. } => tr("VHS"),
            VideoFx::Grain { .. } => tr("Grain de film"),
            VideoFx::Shake { .. } => tr("Tremblement de caméra"),
            VideoFx::Duotone { .. } => tr("Bichromie"),
            VideoFx::Pixelate { .. } => tr("Mosaïque"),
            VideoFx::Noir { .. } => tr("Noir moderne"),
            VideoFx::ColorGrade(_) => tr("Étalonnage"),
            VideoFx::Lut { .. } => tr("Table de correspondance (LUT)"),
            VideoFx::ChromaKey { .. } => tr("Incrustation (fond vert)"),
            VideoFx::Mask { .. } => tr("Masque"),
            VideoFx::Stabilize { .. } => tr("Stabilisation"),
            VideoFx::Deflicker { .. } => tr("Réduction du scintillement"),
        }
    }
    pub fn category(&self) -> &'static str {
        match self {
            VideoFx::Glow { .. } => tr("Lueur"),
            VideoFx::RgbSplit { .. } | VideoFx::Shake { .. } => "Distorsion",
            VideoFx::Glitch { .. } | VideoFx::Vhs { .. } | VideoFx::Grain { .. } => "Stylisation",
            VideoFx::Duotone { .. } | VideoFx::Noir { .. } | VideoFx::Pixelate { .. } => "Stylisation",
            VideoFx::Blur { .. } => tr("Flou"),
            VideoFx::ChromaKey { .. } | VideoFx::Mask { .. } => "Incrustation",
            VideoFx::Stabilize { .. } | VideoFx::Deflicker { .. } => "Correction",
            _ => "Couleur",
        }
    }
    /// Short name used by the Claude assistant tools.
    pub fn key(&self) -> &'static str {
        match self {
            VideoFx::BrightnessContrast { .. } => "brightness_contrast",
            VideoFx::Saturation { .. } => "saturation",
            VideoFx::BlackWhite { .. } => "black_white",
            VideoFx::Sepia { .. } => "sepia",
            VideoFx::Invert { .. } => "invert",
            VideoFx::Blur { .. } => "blur",
            VideoFx::Vignette { .. } => "vignette",
            VideoFx::Glow { .. } => "glow",
            VideoFx::RgbSplit { .. } => "rgb_split",
            VideoFx::Glitch { .. } => "glitch",
            VideoFx::Vhs { .. } => "vhs",
            VideoFx::Grain { .. } => "grain",
            VideoFx::Shake { .. } => "shake",
            VideoFx::Duotone { .. } => "duotone",
            VideoFx::Pixelate { .. } => "pixelate",
            VideoFx::Noir { .. } => "noir",
            VideoFx::ColorGrade(_) => "color_grade",
            VideoFx::Lut { .. } => "lut",
            VideoFx::ChromaKey { .. } => "chroma_key",
            VideoFx::Mask { .. } => "mask",
            VideoFx::Stabilize { .. } => "stabilize",
            VideoFx::Deflicker { .. } => "deflicker",
        }
    }
    pub fn from_key(k: &str) -> Option<VideoFx> {
        VideoFx::presets().into_iter().chain(VideoFx::pack()).chain(VideoFx::pro()).find(|f| f.key() == k)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanCrop {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
}

impl Default for PanCrop {
    fn default() -> Self {
        PanCrop {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
        }
    }
}

impl PanCrop {
    pub fn is_identity(&self) -> bool {
        (self.zoom - 1.0).abs() < 1e-4
            && self.x.abs() < 1e-4
            && self.y.abs() < 1e-4
            && self.rotation.abs() < 1e-3
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Hash)]
pub enum Generator {
    SolidColor([u8; 4]),
    ColorBars,
    Gradient([u8; 4], [u8; 4]),
    Text {
        text: String,
        size_pct: u32,
        color: [u8; 4],
        background: [u8; 4],
    },
    /// A subtitle line, drawn near the bottom on a dark box.
    Subtitle {
        text: String,
        size_pct: u32,
    },
    Tone {
        freq: u32,
    },
    DemoMusic {
        seed: u32,
    },
    /// Animated title (Titles & Text).
    Title {
        text: String,
        size_pct: u32,
        color: [u8; 4],
        anim: TitleAnim,
        anim_ms: u32,
    },
    /// Adjustment event: its effects apply to everything below it.
    Adjustment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum TitleAnim {
    None,
    FadeIn,
    SlideUp,
    Typewriter,
    Zoom,
    Bounce,
}

impl TitleAnim {
    pub const ALL: [TitleAnim; 6] = [TitleAnim::None, TitleAnim::FadeIn, TitleAnim::SlideUp, TitleAnim::Typewriter, TitleAnim::Zoom, TitleAnim::Bounce];
    pub fn label(self) -> &'static str {
        match self {
            TitleAnim::None => tr("Aucune"),
            TitleAnim::FadeIn => tr("Apparition en fondu"),
            TitleAnim::SlideUp => tr("Glissement vers le haut"),
            TitleAnim::Typewriter => tr("Machine à écrire"),
            TitleAnim::Zoom => tr("Zoom"),
            TitleAnim::Bounce => tr("Rebond"),
        }
    }
}

impl Generator {
    pub fn is_audio(&self) -> bool {
        matches!(self, Generator::Tone { .. } | Generator::DemoMusic { .. })
    }
    pub fn label(&self) -> String {
        match self {
            Generator::SolidColor(_) => "Couleur unie".into(),
            Generator::ColorBars => "Mire de couleurs".into(),
            Generator::Gradient(..) => "Dégradé".into(),
            Generator::Text { text, .. } => {
                let first = text.lines().next().unwrap_or("");
                format!("Texte : {}", first)
            }
            Generator::Subtitle { text, .. } => format!("Sous-titre : {}", text),
            Generator::Tone { freq } => format!("Tonalité {} Hz", freq),
            Generator::DemoMusic { .. } => "Musique de démonstration".into(),
            Generator::Title { text, .. } => format!("Titre : {}", text.lines().next().unwrap_or("")),
            Generator::Adjustment => "Ajustement".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MediaSource {
    File(String),
    Image(String),
    Generated(Generator),
    /// Another project used as media (nested timeline).
    Nested(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: Id,
    pub name: String,
    pub source: MediaSource,
    pub has_video: bool,
    pub has_audio: bool,
    /// Seconds; 0 = endless (images, generators).
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Bin (folder) in the project media window; empty = root.
    #[serde(default)]
    pub bin: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Small copy used for the preview (proxy file).
    #[serde(default)]
    pub proxy: Option<String>,
    /// Media effects: applied to every event using this media.
    #[serde(default)]
    pub fx: Vec<VideoFx>,
}

impl Media {
    /// A media with only the basic fields set.
    pub fn basic(name: String, source: MediaSource, has_video: bool, has_audio: bool, duration: f64, width: u32, height: u32, fps: f64) -> Media {
        Media { id: 0, name, source, has_video, has_audio, duration, width, height, fps, bin: String::new(), tags: vec![], proxy: None, fx: vec![] }
    }
    pub fn path(&self) -> Option<&str> {
        match &self.source {
            MediaSource::File(p) | MediaSource::Image(p) | MediaSource::Nested(p) => Some(p),
            MediaSource::Generated(_) => None,
        }
    }
    pub fn is_still(&self) -> bool {
        !matches!(self.source, MediaSource::File(_))
            || (self.has_video && self.duration <= 0.0)
    }
    pub fn type_label(&self) -> &'static str {
        match &self.source {
            MediaSource::File(_) => {
                if self.has_video && self.has_audio {
                    tr("Vidéo + audio")
                } else if self.has_video {
                    tr("Vidéo")
                } else {
                    tr("Audio")
                }
            }
            MediaSource::Image(_) => "Image",
            MediaSource::Nested(_) => tr("Projet imbriqué"),
            MediaSource::Generated(g) => {
                if g.is_audio() {
                    tr("Générateur audio")
                } else {
                    tr("Générateur vidéo")
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Id,
    pub media: Id,
    pub name: String,
    pub start: f64,
    pub length: f64,
    /// In-point inside the media, seconds.
    pub offset: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    pub curve_in: Curve,
    pub curve_out: Curve,
    /// Opacity (video) or volume (audio), 0..1.
    pub gain: f32,
    pub mute: bool,
    pub looped: bool,
    pub group: Option<Id>,
    pub fx: Vec<VideoFx>,
    pub pan: PanCrop,
    pub transition: Transition,
    /// Playback rate (0.25..4).
    #[serde(default = "one_f64")]
    pub rate: f64,
    #[serde(default)]
    pub reverse: bool,
    /// Velocity envelope, values in percent (-100..1000); empty = none.
    #[serde(default)]
    pub velocity: Vec<EnvPoint>,
    /// Keep the pitch when the rate changes (fEventPresevePitchByDefault = 1).
    #[serde(default = "yes")]
    pub preserve_pitch: bool,
    #[serde(default)]
    pub locked: bool,
    /// Other takes of this event (alternative media).
    #[serde(default)]
    pub takes: Vec<Take>,
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub invert_phase: bool,
    /// Pitch change in semitones (cents as fractions).
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub afx: Vec<crate::audio_fx::AudioFx>,
    /// Pan/crop keyframes (local time).
    #[serde(default)]
    pub pan_keys: Vec<PanKey>,
    /// Effect keyframes (local time): the whole effect chain at that time.
    #[serde(default)]
    pub fx_keys: Vec<FxKey>,
    /// Keep the picture's aspect ratio (false = stretch to fill).
    #[serde(default = "yes")]
    pub keep_aspect: bool,
    #[serde(default)]
    pub resample: Resample,
    /// Transition progress envelope (0..1 over the overlap); empty = curve.
    #[serde(default)]
    pub transition_env: Vec<EnvPoint>,
}

fn one_f64() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}
fn one_f32() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub media: Id,
    pub offset: f64,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Resample {
    #[default]
    Smart,
    Force,
    Disable,
}

/// Envelope point: time (seconds), value, curve to the next point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvPoint {
    pub t: f64,
    pub v: f32,
    #[serde(default = "linear_curve")]
    pub curve: Curve,
}

fn linear_curve() -> Curve {
    Curve::Linear
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanKey {
    pub t: f64,
    pub pan: PanCrop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FxKey {
    pub t: f64,
    pub fx: Vec<VideoFx>,
}

/// Value of an envelope at time t (curves as EnvelopeCurve_GetValueAt).
pub fn env_value(points: &[EnvPoint], t: f64, default: f32) -> f32 {
    match points.len() {
        0 => default,
        _ if t <= points[0].t => points[0].v,
        n if t >= points[n - 1].t => points[n - 1].v,
        _ => {
            let i = points.partition_point(|p| p.t <= t).max(1) - 1;
            let (a, b) = (points[i], points[i + 1]);
            let u = if b.t > a.t { (t - a.t) / (b.t - a.t) } else { 1.0 };
            let k = a.curve.gain_in(u) as f32;
            a.v + (b.v - a.v) * k
        }
    }
}

/// Inserts or replaces a point (kept sorted by time).
pub fn env_set(points: &mut Vec<EnvPoint>, t: f64, v: f32) {
    if let Some(p) = points.iter_mut().find(|p| (p.t - t).abs() < 1e-4) {
        p.v = v;
        return;
    }
    let i = points.partition_point(|p| p.t < t);
    points.insert(i, EnvPoint { t, v, curve: Curve::Linear });
}

/// Integral of the velocity envelope (percent / 100) from 0 to lt.
fn velocity_integral(points: &[EnvPoint], lt: f64) -> f64 {
    if points.is_empty() {
        return lt;
    }
    let v = |p: &EnvPoint| p.v as f64 / 100.0;
    let mut acc = 0.0;
    let mut prev_t = 0.0;
    let mut prev_v = env_value(points, 0.0, 100.0) as f64 / 100.0;
    for p in points.iter().filter(|p| p.t > 0.0) {
        if p.t >= lt {
            break;
        }
        acc += (prev_v + v(p)) * 0.5 * (p.t - prev_t);
        prev_t = p.t;
        prev_v = v(p);
    }
    let end_v = env_value(points, lt, 100.0) as f64 / 100.0;
    acc + (prev_v + end_v) * 0.5 * (lt - prev_t)
}

impl Event {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
    /// Time inside the media for local time lt, with rate, velocity and reverse.
    pub fn source_time(&self, lt: f64) -> f64 {
        let rate = self.rate.clamp(0.01, 100.0);
        let mut s = velocity_integral(&self.velocity, lt) * rate;
        if self.reverse {
            s = velocity_integral(&self.velocity, self.length) * rate - s;
        }
        self.offset + s
    }
    /// Length of media used by the event.
    pub fn source_span(&self) -> f64 {
        velocity_integral(&self.velocity, self.length).abs() * self.rate
    }
    /// Pan/crop at local time lt (keyframes interpolated).
    pub fn pan_at(&self, lt: f64) -> PanCrop {
        let k = &self.pan_keys;
        if k.is_empty() {
            return self.pan;
        }
        if lt <= k[0].t {
            return k[0].pan;
        }
        if lt >= k[k.len() - 1].t {
            return k[k.len() - 1].pan;
        }
        let i = k.partition_point(|p| p.t <= lt).max(1) - 1;
        let (a, b) = (&k[i], &k[i + 1]);
        let u = (((lt - a.t) / (b.t - a.t).max(1e-9)) as f32).clamp(0.0, 1.0);
        let l = |x: f32, y: f32| x + (y - x) * u;
        PanCrop {
            zoom: l(a.pan.zoom, b.pan.zoom),
            x: l(a.pan.x, b.pan.x),
            y: l(a.pan.y, b.pan.y),
            rotation: l(a.pan.rotation, b.pan.rotation),
        }
    }
    /// Effect chain at local time lt (numbers interpolated between keyframes).
    pub fn fx_at(&self, lt: f64) -> std::borrow::Cow<'_, [VideoFx]> {
        let k = &self.fx_keys;
        if k.is_empty() {
            return std::borrow::Cow::Borrowed(&self.fx);
        }
        if lt <= k[0].t {
            return std::borrow::Cow::Borrowed(&k[0].fx);
        }
        if lt >= k[k.len() - 1].t {
            return std::borrow::Cow::Borrowed(&k[k.len() - 1].fx);
        }
        let i = k.partition_point(|p| p.t <= lt).max(1) - 1;
        let (a, b) = (&k[i], &k[i + 1]);
        let u = ((lt - a.t) / (b.t - a.t).max(1e-9)).clamp(0.0, 1.0);
        let out = a.fx.iter().zip(b.fx.iter()).map(|(x, y)| lerp_fx(x, y, u)).collect::<Vec<_>>();
        std::borrow::Cow::Owned(out)
    }
    /// An empty event (used for temporary values and empty events).
    pub fn blank() -> Event {
        Event {
            id: 0,
            media: 0,
            name: String::new(),
            start: 0.0,
            length: 0.0,
            offset: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            curve_in: Curve::default(),
            curve_out: Curve::default(),
            gain: 1.0,
            mute: false,
            looped: true,
            group: None,
            fx: vec![],
            pan: PanCrop::default(),
            transition: Transition::default(),
            rate: 1.0,
            reverse: false,
            velocity: vec![],
            preserve_pitch: true,
            locked: false,
            takes: vec![],
            normalize: false,
            invert_phase: false,
            pitch: 0.0,
            afx: vec![],
            pan_keys: vec![],
            fx_keys: vec![],
            keep_aspect: true,
            resample: Resample::Smart,
            transition_env: vec![],
        }
    }
}

/// Interpolates every number of two effects of the same kind.
pub fn lerp_fx(a: &VideoFx, b: &VideoFx, u: f64) -> VideoFx {
    fn mix(a: &serde_json::Value, b: &serde_json::Value, u: f64) -> serde_json::Value {
        use serde_json::Value;
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                let (fx, fy) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
                let v = fx + (fy - fx) * u;
                if x.is_u64() || x.is_i64() {
                    serde_json::json!(v.round() as i64)
                } else {
                    serde_json::json!(v)
                }
            }
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => Value::Array(x.iter().zip(y).map(|(p, q)| mix(p, q, u)).collect()),
            (Value::Object(x), Value::Object(y)) => {
                let mut o = x.clone();
                for (k, v) in o.iter_mut() {
                    if let Some(w) = y.get(k) {
                        *v = mix(v, w, u);
                    }
                }
                Value::Object(o)
            }
            _ => a.clone(),
        }
    }
    let (Ok(ja), Ok(jb)) = (serde_json::to_value(a), serde_json::to_value(b)) else { return a.clone() };
    serde_json::from_value(mix(&ja, &jb, u)).unwrap_or_else(|_| a.clone())
}

/// Composite (blend) mode of a video track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Subtract,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
}

impl BlendMode {
    pub const ALL: [BlendMode; 9] = [
        BlendMode::Normal,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
    ];
    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => tr("Source Alpha (normal)"),
            BlendMode::Add => tr("Addition"),
            BlendMode::Subtract => tr("Soustraction"),
            BlendMode::Multiply => tr("Multiplication"),
            BlendMode::Screen => tr("Écran"),
            BlendMode::Overlay => tr("Superposition"),
            BlendMode::Darken => tr("Assombrir"),
            BlendMode::Lighten => tr("Éclaircir"),
            BlendMode::Difference => tr("Différence"),
        }
    }
}

/// Audio automation mode of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutoMode {
    Off,
    #[default]
    Read,
    Touch,
    Latch,
}

impl AutoMode {
    pub const ALL: [AutoMode; 4] = [AutoMode::Off, AutoMode::Read, AutoMode::Touch, AutoMode::Latch];
    pub fn label(self) -> &'static str {
        match self {
            AutoMode::Off => tr("Désactivé"),
            AutoMode::Read => tr("Lecture"),
            AutoMode::Touch => tr("Toucher"),
            AutoMode::Latch => tr("Verrou"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub kind: TrackKind,
    pub name: String,
    pub events: Vec<Event>,
    pub mute: bool,
    pub solo: bool,
    /// Composite level (video) or volume (audio), 0..1 (audio up to 2 = +6 dB).
    pub level: f32,
    /// Audio pan, -1..1.
    pub pan: f32,
    pub height: f32,
    pub color: usize,
    /// Volume (audio, 0..2) or composite level (video, 0..1) envelope.
    #[serde(default)]
    pub vol_env: Vec<EnvPoint>,
    /// Pan envelope (audio, -1..1).
    #[serde(default)]
    pub pan_env: Vec<EnvPoint>,
    /// Fade-to-colour envelope (video, -1 = bottom colour .. 1 = top colour).
    #[serde(default)]
    pub fade_env: Vec<EnvPoint>,
    #[serde(default = "fade_colors")]
    pub fade_colors: [[u8; 3]; 2],
    #[serde(default)]
    pub blend: BlendMode,
    /// 2D track motion (applied to the whole track).
    #[serde(default)]
    pub motion: PanCrop,
    #[serde(default)]
    pub motion_keys: Vec<PanKey>,
    /// Child of the nearest parent track above (compositing group).
    #[serde(default)]
    pub child: bool,
    /// Track effects (video) applied after event effects.
    #[serde(default)]
    pub fx: Vec<VideoFx>,
    /// Track audio effects (inserts).
    #[serde(default)]
    pub afx: Vec<crate::audio_fx::AudioFx>,
    /// Adjustment track: its effects change everything below.
    #[serde(default)]
    pub adjustment: bool,
    /// Output bus (None = master).
    #[serde(default)]
    pub bus: Option<usize>,
    /// Track group.
    #[serde(default)]
    pub group: Option<Id>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    /// Armed for recording.
    #[serde(default)]
    pub armed: bool,
    /// Surround pan (x: left/right, y: front/back), -1..1.
    #[serde(default)]
    pub surround: [f32; 2],
}

fn fade_colors() -> [[u8; 3]; 2] {
    [[255, 255, 255], [0, 0, 0]]
}

impl Track {
    pub fn motion_at(&self, t: f64) -> PanCrop {
        let tmp = Event { pan: self.motion, pan_keys: self.motion_keys.clone(), ..Event::blank() };
        tmp.pan_at(t)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub time: f64,
    pub name: String,
    /// Command marker (metadata text, e.g. a URL or chapter command).
    #[serde(default)]
    pub command: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub start: f64,
    pub end: f64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bus {
    pub name: String,
    pub level: f32,
    pub pan: f32,
    pub mute: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackGroup {
    pub id: Id,
    pub name: String,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldOrder {
    #[default]
    Progressive,
    UpperFirst,
    LowerFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Deinterlace {
    None,
    /// eDefDeintMethod = 1 (blend fields).
    #[default]
    Blend,
    Interpolate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RenderQuality {
    Draft,
    Preview,
    #[default]
    Good,
    Best,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub sample_rate: u32,
    pub tempo: f64,
    pub tracks: Vec<Track>,
    pub media: Vec<Media>,
    pub markers: Vec<Marker>,
    pub loop_region: Option<(f64, f64)>,
    pub master_volume: f32,
    pub next_id: Id,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub track_groups: Vec<TrackGroup>,
    #[serde(default)]
    pub notes: String,
    /// Video output effects (applied to the final picture).
    #[serde(default)]
    pub output_fx: Vec<VideoFx>,
    #[serde(default = "one_f32")]
    pub pixel_aspect: f32,
    #[serde(default)]
    pub field_order: FieldOrder,
    #[serde(default)]
    pub deinterlace: Deinterlace,
    #[serde(default)]
    pub render_quality: RenderQuality,
    /// Audio channels: 2 (stereo) or 6 (5.1 surround).
    #[serde(default = "two")]
    pub channels: u32,
    /// Motion blur amount (0..1) and supersampling (1..8, cMaxSupersamples = 8).
    #[serde(default)]
    pub motion_blur: f32,
    #[serde(default = "one_u32")]
    pub supersample: u32,
    /// Media bins (folders).
    #[serde(default)]
    pub bins: Vec<String>,
}

fn two() -> u32 {
    2
}
fn one_u32() -> u32 {
    1
}

impl Project {
    pub fn empty() -> Project {
        Project {
            name: "Sans titre".into(),
            width: 1920,
            height: 1080,
            fps: DEFAULT_FPS,
            sample_rate: DEFAULT_SAMPLE_RATE,
            tempo: DEFAULT_TEMPO,
            tracks: vec![],
            media: vec![],
            markers: vec![],
            loop_region: None,
            master_volume: 1.0,
            next_id: 1,
            regions: vec![],
            buses: vec![],
            track_groups: vec![],
            notes: String::new(),
            output_fx: vec![],
            pixel_aspect: 1.0,
            field_order: FieldOrder::Progressive,
            deinterlace: Deinterlace::Blend,
            render_quality: RenderQuality::Good,
            channels: 2,
            motion_blur: 0.0,
            supersample: 1,
            bins: vec![],
        }
    }

    pub fn new_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn media(&self, id: Id) -> Option<&Media> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn add_track(&mut self, kind: TrackKind, at: Option<usize>) -> usize {
        let id = self.new_id();
        let n = self.tracks.len();
        let t = Track {
            id,
            kind,
            name: String::new(),
            events: vec![],
            mute: false,
            solo: false,
            level: 1.0,
            pan: 0.0,
            height: DEFAULT_TRACK_HEIGHT,
            color: n,
            vol_env: vec![],
            pan_env: vec![],
            fade_env: vec![],
            fade_colors: fade_colors(),
            blend: BlendMode::Normal,
            motion: PanCrop::default(),
            motion_keys: vec![],
            child: false,
            fx: vec![],
            afx: vec![],
            adjustment: false,
            bus: None,
            group: None,
            auto_mode: AutoMode::Read,
            armed: false,
            surround: [0.0, 0.0],
        };
        let idx = at.unwrap_or(n).min(n);
        self.tracks.insert(idx, t);
        idx
    }

    pub fn add_media(&mut self, mut m: Media) -> Id {
        m.id = self.new_id();
        let id = m.id;
        self.media.push(m);
        id
    }

    pub fn new_event(&mut self, media: &Media, start: f64, length: f64) -> Event {
        Event {
            id: self.new_id(),
            media: media.id,
            name: media.name.clone(),
            start,
            length,
            looped: true, // fLoopEvents
            ..Event::blank()
        }
    }

    pub fn find_event(&self, id: Id) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ei) = t.events.iter().position(|e| e.id == id) {
                return Some((ti, ei));
            }
        }
        None
    }

    pub fn event(&self, id: Id) -> Option<&Event> {
        self.find_event(id).map(|(t, e)| &self.tracks[t].events[e])
    }

    pub fn event_mut(&mut self, id: Id) -> Option<&mut Event> {
        let (t, e) = self.find_event(id)?;
        Some(&mut self.tracks[t].events[e])
    }

    pub fn end_time(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.events.iter().map(|e| e.end()))
            .fold(0.0, f64::max)
    }

    pub fn sort_events(&mut self) {
        for t in &mut self.tracks {
            t.events
                .sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        }
    }

    pub fn any_solo(&self, kind: TrackKind) -> bool {
        self.tracks.iter().any(|t| t.kind == kind && t.solo)
    }

    pub fn track_audible(&self, ti: usize) -> bool {
        let t = &self.tracks[ti];
        if t.mute {
            return false;
        }
        if self.any_solo(t.kind) {
            return t.solo;
        }
        true
    }

    pub fn quantize(&self, t: f64) -> f64 {
        (t * self.fps).round() / self.fps
    }

    pub fn frame_of(&self, t: f64) -> i64 {
        (t * self.fps + 1e-6).floor() as i64
    }
}

/// Effective fades for an event, including automatic crossfades with
/// overlapping neighbours on the same track (fAutoCrossfade).
pub fn effective_fades(track: &Track, idx: usize, auto_crossfade: bool) -> (f64, f64) {
    let e = &track.events[idx];
    let mut fin = e.fade_in;
    let mut fout = e.fade_out;
    if auto_crossfade {
        for (j, o) in track.events.iter().enumerate() {
            if j == idx {
                continue;
            }
            // o starts before e and overlaps its head -> crossfade in
            if o.start < e.start && o.end() > e.start {
                let ov = (o.end() - e.start).min(e.length);
                fin = fin.max(ov);
            }
            // o starts after e and overlaps its tail -> crossfade out
            if o.start > e.start && o.start < e.end() {
                let ov = (e.end() - o.start).min(e.length);
                fout = fout.max(ov);
            }
        }
    }
    let total = fin + fout;
    if total > e.length && total > 0.0 {
        let k = e.length / total;
        fin *= k;
        fout *= k;
    }
    (fin, fout)
}

/// Gain envelope at local time `lt` (0..length) for an event.
pub fn fade_gain(e: &Event, lt: f64, fin: f64, fout: f64) -> f64 {
    let mut g = 1.0;
    if fin > 0.0 && lt < fin {
        g *= e.curve_in.gain_in(lt / fin);
    }
    let from_end = e.length - lt;
    if fout > 0.0 && from_end < fout {
        g *= e.curve_out.gain_out(1.0 - from_end / fout);
    }
    g
}

/// Format a time as a timecode, drop-frame for 29.97/59.94 (SMPTE Drop).
pub fn timecode(t: f64, fps: f64) -> String {
    let t = t.max(0.0);
    let nominal = fps.round() as i64;
    let drop_frame = (fps - nominal as f64).abs() > 0.01 && (nominal == 30 || nominal == 60);
    let mut frame = (t * fps + 1e-6).floor() as i64;
    if drop_frame {
        let drop = if nominal == 30 { 2 } else { 4 };
        let per_10 = nominal * 600 - drop * 9;
        let per_min = nominal * 60 - drop;
        let d = frame / per_10;
        let m = frame % per_10;
        if m > drop {
            frame += drop * 9 * d + drop * ((m - drop) / per_min);
        } else {
            frame += drop * 9 * d;
        }
    }
    let n = nominal.max(1);
    let ff = frame % n;
    let ss = (frame / n) % 60;
    let mm = (frame / (n * 60)) % 60;
    let hh = frame / (n * 3600);
    let sep = if drop_frame { ';' } else { ':' };
    format!("{:02}:{:02}:{:02}{}{:02}", hh, mm, ss, sep, ff)
}

/// Project format presets.
pub struct Template {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Project formats: common broadcast and web formats plus the frame sizes of
/// the render templates (portrait 9:16,
/// UHD, 5K, 8K) and common sizes asked for (2K, 1440p, square).
pub const TEMPLATES: &[Template] = &[
    Template { name: "HD 720p 29,970 (1280x720)", width: 1280, height: 720, fps: 29.97 },
    Template { name: "HD 720p 25 (1280x720)", width: 1280, height: 720, fps: 25.0 },
    Template { name: "HD 720p 59,94 (1280x720)", width: 1280, height: 720, fps: 59.94 },
    Template { name: "HD 720p 50 (1280x720)", width: 1280, height: 720, fps: 50.0 },
    Template { name: "Full HD 1080p 29,970 (1920x1080)", width: 1920, height: 1080, fps: 29.97 },
    Template { name: "Full HD 1080p 25 (1920x1080)", width: 1920, height: 1080, fps: 25.0 },
    Template { name: "Full HD 1080p 23,976 (1920x1080)", width: 1920, height: 1080, fps: 23.976 },
    Template { name: "Full HD 1080p 59,94 (1920x1080)", width: 1920, height: 1080, fps: 59.94 },
    Template { name: "Full HD 1080p 50 (1920x1080)", width: 1920, height: 1080, fps: 50.0 },
    Template { name: "2K DCI 24 (2048x1080)", width: 2048, height: 1080, fps: 24.0 },
    Template { name: "1440p QHD / 2K 29,970 (2560x1440)", width: 2560, height: 1440, fps: 29.97 },
    Template { name: "1440p QHD / 2K 59,94 (2560x1440)", width: 2560, height: 1440, fps: 59.94 },
    Template { name: "4K UHD 2160p 29,970 (3840x2160)", width: 3840, height: 2160, fps: 29.97 },
    Template { name: "4K UHD 2160p 25 (3840x2160)", width: 3840, height: 2160, fps: 25.0 },
    Template { name: "4K UHD 2160p 23,976 (3840x2160)", width: 3840, height: 2160, fps: 23.976 },
    Template { name: "4K UHD 2160p 59,94 (3840x2160)", width: 3840, height: 2160, fps: 59.94 },
    Template { name: "4K UHD 2160p 50 (3840x2160)", width: 3840, height: 2160, fps: 50.0 },
    Template { name: "4K DCI 24 (4096x2160)", width: 4096, height: 2160, fps: 24.0 },
    Template { name: "5K 2880p 59,94 (5760x2880)", width: 5760, height: 2880, fps: 59.94 },
    Template { name: "8K 4320p 59,94 (7680x4320)", width: 7680, height: 4320, fps: 59.94 },
    Template { name: "Vertical 9:16 720x1280 29,970", width: 720, height: 1280, fps: 29.97 },
    Template { name: "Vertical 9:16 1080x1920 29,970 (Shorts, Reels, TikTok)", width: 1080, height: 1920, fps: 29.97 },
    Template { name: "Vertical 9:16 1080x1920 59,94", width: 1080, height: 1920, fps: 59.94 },
    Template { name: "Vertical 9:16 1440x2560 29,970", width: 1440, height: 2560, fps: 29.97 },
    Template { name: "Vertical 9:16 4K 2160x3840 29,970", width: 2160, height: 3840, fps: 29.97 },
    Template { name: "Vertical 3:4 480x640 29,970", width: 480, height: 640, fps: 29.97 },
    Template { name: "Carré 1:1 1080x1080 29,970", width: 1080, height: 1080, fps: 29.97 },
    Template { name: "Portrait 4:5 1080x1350 29,970", width: 1080, height: 1350, fps: 29.97 },
    Template { name: "Internet 480p 29,970 (854x480)", width: 854, height: 480, fps: 29.97 },
    Template { name: "Internet 360p 29,970 (640x360)", width: 640, height: 360, fps: 29.97 },
    Template { name: "DVD NTSC (720x480; 29,970)", width: 720, height: 480, fps: 29.97 },
    Template { name: "DVD PAL (720x576; 25)", width: 720, height: 576, fps: 25.0 },
];

/// Normalise "29.97" style rates to exact NTSC fractions.
pub fn exact_fps(f: f64) -> f64 {
    for (approx, exact) in [
        (23.976, 24000.0 / 1001.0),
        (29.97, 30000.0 / 1001.0),
        (59.94, 60000.0 / 1001.0),
    ] {
        if (f - approx).abs() < 0.01 {
            return exact;
        }
    }
    f
}

pub const TRACK_COLORS: [[u8; 3]; 8] = [
    [0x7E, 0x5B, 0xD6],
    [0xE0, 0x8A, 0x2E],
    [0x4E, 0xB5, 0x4E],
    [0xD9, 0xC6, 0x3A],
    [0x3A, 0xB0, 0xD9],
    [0xD9, 0x4E, 0x8C],
    [0x9C, 0xC4, 0x3A],
    [0xC0, 0x5A, 0x3A],
];
