//! User preferences (Options > Préférences), saved as JSON in the data folder.
//! Defaults follow the usual values of classic NLEs.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// fEnableAutoSave = 1
    pub autosave: bool,
    /// msAutoSaveInterval = 300000 ms (5 minutes)
    pub autosave_ms: u64,
    /// cQuickfileMaxFiles = 6 recent files in the File menu
    pub recent_max: usize,
    /// fQuickfileShowFullPath = 1
    pub recent_full_path: bool,
    /// szSoundEditorExe: external audio editor
    pub sound_editor: String,
    /// Undo levels kept in memory.
    pub undo_levels: usize,
    /// cPasteRepeatObjectCount = 1, cPasteRepeatMax = 100
    pub paste_repeat_count: u32,
    /// cMetronomeCountInBars = 2
    pub metronome_count_in: u32,
    /// fCreateAudioProxyFiles / proxies for the preview
    pub use_proxies: bool,
    /// fOperateClipboardOnGroups = 1
    pub clipboard_on_groups: bool,
    /// fRecordAsBroadcastWave = 1
    pub record_bwf: bool,
    /// Record folder (szRecordPath); empty = project folder or Videos.
    pub record_dir: String,
    /// Show envelopes on the timeline.
    pub show_envelopes: bool,
    /// Visible toolbar buttons (customise toolbar), by key.
    pub toolbar_hidden: Vec<String>,
    /// "What's this?" help on hover.
    pub whats_this: bool,
    /// Hide the dock tab labels.
    pub hide_tab_labels: bool,
    /// Last chosen text-to-speech voice (system name), empty = default.
    pub tts_voice: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            autosave: true,
            autosave_ms: 300_000,
            recent_max: 6,
            recent_full_path: true,
            sound_editor: String::new(),
            undo_levels: 100,
            paste_repeat_count: 1,
            metronome_count_in: 2,
            use_proxies: false,
            clipboard_on_groups: true,
            record_bwf: true,
            record_dir: String::new(),
            show_envelopes: true,
            toolbar_hidden: vec![],
            whats_this: false,
            hide_tab_labels: false,
            tts_voice: String::new(),
        }
    }
}

fn path() -> std::path::PathBuf {
    crate::plugins::data_dir().join("preferences.json")
}

impl Prefs {
    pub fn load() -> Prefs {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::plugins::data_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path(), s);
        }
    }
}

/// Recent project files (most recent first).
pub fn load_recent() -> Vec<String> {
    std::fs::read_to_string(crate::plugins::data_dir().join("recents.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

pub fn save_recent(list: &[String]) {
    let _ = std::fs::create_dir_all(crate::plugins::data_dir());
    let _ = std::fs::write(crate::plugins::data_dir().join("recents.txt"), list.join("\n"));
}
