//! Interchange with other editors: CMX 3600 EDL, FCPXML 1.9, Final Cut Pro 7
//! XML (xmeml version 5, read by Premiere Pro and DaVinci Resolve), SRT
//! subtitles, and the Broadcast WAV time reference.
//!
//! Exports write XML by hand; imports parse with `roxmltree` into a small
//! intermediate list of clips (`Doc`) which `apply` turns into media, tracks
//! and events of the project.

#![allow(dead_code)]

use crate::model::*;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

type Node<'a, 'i> = roxmltree::Node<'a, 'i>;

const INF: (f64, f64) = (f64::NEG_INFINITY, f64::INFINITY);

macro_rules! w {
    ($($t:tt)*) => {{
        let _ = write!($($t)*);
    }};
}

// ------------------------------------------------------------------ helpers

/// Escapes text for XML content and attribute values.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\n' => o.push_str("&#10;"),
            '\t' => o.push_str("&#9;"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

fn nominal(fps: f64) -> i64 {
    (fps.round() as i64).max(1)
}

/// 23.976, 29.97, 59.94... (rate = nominal * 1000 / 1001).
fn is_ntsc(fps: f64) -> bool {
    let n = fps.round();
    n > 0.0 && (fps - n).abs() > 0.001 && (fps - n * 1000.0 / 1001.0).abs() < 0.01
}

/// Same rule as `model::timecode`: drop-frame for 29.97 and 59.94.
fn drop_capable(fps: f64) -> bool {
    let n = nominal(fps);
    (fps - n as f64).abs() > 0.01 && (n == 30 || n == 60)
}

fn to_frames(t: f64, fps: f64) -> i64 {
    (t * fps).round() as i64
}

/// Timecode of a frame number (drop-frame aware, via `model::timecode`).
fn tc(frames: i64, fps: f64) -> String {
    timecode(frames.max(0) as f64 / fps, fps)
}

/// Parses "hh:mm:ss:ff" (or ';' for drop-frame) into a frame number.
fn parse_tc(s: &str, fps: f64, df_hint: bool) -> Option<i64> {
    let parts: Vec<&str> = s.trim().split([':', ';', '.', ',']).collect();
    if parts.len() != 4 {
        return None;
    }
    let mut v = [0i64; 4];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        v[i] = p.parse().ok()?;
    }
    let n = nominal(fps);
    let mut f = ((v[0] * 60 + v[1]) * 60 + v[2]) * n + v[3];
    if drop_capable(fps) && (df_hint || s.contains(';')) {
        let drop = if n == 30 { 2 } else { 4 };
        let mins = v[0] * 60 + v[1];
        f -= drop * (mins - mins / 10);
    }
    Some(f)
}

/// Frame duration as a rational (numerator, denominator) in seconds.
fn frame_dur(fps: f64) -> (i64, i64) {
    if is_ntsc(fps) {
        (1001, nominal(fps) * 1000)
    } else if (fps - fps.round()).abs() < 1e-6 {
        (100, nominal(fps) * 100)
    } else {
        (1000, ((fps * 1000.0).round() as i64).max(1))
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}

/// FCPXML time value: "0s", "5s", "1001/30000s".
fn rational(num: i64, den: i64) -> String {
    if num == 0 {
        return "0s".into();
    }
    let g = gcd(num.abs(), den.abs());
    let (n, d) = (num / g, den / g);
    if d == 1 {
        format!("{}s", n)
    } else {
        format!("{}/{}s", n, d)
    }
}

fn parse_rational(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('s');
    if let Some((a, b)) = s.split_once('/') {
        let d: f64 = b.trim().parse().ok()?;
        if d == 0.0 {
            return None;
        }
        Some(a.trim().parse::<f64>().ok()? / d)
    } else {
        s.parse().ok()
    }
}

/// "C:\Vidéos\mon clip.mp4" -> "file:///C:/Vid%C3%A9os/mon%20clip.mp4".
fn file_url(path: &str) -> String {
    let p = path.replace('\\', "/");
    let mut out = if p.starts_with("//") {
        String::from("file:")
    } else if p.starts_with('/') {
        String::from("file://")
    } else {
        String::from("file:///")
    };
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => out.push(b as char),
            _ => w!(out, "%{:02X}", b),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// File URL (file:///, file://localhost/, file://server/) to a local path.
fn url_to_path(u: &str) -> String {
    let u = u.trim();
    let lower = u.to_ascii_lowercase();
    let (rest, unc) = if lower.starts_with("file://localhost/") {
        (&u["file://localhost".len()..], false)
    } else if lower.starts_with("file:///") {
        (&u["file://".len()..], false)
    } else if lower.starts_with("file://") {
        (&u["file://".len()..], true)
    } else if lower.starts_with("file:") {
        (&u["file:".len()..], false)
    } else {
        return u.to_string();
    };
    let d = percent_decode(rest);
    let b = d.as_bytes();
    if unc {
        format!("\\\\{}", d.replace('/', "\\"))
    } else if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        d[1..].replace('/', "\\")
    } else {
        d
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn file_stem(path: &str) -> String {
    let n = file_name(path);
    match n.rsplit_once('.') {
        Some((a, _)) if !a.is_empty() => a.to_string(),
        _ => n,
    }
}

fn ext_of(path: &str) -> String {
    let n = file_name(path);
    n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// (has_video, has_audio) guessed from the file extension.
fn guess_av(path: &str) -> (bool, bool) {
    const AUDIO: &[&str] = &["wav", "bwf", "mp3", "aac", "m4a", "flac", "ogg", "oga", "opus", "aif", "aiff", "wma", "ac3", "w64", "caf"];
    const IMAGE: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "tga", "webp", "psd", "exr", "dpx"];
    let e = ext_of(path);
    if AUDIO.contains(&e.as_str()) {
        (false, true)
    } else if IMAGE.contains(&e.as_str()) {
        (true, false)
    } else {
        (true, true)
    }
}

fn decode_text(b: &[u8]) -> String {
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&b[3..]).into_owned();
    }
    if b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]) {
        let le = b[0] == 0xFF;
        let u: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&u);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        // Latin-1 / Windows-1252 fallback (old EDL and SRT files)
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    let b = std::fs::read(path).map_err(|e| format!("Impossible de lire le fichier : {}", e))?;
    Ok(decode_text(&b))
}

fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, String> {
    let opt = roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    roxmltree::Document::parse_with_options(text, opt).map_err(|e| format!("Fichier XML invalide : {}", e))
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn ctext(n: Node<'_, '_>, name: &str) -> Option<String> {
    child(n, name).and_then(|c| c.text()).map(|s| s.trim().to_string())
}

fn cnum(n: Node<'_, '_>, name: &str) -> Option<f64> {
    ctext(n, name).and_then(|s| s.parse::<f64>().ok())
}

fn attr_time(n: Node<'_, '_>, name: &str) -> Option<f64> {
    n.attribute(name).and_then(parse_rational)
}

fn retimed(e: &Event) -> bool {
    (e.rate - 1.0).abs() > 1e-9 || e.reverse
}

/// Text of a text-like generator: (text, size in % of the height, colour).
fn gen_text(m: &Media) -> Option<(String, u32, [u8; 4])> {
    match &m.source {
        MediaSource::Generated(Generator::Text { text, size_pct, color, .. }) => Some((text.clone(), *size_pct, *color)),
        MediaSource::Generated(Generator::Title { text, size_pct, color, .. }) => Some((text.clone(), *size_pct, *color)),
        MediaSource::Generated(Generator::Subtitle { text, size_pct }) => Some((text.clone(), *size_pct, [255, 255, 255, 255])),
        _ => None,
    }
}

fn text_generator(text: String) -> Generator {
    Generator::Text { text, size_pct: 8, color: [255, 255, 255, 255], background: [0, 0, 0, 0] }
}

fn event_name(e: &Event, m: Option<&Media>) -> String {
    let n = if e.name.is_empty() { m.map(|m| m.name.clone()).unwrap_or_default() } else { e.name.clone() };
    n.replace(['\r', '\n'], " ")
}

/// An event placed in frames (export).
#[derive(Clone, Copy, Debug)]
struct Span {
    s: i64,
    e: i64,
    ti: usize,
    ei: usize,
}

fn ev<'a>(p: &'a Project, sp: &Span) -> &'a Event {
    &p.tracks[sp.ti].events[sp.ei]
}

fn track_spans(p: &Project, ti: usize, fps: f64, keep: &dyn Fn(&Media) -> bool) -> Vec<Span> {
    let mut v: Vec<Span> = p.tracks[ti]
        .events
        .iter()
        .enumerate()
        .filter_map(|(ei, e)| {
            let m = p.media(e.media)?;
            if !keep(m) {
                return None;
            }
            let s = to_frames(e.start, fps).max(0);
            let en = to_frames(e.end(), fps);
            (en > s).then_some(Span { s, e: en, ti, ei })
        })
        .collect();
    v.sort_by_key(|x| (x.s, x.e));
    v
}

/// Splits overlapping spans into stacked non-overlapping lanes.
fn sublanes(spans: Vec<Span>) -> Vec<Vec<Span>> {
    let mut lanes: Vec<Vec<Span>> = vec![];
    for sp in spans {
        match lanes.iter_mut().find(|l| l.last().map_or(true, |x| x.e <= sp.s)) {
            Some(l) => l.push(sp),
            None => lanes.push(vec![sp]),
        }
    }
    lanes
}

// ------------------------------------------------------------ import model

#[derive(Clone, Debug)]
enum Src {
    File(String),
    Gen(Generator),
}

#[derive(Clone, Debug)]
struct Clip {
    kind: TrackKind,
    /// Track key: tracks are created sorted by lane, highest first (top).
    lane: i64,
    name: String,
    src: Src,
    start: f64,
    length: f64,
    offset: f64,
    rate: f64,
    reverse: bool,
}

#[derive(Clone, Debug, Default)]
struct Hint {
    duration: f64,
    av: Option<(bool, bool)>,
    size: Option<(u32, u32)>,
    fps: Option<f64>,
}

#[derive(Default)]
struct Doc {
    clips: Vec<Clip>,
    markers: Vec<Marker>,
    fps: Option<f64>,
    size: Option<(u32, u32)>,
    hints: HashMap<String, Hint>,
}

/// Adds a clip, cut to the window `win` (timeline seconds).
#[allow(clippy::too_many_arguments)]
fn push_clip(doc: &mut Doc, kind: TrackKind, lane: i64, name: &str, src: Src, t: f64, dur: f64, offset: f64, rate: f64, reverse: bool, win: (f64, f64)) {
    let s = t.max(win.0).max(0.0);
    let e = (t + dur).min(win.1);
    if e - s <= 1e-6 {
        return;
    }
    let offset = if reverse { offset + ((t + dur) - e) * rate } else { offset + (s - t) * rate };
    doc.clips.push(Clip { kind, lane, name: name.to_string(), src, start: s, length: e - s, offset: offset.max(0.0), rate, reverse });
}

