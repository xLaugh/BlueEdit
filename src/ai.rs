//! Local AI helpers: speech transcription with whisper.cpp (through the
//! `whisper` filter built into ffmpeg) and automatic trimming of silences and
//! hesitations, following CrispTrim's algorithm (github.com/xLaugh/CrispTrim:
//! silence.py and cutter.py defaults).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ---- CrispTrim defaults (cutter.py CutParams / silence.py) ----
pub const FRAME_SECS: f64 = 0.01; // silence.py FRAME
pub const MIN_SOUND_SECS: f64 = 0.08; // silence.py min_sound
pub const DEFAULT_MIN_SILENCE: f64 = 0.5; // cli --min-silence
pub const DEFAULT_MARGIN: f64 = 0.15; // CutParams.margin
pub const DEFAULT_MIN_KEEP: f64 = 0.25; // CutParams.min_keep
pub const FILLER_PAD: f64 = 0.03; // CutParams.filler_pad
pub const MAX_FILLER_EXTEND: f64 = 1.0; // CutParams.max_filler_extend
/// French hesitations named by CrispTrim ("euh", "heu", "hum", "mmh") plus
/// spellings the transcription model produces ("eu", "hmm"...).
pub const FILLERS: &[&str] = &["euh", "heu", "eu", "hum", "hmm", "mmh", "mh", "hm", "euhm", "uh", "um", "erm"];

fn command(program: &str) -> Command {
    crate::platform::command(program)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

// ---------------------------------------------------------------- models

pub fn models_dir() -> PathBuf {
    crate::plugins::data_dir().join("models")
}

/// whisper.cpp models offered for download (official ggml conversions).
pub const DOWNLOADS: &[(&str, &str, &str)] = &[
    ("Base (142 Mo, rapide)", "ggml-base.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"),
    ("Small (466 Mo, recommandé)", "ggml-small.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"),
    ("Medium (1,5 Go, précis)", "ggml-medium.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin"),
];

/// Model files already on this computer.
pub fn find_models() -> Vec<PathBuf> {
    let mut dirs = vec![models_dir()];
    for d in crate::platform::other_app_data("AutoTrim") {
        dirs.push(d.join("whisper"));
    }
    for d in crate::platform::other_app_data("CrispTrim") {
        dirs.push(d.join("models"));
    }
    let mut out = vec![];
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                if n.starts_with("ggml") && n.ends_with(".bin") && !n.contains("silero") && !n.contains("vad") {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[derive(Default)]
pub struct Job {
    pub progress: f32,
    pub message: String,
    pub done: bool,
    pub error: Option<String>,
    pub segments: Vec<Segment>,
    pub words: Vec<Segment>,
    pub path: Option<PathBuf>,
}

pub type JobHandle = Arc<Mutex<Job>>;

pub fn download_model(file: &str, url: &str, cancel: Arc<AtomicBool>) -> JobHandle {
    let job: JobHandle = Arc::new(Mutex::new(Job {
        message: "Téléchargement du modèle…".into(),
        ..Default::default()
    }));
    let j = job.clone();
    let file = file.to_string();
    let url = url.to_string();
    std::thread::spawn(move || {
        let r = (|| -> Result<PathBuf, String> {
            std::fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;
            let dest = models_dir().join(&file);
            let part = dest.with_extension("part");
            let resp = ureq::get(&url).call().map_err(|e| e.to_string())?;
            let total: f64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let mut reader = resp.into_reader();
            let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 1 << 16];
            let mut got = 0f64;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    drop(f);
                    let _ = std::fs::remove_file(&part);
                    return Err("Téléchargement annulé.".into());
                }
                let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                got += n as f64;
                let mut jj = j.lock().unwrap();
                if total > 0.0 {
                    jj.progress = (got / total) as f32;
                }
                jj.message = format!("Téléchargement : {:.0} Mo", got / 1e6);
            }
            drop(f);
            std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
            Ok(dest)
        })();
        let mut jj = j.lock().unwrap();
        jj.done = true;
        match r {
            Ok(p) => {
                jj.message = "Modèle installé.".into();
                jj.path = Some(p);
            }
            Err(e) => jj.error = Some(e),
        }
    });
    job
}

// ---------------------------------------------------------- transcription

