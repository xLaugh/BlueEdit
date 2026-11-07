//! "Rendre en tant que" : renders the project with ffmpeg, using render
//! templates and bit-rate settings like those of common AVC/AAC MP4
//! render presets.

use crate::i18n::tr;

use crate::audio::{build_mix, mix_channels};
use crate::compositor::{Compositor, ExactSource};
use crate::media::{ffmpeg_available, MediaStore};
use crate::model::*;
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Mp4,
    Mov,
    Mkv,
    WebM,
    Wav,
    /// DVD video (MPEG-2 program stream, ready for authoring/burning tools).
    Dvd,
    /// Blu-ray compatible H.264 + AC-3 transport stream.
    BluRay,
}

impl Format {
    pub const ALL: [Format; 7] = [Format::Mp4, Format::Mov, Format::Mkv, Format::WebM, Format::Wav, Format::Dvd, Format::BluRay];
    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4 => tr("MP4 (vidéo + AAC)"),
            Format::Mov => tr("QuickTime MOV"),
            Format::Mkv => tr("Matroska MKV"),
            Format::WebM => tr("WebM (VP9 + Opus)"),
            Format::Wav => tr("Audio seul : WAV 16 bits"),
            Format::Dvd => tr("DVD vidéo (MPEG-2)"),
            Format::BluRay => tr("Blu-ray (H.264 + AC-3)"),
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
            Format::Mov => "mov",
            Format::Mkv => "mkv",
            Format::WebM => "webm",
            Format::Wav => "wav",
            Format::Dvd => "mpg",
            Format::BluRay => "m2ts",
        }
    }
    pub fn has_video(self) -> bool {
        self != Format::Wav
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    H264,
    Hevc,
    Vp9,
    ProRes,
}

impl Codec {
    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "AVC / H.264",
            Codec::Hevc => "HEVC / H.265",
            Codec::Vp9 => "VP9",
            Codec::ProRes => tr("ProRes 422 (sans débit cible)"),
        }
    }
    pub fn for_format(f: Format) -> &'static [Codec] {
        match f {
            Format::Mp4 => &[Codec::H264, Codec::Hevc],
            Format::Mov => &[Codec::H264, Codec::Hevc, Codec::ProRes],
            Format::Mkv => &[Codec::H264, Codec::Hevc, Codec::Vp9],
            Format::WebM => &[Codec::Vp9],
            Format::Wav | Format::Dvd => &[],
            Format::BluRay => &[Codec::H264],
        }
    }
}

/// Encoder families offered (CPU, NVIDIA NVENC, Intel QSV, AMD VCE).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoder {
    Cpu,
    Nvenc,
    Qsv,
    Amf,
}

impl Encoder {
    pub const ALL: [Encoder; 4] = [Encoder::Cpu, Encoder::Nvenc, Encoder::Qsv, Encoder::Amf];
    pub fn label(self) -> &'static str {
        match self {
            Encoder::Cpu => tr("Processeur (logiciel)"),
            Encoder::Nvenc => "NVIDIA NVENC",
            Encoder::Qsv => "Intel QSV",
            Encoder::Amf => "AMD VCE / AMF",
        }
    }
    fn ffmpeg_name(self, c: Codec) -> &'static str {
        match (self, c) {
            (_, Codec::Vp9) => "libvpx-vp9",
            (_, Codec::ProRes) => "prores_ks",
            (Encoder::Cpu, Codec::H264) => "libx264",
            (Encoder::Cpu, Codec::Hevc) => "libx265",
            (Encoder::Nvenc, Codec::H264) => "h264_nvenc",
            (Encoder::Nvenc, Codec::Hevc) => "hevc_nvenc",
            (Encoder::Qsv, Codec::H264) => "h264_qsv",
            (Encoder::Qsv, Codec::Hevc) => "hevc_qsv",
            (Encoder::Amf, Codec::H264) => "h264_amf",
            (Encoder::Amf, Codec::Hevc) => "hevc_amf",
        }
    }
}

/// Encoders actually present in this ffmpeg build (checked once).
pub fn encoder_available(e: Encoder, c: Codec) -> bool {
    static LIST: OnceLock<String> = OnceLock::new();
    let list = LIST.get_or_init(|| {
        let mut cmd = crate::platform::command("ffmpeg");
        cmd.args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    });
    let name = e.ffmpeg_name(c);
    list.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// "Constant bit rate (bps)" or "Variable bit rate" with "Maximum (bps)" and
/// "Average (bps)" — the two rate modes of the render dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateMode {
    Constant,
    Variable,
}