fn summary(n: usize, missing: usize, markers: usize) -> String {
    let mut s = if n == 1 { "1 événement importé".to_string() } else { format!("{} événements importés", n) };
    if markers > 0 {
        w!(s, ", {} marqueur{}", markers, if markers > 1 { "s" } else { "" });
    }
    if missing > 0 {
        let pl = if missing > 1 { "s" } else { "" };
        w!(s, " ({} média{} introuvable{})", missing, pl, pl);
    }
    s.push('.');
    s
}

/// Adds the media, tracks, events and markers of `doc` to the project.
fn apply(p: &mut Project, doc: Doc, probe: &dyn Fn(&str) -> Option<Media>) -> Result<String, String> {
    let clips: Vec<Clip> = doc.clips.into_iter().filter(|c| c.length > 1e-6).collect();
    if clips.is_empty() && doc.markers.is_empty() {
        return Err("Aucun événement trouvé dans le fichier.".into());
    }
    // An empty project takes the format of the document.
    if p.tracks.iter().all(|t| t.events.is_empty()) {
        if let Some(f) = doc.fps.filter(|f| *f > 0.0) {
            p.fps = exact_fps(f);
        }
        if let Some((wd, ht)) = doc.size.filter(|s| s.0 > 0 && s.1 > 0) {
            p.width = wd;
            p.height = ht;
        }
    }
    // media used by the clips
    let mut extent: HashMap<String, f64> = HashMap::new();
    for c in &clips {
        if let Src::File(path) = &c.src {
            let e = extent.entry(path.clone()).or_insert(0.0);
            *e = e.max(c.offset + c.length * c.rate.abs());
        }
    }
    let mut ids: HashMap<String, Id> = HashMap::new();
    let mut missing = 0;
    for c in &clips {
        let Src::File(path) = &c.src else { continue };
        if ids.contains_key(path) {
            continue;
        }
        if let Some(m) = p.media.iter().find(|m| m.path() == Some(path.as_str())) {
            ids.insert(path.clone(), m.id);
            continue;
        }
        let hint = doc.hints.get(path).cloned().unwrap_or_default();
        let exists = Path::new(path).exists();
        let probed = if exists { probe(path) } else { None };
        let m = match probed {
            Some(mut m) => {
                if m.name.is_empty() {
                    m.name = file_name(path);
                }
                m
            }
            None => {
                if !exists {
                    missing += 1;
                }
                let (hv, ha) = hint.av.filter(|a| a.0 || a.1).unwrap_or_else(|| guess_av(path));
                let dur = hint.duration.max(extent.get(path).copied().unwrap_or(0.0));
                let (wd, ht) = hint.size.unwrap_or((p.width, p.height));
                let fps = if hv { hint.fps.unwrap_or(p.fps) } else { 0.0 };
                Media::basic(file_name(path), MediaSource::File(path.clone()), hv, ha, dur, wd, ht, fps)
            }
        };
        let id = p.add_media(m);
        ids.insert(path.clone(), id);
    }
    // tracks: video above the existing audio tracks, audio at the end
    let mut vl: Vec<i64> = clips.iter().filter(|c| c.kind == TrackKind::Video).map(|c| c.lane).collect();
    let mut al: Vec<i64> = clips.iter().filter(|c| c.kind == TrackKind::Audio).map(|c| c.lane).collect();
    for l in [&mut vl, &mut al] {
        l.sort_unstable_by(|a, b| b.cmp(a));
        l.dedup();
    }
    let insert_at = p.tracks.iter().position(|t| t.kind == TrackKind::Audio).unwrap_or(p.tracks.len());
    let mut tracks: HashMap<(bool, i64), Id> = HashMap::new();
    for (k, lane) in vl.iter().enumerate() {
        let idx = p.add_track(TrackKind::Video, Some(insert_at + k));
        tracks.insert((true, *lane), p.tracks[idx].id);
    }
    for lane in &al {
        let idx = p.add_track(TrackKind::Audio, None);
        tracks.insert((false, *lane), p.tracks[idx].id);
    }
    // events
    let mut count = 0;
    for c in clips {
        let media = match &c.src {
            Src::File(path) => ids.get(path).and_then(|id| p.media(*id)).cloned(),
            Src::Gen(g) => {
                let name = if c.name.is_empty() { g.label() } else { c.name.clone() };
                let m = Media::basic(name, MediaSource::Generated(g.clone()), !g.is_audio(), g.is_audio(), 0.0, p.width, p.height, p.fps);
                let id = p.add_media(m);
                p.media(id).cloned()
            }
        };
        let Some(media) = media else { continue };
        let mut e = p.new_event(&media, c.start.max(0.0), c.length);
        if !c.name.is_empty() {
            e.name = c.name.clone();
        }
        e.offset = c.offset.max(0.0);
        e.rate = if c.rate > 0.0 && c.rate.is_finite() { c.rate } else { 1.0 };
        e.reverse = c.reverse;
        let Some(tid) = tracks.get(&(c.kind == TrackKind::Video, c.lane)).copied() else { continue };
        if let Some(t) = p.tracks.iter_mut().find(|t| t.id == tid) {
            t.events.push(e);
            count += 1;
        }
    }
    p.sort_events();
    let mut nm = 0;
    for m in doc.markers {
        if !p.markers.iter().any(|x| (x.time - m.time).abs() < 1e-3 && x.name == m.name) {
            p.markers.push(m);
            nm += 1;
        }
    }
    p.markers.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    Ok(summary(count, missing, nm))
}

// ----------------------------------------------------------------- import

/// Import a file according to its extension (.edl, .fcpxml, .xml (xmeml or
/// fcpxml detected by root element), .srt): adds media (files referenced by
/// path; missing files still added as File media with has_video/has_audio
/// guessed from extension, duration from the document) and new tracks/events
/// into `p`. `probe` lets the caller fill real media info for existing files
/// (return None if it can't). SRT adds a new video track named "Sous-titres".
/// Returns a French summary message or an Err with a French message.
pub fn import(path: &str, p: &mut Project, probe: &dyn Fn(&str) -> Option<Media>) -> Result<String, String> {
    let pth = Path::new(path);
    let ext = ext_of(path);
    let text = if pth.is_dir() && ext == "fcpxmld" { read_text(&pth.join("Info.fcpxml"))? } else { read_text(pth)? };
    match ext.as_str() {
        "edl" => {
            let doc = parse_edl(&text, exact_fps(p.fps));
            apply(p, doc, probe)
        }
        "srt" => import_srt(&text, p),
        "fcpxml" | "fcpxmld" | "xml" => {
            let d = parse_xml(&text)?;
            let doc = match d.root_element().tag_name().name() {
                "fcpxml" => parse_fcpxml(&d)?,
                "xmeml" => parse_xmeml(&d)?,
                other => return Err(format!("Document XML non reconnu (élément racine « {} »).", other)),
            };
            apply(p, doc, probe)
        }
        "" => Err("Format de fichier inconnu (pas d'extension).".into()),
        _ => Err(format!("Format non pris en charge : .{}", ext)),
    }
}

// -------------------------------------------------------------------- EDL

/// CMX 3600 EDL of the first video track and the audio tracks (A, A2, ...).
pub fn export_edl(p: &Project) -> String {
    let fps = exact_fps(p.fps);
    let df = drop_capable(fps);
    let nl = "\r\n";
    let t = |f: i64| tc(f, fps);
    let mut out = String::new();
    let title: String = p.name.chars().filter(|c| !c.is_control()).collect();
    w!(out, "TITLE: {}{nl}FCM: {}{nl}{nl}", title, if df { "DROP FRAME" } else { "NON-DROP FRAME" });
    let base = parse_tc(if df { "01:00:00;00" } else { "01:00:00:00" }, fps, df).unwrap_or(0);

    let mut targets: Vec<(usize, String)> = vec![];
    if let Some(v) = p.tracks.iter().position(|t| t.kind == TrackKind::Video) {
        targets.push((v, "V".into()));
    }
    let mut an = 0;
    for (i, tr) in p.tracks.iter().enumerate() {
        if tr.kind == TrackKind::Audio {
            an += 1;
            targets.push((i, if an == 1 { "A".into() } else { format!("A{}", an) }));
        }
    }
    let reel = |m: Option<&Media>| match m.map(|m| &m.source) {
        Some(MediaSource::Generated(_)) | None => "BL",
        _ => "AX",
    };
    let src_note = |m: Option<&Media>| m.and_then(|m| m.path()).map(|x| format!("* SOURCE FILE: {}", x));

    struct EdlEv {
        rec: i64,
        order: usize,
        lines: Vec<String>,
        notes: Vec<String>,
    }
    let mut evs: Vec<EdlEv> = vec![];
    for (order, (ti, tag)) in targets.iter().enumerate() {
        let spans = track_spans(p, *ti, fps, &|_| true);
        for (k, sp) in spans.iter().enumerate() {
            let e = ev(p, sp);
            let m = p.media(e.media);
            let next_s = spans.get(k + 1).map(|n| n.s).unwrap_or(i64::MAX);
            let rec_out = sp.e.min(next_s).max(sp.s + 1);
            let si = to_frames(e.offset, fps);
            let so = si + (rec_out - sp.s);
            let mut lines = vec![];
            let mut notes = vec![];
            match k.checked_sub(1).map(|j| spans[j]).filter(|pv| pv.e > sp.s) {
                Some(pv) => {
                    // dissolve: zero-length line of the outgoing clip, then the D line
                    let pe = ev(p, &pv);
                    let pm = p.media(pe.media);
                    let psrc = to_frames(pe.offset, fps) + ((sp.s - pv.s) as f64 * pe.rate).round() as i64;
                    let dur = (pv.e - sp.s).clamp(1, rec_out - sp.s);
                    lines.push(format!("{:<8} {:<5} {:<4} {:>3} {} {} {} {}", reel(pm), tag, "C", "", t(psrc), t(psrc), t(base + sp.s), t(base + sp.s)));
                    lines.push(format!("{:<8} {:<5} {:<4} {:03} {} {} {} {}", reel(m), tag, "D", dur, t(si), t(so), t(base + sp.s), t(base + rec_out)));
                    notes.push(format!("* FROM CLIP NAME: {}", event_name(pe, pm)));
                    notes.push(format!("* TO CLIP NAME: {}", event_name(e, m)));
                    notes.extend(src_note(pm));
                    notes.extend(src_note(m));
                }
                None => {
                    lines.push(format!("{:<8} {:<5} {:<4} {:>3} {} {} {} {}", reel(m), tag, "C", "", t(si), t(so), t(base + sp.s), t(base + rec_out)));
                    notes.push(format!("* FROM CLIP NAME: {}", event_name(e, m)));
                    notes.extend(src_note(m));
                }
            }
            if retimed(e) {
                let speed = e.rate * fps * if e.reverse { -1.0 } else { 1.0 };
                notes.insert(0, format!("M2   {:<8}       {:05.1}                {}", reel(m), speed, t(si)));
            }
            evs.push(EdlEv { rec: sp.s, order, lines, notes });
        }
    }
    evs.sort_by_key(|e| (e.rec, e.order));
    for (i, e) in evs.iter().enumerate() {
        for l in &e.lines {
            w!(out, "{:03}  {}{nl}", i + 1, l);
        }
        for n in &e.notes {
            w!(out, "{}{nl}", n);
        }
        out.push_str(nl);
    }
    out
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(s[prefix.len()..].trim())
    } else {
        None
    }
}