fn filter_escape(p: &Path) -> String {
    // inside a quoted filter option, ':' and '\' and '\'' must be escaped
    p.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

fn run_whisper(wav: &Path, model: &Path, lang: &str, max_len: u32, gpu: bool) -> Result<Vec<Segment>, String> {
    let dest = std::env::temp_dir().join(format!("blue_whisper_{}_{}.json", std::process::id(), max_len));
    let _ = std::fs::remove_file(&dest);
    let filter = format!(
        "whisper=model='{}':language={}:use_gpu={}:format=json:destination='{}':queue=15:max_len={}",
        filter_escape(model),
        if lang.is_empty() { "auto" } else { lang },
        if gpu { "true" } else { "false" },
        filter_escape(&dest),
        max_len
    );
    let out = command("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(wav)
        .args(["-af", &filter, "-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg introuvable : {e}"))?;
    let text = std::fs::read_to_string(&dest).unwrap_or_default();
    let _ = std::fs::remove_file(&dest);
    if !out.status.success() && text.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("transcription impossible : {}", err.lines().last().unwrap_or("erreur inconnue")));
    }
    let mut segs = vec![];
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
        let t = v["text"].as_str().unwrap_or("").trim().to_string();
        if t.is_empty() {
            continue;
        }
        segs.push(Segment {
            start: v["start"].as_f64().unwrap_or(0.0) / 1000.0,
            end: v["end"].as_f64().unwrap_or(0.0) / 1000.0,
            text: t,
        });
    }
    Ok(segs)
}

/// Transcribes a WAV file. `max_len` = max characters per segment
/// (1 gives one word per segment, used to find hesitations).
pub fn transcribe(wav: &Path, model: &Path, lang: &str, max_len: u32) -> Result<Vec<Segment>, String> {
    match run_whisper(wav, model, lang, max_len, true) {
        Ok(s) if !s.is_empty() => Ok(s),
        _ => run_whisper(wav, model, lang, max_len, false),
    }
}

/// Runs transcription in the background: subtitle lines and/or words.
pub fn transcribe_job(wav: PathBuf, model: PathBuf, lang: String, lines: bool, words: bool) -> JobHandle {
    let job: JobHandle = Arc::new(Mutex::new(Job {
        message: "Transcription en cours (cela peut prendre un moment)…".into(),
        ..Default::default()
    }));
    let j = job.clone();
    std::thread::spawn(move || {
        let mut err = None;
        if lines {
            match transcribe(&wav, &model, &lang, 42) {
                Ok(s) => j.lock().unwrap().segments = s,
                Err(e) => err = Some(e),
            }
            j.lock().unwrap().progress = if words { 0.5 } else { 1.0 };
        }
        if words && err.is_none() {
            match transcribe(&wav, &model, &lang, 1) {
                Ok(s) => j.lock().unwrap().words = s,
                Err(e) => err = Some(e),
            }
        }
        let _ = std::fs::remove_file(&wav);
        let mut jj = j.lock().unwrap();
        jj.done = true;
        jj.progress = 1.0;
        jj.message = "Transcription terminée.".into();
        jj.error = err;
    });
    job
}

pub fn to_srt(segs: &[Segment]) -> String {
    let ts = |t: f64| {
        let ms = (t.max(0.0) * 1000.0).round() as u64;
        format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
    };
    let mut s = String::new();
    for (i, seg) in segs.iter().enumerate() {
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, ts(seg.start), ts(seg.end), seg.text));
    }
    s
}

// --------------------------------------------------------------- auto trim

#[derive(Clone, Debug, PartialEq)]
pub struct TrimParams {
    /// None = automatic threshold.
    pub threshold_db: Option<f32>,
    pub min_silence: f64,
    pub margin: f64,
    pub min_keep: f64,
    pub cut_fillers: bool,
    pub extra_fillers: String,
}