/// A render template.
pub struct Template {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub avg_bps: u32,
    pub max_bps: u32,
    pub ten_bit: bool,
}

const fn t(name: &'static str, width: u32, height: u32, fps: f64, avg: u32, max: u32, ten_bit: bool) -> Template {
    Template { name, width, height, fps, avg_bps: avg, max_bps: max, ten_bit }
}

pub const TEMPLATES: &[Template] = &[
    t("Internet HD 720p 29,97 i/s", 1280, 720, 29.97, 8_000_000, 16_000_000, false),
    t("Internet HD 720p 25 i/s", 1280, 720, 25.0, 8_000_000, 16_000_000, false),
    t("Internet HD 1080p 29,97 i/s", 1920, 1080, 29.97, 12_000_000, 24_000_000, false),
    t("Internet HD 1080p 25 i/s", 1920, 1080, 25.0, 12_000_000, 24_000_000, false),
    t("Internet HD 1080p 23,976 i/s", 1920, 1080, 23.976, 12_000_000, 24_000_000, false),
    t("Internet HD 1080p 50 i/s", 1920, 1080, 50.0, 20_000_000, 40_000_000, false),
    t("Internet HD 1080p 59,94 i/s", 1920, 1080, 59.94, 20_000_000, 40_000_000, false),
    t("Internet HD 1080p 29,97 i/s 9:16 (vertical)", 1080, 1920, 29.97, 12_000_000, 24_000_000, false),
    t("Internet UHD 2160p 29,97 i/s (4K)", 3840, 2160, 29.97, 50_000_000, 75_000_000, false),
    t("Internet UHD 2160p 25 i/s (4K)", 3840, 2160, 25.0, 50_000_000, 75_000_000, false),
    t("Internet UHD 2160p 50 i/s (4K)", 3840, 2160, 50.0, 70_000_000, 105_000_000, false),
    t("Internet UHD 2160p 59,94 i/s (4K)", 3840, 2160, 59.94, 80_000_000, 120_000_000, false),
    t("Internet UHD 2160p 23,976 i/s 9:16 (vertical)", 2160, 3840, 23.976, 48_000_000, 72_000_000, false),
    t("Internet 4K 23,976 i/s 9:16 (vertical)", 2304, 4096, 23.976, 48_000_000, 72_000_000, false),
    t("Internet 5K 2880p 50 i/s", 5760, 2880, 50.0, 120_000_000, 180_000_000, false),
    t("Internet 5K 2880p 59,94 i/s", 5760, 2880, 59.94, 140_000_000, 210_000_000, false),
    t("Internet 8K 4320p 50 i/s", 7680, 4320, 50.0, 220_000_000, 330_000_000, false),
    t("Internet 8K 4320p 59,94 i/s (10 bits)", 7680, 4320, 59.94, 270_000_000, 405_000_000, true),
];

/// AAC bit rates offered (27 values).
pub const AAC_BITRATES: [u32; 27] = [
    6000, 7000, 8000, 10000, 12000, 14000, 16000, 20000, 24000, 28000, 32000, 40000, 48000, 56000, 64000, 80000,
    96000, 112000, 128000, 160000, 192000, 224000, 256000, 320000, 384000, 448000, 512000,
];
/// Sample rates offered.
pub const SAMPLE_RATES: [u32; 6] = [22050, 24000, 32000, 44100, 48000, 96000];

/// Highest AAC bit rate offered for a sample rate.
pub fn max_aac_for_rate(rate: u32) -> u32 {
    match rate {
        r if r <= 8000 => 96_000,
        r if r < 11026 => 128_000,
        r if r <= 12000 => 144_000,
        r if r <= 16000 => 192_000,
        r if r <= 22050 => 264_600,
        r if r <= 24000 => 288_000,
        r if r <= 32000 => 384_000,
        _ => u32::MAX,
    }
}

/// 10-bit templates get 1.25x the bit rate, rounded down to whole Mbps
/// ((x * 10 / 8 000 000) * 1 000 000).
pub fn ten_bit_rate(bps: u32) -> u32 {
    ((bps as u64 * 10 / 8_000_000) * 1_000_000) as u32
}

#[derive(Clone, Debug)]
pub struct RenderSettings {
    pub format: Format,
    pub codec: Codec,
    pub encoder: Encoder,
    pub use_project: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub rate_mode: RateMode,
    pub cbr_bps: u32,
    pub avg_bps: u32,
    pub max_bps: u32,
    pub ten_bit: bool,
    pub stretch: bool,
    pub audio_rate: u32,
    pub audio_bps: u32,
}