struct EdlLine {
    reel: String,
    track: String,
    trans: String,
    dur: i64,
    si: i64,
    so: i64,
    ri: i64,
    ro: i64,
}

#[derive(Default)]
struct EdlGroup {
    num: String,
    lines: Vec<EdlLine>,
    from: Option<String>,
    to: Option<String>,
    sources: Vec<String>,
    speed: Option<f64>,
}

/// Track keys (is_video, lane) of an EDL track field: V, A, A2, AA, B, AA/V...
fn edl_targets(t: &str) -> Vec<(bool, i64)> {
    let mut v = vec![];
    if t == "NONE" {
        return v;
    }
    if t.contains('V') || t == "B" {
        v.push((true, 0));
    }
    if t.starts_with('A') || t == "B" {
        let ch = t.chars().nth(1).and_then(|c| c.to_digit(10)).unwrap_or(1).max(1) as i64;
        v.push((false, -ch));
    }
    v
}

/// M2 speeds are written with one decimal: snap the rate to 1/100.
fn snap_rate(r: f64, speed: f64, fps: f64) -> f64 {
    let r2 = (r * 100.0).round() / 100.0;
    if r2 > 0.0 && (r2 * fps - speed).abs() <= 0.051 {
        r2
    } else {
        r
    }
}

fn parse_edl(text: &str, fps: f64) -> Doc {
    let mut df = false;
    for l in text.lines() {
        if let Some(r) = strip_ci(l.trim(), "FCM:") {
            let u = r.to_ascii_uppercase();
            df = u.contains("DROP") && !u.contains("NON");
            break;
        }
    }
    let mut groups: Vec<EdlGroup> = vec![];
    let mut semi = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('*') {
            let body = line.trim_start_matches('*').trim();
            let Some(g) = groups.last_mut() else { continue };
            if let Some(v) = strip_ci(body, "FROM CLIP NAME:") {
                g.from = Some(v.to_string());
            } else if let Some(v) = strip_ci(body, "TO CLIP NAME:") {
                g.to = Some(v.to_string());
            } else if let Some(v) = strip_ci(body, "SOURCE FILE:") {
                if !v.is_empty() {
                    g.sources.push(v.to_string());
                }
            }
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks[0].eq_ignore_ascii_case("M2") {
            if toks.len() >= 3 {
                if let (Some(g), Ok(v)) = (groups.last_mut(), toks[toks.len() - 2].parse::<f64>()) {
                    g.speed = Some(v);
                }
            }
            continue;
        }
        if toks.len() < 8 || !toks[0].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let n = toks.len();
        let tcs: Option<Vec<i64>> = toks[n - 4..].iter().map(|t| parse_tc(t, fps, df)).collect();
        let Some(tcs) = tcs else { continue };
        if toks[n - 4..].iter().any(|t| t.contains(';')) {
            semi = true;
        }
        let dur = if n >= 9 { toks[4].parse::<i64>().unwrap_or(0) } else { 0 };
        let l = EdlLine {
            reel: toks[1].to_string(),
            track: toks[2].to_ascii_uppercase(),
            trans: toks[3].to_ascii_uppercase(),
            dur,
            si: tcs[0],
            so: tcs[1],
            ri: tcs[2],
            ro: tcs[3],
        };
        match groups.last_mut() {
            Some(g) if g.num == toks[0] => g.lines.push(l),
            _ => groups.push(EdlGroup { num: toks[0].to_string(), lines: vec![l], ..Default::default() }),
        }
    }

    // record times start at the first record-in, or at 01:00:00:00
    let h1 = parse_tc(if df || semi { "01:00:00;00" } else { "01:00:00:00" }, fps, df || semi).unwrap_or(0);
    let min = groups.iter().flat_map(|g| g.lines.iter()).map(|l| l.ri).min().unwrap_or(0);
    let origin = if min >= h1 { h1 } else { min };

    let mut doc = Doc::default();
    let mut last: HashMap<(bool, i64), usize> = HashMap::new();
    for g in &groups {
        let k = g.lines.len();
        let mut names: Vec<Option<String>> = vec![None; k];
        if k == 1 {
            names[0] = g.from.clone().or_else(|| g.to.clone());
        } else {
            names[0] = g.from.clone();
            names[k - 1] = g.to.clone();
        }
        // SOURCE FILE comments go to the last lines (the incoming clip)
        let mut srcs: Vec<Option<String>> = vec![None; k];
        let ns = g.sources.len();
        if ns >= k {
            for (i, s) in srcs.iter_mut().enumerate() {
                *s = Some(g.sources[i].clone());
            }
        } else {
            for (j, s) in g.sources.iter().enumerate() {
                srcs[k - ns + j] = Some(s.clone());
            }
        }
        let speed_line = (0..k).rev().find(|&i| g.lines[i].ro > g.lines[i].ri);
        for (i, l) in g.lines.iter().enumerate() {
            let len = l.ro - l.ri;
            let targets = edl_targets(&l.track);
            let is_trans = l.trans.starts_with('D') || l.trans.starts_with('W') || l.trans.starts_with('K');
            if is_trans && l.dur > 0 {
                // the outgoing clip runs under the transition
                for key in &targets {
                    if let Some(&ci) = last.get(key) {
                        let c = &mut doc.clips[ci];
                        let end = (l.ri + l.dur - origin) as f64 / fps;
                        if end > c.start + c.length {
                            c.length = end - c.start;
                        }
                    }
                }
            }
            if len <= 0 {
                continue;
            }
            let start = (l.ri - origin) as f64 / fps;
            let length = len as f64 / fps;
            let (mut rate, mut reverse) = (1.0, false);
            match g.speed {
                Some(sp) if Some(i) == speed_line && sp.abs() > 1e-6 => {
                    rate = snap_rate(sp.abs() / fps, sp.abs(), fps);
                    reverse = sp < 0.0;
                }
                None => {
                    let sd = l.so - l.si;
                    if sd > 0 && (sd - len).abs() > 1 {
                        rate = sd as f64 / len as f64;
                    }
                }
                _ => {}
            }
            let bl = l.reel.eq_ignore_ascii_case("BL") || l.reel.eq_ignore_ascii_case("BLACK");
            let name = names[i].clone();
            let src = if bl {
                Src::Gen(Generator::SolidColor([0, 0, 0, 255]))
            } else {
                Src::File(srcs[i].clone().or_else(|| name.clone()).unwrap_or_else(|| l.reel.clone()))
            };
            let cname = name.unwrap_or_else(|| match &src {
                Src::File(x) => file_stem(x),
                Src::Gen(_) => "Noir".into(),
            });
            for key in targets {
                if bl && !key.0 {
                    continue; // black on audio = silence
                }
                let kind = if key.0 { TrackKind::Video } else { TrackKind::Audio };
                doc.clips.push(Clip { kind, lane: key.1, name: cname.clone(), src: src.clone(), start, length, offset: l.si as f64 / fps, rate, reverse });
                last.insert(key, doc.clips.len() - 1);
            }
        }
    }
    doc
}

// ----------------------------------------------------------------- FCPXML

struct FcpOut<'a> {
    p: &'a Project,
    fps: f64,
    fd: (i64, i64),
    assets: HashMap<Id, String>,
    title: Option<String>,
    ts: usize,
}

impl FcpOut<'_> {
    fn t(&self, f: i64) -> String {
        rational(f * self.fd.0, self.fd.1)
    }

    /// Local time of an element at its own start (its `start` attribute).
    fn local0(&self, sp: &Span) -> i64 {
        let e = ev(self.p, sp);
        if self.assets.contains_key(&e.media) && !retimed(e) {
            to_frames(e.offset, self.fps)
        } else {
            0
        }
    }

    fn clip(&mut self, sp: &Span, lane: Option<i64>, offset: i64, inner: &str) -> String {
        let p = self.p;
        let e = ev(p, sp);
        let Some(m) = p.media(e.media) else { return String::new() };
        let audio_track = p.tracks[sp.ti].kind == TrackKind::Audio;
        let len = sp.e - sp.s;
        let lane_a = lane.map(|l| format!(" lane=\"{}\"", l)).unwrap_or_default();
        let name = esc(&event_name(e, Some(m)));
        if let Some(aid) = self.assets.get(&m.id) {
            let src_en = match (audio_track, m.has_video, m.has_audio) {
                (true, true, _) => " srcEnable=\"audio\"",
                (false, _, true) => " srcEnable=\"video\"",
                _ => "",
            };
            let enabled = if e.mute { " enabled=\"0\"" } else { "" };
            if retimed(e) {
                let a = to_frames(e.offset, self.fps);
                let b = a + (len as f64 * e.rate).round() as i64;
                let (v0, v1) = if e.reverse { (b, a) } else { (a, b) };
                format!(
                    "<asset-clip ref=\"{}\"{} offset=\"{}\" name=\"{}\" start=\"0s\" duration=\"{}\"{}{}>\n<timeMap><timept time=\"0s\" value=\"{}\" interp=\"linear\"/><timept time=\"{}\" value=\"{}\" interp=\"linear\"/></timeMap>\n{}</asset-clip>\n",
                    aid,
                    lane_a,
                    self.t(offset),
                    name,
                    self.t(len),
                    src_en,
                    enabled,
                    self.t(v0),
                    self.t(len),
                    self.t(v1),
                    inner
                )
            } else {
                let head = format!(
                    "<asset-clip ref=\"{}\"{} offset=\"{}\" name=\"{}\" start=\"{}\" duration=\"{}\"{}{}",
                    aid,
                    lane_a,
                    self.t(offset),
                    name,
                    self.t(self.local0(sp)),
                    self.t(len),
                    src_en,
                    enabled
                );
                if inner.is_empty() {
                    format!("{}/>\n", head)
                } else {
                    format!("{}>\n{}</asset-clip>\n", head, inner)
                }
            }
        } else if let (Some((text, size, c)), Some(tref)) = (gen_text(m), self.title.clone()) {
            self.ts += 1;
            let id = format!("ts{}", self.ts);
            let fs = ((size.max(1) as f64) * p.height as f64 / 100.0).round().max(8.0);
            let col = format!("{:.3} {:.3} {:.3} {:.3}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0, c[3] as f64 / 255.0);
            format!(
                "<title ref=\"{}\"{} offset=\"{}\" name=\"{}\" start=\"0s\" duration=\"{}\">\n<text><text-style ref=\"{}\">{}</text-style></text>\n<text-style-def id=\"{}\"><text-style font=\"Helvetica\" fontSize=\"{}\" fontColor=\"{}\" alignment=\"center\"/></text-style-def>\n{}</title>\n",
                tref,
                lane_a,
                self.t(offset),
                name,
                self.t(len),
                id,
                esc(&text),
                id,
                fs,
                col,
                inner
            )
        } else {
            String::new()
        }
    }
}

