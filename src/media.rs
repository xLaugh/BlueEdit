//! Reading media: probing with ffprobe, decoding video frames and audio with
//! ffmpeg (if installed), still images, generated media and thumbnails.

use crate::model::*;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const AUDIO_RATE: u32 = DEFAULT_SAMPLE_RATE;
pub const PEAK_BLOCK: usize = 256;

fn command(program: &str) -> Command {
    crate::platform::command(program)
}

pub fn ffmpeg_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        command("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

pub const VIDEO_EXT: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "mts", "m2ts", "wmv", "mpg", "mpeg", "flv", "3gp",
    "mxf",
];
pub const AUDIO_EXT: &[&str] = &["wav", "mp3", "flac", "ogg", "m4a", "aac", "wma", "opus", "aif", "aiff"];
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "webp"];

pub fn ext_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn is_media_file(path: &str) -> bool {
    let e = ext_of(path);
    VIDEO_EXT.contains(&e.as_str()) || AUDIO_EXT.contains(&e.as_str()) || IMAGE_EXT.contains(&e.as_str())
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Look at a file and describe it as a Media (id is assigned by the project).
pub fn probe(path: &str) -> Result<Media, String> {
    let e = ext_of(path);
    if IMAGE_EXT.contains(&e.as_str()) {
        let (w, h) = image::image_dimensions(path).map_err(|e| e.to_string())?;
        return Ok(Media {
            id: 0,
            name: file_name(path),
            source: MediaSource::Image(path.to_string()),
            has_video: true,
            has_audio: false,
            duration: 0.0,
            width: w,
            height: h,
            fps: 0.0,
            bin: String::new(),
            tags: vec![],
            proxy: None,
            fx: vec![],
        });
    }
    if !ffmpeg_available() {
        return Err("ffmpeg est introuvable : installez-le pour importer des vidéos et des sons.".into());
    }
    let out = command("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,r_frame_rate,avg_frame_rate,duration:format=duration",
            "-of",
            "json",
            path,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe : {e}"))?;
    let mut m = Media {
        id: 0,
        name: file_name(path),
        source: MediaSource::File(path.to_string()),
        has_video: false,
        has_audio: false,
        duration: 0.0,
        width: 0,
        height: 0,
        fps: 0.0,
        bin: String::new(),
        tags: vec![],
        proxy: None,
        fx: vec![],
    };
    let parse_rate = |s: &str| -> f64 {
        let mut it = s.split('/');
        let a: f64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
        let b: f64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1.0);
        if b > 0.0 {
            a / b
        } else {
            0.0
        }
    };
    if let Some(streams) = v["streams"].as_array() {
        for s in streams {
            match s["codec_type"].as_str() {
                Some("video") if !m.has_video => {
                    m.has_video = true;
                    m.width = s["width"].as_u64().unwrap_or(0) as u32;
                    m.height = s["height"].as_u64().unwrap_or(0) as u32;
                    let r = parse_rate(s["avg_frame_rate"].as_str().unwrap_or("0/1"));
                    m.fps = if r > 0.0 {
                        r
                    } else {
                        parse_rate(s["r_frame_rate"].as_str().unwrap_or("0/1"))
                    };
                }
                Some("audio") => m.has_audio = true,
                _ => {}
            }
        }
    }
    m.duration = v["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    if !m.has_video && !m.has_audio {
        return Err(format!("« {} » ne contient ni image ni son lisible.", m.name));
    }
    // A single-frame "video" (cover art) counts as a still.
    if m.has_video && m.fps > 0.0 && m.duration > 0.0 && m.duration * m.fps < 1.5 && !m.has_audio {
        m.duration = 0.0;
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// Video decoding

/// Sequential reader: ffmpeg decodes from `start`, scaled to w x h, at `fps`.
pub struct VideoReader {
    child: Child,
    pub next_time: f64,
    pub fps: f64,
    w: u32,
    h: u32,
    eof: bool,
}

impl VideoReader {
    pub fn open(path: &str, start: f64, w: u32, h: u32, fps: f64) -> Option<VideoReader> {
        let child = command("ffmpeg")
            .args(["-v", "quiet", "-ss", &format!("{:.4}", start.max(0.0)), "-i", path, "-an", "-sn"])
            .args(["-vf", &format!("scale={}:{}:flags=bilinear,fps={}", w, h, fps)])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(VideoReader {
            child,
            next_time: start.max(0.0),
            fps,
            w,
            h,
            eof: false,
        })
    }

    /// Read the next frame; returns (time of frame, rgba).
    pub fn read_frame(&mut self) -> Option<(f64, Vec<u8>)> {
        if self.eof {
            return None;
        }
        let mut buf = vec![0u8; (self.w * self.h * 4) as usize];
        let out = self.child.stdout.as_mut()?;
        if out.read_exact(&mut buf).is_err() {
            self.eof = true;
            return None;
        }
        let t = self.next_time;
        self.next_time += 1.0 / self.fps;
        Some((t, buf))
    }

    /// Frame showing time `t` (reads forward). None if t is behind or past the end.
    pub fn frame_at(&mut self, t: f64, last: &mut Option<(f64, Arc<Vec<u8>>)>) -> Option<Arc<Vec<u8>>> {
        loop {
            if let Some((lt, ref f)) = last {
                if *lt <= t + 1e-6 && t < *lt + 1.0 / self.fps - 1e-6 {
                    return Some(f.clone());
                }
            }
            match self.read_frame() {
                Some((ft, f)) => *last = Some((ft, Arc::new(f))),
                None => return last.as_ref().map(|(_, f)| f.clone()),
            }
        }
    }
}

impl Drop for VideoReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Slot {
    want: Mutex<Option<f64>>,
    latest: Mutex<Option<(f64, Arc<Vec<u8>>)>>,
    cv: Condvar,
    alive: AtomicBool,
    last_used: Mutex<Instant>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct SlotKey {
    path: String,
    event: Id,
    w: u32,
    h: u32,
}

/// Background decoders for the preview: one ffmpeg reader per visible event.
pub struct FrameServer {
    slots: Mutex<HashMap<SlotKey, Arc<Slot>>>,
    ctx: egui::Context,
    fps: Mutex<f64>,
}

impl FrameServer {
    pub fn new(ctx: egui::Context) -> Arc<FrameServer> {
        Arc::new(FrameServer {
            slots: Mutex::new(HashMap::new()),
            ctx,
            fps: Mutex::new(DEFAULT_FPS),
        })
    }

    pub fn set_fps(&self, fps: f64) {
        *self.fps.lock().unwrap() = fps;
    }

    /// Latest decoded frame near `t` (may lag a little while decoding).
    pub fn get(&self, path: &str, event: Id, t: f64, w: u32, h: u32) -> Option<Arc<Vec<u8>>> {
        let key = SlotKey {
            path: path.to_string(),
            event,
            w,
            h,
        };
        let slot = {
            let mut slots = self.slots.lock().unwrap();
            slots
                .entry(key.clone())
                .or_insert_with(|| {
                    let s = Arc::new(Slot {
                        want: Mutex::new(None),
                        latest: Mutex::new(None),
                        cv: Condvar::new(),
                        alive: AtomicBool::new(true),
                        last_used: Mutex::new(Instant::now()),
                    });
                    let s2 = s.clone();
                    let ctx = self.ctx.clone();
                    let fps = *self.fps.lock().unwrap();
                    let path = key.path.clone();
                    std::thread::spawn(move || decoder_thread(s2, path, w, h, fps, ctx));
                    s
                })
                .clone()
        };
        *slot.last_used.lock().unwrap() = Instant::now();
        {
            let mut want = slot.want.lock().unwrap();
            if *want != Some(t) {
                *want = Some(t);
                slot.cv.notify_one();
            }
        }
        let latest = slot.latest.lock().unwrap();
        latest.as_ref().map(|(_, f)| f.clone())
    }

    /// Stop decoders that were not used recently.
    pub fn collect(&self) {
        let mut slots = self.slots.lock().unwrap();
        slots.retain(|_, s| {
            let keep = s.last_used.lock().unwrap().elapsed() < Duration::from_secs(3);
            if !keep {
                s.alive.store(false, Ordering::Relaxed);
                s.cv.notify_one();
            }
            keep
        });
    }
}

fn decoder_thread(slot: Arc<Slot>, path: String, w: u32, h: u32, fps: f64, ctx: egui::Context) {
    let mut reader: Option<VideoReader> = None;
    let mut last: Option<(f64, Arc<Vec<u8>>)> = None;
    let mut done_for: Option<f64> = None;
    while slot.alive.load(Ordering::Relaxed) {
        let want = {
            let mut g = slot.want.lock().unwrap();
            while (*g == done_for || g.is_none()) && slot.alive.load(Ordering::Relaxed) {
                let (ng, _) = slot.cv.wait_timeout(g, Duration::from_millis(200)).unwrap();
                g = ng;
            }
            *g
        };
        let Some(t) = want else { continue };
        if !slot.alive.load(Ordering::Relaxed) {
            break;
        }
        let need_seek = match &reader {
            None => true,
            Some(r) => {
                let behind = match &last {
                    Some((lt, _)) => t < *lt - 1e-6,
                    None => t < r.next_time - 1e-6,
                };
                behind || t > r.next_time + 2.0
            }
        };
        if need_seek {
            reader = VideoReader::open(&path, t, w, h, fps);
            last = None;
        }
        if let Some(r) = reader.as_mut() {
            if let Some(f) = r.frame_at(t, &mut last) {
                let lt = last.as_ref().map(|(x, _)| *x).unwrap_or(t);
                *slot.latest.lock().unwrap() = Some((lt, f));
                ctx.request_repaint();
            }
        }
        done_for = Some(t);
    }
}

// ---------------------------------------------------------------------------
// Audio

pub struct AudioData {
    /// Interleaved stereo at AUDIO_RATE.
    pub samples: Vec<i16>,
    /// Per PEAK_BLOCK frames: (min, max) for left and right.
    pub peaks: Vec<[i16; 4]>,
    /// Loudest sample (0..1), used to draw normalised waveforms.
    pub max_peak: f32,
}

impl AudioData {
    pub fn from_samples(samples: Vec<i16>) -> AudioData {
        let frames = samples.len() / 2;
        let mut peaks = Vec::with_capacity(frames / PEAK_BLOCK + 1);
        let mut i = 0;
        while i < frames {
            let end = (i + PEAK_BLOCK).min(frames);
            let mut p = [i16::MAX, i16::MIN, i16::MAX, i16::MIN];
            for f in i..end {
                let l = samples[f * 2];
                let r = samples[f * 2 + 1];
                p[0] = p[0].min(l);
                p[1] = p[1].max(l);
                p[2] = p[2].min(r);
                p[3] = p[3].max(r);
            }
            peaks.push(p);
            i = end;
        }
        let max_peak = peaks
            .iter()
            .map(|p| p.iter().map(|v| (*v as i32).unsigned_abs()).max().unwrap_or(0))
            .max()
            .unwrap_or(0) as f32
            / 32768.0;
        AudioData { samples, peaks, max_peak }
    }
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / AUDIO_RATE as f64
    }
}

pub fn decode_audio(path: &str) -> Option<AudioData> {
    let mut child = command("ffmpeg")
        .args(["-v", "quiet", "-i", path, "-vn", "-sn", "-ac", "2", "-ar", &AUDIO_RATE.to_string()])
        .args(["-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut bytes = Vec::new();
    child.stdout.as_mut()?.read_to_end(&mut bytes).ok()?;
    let _ = child.wait();
    let samples: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    if samples.is_empty() {
        return None;
    }
    Some(AudioData::from_samples(samples))
}

pub fn generate_audio(g: &Generator, seconds: f64) -> AudioData {
    let n = (seconds * AUDIO_RATE as f64) as usize;
    let sr = AUDIO_RATE as f64;
    let mut s = Vec::with_capacity(n * 2);
    match g {
        Generator::Tone { freq } => {
            for i in 0..n {
                let v = (2.0 * std::f64::consts::PI * *freq as f64 * i as f64 / sr).sin() * 0.25;
                let x = (v * 32767.0) as i16;
                s.push(x);
                s.push(x);
            }
        }
        Generator::DemoMusic { seed } => {
            // A soft chord progression with plucked notes: placeholder music.
            let chords: [[f64; 3]; 4] = [
                [220.00, 277.18, 329.63],
                [196.00, 246.94, 293.66],
                [174.61, 220.00, 261.63],
                [196.00, 246.94, 329.63],
            ];
            let beat = 60.0 / DEFAULT_TEMPO;
            let mut rng = *seed as u64 * 2654435761 + 12345;
            for i in 0..n {
                let t = i as f64 / sr;
                let bar = ((t / (beat * 4.0)) as usize) % 4;
                let ch = chords[(bar + *seed as usize) % 4];
                let in_beat = t % beat;
                let note = ch[((t / beat) as usize) % 3] * 2.0;
                let env = (-in_beat * 6.0).exp();
                let mut v = 0.0;
                for f in ch {
                    v += (2.0 * std::f64::consts::PI * f * t).sin() * 0.06;
                }
                v += (2.0 * std::f64::consts::PI * note * t).sin() * 0.18 * env;
                // soft hat on off-beats
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let noise = ((rng % 2000) as f64 / 1000.0 - 1.0) * 0.03;
                let off = (t + beat / 2.0) % beat;
                v += noise * (-off * 40.0).exp();
                let pan = (t * 0.5).sin() * 0.2;
                let l = (v * (1.0 - pan) * 32767.0).clamp(-32767.0, 32767.0) as i16;
                let r = (v * (1.0 + pan) * 32767.0).clamp(-32767.0, 32767.0) as i16;
                s.push(l);
                s.push(r);
            }
        }
        _ => s.resize(n * 2, 0),
    }
    AudioData::from_samples(s)
}

// ---------------------------------------------------------------------------
// Images, generators, text

pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

pub fn load_image(path: &str) -> Option<Rgba> {
    let img = image::open(path).ok()?.to_rgba8();
    Some(Rgba {
        w: img.width(),
        h: img.height(),
        data: img.into_raw(),
    })
}

pub fn resize_nearest(src: &Rgba, w: u32, h: u32) -> Rgba {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = ((y as u64 * src.h as u64) / h.max(1) as u64).min(src.h as u64 - 1) as u32;
        for x in 0..w {
            let sx = ((x as u64 * src.w as u64) / w.max(1) as u64).min(src.w as u64 - 1) as u32;
            let si = ((sy * src.w + sx) * 4) as usize;
            let di = ((y * w + x) * 4) as usize;
            data[di..di + 4].copy_from_slice(&src.data[si..si + 4]);
        }
    }
    Rgba { w, h, data }
}

fn font() -> &'static Option<ab_glyph::FontVec> {
    static F: OnceLock<Option<ab_glyph::FontVec>> = OnceLock::new();
    F.get_or_init(|| {
        let defs = egui::FontDefinitions::default();
        let data = defs
            .font_data
            .get("Ubuntu-Light")
            .or_else(|| defs.font_data.values().next())?;
        ab_glyph::FontVec::try_from_vec(data.font.to_vec()).ok()
    })
}

/// Render text (centred, multi-line) over a background colour.
pub fn render_text(text: &str, size_pct: u32, color: [u8; 4], bg: [u8; 4], w: u32, h: u32) -> Rgba {
    use ab_glyph::{Font, ScaleFont};
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        data.extend_from_slice(&bg);
    }
    let mut img = Rgba { w, h, data };
    let Some(font) = font() else { return img };
    let px = (h as f32 * size_pct as f32 / 100.0).max(4.0);
    let scaled = font.as_scaled(px);
    let lines: Vec<&str> = text.lines().collect();
    let line_h = scaled.height() + scaled.line_gap();
    let total_h = line_h * lines.len() as f32;
    let mut y0 = (h as f32 - total_h) / 2.0 + scaled.ascent();
    for line in &lines {
        let width: f32 = line.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum();
        let mut x = (w as f32 - width) / 2.0;
        for c in line.chars() {
            let gid = font.glyph_id(c);
            for (off, col) in [(px * 0.04, [0u8, 0, 0, 160]), (0.0, color)] {
                let g = gid.with_scale_and_position(px, ab_glyph::point(x + off, y0 + off));
                if let Some(og) = font.outline_glyph(g) {
                    let b = og.px_bounds();
                    og.draw(|gx, gy, cov| {
                        let ix = b.min.x as i32 + gx as i32;
                        let iy = b.min.y as i32 + gy as i32;
                        if ix >= 0 && iy >= 0 && (ix as u32) < w && (iy as u32) < h {
                            let i = ((iy as u32 * w + ix as u32) * 4) as usize;
                            let a = cov * col[3] as f32 / 255.0;
                            for k in 0..3 {
                                img.data[i + k] =
                                    (img.data[i + k] as f32 * (1.0 - a) + col[k] as f32 * a) as u8;
                            }
                            img.data[i + 3] = (img.data[i + 3] as f32 + (255.0 - img.data[i + 3] as f32) * a) as u8;
                        }
                    });
                }
            }
            x += scaled.h_advance(gid);
        }
        y0 += line_h;
    }
    img
}

pub fn render_generator(g: &Generator, w: u32, h: u32) -> Rgba {
    let mut data = vec![0u8; (w * h * 4) as usize];
    match g {
        Generator::SolidColor(c) => {
            for p in data.chunks_exact_mut(4) {
                p.copy_from_slice(c);
            }
        }
        Generator::Gradient(a, b) => {
            for y in 0..h {
                for x in 0..w {
                    let t = (x as f32 / w.max(1) as f32 * 0.6 + y as f32 / h.max(1) as f32 * 0.4).clamp(0.0, 1.0);
                    let i = ((y * w + x) * 4) as usize;
                    for k in 0..4 {
                        data[i + k] = (a[k] as f32 * (1.0 - t) + b[k] as f32 * t) as u8;
                    }
                }
            }
        }
        Generator::ColorBars => {
            let top: [[u8; 3]; 7] = [
                [192, 192, 192],
                [192, 192, 0],
                [0, 192, 192],
                [0, 192, 0],
                [192, 0, 192],
                [192, 0, 0],
                [0, 0, 192],
            ];
            let mid: [[u8; 3]; 7] = [
                [0, 0, 192],
                [19, 19, 19],
                [192, 0, 192],
                [19, 19, 19],
                [0, 192, 192],
                [19, 19, 19],
                [192, 192, 192],
            ];
            for y in 0..h {
                for x in 0..w {
                    let band = ((x * 7) / w.max(1)).min(6) as usize;
                    let c = if y < h * 2 / 3 {
                        top[band]
                    } else if y < h * 3 / 4 {
                        mid[band]
                    } else {
                        let q = (x * 6) / w.max(1);
                        match q {
                            0 => [0, 33, 76],
                            1 => [255, 255, 255],
                            2 => [50, 0, 106],
                            _ => [19, 19, 19],
                        }
                    };
                    let i = ((y * w + x) * 4) as usize;
                    data[i..i + 3].copy_from_slice(&c);
                    data[i + 3] = 255;
                }
            }
        }
        Generator::Text {
            text,
            size_pct,
            color,
            background,
        } => return render_text(text, *size_pct, *color, *background, w, h),
        Generator::Subtitle { text, size_pct } => return render_subtitle(text, *size_pct, w, h),
        Generator::Title { .. } => return render_title(g, w, h, 1e9),
        Generator::Tone { .. } | Generator::DemoMusic { .. } | Generator::Adjustment => {}
    }
    Rgba { w, h, data }
}

/// Animated title at local time `lt` (seconds since the event start).
pub fn render_title(g: &Generator, w: u32, h: u32, lt: f64) -> Rgba {
    let Generator::Title { text, size_pct, color, anim, anim_ms } = g else {
        return render_generator(g, w, h);
    };
    let dur = (*anim_ms as f64 / 1000.0).max(0.05);
    let u = (lt / dur).clamp(0.0, 1.0) as f32;
    // ease-out cubic
    let e = 1.0 - (1.0 - u).powi(3);
    let shown: String = if *anim == TitleAnim::Typewriter {
        let n = text.chars().count();
        text.chars().take(((n as f32) * u).ceil() as usize).collect()
    } else {
        text.clone()
    };
    let mut img = render_text(&shown, *size_pct, *color, [0, 0, 0, 0], w, h);
    match anim {
        TitleAnim::None | TitleAnim::Typewriter => {}
        TitleAnim::FadeIn => {
            for p in img.data.chunks_exact_mut(4) {
                p[3] = (p[3] as f32 * e) as u8;
            }
        }
        TitleAnim::SlideUp | TitleAnim::Bounce => {
            let off = if *anim == TitleAnim::SlideUp {
                (1.0 - e) * h as f32 * 0.25
            } else {
                // damped bounce falling from above
                let b = (1.0 - u) * (u * 12.0).cos().abs();
                -(b * h as f32 * 0.3)
            };
            let dy = off.round() as i64;
            let mut out = vec![0u8; img.data.len()];
            for y in 0..h as i64 {
                let sy = y - dy;
                if sy >= 0 && sy < h as i64 {
                    let row = (w * 4) as usize;
                    out[y as usize * row..(y as usize + 1) * row].copy_from_slice(&img.data[sy as usize * row..(sy as usize + 1) * row]);
                }
            }
            img.data = out;
            if *anim == TitleAnim::SlideUp {
                for p in img.data.chunks_exact_mut(4) {
                    p[3] = (p[3] as f32 * e) as u8;
                }
            }
        }
        TitleAnim::Zoom => {
            let s = 0.3 + 0.7 * e;
            let mut out = vec![0u8; img.data.len()];
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            for y in 0..h {
                for x in 0..w {
                    let sx = ((x as f32 - cx) / s + cx) as i64;
                    let sy = ((y as f32 - cy) / s + cy) as i64;
                    if sx >= 0 && sy >= 0 && sx < w as i64 && sy < h as i64 {
                        let si = ((sy as u32 * w + sx as u32) * 4) as usize;
                        let di = ((y * w + x) * 4) as usize;
                        out[di..di + 4].copy_from_slice(&img.data[si..si + 4]);
                    }
                }
            }
            for p in out.chunks_exact_mut(4) {
                p[3] = (p[3] as f32 * e) as u8;
            }
            img.data = out;
        }
    }
    img
}

// ---------------------------------------------------------------------------
// Thumbnails

pub const THUMB_H: u32 = 54;

pub struct Thumbs {
    /// (time, image) pairs for a video, or a single still.
    pub frames: Vec<(f64, Rgba)>,
}

pub fn thumb_size(m: &Media) -> (u32, u32) {
    let (w, h) = if m.width > 0 && m.height > 0 {
        (m.width, m.height)
    } else {
        (16, 9)
    };
    let tw = ((THUMB_H as f32 * w as f32 / h as f32).round() as u32).clamp(16, 200) & !1;
    (tw, THUMB_H)
}

pub fn video_thumbnails(path: &str, m: &Media) -> Thumbs {
    let (tw, th) = thumb_size(m);
    let count = 48.0;
    let step = (m.duration / count).max(0.5);
    let mut frames = vec![];
    if let Ok(mut child) = command("ffmpeg")
        .args(["-v", "quiet", "-i", path, "-an", "-sn"])
        .args(["-vf", &format!("fps=1/{:.4},scale={}:{}", step, tw, th)])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(out) = child.stdout.as_mut() {
            let mut i = 0;
            loop {
                let mut buf = vec![0u8; (tw * th * 4) as usize];
                if out.read_exact(&mut buf).is_err() {
                    break;
                }
                frames.push((i as f64 * step, Rgba { w: tw, h: th, data: buf }));
                i += 1;
            }
        }
        let _ = child.wait();
    }
    Thumbs { frames }
}

/// Shared store filled by background loaders.
#[derive(Default)]
pub struct MediaStore {
    pub audio: Mutex<HashMap<Id, Arc<AudioData>>>,
    pub thumbs: Mutex<HashMap<Id, Arc<Thumbs>>>,
    pub images: Mutex<HashMap<Id, Arc<Rgba>>>,
    pub loading: Mutex<std::collections::HashSet<Id>>,
}

impl MediaStore {
    /// Start loading what a media needs (audio, thumbnails, image).
    pub fn ensure(self: &Arc<Self>, m: &Media, ctx: &egui::Context) {
        {
            let mut l = self.loading.lock().unwrap();
            if l.contains(&m.id) {
                return;
            }
            l.insert(m.id);
        }
        let me = self.clone();
        let m = m.clone();
        let ctx = ctx.clone();
        match &m.source {
            MediaSource::Generated(g) => {
                if g.is_audio() {
                    let a = generate_audio(g, m.duration.max(1.0));
                    me.audio.lock().unwrap().insert(m.id, Arc::new(a));
                } else {
                    let (tw, th) = thumb_size(&Media { width: 16, height: 9, ..m.clone() });
                    let img = render_generator(g, tw * 4, th * 4);
                    let small = resize_nearest(&img, tw, th);
                    me.thumbs.lock().unwrap().insert(m.id, Arc::new(Thumbs { frames: vec![(0.0, small)] }));
                }
            }
            MediaSource::Image(path) => {
                let path = path.clone();
                std::thread::spawn(move || {
                    if let Some(img) = load_image(&path) {
                        let (tw, th) = thumb_size(&m);
                        let small = resize_nearest(&img, tw, th);
                        me.thumbs.lock().unwrap().insert(m.id, Arc::new(Thumbs { frames: vec![(0.0, small)] }));
                        me.images.lock().unwrap().insert(m.id, Arc::new(img));
                        ctx.request_repaint();
                    }
                });
            }
            MediaSource::File(path) => {
                let path = path.clone();
                if m.has_audio {
                    let me2 = me.clone();
                    let p2 = path.clone();
                    let ctx2 = ctx.clone();
                    let id = m.id;
                    std::thread::spawn(move || {
                        if let Some(a) = decode_audio(&p2) {
                            me2.audio.lock().unwrap().insert(id, Arc::new(a));
                            ctx2.request_repaint();
                        }
                    });
                }
                if m.has_video {
                    std::thread::spawn(move || {
                        let t = video_thumbnails(&path, &m);
                        me.thumbs.lock().unwrap().insert(m.id, Arc::new(t));
                        ctx.request_repaint();
                    });
                }
            }
            MediaSource::Nested(path) => {
                let path = path.clone();
                std::thread::spawn(move || {
                    let Some(p) = load_nested(&path) else { return };
                    // audio: mix the nested project down
                    let sub = Arc::new(MediaStore::default());
                    for nm in &p.media {
                        let a = match &nm.source {
                            MediaSource::File(f) if nm.has_audio => decode_audio(f),
                            MediaSource::Generated(g) if g.is_audio() => Some(generate_audio(g, nm.duration.max(1.0))),
                            _ => None,
                        };
                        if let Some(a) = a {
                            sub.audio.lock().unwrap().insert(nm.id, Arc::new(a));
                        }
                    }
                    let mix = crate::audio::build_mix(&p, &sub, true);
                    let len = p.end_time().max(0.1);
                    let mut buf = vec![0f32; (len * AUDIO_RATE as f64) as usize * 2];
                    crate::audio::mix_into(&mix, 0.0, AUDIO_RATE as f64, &mut buf);
                    let pcm: Vec<i16> = buf.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                    me.audio.lock().unwrap().insert(m.id, Arc::new(AudioData::from_samples(pcm)));
                    // thumbnail: one picture of the nested timeline
                    let (tw, th) = thumb_size(&m);
                    let mut comp = crate::compositor::Compositor::default();
                    let mut src = crate::compositor::ExactSource::new(p.fps);
                    let img = comp.compose(&p, (len * 0.25).min(2.0), tw, th, true, &mut src);
                    me.thumbs.lock().unwrap().insert(m.id, Arc::new(Thumbs { frames: vec![(0.0, Rgba { w: tw, h: th, data: img })] }));
                    ctx.request_repaint();
                });
            }
        }
    }

    pub fn audio(&self, id: Id) -> Option<Arc<AudioData>> {
        self.audio.lock().unwrap().get(&id).cloned()
    }
    pub fn thumbs(&self, id: Id) -> Option<Arc<Thumbs>> {
        self.thumbs.lock().unwrap().get(&id).cloned()
    }
    pub fn image(&self, id: Id) -> Option<Arc<Rgba>> {
        self.images.lock().unwrap().get(&id).cloned()
    }
}

/// A subtitle: white text with a dark box, near the bottom of the frame.
pub fn render_subtitle(text: &str, size_pct: u32, w: u32, h: u32) -> Rgba {
    use ab_glyph::{Font, ScaleFont};
    let mut img = Rgba { w, h, data: vec![0u8; (w * h * 4) as usize] };
    let Some(font) = font() else { return img };
    let px = (h as f32 * size_pct as f32 / 100.0).max(4.0);
    let scaled = font.as_scaled(px);
    let lines: Vec<&str> = text.lines().collect();
    let line_h = scaled.height() + scaled.line_gap();
    let total_h = line_h * lines.len() as f32;
    let bottom = h as f32 * 0.9;
    let top = bottom - total_h;
    let widths: Vec<f32> = lines
        .iter()
        .map(|l| l.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum())
        .collect();
    let maxw = widths.iter().cloned().fold(0.0, f32::max);
    let pad = px * 0.35;
    let (bx0, bx1) = (((w as f32 - maxw) / 2.0 - pad).max(0.0), ((w as f32 + maxw) / 2.0 + pad).min(w as f32));
    let (by0, by1) = ((top - pad * 0.6).max(0.0), (bottom + pad * 0.6).min(h as f32));
    for y in by0 as u32..by1 as u32 {
        for x in bx0 as u32..bx1 as u32 {
            let i = ((y * w + x) * 4) as usize;
            img.data[i..i + 4].copy_from_slice(&[0, 0, 0, 165]);
        }
    }
    let mut y0 = top + scaled.ascent();
    for (li, line) in lines.iter().enumerate() {
        let mut x = (w as f32 - widths[li]) / 2.0;
        for c in line.chars() {
            let gid = font.glyph_id(c);
            let g = gid.with_scale_and_position(px, ab_glyph::point(x, y0));
            if let Some(og) = font.outline_glyph(g) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let ix = b.min.x as i32 + gx as i32;
                    let iy = b.min.y as i32 + gy as i32;
                    if ix >= 0 && iy >= 0 && (ix as u32) < w && (iy as u32) < h {
                        let i = ((iy as u32 * w + ix as u32) * 4) as usize;
                        for k in 0..3 {
                            img.data[i + k] = (img.data[i + k] as f32 * (1.0 - cov) + 255.0 * cov) as u8;
                        }
                        img.data[i + 3] = (img.data[i + 3] as f32 + (255.0 - img.data[i + 3] as f32) * cov) as u8;
                    }
                });
            }
            x += scaled.h_advance(gid);
        }
        y0 += line_h;
    }
    img
}

