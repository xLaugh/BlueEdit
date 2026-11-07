#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Blue Edit ("Blue"): a track-based video editor in the spirit of the classic
//! Windows NLEs, written in Rust with egui.

mod ai;
mod analysis;
mod audio_fx;
mod color;
mod i18n_asia;
mod i18n_eu;
mod interchange;
mod loudness;
mod scopes;
mod ai_tools;
mod ai_ui;
mod app;
mod commands;
mod dialogs;
mod dialogs2;
mod docks;
mod files;
mod keymap;
mod media_tools;
mod menus;
mod prefs;
mod record;
mod tabs;
mod tools;
mod app_icon;
mod audio;
mod claude;
mod compositor;
mod events;
mod export;
mod fx_pack;
mod i18n;
mod icons;
mod media;
mod model;
mod panels;
mod platform;
mod plugins;
mod theme;
mod timeline;
mod windows;

fn main() -> eframe::Result<()> {
    i18n::init();
    let args: Vec<String> = std::env::args().collect();
    // `--write-icon <file.png> [size]`: used by the packaging scripts.
    if let Some(out) = args.iter().position(|a| a == "--write-icon").and_then(|i| args.get(i + 1)) {
        let size = args
            .iter()
            .position(|a| a == "--write-icon")
            .and_then(|i| args.get(i + 2))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        if let Err(e) = app_icon::write_png(out, size) {
            eprintln!("{e}");
        }
        return Ok(());
    }
    let shot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    // `--render <file>`: render the demo project without opening a window.
    if let Some(out) = args.iter().position(|a| a == "--render").and_then(|i| args.get(i + 1)) {
        let p = app::demo_project();
        let store = std::sync::Arc::new(media::MediaStore::default());
        let ctx = egui::Context::default();
        for m in &p.media {
            store.ensure(m, &ctx);
        }
        let mut settings = export::RenderSettings::for_project(&p);
        settings.format = match media::ext_of(out).as_str() {
            "wav" => export::Format::Wav,
            "webm" => export::Format::WebM,
            "mov" => export::Format::Mov,
            "mkv" => export::Format::Mkv,
            _ => export::Format::Mp4,
        };
        if settings.format == export::Format::WebM {
            settings.codec = export::Codec::Vp9;
        }
        // `--template <n>`: use render template n (e.g. a vertical one)
        if let Some(n) = args.iter().position(|a| a == "--template").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<usize>().ok()) {
            if let Some(t) = export::TEMPLATES.get(n) {
                settings.apply_template(t);
                settings.codec = export::Codec::H264;
                settings.ten_bit = false;
            }
        }
        let end = p.end_time();
        let job = export::start(p, store, out.clone(), settings, (0.0, end), true);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let pr = job.progress.lock().unwrap();
            if pr.done {
                match &pr.error {
                    Some(e) => eprintln!("{e}"),
                    None => println!("OK {}", out),
                }
                return Ok(());
            }
        }
    }
    // `--selftest-ai <wav>`: run the auto-trim analysis and transcription on a file.
    if let Some(wav) = args.iter().position(|a| a == "--selftest-ai").and_then(|i| args.get(i + 1)) {
        let Some(pcm) = media::decode_audio(wav) else {
            eprintln!("cannot decode {wav}");
            return Ok(());
        };
        let dur = pcm.duration();
        let levels = ai::frame_levels(&pcm.samples, media::AUDIO_RATE, 0.0, dur);
        let words = match ai::find_models().first() {
            Some(m) => ai::transcribe(std::path::Path::new(wav), m, "fr", 1).unwrap_or_default(),
            None => vec![],
        };
        let lines = match ai::find_models().first() {
            Some(m) => ai::transcribe(std::path::Path::new(wav), m, "fr", 42).unwrap_or_default(),
            None => vec![],
        };
        let r = ai::compute(levels, dur, &words, &ai::TrimParams::default());
        println!("duration {dur:.2}s threshold {:.1} dB kept {:.2}s fillers {}", r.threshold_db, r.kept(), r.fillers);
        for (a, b) in &r.keep {
            println!("  keep {a:.2}-{b:.2}");
        }
        print!("{}", ai::to_srt(&lines));
        println!("ofx plugins: {}", plugins::scan_ofx().len());
        return Ok(());
    }
    // `--selftest-claude`: send one request with an invalid key (checks the
    // network path and error handling without using a real key).
    if args.iter().any(|a| a == "--selftest-claude") {
        let mut chat = claude::Chat::new();
        chat.key = "sk-ant-invalid-selftest".into();
        chat.messages.push(serde_json::json!({"role": "user", "content": "test"}));
        chat.send("Projet de test".into());
        let r = chat.pending.take().unwrap().recv().unwrap();
        println!("{:?}", r.err());
        println!("tools: {}", claude::tools().as_array().map(|a| a.len()).unwrap_or(0));
        return Ok(());
    }
    // `--media <file>` (repeatable): import files and put them on the timeline at start.
    let media: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--media")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 940.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("Blue Edit")
            .with_app_id("blue-edit")
            .with_icon(std::sync::Arc::new(app_icon::icon_data(128))),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Blue Edit",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, shot.clone(), media.clone())))),
    )
}