fn audio_rate_label(sr: u32) -> &'static str {
    match sr {
        32000 => "32k",
        44100 => "44.1k",
        88200 => "88.2k",
        96000 => "96k",
        176400 => "176.4k",
        192000 => "192k",
        _ => "48k",
    }
}

fn fcp_keep_video(m: &Media, titles: bool) -> bool {
    matches!(m.source, MediaSource::File(_) | MediaSource::Image(_)) || (titles && gen_text(m).is_some())
}

/// FCPXML 1.9. The lowest video track (the background) is the primary
/// storyline (spine); overlapping spine events and the other video tracks are
/// connected clips on lanes 1, 2... (higher = on top); audio tracks are
/// connected clips on lanes -1, -2...
pub fn export_fcpxml(p: &Project) -> String {
    let fps = exact_fps(p.fps);
    let fd = frame_dur(fps);
    let mut nid = 1;
    let mut res = String::new();

    // formats and assets
    let mut formats: Vec<((u32, u32, i64, i64), String)> = vec![];
    let proj_fmt = format!("r{}", nid);
    nid += 1;
    w!(res, "<format id=\"{}\" frameDuration=\"{}\" width=\"{}\" height=\"{}\"/>\n", proj_fmt, rational(fd.0, fd.1), p.width, p.height);
    formats.push(((p.width, p.height, fd.0, fd.1), proj_fmt.clone()));
    let mut assets: HashMap<Id, String> = HashMap::new();
    let mut asset_xml = String::new();
    for m in &p.media {
        let path = match &m.source {
            MediaSource::File(x) | MediaSource::Image(x) => x,
            _ => continue,
        };
        let still = matches!(m.source, MediaSource::Image(_));
        let has_video = m.has_video || still;
        let mfps = if has_video && m.fps > 0.0 { exact_fps(m.fps) } else { fps };
        let mfd = frame_dur(mfps);
        let mut fmt_attr = String::new();
        if has_video {
            let (wd, ht) = if m.width > 0 && m.height > 0 { (m.width, m.height) } else { (p.width, p.height) };
            let key = if still { (wd, ht, 0, 0) } else { (wd, ht, mfd.0, mfd.1) };
            let id = match formats.iter().find(|(k, _)| *k == key) {
                Some((_, id)) => id.clone(),
                None => {
                    let id = format!("r{}", nid);
                    nid += 1;
                    if still {
                        w!(res, "<format id=\"{}\" name=\"FFVideoFormatRateUndefined\" width=\"{}\" height=\"{}\"/>\n", id, wd, ht);
                    } else {
                        w!(res, "<format id=\"{}\" frameDuration=\"{}\" width=\"{}\" height=\"{}\"/>\n", id, rational(mfd.0, mfd.1), wd, ht);
                    }
                    formats.push((key, id.clone()));
                    id
                }
            };
            fmt_attr = format!(" format=\"{}\" videoSources=\"1\"", id);
        }
        let id = format!("r{}", nid);
        nid += 1;
        let dur = if m.duration > 0.0 { rational(to_frames(m.duration, mfps) * mfd.0, mfd.1) } else { "0s".into() };
        let has_audio = m.has_audio && !still;
        let audio_attr = if has_audio { format!(" audioSources=\"1\" audioChannels=\"2\" audioRate=\"{}\"", p.sample_rate) } else { String::new() };
        w!(
            asset_xml,
            "<asset id=\"{}\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\"{}{}>\n<media-rep kind=\"original-media\" src=\"{}\"/>\n</asset>\n",
            id,
            esc(&m.name),
            dur,
            has_video as u8,
            has_audio as u8,
            fmt_attr,
            audio_attr,
            esc(&file_url(path))
        );
        assets.insert(m.id, id);
    }
    res.push_str(&asset_xml);
    let video: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].kind == TrackKind::Video).collect();
    let audio: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].kind == TrackKind::Audio).collect();
    let needs_title = video.iter().flat_map(|&i| p.tracks[i].events.iter()).any(|e| p.media(e.media).map_or(false, |m| gen_text(m).is_some()));
    let title = if needs_title {
        let id = format!("r{}", nid);
        w!(res, "<effect id=\"{}\" name=\"Basic Title\" uid=\".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti\"/>\n", id);
        Some(id)
    } else {
        None
    };

    let mut fo = FcpOut { p, fps, fd, assets, title, ts: 0 };
    let vkeep = |m: &Media| fcp_keep_video(m, needs_title);
    let akeep = |m: &Media| matches!(m.source, MediaSource::File(_));

    // spine and lanes
    let mut spine_clips: Vec<Span> = vec![];
    let mut connected: Vec<(i64, Span)> = vec![];
    let mut lane = 1i64;
    if let Some(&main) = video.last() {
        let mut over = vec![];
        let mut end = i64::MIN;
        for sp in track_spans(p, main, fps, &vkeep) {
            if sp.s >= end {
                end = sp.e;
                spine_clips.push(sp);
            } else {
                over.push(sp);
            }
        }
        for l in sublanes(over) {
            connected.extend(l.into_iter().map(|sp| (lane, sp)));
            lane += 1;
        }
        for &ti in video.iter().rev().skip(1) {
            for l in sublanes(track_spans(p, ti, fps, &vkeep)) {
                connected.extend(l.into_iter().map(|sp| (lane, sp)));
                lane += 1;
            }
        }
    }
    let mut alane = -1i64;
    for &ti in &audio {
        for l in sublanes(track_spans(p, ti, fps, &akeep)) {
            connected.extend(l.into_iter().map(|sp| (alane, sp)));
            alane -= 1;
        }
    }
    let marks: Vec<(i64, &Marker)> = p.markers.iter().map(|m| (to_frames(m.time, fps).max(0), m)).collect();
    let total = spine_clips
        .iter()
        .map(|s| s.e)
        .chain(connected.iter().map(|c| c.1.e))
        .chain(marks.iter().map(|m| m.0 + 1))
        .max()
        .unwrap_or(0);

    // spine elements cover the whole timeline (gaps between clips), so that
    // every connected clip and marker has a parent
    struct El {
        s: i64,
        e: i64,
        clip: Option<Span>,
        local0: i64,
        kids: String,
        marks: String,
    }
    let gap = |s: i64, e: i64| El { s, e, clip: None, local0: 0, kids: String::new(), marks: String::new() };
    let mut els: Vec<El> = vec![];
    let mut cur = 0;
    for sp in &spine_clips {
        if sp.s > cur {
            els.push(gap(cur, sp.s));
        }
        els.push(El { s: sp.s, e: sp.e, clip: Some(*sp), local0: fo.local0(sp), kids: String::new(), marks: String::new() });
        cur = sp.e;
    }
    if total > cur {
        els.push(gap(cur, total));
    }
    for (l, sp) in &connected {
        if let Some(k) = els.iter().position(|el| el.s <= sp.s && sp.s < el.e) {
            let off = els[k].local0 + (sp.s - els[k].s);
            let x = fo.clip(sp, Some(*l), off, "");
            els[k].kids.push_str(&x);
        }
    }
    for (f, m) in &marks {
        if let Some(k) = els.iter().position(|el| el.s <= *f && *f < el.e) {
            let x = format!("<marker start=\"{}\" duration=\"{}\" value=\"{}\"/>\n", fo.t(els[k].local0 + (*f - els[k].s)), fo.t(1), esc(&m.name));
            els[k].marks.push_str(&x);
        }
    }
    let mut spine = String::new();
    for el in &els {
        let inner = format!("{}{}", el.kids, el.marks);
        match &el.clip {
            Some(sp) => {
                let x = fo.clip(sp, None, el.s, &inner);
                spine.push_str(&x);
            }
            None if inner.is_empty() => w!(spine, "<gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"/>\n", fo.t(el.s), fo.t(el.e - el.s)),
            None => w!(spine, "<gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\">\n{}</gap>\n", fo.t(el.s), fo.t(el.e - el.s), inner),
        }
    }

    let name = esc(&p.name);
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n<resources>\n");
    out.push_str(&res);
    out.push_str("</resources>\n<library>\n");
    w!(out, "<event name=\"{}\">\n<project name=\"{}\">\n", name, name);
    w!(
        out,
        "<sequence format=\"{}\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"{}\" audioLayout=\"{}\" audioRate=\"{}\">\n<spine>\n",
        proj_fmt,
        fo.t(total),
        if drop_capable(fps) { "DF" } else { "NDF" },
        if p.channels >= 6 { "surround" } else { "stereo" },
        audio_rate_label(p.sample_rate)
    );
    out.push_str(&spine);
    out.push_str("</spine>\n</sequence>\n</project>\n</event>\n</library>\n</fcpxml>\n");
    out
}

#[derive(Clone)]
struct FcpAsset {
    path: String,
    start: f64,
    duration: f64,
    has_video: bool,
    has_audio: bool,
    size: Option<(u32, u32)>,
    fps: Option<f64>,
}

struct FcpIn<'a, 'i> {
    assets: HashMap<String, FcpAsset>,
    medias: HashMap<String, Node<'a, 'i>>,
    doc: Doc,
    depth: usize,
}

