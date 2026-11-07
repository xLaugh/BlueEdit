//! Keyboard shortcuts as editable bindings (Options > Personnaliser le clavier).
//! Defaults follow the usual shortcuts of classic NLEs. Mods: 0x02 Shift,
//! 0x04 Ctrl, 0x08 Alt.

use crate::i18n::tr;
use egui::{Key, Modifiers};
use std::collections::HashMap;

pub struct Cmd {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static [&'static str],
}

macro_rules! c {
    ($id:expr, $label:expr, [$($k:expr),*]) => {
        Cmd { id: $id, label: $label, keys: &[$($k),*] }
    };
}

/// Every command that can have a shortcut, with its default shortcut.
pub const COMMANDS: &[Cmd] = &[
    c!("play_toggle", "Lecture / arrêt", ["Space"]),
    c!("play_start", "Lire depuis le début", ["Shift+Space"]),
    c!("play_pause", "Lecture / pause", ["Enter"]),
    c!("stop", "Arrêt", ["Escape"]),
    c!("project_props", "Propriétés du projet", ["Alt+Enter"]),
    c!("new", "Nouveau projet", ["Ctrl+N"]),
    c!("open", "Ouvrir", ["Ctrl+O"]),
    c!("save", "Enregistrer", ["Ctrl+S"]),
    c!("save_as", "Enregistrer sous", ["Ctrl+Shift+S"]),
    c!("save_inc", "Enregistrement incrémental", ["Ctrl+Alt+S"]),
    c!("import", "Importer un média", ["Ctrl+I"]),
    c!("render", "Rendre en tant que", ["Ctrl+R"]),
    c!("render_new_track", "Rendre vers une nouvelle piste", ["Ctrl+M"]),
    c!("split", "Diviser", ["S"]),
    c!("delete", "Supprimer", ["Delete"]),
    c!("undo", "Annuler", ["Ctrl+Z"]),
    c!("redo", "Rétablir", ["Ctrl+Y", "Ctrl+Shift+Z"]),
    c!("copy", "Copier", ["Ctrl+C"]),
    c!("cut", "Couper", ["Ctrl+X"]),
    c!("paste", "Coller", ["Ctrl+V"]),
    c!("paste_insert", "Coller en insérant", ["Ctrl+Shift+V"]),
    c!("paste_repeat", "Coller plusieurs fois", ["Ctrl+B"]),
    c!("paste_attrs", "Coller les attributs de l'événement", ["Ctrl+Alt+V"]),
    c!("select_all", "Sélectionner tout", ["Ctrl+A"]),
    c!("heal", "Recoller (heal)", ["H"]),
    c!("trim", "Rogner à la sélection", ["Ctrl+T"]),
    c!("trim_start", "Rogner le début au curseur", ["Alt+OpenBracket"]),
    c!("trim_end", "Rogner la fin au curseur", ["Alt+CloseBracket"]),
    c!("take_next", "Prise suivante", ["T"]),
    c!("take_prev", "Prise précédente", ["Shift+T"]),
    c!("group", "Grouper", ["G"]),
    c!("ungroup", "Retirer du groupe", ["U"]),
    c!("auto_xf", "Fondu enchaîné automatique", ["Ctrl+Shift+X"]),
    c!("ripple", "Montage en cascade automatique", ["Ctrl+L"]),
    c!("loop", "Lecture en boucle", ["Q", "Ctrl+Shift+L"]),
    c!("add_audio_track", "Insérer une piste audio", ["Ctrl+Q"]),
    c!("add_video_track", "Insérer une piste vidéo", ["Ctrl+Shift+Q"]),
    c!("add_adjust_track", "Insérer une piste d'ajustement", ["Ctrl+Shift+Alt+Q"]),
    c!("marker", "Insérer un marqueur", ["M"]),
    c!("region", "Insérer une région", ["R"]),
    c!("command_marker", "Insérer un marqueur de commande", ["Alt+C"]),
    c!("mark_in", "Point d'entrée", ["I"]),
    c!("mark_out", "Point de sortie", ["O"]),
    c!("mute_track", "Muet (piste)", ["Z"]),
    c!("mute_excl", "Muet exclusif", ["Shift+Z"]),
    c!("solo_track", "Solo (piste)", ["X"]),
    c!("solo_excl", "Solo exclusif", ["Shift+X"]),
    c!("scrub_left", "Lecture arrière (J)", ["J"]),
    c!("scrub_pause", "Pause (K)", ["K"]),
    c!("scrub_right", "Lecture avant (L)", ["L"]),
    c!("snap", "Aimantation", ["F8"]),
    c!("snap_grid", "Aimanter à la grille", ["Ctrl+F8"]),
    c!("quantize", "Quantifier aux images", ["Alt+F8"]),
    c!("tool_normal", "Outil normal", ["Ctrl+D"]),
    c!("tool_next", "Outil suivant", ["D"]),
    c!("tool_prev", "Outil précédent", ["Shift+D"]),
    c!("show_env", "Afficher les enveloppes", ["Ctrl+Shift+E"]),
    c!("env_volume", "Insérer l'enveloppe de volume", ["Shift+V"]),
    c!("env_pan", "Insérer l'enveloppe de panoramique", ["Shift+P"]),
    c!("pitch_up", "Hauteur +1 demi-ton", ["Equals"]),
    c!("pitch_down", "Hauteur -1 demi-ton", ["Minus"]),
    c!("pitch_cent_up", "Hauteur +1 centième", ["Ctrl+Equals"]),
    c!("pitch_cent_down", "Hauteur -1 centième", ["Ctrl+Minus"]),
    c!("pitch_oct_up", "Hauteur +1 octave", ["Shift+Equals"]),
    c!("pitch_oct_down", "Hauteur -1 octave", ["Shift+Minus"]),
    c!("pitch_reset", "Hauteur d'origine", ["Ctrl+Shift+Equals"]),
    c!("nudge_left", "Décaler d'une image à gauche", ["Alt+Left"]),
    c!("nudge_right", "Décaler d'une image à droite", ["Alt+Right"]),
    c!("nudge_left_px", "Décaler d'un pixel à gauche", ["Ctrl+Alt+Left"]),
    c!("nudge_right_px", "Décaler d'un pixel à droite", ["Ctrl+Alt+Right"]),
    c!("swap_cursor", "Échanger curseur et sélection", ["Alt+O"]),
    c!("next_like", "Événement semblable suivant", ["Ctrl+Alt+N"]),
    c!("lock", "Verrouiller l'événement", ["Ctrl+Alt+L"]),
    c!("ram_preview", "Construire l'aperçu RAM", ["Shift+B"]),
    c!("min_tracks", "Hauteur de piste minimale", ["Backtick"]),
    c!("default_tracks", "Hauteur de piste par défaut", ["Ctrl+Backtick"]),
    c!("more_height", "Pistes plus hautes", ["Ctrl+Shift+Up"]),
    c!("less_height", "Pistes moins hautes", ["Ctrl+Shift+Down"]),
    c!("dock_toggle", "Réduire les zones d'ancrage", ["F11"]),
    c!("labels_toggle", "Masquer les étiquettes d'onglets", ["Ctrl+F11"]),
    c!("dim", "Atténuation (dim)", ["Ctrl+Shift+F12"]),
    c!("trimmer", "Rogneuse", ["Alt+2"]),
    c!("mixer", "Table de mixage", ["Alt+3"]),
    c!("scopes", "Oscilloscopes vidéo", ["Ctrl+Alt+2"]),
    c!("edit_details", "Détails de montage", ["Ctrl+Alt+4"]),
    c!("loudness", "Vu-mètres de sonie", ["Ctrl+Alt+7"]),
    c!("color_grading", "Étalonnage", ["Alt+G"]),
    c!("notes", "Notes du projet", ["Alt+6"]),
    c!("explorer", "Explorateur", ["Alt+1"]),
    c!("project_media", "Média du projet", ["Alt+5"]),
    c!("transitions", "Transitions", ["Alt+7"]),
    c!("video_fx", "Effets vidéo", ["Alt+8"]),
    c!("generators", "Générateurs de médias", ["Alt+9"]),
    c!("surround", "Panoramique surround", ["Ctrl+Alt+3"]),
    c!("scripts", "Éditeur de scripts", ["Ctrl+Alt+0"]),
    c!("help", "Aide", ["F1"]),
    c!("whats_this", "Qu'est-ce que c'est ?", ["Shift+F1"]),
    c!("open_nested", "Ouvrir comme timeline imbriquée", ["Alt+N"]),
    c!("home", "Aller au début", ["Home"]),
    c!("end", "Aller à la fin", ["End"]),
    c!("frame_prev", "Image précédente", ["Left"]),
    c!("frame_next", "Image suivante", ["Right"]),
    c!("zoom_in", "Zoom avant", ["Up"]),
    c!("zoom_out", "Zoom arrière", ["Down"]),
    c!("layout_default", "Disposition par défaut", ["Ctrl+Alt+D"]),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Combo {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Combo {
    pub fn parse(s: &str) -> Option<Combo> {
        let mut c = Combo { key: Key::Space, ctrl: false, shift: false, alt: false };
        let mut key = None;
        for part in s.split('+') {
            match part.trim() {
                "Ctrl" | "Cmd" => c.ctrl = true,
                "Shift" | "Maj" => c.shift = true,
                "Alt" => c.alt = true,
                k => key = Key::from_name(k),
            }
        }
        c.key = key?;
        Some(c)
    }
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str(if cfg!(target_os = "macos") { "Cmd+" } else { "Ctrl+" });
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.name());
        s
    }
    pub fn of(key: Key, m: Modifiers) -> Combo {
        Combo { key, ctrl: m.command, shift: m.shift, alt: m.alt }
    }
}

pub struct Keymap {
    /// command id → combos
    pub binds: HashMap<String, Vec<Combo>>,
}

fn file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("clavier.json")
}