/// Loads a project file used as nested media (cached by path and date).
pub fn load_nested(path: &str) -> Option<Arc<Project>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (std::time::SystemTime, Arc<Project>)>>> = OnceLock::new();
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((t, p)) = cache.lock().unwrap().get(path) {
        if *t == modified {
            return Some(p.clone());
        }
    }
    let s = std::fs::read_to_string(path).ok()?;
    let p: Project = serde_json::from_str(&s).ok()?;
    let p = Arc::new(p);
    cache.lock().unwrap().insert(path.to_string(), (modified, p.clone()));
    Some(p)
}

/// Media entry for a nested project file.
pub fn probe_nested(path: &str) -> Result<Media, String> {
    let p = load_nested(path).ok_or_else(|| format!("Projet illisible : {path}"))?;
    let has_audio = p.tracks.iter().any(|t| t.kind == TrackKind::Audio && !t.events.is_empty());
    Ok(Media::basic(file_name(path), MediaSource::Nested(path.to_string()), true, has_audio, p.end_time(), p.width, p.height, p.fps))
}

/// Proxy files used by the preview (original path → proxy path).
fn proxy_map() -> &'static std::sync::RwLock<HashMap<String, String>> {
    static M: OnceLock<std::sync::RwLock<HashMap<String, String>>> = OnceLock::new();
    M.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

pub fn proxy_for(path: &str) -> Option<String> {
    proxy_map().read().ok()?.get(path).cloned()
}

/// Sets the proxies used by the preview (empty map = originals).
pub fn set_proxies(map: HashMap<String, String>) {
    if let Ok(mut m) = proxy_map().write() {
        *m = map;
    }
}

/// Creates a small preview copy (proxy) of a video file with ffmpeg:
/// 640 pixels wide, H.264 with frequent key frames for fast seeking.
pub fn make_proxy(path: &str, out_dir: &std::path::Path) -> Result<String, String> {
    let _ = std::fs::create_dir_all(out_dir);
    let stem = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "media".into());
    let hash = path.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211));
    let out = out_dir.join(format!("{stem}-{hash:016x}.proxy.mp4"));
    if out.exists() {
        return Ok(out.to_string_lossy().to_string());
    }
    let st = command("ffmpeg")
        .args(["-y", "-v", "error", "-i", path, "-vf", "scale=640:-2", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26", "-g", "15", "-c:a", "aac", "-b:a", "96k"])
        .arg(&out)
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(out.to_string_lossy().to_string())
    } else {
        Err(format!("ffmpeg n'a pas pu créer le proxy de {path}"))
    }
}
