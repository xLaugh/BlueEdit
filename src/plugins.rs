//! Plugins: installable effect packs (.bluefx JSON files holding chains of
//! effects) and detection of OFX plugins installed for other editors.
//! OFX binaries are listed but not loaded (no OFX host yet); when one has a
//! built-in look-alike, the manager offers it.

use crate::model::VideoFx;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginEffect {
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    pub chain: Vec<VideoFx>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginPack {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    pub effects: Vec<PluginEffect>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
    #[serde(skip)]
    pub builtin: bool,
    #[serde(skip)]
    pub disabled: bool,
}

#[derive(Clone, Debug)]
pub struct OfxPlugin {
    pub name: String,
    pub vendor: String,
    pub category: String,
    pub path: PathBuf,
    pub equivalent: Option<Vec<VideoFx>>,
}

/// Effect packs are saved as ".bluefx"; ".bmfx" (earlier versions) and
/// ".json" are still read.
pub const PLUGIN_EXT: &str = "bluefx";
pub const PLUGIN_EXTS: &[&str] = &["bluefx", "bmfx", "json"];
pub const PLUGIN_FILTER: &str = "Plugin Blue Edit";

pub fn data_dir() -> PathBuf {
    crate::platform::data_dir()
}

pub fn plugins_dir() -> PathBuf {
    data_dir().join("plugins")
}

fn one(name: &str, cat: &str, desc: &str, chain: Vec<VideoFx>) -> PluginEffect {
    PluginEffect {
        name: name.into(),
        category: cat.into(),
        description: desc.into(),
        chain,
    }
}

/// The pack shipped with the program.
pub fn builtin_pack() -> PluginPack {
    let mut effects: Vec<PluginEffect> = VideoFx::pack()
        .into_iter()
        .map(|f| one(f.label(), f.category(), "", vec![f]))
        .collect();
    effects.push(one(
        "Rétro 90",
        "Préréglages",
        "VHS + grain + vignettage",
        vec![
            VideoFx::Vhs { amount: 0.7 },
            VideoFx::Grain { amount: 0.3 },
            VideoFx::Vignette { amount: 0.5 },
        ],
    ));
    effects.push(one(
        "Clip musical",
        "Préréglages",
        "Lueur + séparation RVB + tremblement",
        vec![
            VideoFx::Glow { radius: 14.0, intensity: 1.0, threshold: 0.55 },
            VideoFx::RgbSplit { amount: 0.5 },
            VideoFx::Shake { amount: 0.25 },
        ],
    ));
    effects.push(one(
        "Film noir",
        "Préréglages",
        "Noir moderne + grain + vignettage",
        vec![
            VideoFx::Noir { contrast: 0.7 },
            VideoFx::Grain { amount: 0.25 },
            VideoFx::Vignette { amount: 0.7 },
        ],
    ));
    PluginPack {
        name: "Univers (intégré)".into(),
        author: "Blue Edit".into(),
        version: "1.0".into(),
        effects,
        path: None,
        builtin: true,
        disabled: false,
    }
}

pub fn load_installed() -> Vec<PluginPack> {
    let mut packs = vec![];
    if let Ok(rd) = std::fs::read_dir(plugins_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !PLUGIN_EXTS.contains(&ext.as_str()) {
                continue;
            }
            if let Ok(mut pack) = read_pack(&p) {
                pack.path = Some(p);
                packs.push(pack);
            }
        }
    }
    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packs
}

pub fn read_pack(path: &Path) -> Result<PluginPack, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let pack: PluginPack = serde_json::from_str(&s).map_err(|e| format!("fichier de plugin invalide : {e}"))?;
    if pack.name.trim().is_empty() || pack.effects.is_empty() {
        return Err("le plugin n'a pas de nom ou ne contient aucun effet".into());
    }
    Ok(pack)
}

/// Copies a pack file into the plugins folder after checking it.
pub fn install(src: &Path) -> Result<String, String> {
    let pack = read_pack(src)?;
    let dir = plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = pack
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = dir.join(format!("{safe}.{PLUGIN_EXT}"));
    std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(pack.name)
}

pub fn uninstall(pack: &PluginPack) -> Result<(), String> {
    match &pack.path {
        Some(p) => std::fs::remove_file(p).map_err(|e| e.to_string()),
        None => Err("ce pack est intégré au logiciel".into()),
    }
}