fn parse_fcpxml(d: &roxmltree::Document<'_>) -> Result<Doc, String> {
    let root = d.root_element();
    let mut formats: HashMap<String, (Option<f64>, Option<(u32, u32)>)> = HashMap::new();
    for f in root.descendants().filter(|n| n.has_tag_name("format")) {
        let Some(id) = f.attribute("id") else { continue };
        let fps = attr_time(f, "frameDuration").filter(|x| *x > 0.0).map(|x| 1.0 / x);
        let size = match (f.attribute("width").and_then(|v| v.parse().ok()), f.attribute("height").and_then(|v| v.parse().ok())) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        formats.insert(id.to_string(), (fps, size));
    }
    let mut cx = FcpIn { assets: HashMap::new(), medias: HashMap::new(), doc: Doc::default(), depth: 0 };
    for a in root.descendants().filter(|n| n.has_tag_name("asset")) {
        let Some(id) = a.attribute("id") else { continue };
        let src = a.attribute("src").or_else(|| a.children().find(|c| c.has_tag_name("media-rep")).and_then(|c| c.attribute("src")));
        let name = a.attribute("name").unwrap_or("");
        let path = match src {
            Some(s) => url_to_path(s),
            None if !name.is_empty() => name.to_string(),
            None => continue,
        };
        let (gv, ga) = guess_av(&path);
        let has_video = a.attribute("hasVideo").map_or(gv, |v| v == "1");
        let has_audio = a.attribute("hasAudio").map_or(ga, |v| v == "1");
        let fmt = a.attribute("format").and_then(|f| formats.get(f)).copied();
        let asset = FcpAsset {
            path: path.clone(),
            start: attr_time(a, "start").unwrap_or(0.0),
            duration: attr_time(a, "duration").unwrap_or(0.0),
            has_video,
            has_audio,
            size: fmt.and_then(|f| f.1),
            fps: fmt.and_then(|f| f.0),
        };
        cx.doc.hints.entry(path).or_insert(Hint { duration: asset.duration, av: Some((has_video, has_audio)), size: asset.size, fps: asset.fps });
        cx.assets.insert(id.to_string(), asset);
    }
    for m in root.descendants().filter(|n| n.has_tag_name("media")) {
        if let Some(id) = m.attribute("id") {
            cx.medias.insert(id.to_string(), m);
        }
    }
    let seq = root
        .descendants()
        .find(|n| n.has_tag_name("sequence") && n.ancestors().any(|a| a.has_tag_name("project")))
        .or_else(|| root.descendants().find(|n| n.has_tag_name("sequence")))
        .ok_or_else(|| "Aucune séquence trouvée dans le fichier FCPXML.".to_string())?;
    if let Some((fps, size)) = seq.attribute("format").and_then(|f| formats.get(f)) {
        cx.doc.fps = *fps;
        cx.doc.size = *size;
    }
    let tc_start = attr_time(seq, "tcStart").unwrap_or(0.0);
    if let Some(spine) = child(seq, "spine") {
        fcp_walk(&mut cx, spine, 0.0, tc_start, 0, INF);
    }
    Ok(cx.doc)
}

/// Walks the children of `node`. A child at `offset` (in the parent's local
/// time) sits at timeline time `t_par + offset - par_start`. Children without
/// a lane are cut to `win` (the parent's range for container clips).
fn fcp_walk(cx: &mut FcpIn<'_, '_>, node: Node<'_, '_>, t_par: f64, par_start: f64, lane: i64, win: (f64, f64)) {
    if cx.depth > 32 {
        return;
    }
    for c in node.children().filter(|c| c.is_element()) {
        let tag = c.tag_name().name();
        match tag {
            "marker" | "chapter-marker" => {
                let t = t_par + (attr_time(c, "start").unwrap_or(0.0) - par_start);
                if t >= win.0 - 1e-9 && t < win.1 && t >= -1e-9 {
                    cx.doc.markers.push(Marker { time: t.max(0.0), name: c.attribute("value").unwrap_or("").to_string(), command: false });
                }
                continue;
            }
            "asset-clip" | "video" | "audio" | "title" | "gap" | "clip" | "sync-clip" | "ref-clip" | "spine" | "mc-clip" => {}
            _ => continue,
        }
        let off = attr_time(c, "offset").unwrap_or(par_start);
        let t = t_par + (off - par_start);
        let start = attr_time(c, "start").unwrap_or(if tag == "spine" { off } else { 0.0 });
        let dur = attr_time(c, "duration").unwrap_or(0.0);
        let lane_attr = c.attribute("lane").and_then(|l| l.trim().parse::<i64>().ok());
        let el_lane = lane + lane_attr.unwrap_or(0);
        let my_win = if lane_attr.is_some() { INF } else { win };
        let inner_win = if tag == "spine" { my_win } else { (my_win.0.max(t), my_win.1.min(t + dur)) };
        let name = c.attribute("name").unwrap_or("").to_string();
        match tag {
            "asset-clip" | "video" | "audio" => {
                if let Some(a) = c.attribute("ref").and_then(|r| cx.assets.get(r)).cloned() {
                    let tm: Vec<(f64, f64)> = child(c, "timeMap")
                        .map(|m| m.children().filter(|x| x.has_tag_name("timept")).filter_map(|x| Some((attr_time(x, "time")?, attr_time(x, "value")?))).collect())
                        .unwrap_or_default();
                    let (offset, rate, reverse) = if tm.len() >= 2 {
                        let (t0, v0) = tm[0];
                        let (t1, v1) = tm[tm.len() - 1];
                        let r = if (t1 - t0).abs() > 1e-9 { (v1 - v0) / (t1 - t0) } else { 1.0 };
                        let at = |lt: f64| v0 + (lt - t0) * r;
                        if r < 0.0 {
                            (at(start + dur) - a.start, (-r).max(0.01), true)
                        } else {
                            (at(start) - a.start, r.max(0.01), false)
                        }
                    } else {
                        (start - a.start, 1.0, false)
                    };
                    let src_en = c.attribute("srcEnable").unwrap_or("all");
                    let (v, au) = match tag {
                        "video" => (true, false),
                        "audio" => (false, true),
                        _ => (a.has_video && src_en != "audio", a.has_audio && src_en != "video"),
                    };
                    let nm = if name.is_empty() { file_stem(&a.path) } else { name.clone() };
                    if v {
                        push_clip(&mut cx.doc, TrackKind::Video, el_lane, &nm, Src::File(a.path.clone()), t, dur, offset, rate, reverse, my_win);
                    }
                    if au {
                        push_clip(&mut cx.doc, TrackKind::Audio, el_lane, &nm, Src::File(a.path.clone()), t, dur, offset, rate, reverse, my_win);
                    }
                }
                fcp_walk(cx, c, t, start, el_lane, inner_win);
            }
            "title" => {
                let text = c
                    .children()
                    .filter(|x| x.has_tag_name("text"))
                    .map(|x| x.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if text.trim().is_empty() { name.clone() } else { text };
                push_clip(&mut cx.doc, TrackKind::Video, el_lane, &name, Src::Gen(text_generator(text)), t, dur, 0.0, 1.0, false, my_win);
                fcp_walk(cx, c, t, start, el_lane, inner_win);
            }
            "ref-clip" => {
                // compound clip: walk the spine of the referenced media
                let media = c.attribute("ref").and_then(|r| cx.medias.get(r)).copied();
                if let Some(sp) = media.and_then(|m| m.descendants().find(|n| n.has_tag_name("spine"))) {
                    cx.depth += 1;
                    fcp_walk(cx, sp, t, start, el_lane, inner_win);
                    cx.depth -= 1;
                }
                fcp_walk(cx, c, t, start, el_lane, inner_win);
            }
            // gap, clip, sync-clip, mc-clip and secondary storylines
            _ => fcp_walk(cx, c, t, start, el_lane, inner_win),
        }
    }
}

// ------------------------------------------------------------------ xmeml

/// Frame rate used for frame numbers and its <rate> element.
fn xrate(fps: f64) -> (f64, String) {
    let nom = nominal(fps);
    let ntsc = is_ntsc(fps);
    let x = if ntsc { nom as f64 * 1000.0 / 1001.0 } else { nom as f64 };
    (x, format!("<rate><timebase>{}</timebase><ntsc>{}</ntsc></rate>", nom, if ntsc { "TRUE" } else { "FALSE" }))
}

/// Final Cut Pro 7 XML (xmeml version 5), for Premiere Pro and Resolve.
/// Overlapping events of a track go to extra tracks.
pub fn export_xmeml(p: &Project) -> String {
    let (xfps, rate) = xrate(exact_fps(p.fps));
    let df = drop_capable(xfps);
    let video: Vec<usize> = (0..p.tracks.len()).rev().filter(|&i| p.tracks[i].kind == TrackKind::Video).collect();
    let audio: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].kind == TrackKind::Audio).collect();
    let vkeep = |m: &Media| match &m.source {
        MediaSource::Generated(g) => !g.is_audio(),
        _ => true,
    };
    let akeep = |m: &Media| matches!(m.source, MediaSource::File(_));
    let mut defined: HashSet<Id> = HashSet::new();
    let mut n = 0usize;
    let mut total = 0i64;

    let mut item = |out: &mut String, sp: &Span, is_audio: bool| {
        let e = ev(p, sp);
        let Some(m) = p.media(e.media) else { return };
        n += 1;
        total = total.max(sp.e);
        let len_s = (sp.e - sp.s) as f64 / xfps;
        let name = esc(&event_name(e, Some(m)));
        let enabled = if e.mute { "FALSE" } else { "TRUE" };
        if let MediaSource::Generated(_) = &m.source {
            let lenf = sp.e - sp.s;
            w!(out, "<generatoritem id=\"generatoritem-{}\">\n<name>{}</name>\n<enabled>{}</enabled>\n<duration>{}</duration>\n{}\n", n, name, enabled, lenf, rate);
            w!(out, "<start>{}</start>\n<end>{}</end>\n<in>0</in>\n<out>{}</out>\n<anamorphic>FALSE</anamorphic>\n<alphatype>black</alphatype>\n", sp.s, sp.e, lenf);
            match gen_text(m) {
                Some((text, size, _)) => {
                    let fs = ((size.max(1) as f64) * p.height as f64 / 100.0).round().max(8.0);
                    out.push_str("<effect>\n<name>Text</name>\n<effectid>Text</effectid>\n<effectcategory>Text</effectcategory>\n<effecttype>generator</effecttype>\n<mediatype>video</mediatype>\n");
                    w!(out, "<parameter>\n<parameterid>str</parameterid>\n<name>Text</name>\n<value>{}</value>\n</parameter>\n", esc(&text));
                    w!(out, "<parameter>\n<parameterid>fontsize</parameterid>\n<name>Size</name>\n<valuemin>0</valuemin>\n<valuemax>1000</valuemax>\n<value>{}</value>\n</parameter>\n</effect>\n", fs);
                }
                None => {
                    out.push_str("<effect>\n<name>Slug</name>\n<effectid>slug</effectid>\n<effectcategory>Matte</effectcategory>\n<effecttype>generator</effecttype>\n<mediatype>video</mediatype>\n</effect>\n");
                }
            }
            out.push_str("</generatoritem>\n");
            return;
        }
        let Some(path) = m.path() else { return };
        let (mfps, mrate) = if m.has_video && m.fps > 0.0 { xrate(exact_fps(m.fps)) } else { (xfps, rate.clone()) };
        let inn = to_frames(e.offset, mfps);
        let out_f = inn + to_frames(len_s * e.rate, mfps).max(1);
        let mdur = if m.duration > 0.0 { to_frames(m.duration, mfps).max(out_f) } else { out_f };
        w!(out, "<clipitem id=\"clipitem-{}\">\n<masterclipid>masterclip-{}</masterclipid>\n<name>{}</name>\n<enabled>{}</enabled>\n<duration>{}</duration>\n{}\n", n, m.id, name, enabled, mdur, mrate);
        w!(out, "<start>{}</start>\n<end>{}</end>\n<in>{}</in>\n<out>{}</out>\n", sp.s, sp.e, inn, out_f);
        if defined.insert(m.id) {
            w!(out, "<file id=\"file-{}\">\n<name>{}</name>\n<pathurl>{}</pathurl>\n{}\n<duration>{}</duration>\n", m.id, esc(&file_name(path)), esc(&file_url(path)), mrate, mdur);
            w!(out, "<timecode>\n{}\n<string>00:00:00:00</string>\n<frame>0</frame>\n<displayformat>NDF</displayformat>\n</timecode>\n<media>\n", mrate);
            if m.has_video || matches!(m.source, MediaSource::Image(_)) {
                let (wd, ht) = if m.width > 0 && m.height > 0 { (m.width, m.height) } else { (p.width, p.height) };
                w!(out, "<video>\n<samplecharacteristics>\n{}\n<width>{}</width>\n<height>{}</height>\n<anamorphic>FALSE</anamorphic>\n<pixelaspectratio>square</pixelaspectratio>\n<fielddominance>none</fielddominance>\n</samplecharacteristics>\n</video>\n", mrate, wd, ht);
            }
            if m.has_audio {
                w!(out, "<audio>\n<samplecharacteristics>\n<depth>16</depth>\n<samplerate>{}</samplerate>\n</samplecharacteristics>\n<channelcount>2</channelcount>\n</audio>\n", p.sample_rate);
            }
            out.push_str("</media>\n</file>\n");
        } else {
            w!(out, "<file id=\"file-{}\"/>\n", m.id);
        }
        if retimed(e) {
            out.push_str("<filter>\n<effect>\n<name>Time Remap</name>\n<effectid>timeremap</effectid>\n<effectcategory>motion</effectcategory>\n<effecttype>motion</effecttype>\n<mediatype>video</mediatype>\n");
            out.push_str("<parameter authoringApp=\"PremierePro\">\n<parameterid>variablespeed</parameterid>\n<name>variablespeed</name>\n<valuemin>0</valuemin>\n<valuemax>1</valuemax>\n<value>0</value>\n</parameter>\n");
            w!(out, "<parameter authoringApp=\"PremierePro\">\n<parameterid>speed</parameterid>\n<name>speed</name>\n<valuemin>-100000</valuemin>\n<valuemax>100000</valuemax>\n<value>{}</value>\n</parameter>\n", e.rate * 100.0);
            w!(out, "<parameter authoringApp=\"PremierePro\">\n<parameterid>reverse</parameterid>\n<name>reverse</name>\n<value>{}</value>\n</parameter>\n</effect>\n</filter>\n", if e.reverse { "TRUE" } else { "FALSE" });
        }
        if is_audio {
            out.push_str("<sourcetrack>\n<mediatype>audio</mediatype>\n<trackindex>1</trackindex>\n</sourcetrack>\n");
        }
        out.push_str("</clipitem>\n");
    };

    let mut vx = String::new();
    for &ti in &video {
        for lane in sublanes(track_spans(p, ti, xfps, &vkeep)) {
            vx.push_str("<track>\n");
            for sp in &lane {
                item(&mut vx, sp, false);
            }
            vx.push_str("<enabled>TRUE</enabled>\n<locked>FALSE</locked>\n</track>\n");
        }
    }
    let mut ax = String::new();
    for &ti in &audio {
        for lane in sublanes(track_spans(p, ti, xfps, &akeep)) {
            ax.push_str("<track>\n");
            for sp in &lane {
                item(&mut ax, sp, true);
            }
            ax.push_str("<enabled>TRUE</enabled>\n<locked>FALSE</locked>\n</track>\n");
        }
    }
    drop(item);
    let mut mk = String::new();
    for m in &p.markers {
        let f = to_frames(m.time, xfps).max(0);
        total = total.max(f + 1);
        w!(mk, "<marker>\n<comment></comment>\n<name>{}</name>\n<in>{}</in>\n<out>-1</out>\n</marker>\n", esc(&m.name), f);
    }

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"5\">\n<sequence id=\"sequence-1\">\n");
    w!(out, "<name>{}</name>\n<duration>{}</duration>\n{}\n", esc(&p.name), total, rate);
    w!(out, "<timecode>\n{}\n<string>{}</string>\n<frame>0</frame>\n<displayformat>{}</displayformat>\n</timecode>\n", rate, tc(0, xfps), if df { "DF" } else { "NDF" });
    out.push_str("<media>\n<video>\n");
    w!(out, "<format>\n<samplecharacteristics>\n{}\n<width>{}</width>\n<height>{}</height>\n<anamorphic>FALSE</anamorphic>\n<pixelaspectratio>square</pixelaspectratio>\n<fielddominance>none</fielddominance>\n</samplecharacteristics>\n</format>\n", rate, p.width, p.height);
    out.push_str(&vx);
    out.push_str("</video>\n<audio>\n");
    w!(out, "<numOutputChannels>2</numOutputChannels>\n<format>\n<samplecharacteristics>\n<depth>16</depth>\n<samplerate>{}</samplerate>\n</samplecharacteristics>\n</format>\n", p.sample_rate);
    out.push_str(&ax);
    out.push_str("</audio>\n</media>\n");
    out.push_str(&mk);
    out.push_str("</sequence>\n</xmeml>\n");
    out
}