impl RenderSettings {
    pub fn for_project(p: &Project) -> RenderSettings {
        let (avg, max) = suggested_rates(p.width, p.height, p.fps);
        RenderSettings {
            format: Format::Mp4,
            codec: Codec::H264,
            encoder: Encoder::Cpu,
            use_project: true,
            width: p.width,
            height: p.height,
            fps: p.fps,
            rate_mode: RateMode::Variable,
            cbr_bps: avg,
            avg_bps: avg,
            max_bps: max,
            ten_bit: false,
            stretch: false,
            audio_rate: 48_000,
            audio_bps: 192_000,
        }
    }
    pub fn apply_template(&mut self, t: &Template) {
        self.use_project = false;
        self.width = t.width;
        self.height = t.height;
        self.fps = exact_fps(t.fps);
        self.avg_bps = t.avg_bps;
        self.max_bps = t.max_bps;
        self.cbr_bps = t.avg_bps;
        self.ten_bit = t.ten_bit;
        if t.ten_bit {
            self.codec = Codec::Hevc;
        }
    }
    pub fn output_size(&self, p: &Project) -> (u32, u32, f64) {
        if self.use_project {
            (p.width & !1, p.height & !1, p.fps)
        } else {
            (self.width.max(16) & !1, self.height.max(16) & !1, self.fps)
        }
    }
}

/// Template rates for the closest frame size (by pixel count and frame rate).
pub fn suggested_rates(w: u32, h: u32, fps: f64) -> (u32, u32) {
    let px = (w as u64) * (h as u64);
    let best = TEMPLATES.iter().min_by(|a, b| {
        let da = ((a.width as u64 * a.height as u64) as f64 - px as f64).abs() + (a.fps - fps).abs() * 1000.0;
        let db = ((b.width as u64 * b.height as u64) as f64 - px as f64).abs() + (b.fps - fps).abs() * 1000.0;
        da.partial_cmp(&db).unwrap()
    });
    match best {
        Some(t) => (t.avg_bps, t.max_bps),
        None => (12_000_000, 24_000_000),
    }
}

/// Exact NTSC rates as fractions (30000/1001...), others as plain numbers.
fn fps_fraction(fps: f64) -> String {
    for n in [24000u32, 30000, 48000, 60000, 120000] {
        if (fps - n as f64 / 1001.0).abs() < 1e-3 {
            return format!("{n}/1001");
        }
    }
    if (fps - fps.round()).abs() < 1e-6 {
        format!("{}", fps.round() as u32)
    } else {
        format!("{:.6}", fps)
    }
}

pub fn fmt_bps(bps: u32) -> String {
    if bps >= 1_000_000 {
        format!("{} Mbps", (bps as f64 / 1e6).to_string().replace('.', ","))
    } else {
        format!("{} Kbps", bps / 1000)
    }
}

#[derive(Default)]
pub struct Progress {
    pub fraction: f32,
    pub message: String,
    pub done: bool,
    pub error: Option<String>,
}

pub struct Job {
    pub progress: Arc<Mutex<Progress>>,
    pub cancel: Arc<AtomicBool>,
    pub path: String,
}

pub fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    write_wav_ch(path, samples, rate, 2)
}

/// 16-bit PCM WAV with any channel count (2 = stereo, 6 = 5.1).
pub fn write_wav_ch(path: &std::path::Path, samples: &[f32], rate: u32, channels: u16) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = (samples.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2 * channels as u32).to_le_bytes())?;
    f.write_all(&(2 * channels).to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}