/// Writes a pack holding one effect chain (used to share an event's effects).
pub fn save_pack(path: &Path, pack_name: &str, effect_name: &str, chain: Vec<VideoFx>) -> Result<(), String> {
    let pack = PluginPack {
        name: pack_name.into(),
        author: String::new(),
        version: "1.0".into(),
        effects: vec![one(effect_name, "Mes effets", "", chain)],
        path: None,
        builtin: false,
        disabled: false,
    };
    let s = serde_json::to_string_pretty(&pack).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

/// Closest built-in look for a known OFX effect name.
fn equivalent(name: &str) -> Option<Vec<VideoFx>> {
    let n = name.to_lowercase();
    let pick = |k: &str| VideoFx::from_key(k).map(|f| vec![f]);
    if n.contains("glow") || n.contains("glimmer") || n.contains("light factory") || n.contains("overlight") {
        pick("glow")
    } else if n.contains("chromatic") || n.contains("rgb separation") || n.contains("channel") || n.contains("prism") {
        pick("rgb_split")
    } else if n.contains("glitch") || n.contains("misfire") || n.contains("flicker") {
        pick("glitch")
    } else if n.contains("vhs") || n.contains("analog") || n.contains("retrograde") {
        pick("vhs")
    } else if n.contains("grain") || n.contains("texturize") {
        pick("grain")
    } else if n.contains("shake") || n.contains("heatwave") {
        pick("shake")
    } else if n.contains("multitone") || n.contains("chromatown") {
        pick("duotone")
    } else if n.contains("mosaic") || n.contains("symbol mapper") {
        pick("pixelate")
    } else if n.contains("noir") {
        pick("noir")
    } else if n.contains("blur") || n.contains("bokeh") {
        pick("blur")
    } else {
        None
    }
}

/// OFX search folders, in this order:
/// 1. "OFX Video Plug-Ins" next to the program, 2. every entry of the
/// OFX_PLUGIN_PATH variable (split on ';'), 3. the Common Files folder
/// (CSIDL 0x2b) + "\OFX\Plugins" when it differs from 4. the literal
/// "C:\Program Files\Common Files\OFX\Plugins". On macOS and Linux the
/// OpenFX standard folders are used (/Library/OFX/Plugins, /usr/OFX/Plugins).
pub fn ofx_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        v.push(dir.join("OFX Video Plug-Ins"));
    }
    if let Some(extra) = std::env::var_os("OFX_PLUGIN_PATH") {
        for p in extra.to_string_lossy().split(';').filter(|s| !s.trim().is_empty()) {
            v.push(PathBuf::from(p));
        }
    }
    v.extend(crate::platform::ofx_standard_dirs());
    v
}

/// Folder walk: in each folder, *.DLL and
/// *.OFX files are candidates; sub-folders are searched recursively, except
/// "." / ".." and names starting with '@'; a "*.ofx.bundle" folder is entered
/// through "Contents\Win64".
fn walk(dir: &Path, vendor: &str, seen: &mut std::collections::HashSet<String>, out: &mut Vec<OfxPlugin>, depth: u32) {
    if depth > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subdirs = vec![];
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if p.is_dir() {
            if name.starts_with('@') || name == "." || name == ".." {
                continue;
            }
            if lower.ends_with(".ofx.bundle") {
                subdirs.push((p.join("Contents").join(crate::platform::ofx_binary_dir()), true));
            } else {
                subdirs.push((p, false));
            }
        } else if lower.ends_with(".ofx") {
            let key = lower.clone();
            if !seen.insert(key) {
                continue;
            }
            out.push(describe(&p, vendor));
        }
    }
    for (d, _bundle) in subdirs {
        let v = if depth == 0 {
            d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| vendor.to_string())
        } else {
            vendor.to_string()
        };
        walk(&d, &v, seen, out, depth + 1);
    }
}

fn describe(file: &Path, vendor: &str) -> OfxPlugin {
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = stem.trim_end_matches("_OFX");
    let mut parts: Vec<&str> = stem.split('_').collect();
    if parts.first() == Some(&"Universe") {
        parts.remove(0);
    }
    let (category, rest) = if parts.len() > 1 {
        if parts[0] == "Motion" && parts.get(1) == Some(&"Graphics") {
            ("Motion Graphics".to_string(), parts[2..].join(" "))
        } else {
            (parts[0].to_string(), parts[1..].join(" "))
        }
    } else {
        ("Divers".to_string(), parts.join(" "))
    };
    let name = if rest.is_empty() { stem.replace('_', " ") } else { rest };
    let vendor = if vendor.is_empty() || vendor.ends_with(".ofx.bundle") { "OFX".to_string() } else { vendor.to_string() };
    OfxPlugin {
        equivalent: equivalent(&name),
        name,
        vendor,
        category,
        path: file.to_path_buf(),
    }
}

pub fn scan_ofx() -> Vec<OfxPlugin> {
    let mut out = vec![];
    let mut seen = std::collections::HashSet::new();
    for root in ofx_dirs() {
        walk(&root, "OFX", &mut seen, &mut out, 0);
    }
    out.sort_by(|a, b| (a.vendor.clone(), a.category.clone(), a.name.clone()).cmp(&(b.vendor.clone(), b.category.clone(), b.name.clone())));
    out
}

pub struct PluginManager {
    pub packs: Vec<PluginPack>,
    pub ofx: Vec<OfxPlugin>,
    pub scanned: bool,
}

impl PluginManager {
    pub fn new() -> PluginManager {
        let mut packs = vec![builtin_pack()];
        packs.extend(load_installed());
        PluginManager {
            packs,
            ofx: vec![],
            scanned: false,
        }
    }
    pub fn reload(&mut self) {
        let disabled: Vec<String> = self.packs.iter().filter(|p| p.disabled).map(|p| p.name.clone()).collect();
        let mut packs = vec![builtin_pack()];
        packs.extend(load_installed());
        for p in &mut packs {
            p.disabled = disabled.contains(&p.name);
        }
        self.packs = packs;
    }
    pub fn rescan_ofx(&mut self) {
        self.ofx = scan_ofx();
        self.scanned = true;
    }
}