fn parse_rate(n: Node<'_, '_>) -> Option<f64> {
    let tb: f64 = ctext(n, "timebase")?.parse().ok()?;
    let ntsc = ctext(n, "ntsc").map_or(false, |s| s.eq_ignore_ascii_case("TRUE"));
    (tb > 0.0).then_some(if ntsc { tb * 1000.0 / 1001.0 } else { tb })
}

fn xmeml_speed(it: Node<'_, '_>) -> (f64, bool) {
    for eff in it.descendants().filter(|n| n.has_tag_name("effect")) {
        let id = ctext(eff, "effectid").unwrap_or_default().to_ascii_lowercase();
        if id != "timeremap" {
            continue;
        }
        let (mut rate, mut rev) = (1.0, false);
        for par in eff.children().filter(|n| n.has_tag_name("parameter")) {
            let pid = ctext(par, "parameterid").unwrap_or_default().to_ascii_lowercase();
            let val = ctext(par, "value").unwrap_or_default();
            match pid.as_str() {
                "speed" => {
                    if let Ok(v) = val.parse::<f64>() {
                        if v.abs() > 1e-6 {
                            rate = v.abs() / 100.0;
                            rev |= v < 0.0;
                        }
                    }
                }
                "reverse" => rev |= val.eq_ignore_ascii_case("TRUE"),
                _ => {}
            }
        }
        return (rate, rev);
    }
    (1.0, false)
}

fn xmeml_generator(it: Node<'_, '_>) -> Option<Generator> {
    let eff = it
        .descendants()
        .find(|n| n.has_tag_name("effect") && ctext(*n, "effecttype").map_or(false, |t| t.eq_ignore_ascii_case("generator")))?;
    let text = eff
        .children()
        .filter(|n| n.has_tag_name("parameter"))
        .find(|par| ctext(*par, "parameterid").map_or(false, |s| s.eq_ignore_ascii_case("str")))
        .and_then(|par| child(par, "value").and_then(|v| v.text()).map(|s| s.to_string()));
    let id = format!("{} {}", ctext(eff, "effectid").unwrap_or_default(), ctext(eff, "name").unwrap_or_default()).to_ascii_lowercase();
    match text {
        Some(t) if !t.trim().is_empty() => Some(text_generator(t)),
        _ if id.contains("text") || id.contains("title") => Some(text_generator(ctext(it, "name").unwrap_or_default())),
        _ => Some(Generator::SolidColor([0, 0, 0, 255])),
    }
}

struct XFile {
    path: String,
    fps: f64,
    hint: Hint,
}

fn xmeml_track(doc: &mut Doc, files: &HashMap<String, XFile>, tr: Node<'_, '_>, kind: TrackKind, lane: i64, sfps: f64) {
    let items: Vec<Node<'_, '_>> = tr
        .children()
        .filter(|c| c.is_element() && matches!(c.tag_name().name(), "clipitem" | "generatoritem" | "transitionitem"))
        .collect();
    let is_trans = |n: &Node<'_, '_>| n.tag_name().name() == "transitionitem";
    let trans_at = |n: Node<'_, '_>| -> Option<(f64, f64)> { Some((cnum(n, "start")?, cnum(n, "end")?)) };
    for (i, it) in items.iter().enumerate() {
        if is_trans(it) {
            continue;
        }
        let file = child(*it, "file").and_then(|f| f.attribute("id")).and_then(|id| files.get(id));
        let src = match file {
            Some(f) => Src::File(f.path.clone()),
            None if child(*it, "sequence").is_some() => continue,
            None => match xmeml_generator(*it) {
                Some(g) => Src::Gen(g),
                None => continue,
            },
        };
        let cfps = child(*it, "rate").and_then(parse_rate).or(file.map(|f| f.fps)).unwrap_or(sfps);
        let (Some(st), Some(en)) = (cnum(*it, "start"), cnum(*it, "end")) else { continue };
        let inn = cnum(*it, "in").unwrap_or(0.0).max(0.0);
        let out = cnum(*it, "out").unwrap_or(inn);
        let (rate, reverse) = xmeml_speed(*it);
        // visible part on the timeline (between the edit points)
        let vis = (out - inn).max(0.0) / cfps / rate;
        // start/end -1: the clip begins/ends inside a transition
        let prev = if i > 0 { Some(items[i - 1]).filter(|n| is_trans(n)).and_then(trans_at) } else { None };
        let next = items.get(i + 1).copied().filter(|n| is_trans(n)).and_then(trans_at);
        let mut offset = inn / cfps;
        let s;
        if st >= 0.0 {
            s = st / sfps;
        } else if let Some((ts, te)) = prev {
            let edit = if en >= 0.0 { en / sfps - vis } else { (ts + te) / 2.0 / sfps };
            s = ts / sfps;
            offset -= (edit - s) * rate;
        } else if en >= 0.0 {
            s = en / sfps - vis;
        } else {
            continue;
        }
        let e = if en >= 0.0 {
            en / sfps
        } else if let Some((_, te)) = next {
            te / sfps
        } else {
            s + vis
        };
        let name = ctext(*it, "name").unwrap_or_default();
        push_clip(doc, kind, lane, &name, src, s, e - s, offset.max(0.0), rate, reverse, INF);
        if let Some(f) = file {
            doc.hints.entry(f.path.clone()).or_insert_with(|| f.hint.clone());
        }
    }
}

