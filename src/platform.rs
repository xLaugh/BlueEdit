//! Everything that differs between Windows, macOS and Linux: data folder,
//! home and video folders, opening a folder in the file manager, finding
//! ffmpeg / ffprobe, and the standard OpenFX plug-in folders.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|_| cfg!(windows))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// "major.minor" of this program, used for the data folder
/// (app name + version folder).
pub fn version_folder() -> String {
    let v = env!("CARGO_PKG_VERSION");
    let mut it = v.split('.');
    format!("{}.{}", it.next().unwrap_or("0"), it.next().unwrap_or("0"))
}

/// Where settings, plug-ins and models are kept.
/// Here: Windows %LOCALAPPDATA%\Blue Edit\<version>,
/// macOS ~/Library/Application Support/Blue Edit/<version>,
/// Linux $XDG_DATA_HOME (or ~/.local/share)/Blue Edit/<version>.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")).map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home().map(|h| h.join("Library").join("Application Support"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| home().map(|h| h.join(".local").join("share")))
        };
        let base = base.unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("Blue Edit").join(version_folder());
        // the program was called "Bleu Montage" before
        let previous = base.join("Bleu Montage").join(version_folder());
        if !dir.exists() && previous.is_dir() {
            let _ = std::fs::create_dir_all(base.join("Blue Edit"));
            let _ = std::fs::rename(&previous, &dir);
        }
        migrate_old_data(&dir);
        dir
    })
    .clone()
}

/// Earlier versions kept their data in %APPDATA%\BleuMontage (or
/// .../BleuMontage on macOS/Linux): move it once to the new folder.
fn migrate_old_data(new_dir: &std::path::Path) {
    if new_dir.exists() {
        return;
    }
    let old = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("BleuMontage"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support").join("BleuMontage"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local").join("share")))
            .map(|b| b.join("BleuMontage"))
    };
    if let Some(old) = old.filter(|o| o.is_dir()) {
        if let Some(parent) = new_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&old, new_dir);
    }
}

/// Other programs' data folders (used to find whisper models already installed).
pub fn other_app_data(name: &str) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(a) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(a).join(name));
    }
    if let Some(a) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(a).join(name));
    }
    if let Some(h) = home() {
        v.push(h.join("Library").join("Application Support").join(name));
        v.push(h.join(".local").join("share").join(name));
    }
    v
}

/// The user's video folder ("Videos" on Windows/Linux, "Movies" on macOS).
pub fn videos_dir() -> Option<PathBuf> {
    let h = home()?;
    let name = if cfg!(target_os = "macos") { "Movies" } else { "Videos" };
    let v = h.join(name);
    Some(if v.is_dir() { v } else { h })
}

/// Opens a folder in Explorer / Finder / the desktop's file manager.
pub fn open_folder(dir: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(dir).spawn();
}

/// Full path of ffmpeg / ffprobe. A program started from the Finder or a
/// desktop menu may not see the shell's PATH, so the usual install folders
/// (Homebrew, MacPorts, /usr/local, next to the program) are checked too.
pub fn tool(name: &str) -> PathBuf {
    static FFMPEG: OnceLock<PathBuf> = OnceLock::new();
    static FFPROBE: OnceLock<PathBuf> = OnceLock::new();
    let cell = if name == "ffprobe" { &FFPROBE } else { &FFMPEG };
    cell.get_or_init(|| find_tool(name)).clone()
}

fn find_tool(name: &str) -> PathBuf {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        dirs.push(d.clone());
        dirs.push(d.join("ffmpeg"));
        // inside a macOS .app bundle: Contents/MacOS -> Contents/Resources
        dirs.push(d.join("..").join("Resources"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if !cfg!(windows) {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin", "/snap/bin"] {
            dirs.push(PathBuf::from(d));
        }
    }
    for d in dirs {
        let p = d.join(&exe);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from(name)
}

/// A command for an external tool, without a console window on Windows.
pub fn command(name: &str) -> Command {
    let program = if name == "ffmpeg" || name == "ffprobe" { tool(name) } else { PathBuf::from(name) };
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

/// Standard OpenFX folders for this system (the usual Windows order on
/// Windows; the OpenFX standard paths are used on macOS and Linux).
pub fn ofx_standard_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let literal = PathBuf::from(r"C:\Program Files\Common Files\OFX\Plugins");
        let mut v = vec![];
        if let Some(common) = std::env::var_os("CommonProgramFiles").map(|c| PathBuf::from(c).join("OFX").join("Plugins")) {
            if common.to_string_lossy().to_lowercase() != literal.to_string_lossy().to_lowercase() {
                v.push(common);
            }
        }
        v.push(literal);
        v
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/OFX/Plugins")]
    } else {
        vec![PathBuf::from("/usr/OFX/Plugins")]
    }
}

/// Folder holding the binary inside an *.ofx.bundle for this system.
pub fn ofx_binary_dir() -> &'static str {
    if cfg!(windows) {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else if cfg!(target_arch = "aarch64") {
        "Linux-arm-64"
    } else {
        "Linux-x86-64"
    }
}

/// "Ctrl" shortcuts are shown as "Cmd" on macOS (egui maps them to ⌘).
pub fn shortcut(s: &'static str) -> String {
    if cfg!(target_os = "macos") {
        s.replace("Ctrl+", "Cmd+")
    } else {
        s.to_string()
    }
}

/// Free bytes on the disk holding `path` (status bar: recording time left).
pub fn free_space(path: &std::path::Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        }
        let mut p = path.to_path_buf();
        while !p.exists() {
            p = p.parent()?.to_path_buf();
        }
        let w: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) };
        if ok != 0 {
            Some(a)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        static CACHE: std::sync::Mutex<Option<(std::time::Instant, u64)>> = std::sync::Mutex::new(None);
        if let Some((t, v)) = *CACHE.lock().ok()? {
            if t.elapsed().as_secs() < 10 {
                return Some(v);
            }
        }
        let out = std::process::Command::new("df").arg("-Pk").arg(path).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let v = s.lines().nth(1)?.split_whitespace().nth(3)?.parse::<u64>().ok()? * 1024;
        *CACHE.lock().ok()? = Some((std::time::Instant::now(), v));
        Some(v)
    }
}