pub fn start(
    project: Project,
    store: Arc<MediaStore>,
    path: String,
    settings: RenderSettings,
    range: (f64, f64),
    auto_crossfade: bool,
) -> Job {
    let progress = Arc::new(Mutex::new(Progress {
        message: "Préparation…".into(),
        ..Default::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    let job = Job {
        progress: progress.clone(),
        cancel: cancel.clone(),
        path: path.clone(),
    };
    std::thread::spawn(move || {
        let r = run(&project, &store, &path, &settings, range, auto_crossfade, &progress, &cancel);
        let mut p = progress.lock().unwrap();
        p.done = true;
        match r {
            Ok(()) => {
                p.fraction = 1.0;
                p.message = "Rendu terminé.".into();
            }
            Err(e) => p.error = Some(e),
        }
    });
    job
}

/// ffmpeg arguments for the video stream.
fn video_args(s: &RenderSettings) -> Vec<String> {
    let mut a: Vec<String> = vec![];
    let enc = s.encoder.ffmpeg_name(s.codec);
    a.extend(["-c:v".into(), enc.into()]);
    let ten = s.ten_bit && s.codec == Codec::Hevc;
    let rates = |avg: u32, max: u32| -> Vec<String> {
        let (avg, max) = if ten { (ten_bit_rate(avg), ten_bit_rate(max)) } else { (avg, max) };
        vec![
            "-b:v".into(),
            avg.to_string(),
            "-maxrate".into(),
            max.to_string(),
            "-bufsize".into(),
            (max as u64 * 2).to_string(),
        ]
    };
    match s.codec {
        Codec::ProRes => {
            a.extend(["-profile:v".into(), "2".into(), "-pix_fmt".into(), "yuv422p10le".into()]);
            return a;
        }
        Codec::Vp9 => {
            a.extend(["-row-mt".into(), "1".into()]);
        }
        _ => {}
    }
    match s.rate_mode {
        RateMode::Constant => {
            let mut r = rates(s.cbr_bps, s.cbr_bps);
            r.extend(["-minrate".into(), (if ten { ten_bit_rate(s.cbr_bps) } else { s.cbr_bps }).to_string()]);
            a.extend(r);
            if enc == "libx264" {
                a.extend(["-x264-params".into(), "nal-hrd=cbr".into()]);
            } else if enc.ends_with("_nvenc") {
                a.extend(["-rc".into(), "cbr".into()]);
            }
        }
        RateMode::Variable => {
            a.extend(rates(s.avg_bps, s.max_bps));
            if enc.ends_with("_nvenc") {
                a.extend(["-rc".into(), "vbr".into()]);
            }
        }
    }
    let pix = if ten { "yuv420p10le" } else { "yuv420p" };
    a.extend(["-pix_fmt".into(), pix.into()]);
    if s.codec == Codec::Hevc && s.format != Format::Mkv {
        a.extend(["-tag:v".into(), "hvc1".into()]);
    }
    a
}

#[allow(clippy::too_many_arguments)]
fn run(
    p: &Project,
    store: &MediaStore,
    path: &str,
    s: &RenderSettings,
    (t0, t1): (f64, f64),
    auto_crossfade: bool,
    progress: &Mutex<Progress>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let (ow, oh, ofps) = s.output_size(p);
    let dur = (t1 - t0).max(1.0 / ofps);
    // 1. audio mixdown at the chosen sample rate (2 channels, or 6 for 5.1)
    let rate = if s.format == Format::Dvd || s.format == Format::BluRay { 48_000 } else { s.audio_rate.max(8000) };
    let chans: usize = if p.channels == 6 { 6 } else { 2 };
    let mix = build_mix(p, store, auto_crossfade);
    let frames = (dur * rate as f64) as usize;
    let mut samples = vec![0f32; frames * chans];
    let chunk = rate as usize;
    let mut i = 0;
    while i < frames {
        let n = chunk.min(frames - i);
        mix_channels(&mix, t0 + i as f64 / rate as f64, rate as f64, &mut samples[i * chans..(i + n) * chans], chans);
        i += n;
    }
    if !s.format.has_video() {
        write_wav_ch(std::path::Path::new(path), &samples, rate, chans as u16).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if !ffmpeg_available() {
        return Err("ffmpeg est introuvable : il est nécessaire pour créer des fichiers vidéo.".into());
    }
    if s.format != Format::Dvd && !encoder_available(s.encoder, s.codec) {
        return Err(format!("L'encodeur {} n'est pas disponible sur cet ordinateur.", s.encoder.label()));
    }
    let wav = std::env::temp_dir().join(format!("blue_render_{}.wav", std::process::id()));
    write_wav_ch(&wav, &samples, rate, chans as u16).map_err(|e| e.to_string())?;
    drop(samples);

    // 2. picture size: the project frame fitted into the output frame
    // (letterbox / pillarbox) unless "stretch to fill" is on.
    let (pw, ph) = (p.width.max(2) as f64, p.height.max(2) as f64);
    let (cw, ch) = if s.stretch {
        (ow, oh)
    } else {
        let k = (ow as f64 / pw).min(oh as f64 / ph);
        (((pw * k).round() as u32).max(2) & !1, ((ph * k).round() as u32).max(2) & !1)
    };
    let mut cmd = crate::platform::command("ffmpeg");
    cmd.args(["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgba"])
        .args(["-s", &format!("{}x{}", cw, ch), "-r", &fps_fraction(ofps), "-i", "-"])
        .arg("-i")
        .arg(&wav);
    if (cw, ch) != (ow, oh) {
        cmd.args(["-vf", &format!("pad={ow}:{oh}:(ow-iw)/2:(oh-ih)/2:black")]);
    }
    match s.format {
        Format::Dvd => {
            // DVD: MPEG-2 at the NTSC or PAL frame (720x480 / 720x576)
            let pal = (ofps - 25.0).abs() < 0.5;
            cmd.args(["-target", if pal { "pal-dvd" } else { "ntsc-dvd" }]);
            if p.width * 3 >= p.height * 4 + 1 && p.width * 9 >= p.height * 16 - 16 {
                cmd.args(["-aspect", "16:9"]);
            }
        }
        Format::BluRay => {
            cmd.args(video_args(s));
            cmd.args(["-bluray-compat", "1", "-level", "4.1", "-x264-params", "nal-hrd=vbr:b-pyramid=strict"]);
            cmd.args(["-c:a", "ac3", "-b:a", if chans == 6 { "448000" } else { "192000" }]);
            cmd.args(["-f", "mpegts", "-mpegts_m2ts_mode", "1"]);
        }
        Format::WebM => {
            cmd.args(video_args(s));
            cmd.args(["-c:a", "libopus", "-b:a", &s.audio_bps.min(512_000).to_string()]);
        }
        Format::Mov if s.codec == Codec::ProRes => {
            cmd.args(video_args(s));
            cmd.args(["-c:a", "pcm_s16le"]);
        }
        _ => {
            cmd.args(video_args(s));
            let bps = if chans == 6 { s.audio_bps.max(384_000) } else { s.audio_bps };
            cmd.args(["-c:a", "aac", "-b:a", &bps.min(max_aac_for_rate(rate).max(if chans == 6 { 512_000 } else { 0 })).to_string()]);
        }
    };
    // interlaced output (project field order)
    match p.field_order {
        FieldOrder::Progressive => {}
        FieldOrder::UpperFirst => {
            cmd.args(["-flags", "+ilme+ildct", "-field_order", "tt"]);
        }
        FieldOrder::LowerFirst => {
            cmd.args(["-flags", "+ilme+ildct", "-field_order", "bb"]);
        }
    }
    if chans == 6 {
        cmd.args(["-ac", "6"]);
    }
    if s.format == Format::Mp4 || s.format == Format::Mov {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-ar", &rate.to_string(), "-shortest", path]);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Impossible de lancer ffmpeg : {e}"))?;
    let mut stdin = child.stdin.take().ok_or("ffmpeg : pas d'entrée")?;
    let mut comp = Compositor::default();
    let mut src = ExactSource::new(ofps);
    let n_frames = (dur * ofps).ceil() as u64;
    for f in 0..n_frames {
        if cancel.load(Ordering::Relaxed) {
            drop(stdin);
            let _ = child.kill();
            let _ = std::fs::remove_file(&wav);
            return Err("Rendu annulé.".into());
        }
        let t = t0 + f as f64 / ofps;
        let img = if p.supersample > 1 && p.motion_blur > 0.0 {
            // motion blur: average `supersample` pictures over the shutter
            // (motion_blur x one frame), cMaxSupersamples = 8
            let n = p.supersample.min(8) as usize;
            let shutter = p.motion_blur.clamp(0.0, 1.0) as f64 / ofps;
            let mut acc = vec![0u32; (cw * ch * 4) as usize];
            for k in 0..n {
                let tk = t + shutter * (k as f64 / n as f64 - 0.5);
                let im = comp.compose(p, tk.max(0.0), cw, ch, auto_crossfade, &mut src);
                for (a, v) in acc.iter_mut().zip(im.iter()) {
                    *a += *v as u32;
                }
            }
            acc.iter().map(|a| (*a / n as u32) as u8).collect()
        } else {
            comp.compose(p, t, cw, ch, auto_crossfade, &mut src)
        };
        if stdin.write_all(&img).is_err() {
            break;
        }
        let mut pr = progress.lock().unwrap();
        pr.fraction = (f + 1) as f32 / n_frames as f32;
        pr.message = format!("Image {} / {}", f + 1, n_frames);
    }
    drop(stdin);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&wav);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffmpeg a échoué : {}", err.lines().last().unwrap_or("")));
    }
    Ok(())
}