impl Keymap {
    pub fn defaults() -> Keymap {
        let binds = COMMANDS.iter().map(|c| (c.id.to_string(), c.keys.iter().filter_map(|k| Combo::parse(k)).collect())).collect();
        Keymap { binds }
    }
    pub fn load() -> Keymap {
        let mut k = Keymap::defaults();
        if let Ok(s) = std::fs::read_to_string(file()) {
            if let Ok(m) = serde_json::from_str::<HashMap<String, Vec<String>>>(&s) {
                for (id, v) in m {
                    k.binds.insert(id, v.iter().filter_map(|x| Combo::parse(x)).collect());
                }
            }
        }
        k
    }
    pub fn save(&self) {
        let m: HashMap<&String, Vec<String>> = self.binds.iter().map(|(k, v)| (k, v.iter().map(|c| c.text().replace("Cmd+", "Ctrl+")).collect())).collect();
        let _ = std::fs::create_dir_all(crate::plugins::data_dir());
        if let Ok(s) = serde_json::to_string_pretty(&m) {
            let _ = std::fs::write(file(), s);
        }
    }
    /// Command bound to a key press.
    pub fn lookup(&self, c: Combo) -> Option<&str> {
        // keep the order of COMMANDS so the first matching command wins
        COMMANDS.iter().find(|cmd| self.binds.get(cmd.id).map(|v| v.contains(&c)).unwrap_or(false)).map(|cmd| cmd.id)
    }
    pub fn text_of(&self, id: &str) -> String {
        self.binds.get(id).map(|v| v.iter().map(|c| c.text()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    }
}

pub fn label(id: &str) -> &'static str {
    COMMANDS.iter().find(|c| c.id == id).map(|c| tr(c.label)).unwrap_or("")
}