fn parse_xmeml(d: &roxmltree::Document<'_>) -> Result<Doc, String> {
    let root = d.root_element();
    let seq = root
        .descendants()
        .find(|n| n.has_tag_name("sequence"))
        .ok_or_else(|| "Aucune séquence trouvée dans le fichier XML.".to_string())?;
    let sfps = child(seq, "rate").and_then(parse_rate).unwrap_or(DEFAULT_FPS);
    let mut doc = Doc { fps: Some(sfps), ..Default::default() };
    let mut files: HashMap<String, XFile> = HashMap::new();
    for f in root.descendants().filter(|n| n.has_tag_name("file")) {
        let Some(id) = f.attribute("id") else { continue };
        let Some(url) = ctext(f, "pathurl").filter(|u| !u.is_empty()) else { continue };
        let path = url_to_path(&url);
        let ffps = child(f, "rate").and_then(parse_rate).unwrap_or(sfps);
        let dur = cnum(f, "duration").map(|d| d / ffps).unwrap_or(0.0);
        let media = child(f, "media");
        let vid = media.and_then(|m| child(m, "video"));
        let has_video = vid.is_some();
        let has_audio = media.map_or(false, |m| child(m, "audio").is_some());
        let size = vid.and_then(|v| child(v, "samplecharacteristics")).and_then(|sc| {
            let wd = ctext(sc, "width")?.parse().ok()?;
            let ht = ctext(sc, "height")?.parse().ok()?;
            Some((wd, ht))
        });
        let av = media.map(|_| (has_video, has_audio));
        let hint = Hint { duration: dur, av, size, fps: has_video.then_some(ffps) };
        files.insert(id.to_string(), XFile { path, fps: ffps, hint });
    }
    if let Some(media) = child(seq, "media") {
        if let Some(v) = child(media, "video") {
            doc.size = child(v, "format").and_then(|f| child(f, "samplecharacteristics")).and_then(|sc| {
                let wd = ctext(sc, "width")?.parse().ok()?;
                let ht = ctext(sc, "height")?.parse().ok()?;
                Some((wd, ht))
            });
            // V1 (bottom) first
            for (i, tr) in v.children().filter(|c| c.has_tag_name("track")).enumerate() {
                xmeml_track(&mut doc, &files, tr, TrackKind::Video, i as i64, sfps);
            }
        }
        if let Some(a) = child(media, "audio") {
            for (i, tr) in a.children().filter(|c| c.has_tag_name("track")).enumerate() {
                xmeml_track(&mut doc, &files, tr, TrackKind::Audio, -(i as i64) - 1, sfps);
            }
        }
    }
    for m in seq.children().filter(|c| c.has_tag_name("marker")) {
        let Some(f) = cnum(m, "in").filter(|f| *f >= 0.0) else { continue };
        let name = ctext(m, "name").filter(|s| !s.is_empty()).or_else(|| ctext(m, "comment")).unwrap_or_default();
        doc.markers.push(Marker { time: f / sfps, name, command: false });
    }
    Ok(doc)
}

// -------------------------------------------------------------------- SRT

fn srt_ts(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as i64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

/// SRT from the subtitle and text events of every track, sorted by time.
pub fn export_srt(p: &Project) -> String {
    let mut cues: Vec<(f64, f64, String)> = vec![];
    for t in &p.tracks {
        for e in &t.events {
            let Some(m) = p.media(e.media) else { continue };
            let text = match &m.source {
                MediaSource::Generated(Generator::Subtitle { text, .. }) | MediaSource::Generated(Generator::Text { text, .. }) => text,
                _ => continue,
            };
            let text = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\r\n");
            if !text.is_empty() && e.length > 0.0 {
                cues.push((e.start, e.end(), text));
            }
        }
    }
    cues.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = String::new();
    for (i, (s, e, text)) in cues.iter().enumerate() {
        w!(out, "{}\r\n{} --> {}\r\n{}\r\n\r\n", i + 1, srt_ts(*s), srt_ts(*e), text);
    }
    out
}

fn srt_time(s: &str) -> Option<f64> {
    let tok = s.split_whitespace().next()?.replace(',', ".");
    let parts: Vec<&str> = tok.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Removes <i>-style tags and {\an8}-style overrides.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut angle, mut brace) = (false, false);
    for c in s.chars() {
        match c {
            '<' => angle = true,
            '>' if angle => angle = false,
            '{' => brace = true,
            '}' if brace => brace = false,
            _ if angle || brace => {}
            c => out.push(c),
        }
    }
    out
}

fn parse_srt(text: &str) -> Vec<(f64, f64, String)> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        if let Some((a, b)) = lines[i].split_once("-->") {
            if let (Some(s), Some(e)) = (srt_time(a), srt_time(b)) {
                i += 1;
                let mut body = vec![];
                while i < lines.len() && !lines[i].trim().is_empty() {
                    body.push(strip_tags(lines[i].trim()));
                    i += 1;
                }
                let t = body.join("\n").trim().to_string();
                if !t.is_empty() && e > s {
                    out.push((s, e, t));
                }
                continue;
            }
        }
        i += 1;
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn import_srt(text: &str, p: &mut Project) -> Result<String, String> {
    let cues = parse_srt(text);
    if cues.is_empty() {
        return Err("Aucun sous-titre trouvé dans le fichier.".into());
    }
    let ti = p.add_track(TrackKind::Video, Some(0));
    p.tracks[ti].name = "Sous-titres".into();
    let mut evs = vec![];
    for (s, e, text) in &cues {
        let name: String = text.lines().next().unwrap_or("").chars().take(40).collect();
        let mut m = Media::basic(name, MediaSource::Generated(Generator::Subtitle { text: text.clone(), size_pct: 5 }), true, false, 0.0, p.width, p.height, p.fps);
        m.id = p.add_media(m.clone());
        evs.push(p.new_event(&m, *s, e - s));
    }
    p.tracks[ti].events.extend(evs);
    p.sort_events();
    Ok(summary(cues.len(), 0, 0))
}

// -------------------------------------------------------------------- BWF

