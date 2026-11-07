//! Audio recording from the default input device into armed tracks
//! (fRecordAsBroadcastWave = 1: files get a 'bext' chunk with the time stamp).

use std::sync::{Arc, Mutex};

pub struct Recorder {
    _stream: cpal::Stream,
    pub samples: Arc<Mutex<Vec<f32>>>,
    pub rate: u32,
    pub start: f64,
    pub device: String,
}

impl Recorder {
    /// Opens the default input and starts capturing (stereo f32).
    pub fn start(at: f64) -> Result<Recorder, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| crate::i18n::tr("Aucune entrée audio (micro) trouvée.").to_string())?;
        let name = device.name().unwrap_or_default();
        let cfg = device.default_input_config().map_err(|e| e.to_string())?;
        let config: cpal::StreamConfig = cfg.config();
        let ch = config.channels as usize;
        let rate = config.sample_rate.0;
        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * 60 * 2)));
        let s2 = samples.clone();
        let err = |_e: cpal::StreamError| {};
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| push(&s2, data.iter().copied(), ch),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| push(&s2, data.iter().map(|v| *v as f32 / 32768.0), ch),
                err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| push(&s2, data.iter().map(|v| (*v as f32 - 32768.0) / 32768.0), ch),
                err,
                None,
            ),
            _ => return Err("Format d'entrée non pris en charge.".into()),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder { _stream: stream, samples, rate, start: at, device: name })
    }

    pub fn seconds(&self) -> f64 {
        self.samples.lock().map(|s| s.len() as f64 / 2.0 / self.rate as f64).unwrap_or(0.0)
    }

    /// Current input peak (for the record meter).
    pub fn level(&self) -> f32 {
        let s = self.samples.lock().unwrap();
        let n = (self.rate as usize / 10 * 2).min(s.len());
        s[s.len() - n..].iter().fold(0.0f32, |a, v| a.max(v.abs()))
    }

    /// Stops and writes a 16-bit WAV (Broadcast WAV with a bext chunk when
    /// `bwf`), returns its path.
    pub fn finish(self, path: &std::path::Path, bwf: bool) -> std::io::Result<()> {
        let samples = std::mem::take(&mut *self.samples.lock().unwrap());
        drop(self._stream);
        write_bwf(path, &samples, self.rate, if bwf { Some(self.start) } else { None })
    }
}

fn push(buf: &Arc<Mutex<Vec<f32>>>, data: impl Iterator<Item = f32>, ch: usize) {
    let v: Vec<f32> = data.collect();
    let mut b = buf.lock().unwrap();
    for frame in v.chunks(ch.max(1)) {
        let l = frame[0];
        let r = if frame.len() > 1 { frame[1] } else { l };
        b.push(l);
        b.push(r);
    }
}

/// WAV writer; with a time reference it adds a minimal 'bext' chunk
/// (602 bytes, TimeReference = samples since the project start).
pub fn write_bwf(path: &std::path::Path, samples: &[f32], rate: u32, time_ref: Option<f64>) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = (samples.len() * 2) as u32;
    let bext_len: u32 = if time_ref.is_some() { 602 } else { 0 };
    let riff = 4 + (8 + 16) + if bext_len > 0 { 8 + bext_len } else { 0 } + 8 + data_len;
    f.write_all(b"RIFF")?;
    f.write_all(&riff.to_le_bytes())?;
    f.write_all(b"WAVE")?;
    if let Some(t) = time_ref {
        f.write_all(b"bext")?;
        f.write_all(&bext_len.to_le_bytes())?;
        let mut b = vec![0u8; bext_len as usize];
        let desc = b"Blue Edit recording";
        b[..desc.len()].copy_from_slice(desc);
        let orig = b"Blue Edit";
        b[256..256 + orig.len()].copy_from_slice(orig);
        let tr = (t.max(0.0) * rate as f64).round() as u64;
        b[338..346].copy_from_slice(&tr.to_le_bytes());
        b[346..348].copy_from_slice(&1u16.to_le_bytes());
        f.write_all(&b)?;
    }
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}

impl crate::app::App {
    /// Folder for recordings: preference, else the project's folder, else Videos.
    pub fn record_dir(&self) -> std::path::PathBuf {
        if !self.prefs.record_dir.trim().is_empty() {
            return std::path::PathBuf::from(&self.prefs.record_dir);
        }
        self.path
            .as_ref()
            .and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()))
            .or_else(crate::platform::videos_dir)
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Record button: starts recording into the armed track (playback runs
    /// along, fSRP = 1), or stops and places the new take.
    pub fn toggle_record(&mut self, ctx: &egui::Context) {
        use crate::i18n::tr;
        use crate::model::*;
        if let Some(r) = self.recorder.take() {
            let start = r.start;
            let dir = self.record_dir();
            let _ = std::fs::create_dir_all(&dir);
            let mut k = 1;
            let path = loop {
                let c = dir.join(format!("{} {}{:03}.wav", self.p.name, tr("Enregistrement "), k));
                if !c.exists() {
                    break c;
                }
                k += 1;
            };
            self.playing = false;
            match r.finish(&path, self.prefs.record_bwf) {
                Ok(()) => {
                    let ids = self.import_files(vec![path.to_string_lossy().to_string()], ctx);
                    if let Some(id) = ids.first().copied() {
                        self.push_undo();
                        let ti = self
                            .p
                            .tracks
                            .iter()
                            .position(|t| t.kind == TrackKind::Audio && t.armed)
                            .unwrap_or_else(|| self.p.add_track(TrackKind::Audio, None));
                        if let Some(m) = self.p.media(id).cloned() {
                            let len = m.duration.max(0.05);
                            let e = self.p.new_event(&m, start, len);
                            self.p.tracks[ti].events.push(e);
                            self.p.sort_events();
                        }
                        self.status = format!("{} {}", tr("Enregistrement ajouté :"), path.display());
                    }
                }
                Err(e) => self.status = format!("{} {e}", tr("Échec de l'enregistrement :")),
            }
            return;
        }
        if !self.p.tracks.iter().any(|t| t.kind == TrackKind::Audio && t.armed) {
            // fRecordWarningPrompt: arm the focused audio track (or a new one)
            let ti = self
                .p
                .tracks
                .get(self.focus_track)
                .filter(|t| t.kind == TrackKind::Audio)
                .map(|_| self.focus_track)
                .unwrap_or_else(|| self.p.add_track(TrackKind::Audio, None));
            self.p.tracks[ti].armed = true;
            self.status = tr("Aucune piste n'était armée : la piste audio active a été armée.").into();
        }
        match Recorder::start(self.cursor) {
            Ok(r) => {
                self.status = format!("{} {}", tr("Enregistrement depuis :"), r.device);
                self.recorder = Some(r);
                self.play();
            }
            Err(e) => self.status = e,
        }
    }
}