impl Default for TrimParams {
    fn default() -> Self {
        TrimParams {
            threshold_db: None,
            min_silence: DEFAULT_MIN_SILENCE,
            margin: DEFAULT_MARGIN,
            min_keep: DEFAULT_MIN_KEEP,
            cut_fillers: true,
            extra_fillers: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegionKind {
    Keep,
    Silence,
    Filler,
}

#[derive(Clone, Debug, Default)]
pub struct TrimResult {
    pub keep: Vec<(f64, f64)>,
    pub regions: Vec<(f64, f64, RegionKind)>,
    pub threshold_db: f32,
    pub duration: f64,
    pub fillers: usize,
    /// dB level per 10 ms frame, for drawing.
    pub levels: Vec<f32>,
}

impl TrimResult {
    pub fn kept(&self) -> f64 {
        self.keep.iter().map(|(a, b)| b - a).sum()
    }
}

fn percentile(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (s.len() - 1) as f32).round() as usize;
    s[idx.min(s.len() - 1)]
}

/// dB per 10 ms frame of interleaved stereo i16 at `rate`, over [t0, t1).
pub fn frame_levels(samples: &[i16], rate: u32, t0: f64, t1: f64) -> Vec<f32> {
    let per = (rate as f64 * FRAME_SECS) as usize;
    let frames_total = samples.len() / 2;
    let a = ((t0 * rate as f64) as usize).min(frames_total);
    let b = ((t1 * rate as f64) as usize).min(frames_total);
    let mut out = vec![];
    let mut i = a;
    while i < b {
        let e = (i + per).min(b);
        let mut acc = 0f64;
        for f in i..e {
            let m = (samples[f * 2] as f64 + samples[f * 2 + 1] as f64) / 2.0 / 32768.0;
            acc += m * m;
        }
        let rms = (acc / (e - i).max(1) as f64).sqrt();
        out.push((20.0 * (rms + 1e-10).log10()) as f32);
        i = e;
    }
    out
}

/// CrispTrim's silence.py: speech intervals and the threshold used.
pub fn speech_intervals(levels: &[f32], p: &TrimParams) -> (Vec<(f64, f64)>, f32) {
    let thr = match p.threshold_db {
        Some(t) => t,
        None => {
            if levels.is_empty() {
                -45.0
            } else {
                let floor = percentile(levels, 10.0).max(-80.0);
                let peak = percentile(levels, 95.0);
                (floor + 0.3 * (peak - floor)).clamp(-60.0, (peak - 6.0).max(-60.0))
            }
        }
    };
    let n = levels.len();
    let mut loud: Vec<bool> = levels.iter().map(|d| *d > thr).collect();
    // short pauses inside speech count as speech
    let min_sil = (p.min_silence / FRAME_SECS).round() as usize;
    let mut i = 0;
    while i < n {
        if !loud[i] {
            let s = i;
            while i < n && !loud[i] {
                i += 1;
            }
            if s > 0 && i < n && i - s < min_sil {
                for k in s..i {
                    loud[k] = true;
                }
            }
        } else {
            i += 1;
        }
    }
    // drop clicks shorter than min_sound
    let min_sound = (MIN_SOUND_SECS / FRAME_SECS).round() as usize;
    let mut out = vec![];
    let mut i = 0;
    while i < n {
        if loud[i] {
            let s = i;
            while i < n && loud[i] {
                i += 1;
            }
            if i - s >= min_sound {
                out.push((s as f64 * FRAME_SECS, i as f64 * FRAME_SECS));
            }
        } else {
            i += 1;
        }
    }
    (out, thr)
}

pub fn is_filler(word: &str, extra: &str) -> bool {
    let w: String = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect();
    if w.is_empty() {
        return false;
    }
    if FILLERS.contains(&w.as_str()) {
        return true;
    }
    // noises such as [cough], [yawn], breaths
    let raw = word.to_lowercase();
    if raw.starts_with('[') || raw.starts_with('(') {
        return true;
    }
    extra
        .split(|c: char| c == ',' || c.is_whitespace())
        .any(|x| !x.is_empty() && x.to_lowercase() == w)
}

fn merge(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out: Vec<(f64, f64)> = vec![];
    for (a, b) in v {
        if let Some(last) = out.last_mut() {
            if a <= last.1 {
                last.1 = last.1.max(b);
                continue;
            }
        }
        out.push((a, b));
    }
    out
}

fn subtract(keep: &[(f64, f64)], cut: (f64, f64)) -> Vec<(f64, f64)> {
    let mut out = vec![];
    for &(a, b) in keep {
        if cut.1 <= a || cut.0 >= b {
            out.push((a, b));
            continue;
        }
        if cut.0 > a {
            out.push((a, cut.0));
        }
        if cut.1 < b {
            out.push((cut.1, b));
        }
    }
    out
}

/// CrispTrim's cutter.py: kept segments = speech + margins − hesitations.
/// `words` times are relative to the analysed range.
pub fn compute(levels: Vec<f32>, duration: f64, words: &[Segment], p: &TrimParams) -> TrimResult {
    let (speech, thr) = speech_intervals(&levels, p);
    let mut keep = merge(
        speech
            .iter()
            .map(|(a, b)| ((a - p.margin).max(0.0), (b + p.margin).min(duration)))
            .collect(),
    );
    let mut fillers = 0;
    let mut filler_cuts = vec![];
    if p.cut_fillers {
        for w in words.iter().filter(|w| is_filler(&w.text, &p.extra_fillers)) {
            let a = (w.start - FILLER_PAD).max(w.start - MAX_FILLER_EXTEND).max(0.0);
            let b = (w.end + FILLER_PAD).min(w.end + MAX_FILLER_EXTEND).min(duration);
            if b > a {
                keep = subtract(&keep, (a, b));
                filler_cuts.push((a, b));
                fillers += 1;
            }
        }
    }
    keep.retain(|(a, b)| b - a >= p.min_keep);
    // regions for display
    let mut regions = vec![];
    let mut t = 0.0;
    for &(a, b) in &keep {
        if a > t {
            let kind = if filler_cuts.iter().any(|(fa, fb)| *fa < a && *fb > t) {
                RegionKind::Filler
            } else {
                RegionKind::Silence
            };
            regions.push((t, a, kind));
        }
        regions.push((a, b, RegionKind::Keep));
        t = b;
    }
    if t < duration {
        regions.push((t, duration, RegionKind::Silence));
    }
    TrimResult {
        keep,
        regions,
        threshold_db: thr,
        duration,
        fillers,
        levels,
    }
}