/// Broadcast WAV: read the 'bext' chunk TimeReference (u64 samples since
/// midnight at offset 338 within bext data) and the fmt sample rate; returns
/// seconds or None.
pub fn bwf_time_reference(path: &str) -> Option<f64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr).ok()?;
    if !matches!(&hdr[0..4], b"RIFF" | b"RF64" | b"BW64") || &hdr[8..12] != b"WAVE" {
        return None;
    }
    let mut rate: Option<u32> = None;
    let mut tref: Option<u64> = None;
    for _ in 0..4096 {
        let mut ch = [0u8; 8];
        if f.read_exact(&mut ch).is_err() {
            break;
        }
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as i64;
        let pad = size & 1;
        match &ch[0..4] {
            b"fmt " if size >= 8 => {
                let mut b = [0u8; 8];
                f.read_exact(&mut b).ok()?;
                rate = Some(u32::from_le_bytes([b[4], b[5], b[6], b[7]]));
                f.seek(SeekFrom::Current(size - 8 + pad)).ok()?;
            }
            b"bext" if size >= 346 => {
                let mut b = vec![0u8; 346];
                f.read_exact(&mut b).ok()?;
                let lo = u32::from_le_bytes([b[338], b[339], b[340], b[341]]) as u64;
                let hi = u32::from_le_bytes([b[342], b[343], b[344], b[345]]) as u64;
                tref = Some(lo | (hi << 32));
                f.seek(SeekFrom::Current(size - 346 + pad)).ok()?;
            }
            _ => {
                if size == 0xFFFF_FFFF {
                    break; // RF64 data chunk (real size in ds64): nothing useful after it
                }
                f.seek(SeekFrom::Current(size + pad)).ok()?;
            }
        }
        if let (Some(r), Some(t)) = (rate, tref) {
            return (r > 0).then(|| t as f64 / r as f64);
        }
    }
    None
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn no_probe(_: &str) -> Option<Media> {
        None
    }

    fn tmp(name: &str, content: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("bleu_interchange_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn add_file(p: &mut Project, name: &str, path: &str, v: bool, a: bool) -> Media {
        let fps = if v { p.fps } else { 0.0 };
        let id = p.add_media(Media::basic(name.into(), MediaSource::File(path.into()), v, a, 120.0, 1920, 1080, fps));
        p.media(id).unwrap().clone()
    }

    fn push(p: &mut Project, ti: usize, m: &Media, start: f64, len: f64, offset: f64, rate: f64) {
        let mut e = p.new_event(m, start, len);
        e.offset = offset;
        e.rate = rate;
        p.tracks[ti].events.push(e);
    }

    /// One video track and one audio track (what an EDL carries).
    fn simple() -> Project {
        let mut p = Project::empty();
        p.name = "Essai <EDL> & co".into();
        let v = p.add_track(TrackKind::Video, None);
        let a = p.add_track(TrackKind::Audio, None);
        let m1 = add_file(&mut p, "plan un", "C:\\Vidéos\\plan un.mp4", true, true);
        let m2 = add_file(&mut p, "plan2", "C:\\Vidéos\\plan2.mov", true, true);
        let ma = add_file(&mut p, "musique", "C:\\Audio\\musique.wav", false, true);
        push(&mut p, v, &m1, 0.0, 6.0, 2.0, 1.0);
        push(&mut p, v, &m2, 5.0, 10.0, 0.0, 1.0); // 1 s dissolve
        push(&mut p, v, &m1, 20.0, 4.0, 10.0, 2.0); // double speed
        push(&mut p, a, &ma, 0.0, 15.0, 0.0, 1.0);
        push(&mut p, a, &ma, 16.0, 5.0, 30.0, 1.0);
        p
    }

    /// Adds a top video track with a title and a clip, and markers.
    fn rich() -> Project {
        let mut p = simple();
        p.add_track(TrackKind::Video, Some(0));
        let g = Generator::Text { text: "Bonjour\nle monde".into(), size_pct: 10, color: [255, 255, 255, 255], background: [0, 0, 0, 0] };
        let id = p.add_media(Media::basic("Titre".into(), MediaSource::Generated(g), true, false, 0.0, 1920, 1080, p.fps));
        let mt = p.media(id).unwrap().clone();
        let m2 = p.media.iter().find(|m| m.name == "plan2").unwrap().clone();
        push(&mut p, 0, &mt, 2.0, 3.0, 0.0, 1.0);
        push(&mut p, 0, &m2, 8.0, 4.0, 1.0, 1.0);
        p.markers.push(Marker { time: 3.0, name: "Repère « A »".into(), command: false });
        p.markers.push(Marker { time: 30.0, name: "Fin".into(), command: false });
        p
    }

    fn spans(p: &Project, kind: TrackKind) -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = p.tracks.iter().filter(|t| t.kind == kind).flat_map(|t| t.events.iter().map(|e| (e.start, e.length))).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    }

    fn check(a: &Project, b: &Project) {
        let tol = 1.0 / a.fps + 1e-6;
        for kind in [TrackKind::Video, TrackKind::Audio] {
            let (x, y) = (spans(a, kind), spans(b, kind));
            assert_eq!(x.len(), y.len(), "{:?}: {:?} / {:?}", kind, x, y);
            for (p, q) in x.iter().zip(&y) {
                assert!((p.0 - q.0).abs() <= tol && (p.1 - q.1).abs() <= tol, "{:?}: {:?} / {:?}", kind, x, y);
            }
        }
    }

    fn roundtrip(name: &str, text: &str) -> Project {
        let path = tmp(name, text.as_bytes());
        let mut q = Project::empty();
        let msg = import(&path, &mut q, &no_probe).unwrap();
        assert!(msg.contains("importé"), "{}", msg);
        q
    }

    fn event_at(p: &Project, t: f64) -> &Event {
        p.tracks.iter().flat_map(|t| t.events.iter()).find(|e| (e.start - t).abs() < 0.05).unwrap()
    }

    #[test]
    fn timecodes() {
        assert_eq!(parse_tc("01:00:00;00", DEFAULT_FPS, false), Some(107_892));
        assert_eq!(tc(107_892, DEFAULT_FPS), "01:00:00;00");
        assert_eq!(tc(1800, DEFAULT_FPS), "00:01:00;02");
        assert_eq!(parse_tc("00:01:00;02", DEFAULT_FPS, false), Some(1800));
        assert_eq!(parse_tc("00:00:01:05", 25.0, false), Some(30));
        assert_eq!(rational(1001, 30000), "1001/30000s");
        assert_eq!(rational(30000, 1000), "30s");
        assert_eq!(url_to_path(&file_url("C:\\Vidéos\\mon clip.mp4")), "C:\\Vidéos\\mon clip.mp4");
        assert_eq!(file_url("C:\\a b.mov"), "file:///C:/a%20b.mov");
        assert_eq!(url_to_path("file://localhost/C%3a/x/y.mov"), "C:\\x\\y.mov");
    }

    #[test]
    fn edl_roundtrip() {
        let p = simple();
        let txt = export_edl(&p);
        assert!(txt.contains("FCM: DROP FRAME"));
        assert!(txt.contains("* SOURCE FILE: C:\\Vidéos\\plan un.mp4"));
        assert!(txt.lines().any(|l| l.split_whitespace().nth(3) == Some("D")), "{}", txt);
        assert!(txt.contains("M2   AX"));
        let q = roundtrip("rt.edl", &txt);
        check(&p, &q);
        let e = event_at(&q, 20.0);
        assert!((e.rate - 2.0).abs() < 1e-6 && (e.offset - 10.0).abs() < 0.05);
        assert!(q.media.iter().any(|m| m.path() == Some("C:\\Vidéos\\plan un.mp4") && m.has_video));
        assert!(q.media.iter().any(|m| m.path() == Some("C:\\Audio\\musique.wav") && !m.has_video && m.has_audio));
    }

    #[test]
    fn fcpxml_roundtrip() {
        let p = rich();
        let txt = export_fcpxml(&p);
        assert!(txt.contains("frameDuration=\"1001/30000s\""));
        assert!(txt.contains("file:///C:/Vid%C3%A9os/plan%20un.mp4"));
        assert!(txt.contains("lane=\"-1\""));
        assert!(txt.contains("<timeMap>"));
        parse_xml(&txt).unwrap();
        let q = roundtrip("rt.fcpxml", &txt);
        check(&p, &q);
        assert_eq!(q.markers.len(), 2);
        assert!((q.markers[0].time - 3.0).abs() < 0.04 && q.markers[0].name == "Repère « A »");
        let e = event_at(&q, 20.0);
        assert!((e.rate - 2.0).abs() < 1e-3 && (e.offset - 10.0).abs() < 0.05);
        let e = event_at(&q, 8.0);
        assert!((e.offset - 1.0).abs() < 0.05);
        assert!(q.media.iter().any(|m| matches!(&m.source, MediaSource::Generated(Generator::Text { text, .. }) if text == "Bonjour\nle monde")));
        assert!(q.media.iter().any(|m| m.path() == Some("C:\\Vidéos\\plan un.mp4")));
    }

    #[test]
    fn xmeml_roundtrip() {
        let p = rich();
        let txt = export_xmeml(&p);
        assert!(txt.contains("<xmeml version=\"5\">"));
        assert!(txt.contains("<ntsc>TRUE</ntsc>"));
        parse_xml(&txt).unwrap();
        let q = roundtrip("rt.xml", &txt);
        check(&p, &q);
        assert_eq!(q.markers.len(), 2);
        let e = event_at(&q, 20.0);
        assert!((e.rate - 2.0).abs() < 1e-3 && (e.offset - 10.0).abs() < 0.05);
        assert!(q.media.iter().any(|m| matches!(&m.source, MediaSource::Generated(Generator::Text { text, .. }) if text == "Bonjour\nle monde")));
    }

    #[test]
    fn srt_roundtrip() {
        let mut p = Project::empty();
        let v = p.add_track(TrackKind::Video, None);
        let cues = [(4.0, 2.0, "Deux\nlignes"), (1.0, 2.5, "Bonjour")];
        for (s, l, t) in cues {
            let g = Generator::Subtitle { text: t.into(), size_pct: 5 };
            let id = p.add_media(Media::basic(t.into(), MediaSource::Generated(g), true, false, 0.0, 1920, 1080, p.fps));
            let m = p.media(id).unwrap().clone();
            push(&mut p, v, &m, s, l, 0.0, 1.0);
        }
        let txt = export_srt(&p);
        assert!(txt.starts_with("1\r\n00:00:01,000 --> 00:00:03,500\r\nBonjour\r\n"), "{}", txt);
        let q = roundtrip("rt.srt", &txt);
        assert_eq!(q.tracks.len(), 1);
        assert_eq!(q.tracks[0].name, "Sous-titres");
        let evs = &q.tracks[0].events;
        assert_eq!(evs.len(), 2);
        assert!((evs[0].start - 1.0).abs() < 1e-3 && (evs[0].length - 2.5).abs() < 1e-3);
        assert!((evs[1].start - 4.0).abs() < 1e-3 && (evs[1].length - 2.0).abs() < 1e-3);
        let m = q.media(evs[1].media).unwrap();
        assert!(matches!(&m.source, MediaSource::Generated(Generator::Subtitle { text, size_pct: 5 }) if text == "Deux\nlignes"));
    }

    #[test]
    fn edl_cmx3600_sample() {
        let edl = "TITLE: SAMPLE\r\nFCM: NON-DROP FRAME\r\n\r\n\
001  AX       V     C        00:00:10:00 00:00:15:00 01:00:00:00 01:00:05:00\r\n\
* FROM CLIP NAME: plan1.mov\r\n\
* SOURCE FILE: /introuvable/plan1.mov\r\n\
\r\n\
002  AX       V     C        00:00:15:00 00:00:15:00 01:00:05:00 01:00:05:00\r\n\
002  AX       V     D    025 00:01:00:00 00:01:04:00 01:00:05:00 01:00:09:00\r\n\
* FROM CLIP NAME: plan1.mov\r\n\
* TO CLIP NAME: plan2.mov\r\n\
\r\n\
003  AX       A     C        00:00:00:00 00:00:09:00 01:00:00:00 01:00:09:00\r\n\
* FROM CLIP NAME: son.wav\r\n\
\r\n\
004  BL       V     C        00:00:00:00 00:00:01:00 01:00:09:00 01:00:10:00\r\n\
005  AX       A2    C        00:00:02:00 00:00:04:00 01:00:02:00 01:00:03:00\r\n\
M2   AX       050.0                00:00:02:00\r\n\
* FROM CLIP NAME: son.wav\r\n";
        let path = tmp("sample.edl", edl.as_bytes());
        let mut p = Project::empty();
        p.fps = 25.0;
        let msg = import(&path, &mut p, &no_probe).unwrap();
        assert!(msg.starts_with("5 événements importés"), "{}", msg);
        assert_eq!(p.tracks.len(), 3);
        assert_eq!(p.tracks[0].kind, TrackKind::Video);
        let v = &p.tracks[0].events;
        assert_eq!(v.len(), 3);
        assert!(v[0].start.abs() < 1e-9 && (v[0].length - 6.0).abs() < 1e-9 && (v[0].offset - 10.0).abs() < 1e-9);
        assert!((v[1].start - 5.0).abs() < 1e-9 && (v[1].length - 4.0).abs() < 1e-9 && (v[1].offset - 60.0).abs() < 1e-9);
        assert_eq!(v[1].name, "plan2.mov");
        assert!((v[2].start - 9.0).abs() < 1e-9);
        assert!(matches!(p.media(v[2].media).unwrap().source, MediaSource::Generated(Generator::SolidColor(_))));
        let a1 = &p.tracks[1].events;
        assert_eq!(a1.len(), 1);
        assert!((a1[0].length - 9.0).abs() < 1e-9);
        let a2 = &p.tracks[2].events;
        assert!((a2[0].start - 2.0).abs() < 1e-9 && (a2[0].rate - 2.0).abs() < 1e-9);
        let son = p.media(a1[0].media).unwrap();
        assert!(!son.has_video && son.has_audio && son.duration >= 9.0);
        assert_eq!(p.media(v[0].media).unwrap().path(), Some("/introuvable/plan1.mov"));
    }

    #[test]
    fn bwf_reference() {
        let mut fmt: Vec<u8> = vec![];
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&48_000u32.to_le_bytes());
        fmt.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        fmt.extend_from_slice(&4u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        let mut bext = vec![0u8; 602];
        bext[338..346].copy_from_slice(&(48_000u64 * 3600).to_le_bytes());
        let mut body: Vec<u8> = b"WAVE".to_vec();
        for (id, data) in [(b"bext", bext), (b"fmt ", fmt), (b"data", vec![])] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(&data);
        }
        let mut b: Vec<u8> = b"RIFF".to_vec();
        b.extend_from_slice(&(body.len() as u32).to_le_bytes());
        b.extend_from_slice(&body);
        let path = tmp("bwf.wav", &b);
        assert_eq!(bwf_time_reference(&path), Some(3600.0));
    }
}
