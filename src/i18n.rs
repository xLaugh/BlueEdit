//! Interface language: French (source) or English. `tr("texte")` returns
//! the English text when English is selected. The choice is saved in the
//! program's data folder.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Fr,
    En,
    De,
    Es,
    It,
    Pt,
    Pl,
    Ja,
    Ko,
    Zh,
}

impl Lang {
    /// The 10 interface languages.
    pub const ALL: [Lang; 10] = [Lang::Fr, Lang::En, Lang::De, Lang::Es, Lang::It, Lang::Pt, Lang::Pl, Lang::Ja, Lang::Ko, Lang::Zh];
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::It => "it",
            Lang::Pt => "pt",
            Lang::Pl => "pl",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Zh => "zh",
        }
    }
    /// Name of the language in itself (never translated).
    pub fn name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::It => "Italiano",
            Lang::Pt => "Português",
            Lang::Pl => "Polski",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Zh => "简体中文",
        }
    }
    pub fn from_code(c: &str) -> Option<Lang> {
        Lang::ALL.iter().copied().find(|l| l.code() == c.trim())
    }
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::Fr => &[],
            Lang::En => EN,
            Lang::De => crate::i18n_eu::DE,
            Lang::Es => crate::i18n_eu::ES,
            Lang::It => crate::i18n_eu::IT,
            Lang::Pt => crate::i18n_eu::PT,
            Lang::Pl => crate::i18n_eu::PL,
            Lang::Ja => crate::i18n_asia::JA,
            Lang::Ko => crate::i18n_asia::KO,
            Lang::Zh => crate::i18n_asia::ZH,
        }
    }
    pub fn needs_cjk(self) -> bool {
        matches!(self, Lang::Ja | Lang::Ko | Lang::Zh)
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    Lang::ALL[(LANG.load(Ordering::Relaxed) as usize).min(Lang::ALL.len() - 1)]
}

fn store(l: Lang) {
    let i = Lang::ALL.iter().position(|x| *x == l).unwrap_or(0);
    LANG.store(i as u8, Ordering::Relaxed);
}

fn lang_file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("langue.txt")
}

pub fn set_lang(l: Lang) {
    store(l);
    let _ = std::fs::create_dir_all(crate::plugins::data_dir());
    let _ = std::fs::write(lang_file(), l.code());
}

/// Primary language of the system:
/// `GetUserDefaultLangID() & 0x3FF` on Windows, $LANG / $LC_ALL elsewhere.
fn system_lang() -> Lang {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultLangID() -> u16;
        }
        // primary language ids; anything without a translation falls back to
        // English.
        match unsafe { GetUserDefaultLangID() } & 0x3ff {
            0x0c => Lang::Fr,
            0x07 => Lang::De,
            0x0a => Lang::Es,
            0x10 => Lang::It,
            0x16 => Lang::Pt,
            0x15 => Lang::Pl,
            0x11 => Lang::Ja,
            0x12 => Lang::Ko,
            0x04 => Lang::Zh,
            _ => Lang::En,
        }
    }
    #[cfg(not(windows))]
    {
        let v = std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default()
            .to_lowercase();
        Lang::from_code(v.get(..2).unwrap_or("")).unwrap_or(Lang::En)
    }
}

/// The user's saved choice first; without one, the system language
/// (saved choice, then GetUserDefaultLangID).
pub fn init() {
    let saved = std::fs::read_to_string(lang_file()).unwrap_or_default();
    let l = std::env::var("BLUE_LANG")
        .ok()
        .and_then(|c| Lang::from_code(&c))
        .or_else(|| Lang::from_code(&saved))
        .unwrap_or_else(system_lang);
    store(l);
}

pub fn tr(s: &'static str) -> &'static str {
    let l = lang();
    if l == Lang::Fr {
        return s;
    }
    static MAPS: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let maps = MAPS.get_or_init(|| {
        Lang::ALL
            .iter()
            .map(|l| {
                let mut m: HashMap<&'static str, &'static str> = l.table().iter().copied().collect();
                if *l == Lang::En {
                    for (k, v) in crate::i18n_eu::EN_EXTRA {
                        m.entry(k).or_insert(v);
                    }
                }
                m
            })
            .collect()
    });
    let i = Lang::ALL.iter().position(|x| *x == l).unwrap_or(1);
    maps[i].get(s).or_else(|| maps[1].get(s)).copied().unwrap_or(s)
}

/// Adds a system font with Japanese, Korean and Chinese glyphs as a fallback
/// (egui's own fonts have none). Done once, whatever the language, so that
/// switching language never shows empty boxes.
pub fn install_fonts(ctx: &egui::Context) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/YuGothM.ttc", "C:/Windows/Fonts/meiryo.ttc", "C:/Windows/Fonts/malgun.ttf", "C:/Windows/Fonts/simsun.ttc"]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/AppleSDGothicNeo.ttc", "/Library/Fonts/Arial Unicode.ttf"]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ]
    };
    let mut fonts = egui::FontDefinitions::default();
    let mut added = 0;
    for (i, p) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) {
            let name = format!("cjk{i}");
            fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.clone());
            }
            added += 1;
            // one Chinese + one Korean face is enough to cover the three languages
            if added >= 2 {
                break;
            }
        }
    }
    if added > 0 {
        ctx.set_fonts(fonts);
    }
}

const EN: &[(&str, &str)] = &[
    // menus
    ("Fichier", "File"),
    ("Édition", "Edit"),
    ("Affichage", "View"),
    ("Insertion", "Insert"),
    ("Outils", "Tools"),
    ("Options", "Options"),
    ("IA", "AI"),
    ("Plugins", "Plug-ins"),
    ("Aide", "Help"),
    ("Luminosité de l'interface", "Interface Brightness"),
    ("Très sombre (par défaut)", "Darkest (default)"),
    ("Sombre", "Dark"),
    ("Clair", "Light"),
    ("Très clair", "Lightest"),
    ("Nouveau", "New"),
    ("Ouvrir…", "Open…"),
    ("Enregistrer", "Save"),
    ("Enregistrer sous…", "Save As…"),
    ("Importer un média…", "Import Media…"),
    ("Rendre en tant que…", "Render As…"),
    ("Propriétés du projet…", "Project Properties…"),
    ("Quitter", "Exit"),
    ("Annuler", "Undo"),
    ("Rétablir", "Redo"),
    ("Couper", "Cut"),
    ("Copier", "Copy"),
    ("Coller", "Paste"),
    ("Supprimer", "Delete"),
    ("Diviser", "Split"),
    ("Sélectionner tout", "Select All"),
    ("Montage en cascade automatique (Ctrl+L)", "Auto Ripple (Ctrl+L)"),
    ("Zones de sécurité", "Safe Areas"),
    ("Grille de l'aperçu", "Preview Grid"),
    ("Zoom avant", "Zoom In"),
    ("Zoom arrière", "Zoom Out"),
    ("Afficher tout le projet", "Show Whole Project"),
    ("Média du projet", "Project Media"),
    ("Explorateur", "Explorer"),
    ("Transitions", "Transitions"),
    ("Effets vidéo", "Video FX"),
    ("Générateurs de médias", "Media Generators"),
    ("Piste audio", "Audio Track"),
    ("Piste vidéo", "Video Track"),
    ("Marqueur", "Marker"),
    ("Texte", "Text"),
    ("Média généré…", "Generated Media…"),
    ("Effets vidéo de l'événement…", "Video Event FX…"),
    ("Panoramique/Recadrage de l'événement…", "Event Pan/Crop…"),
    ("Propriétés de l'événement…", "Event Properties…"),
    ("Enregistrer un instantané de l'aperçu…", "Save Snapshot of Preview…"),
    ("Activer l'aimantation (F8)", "Enable Snapping (F8)"),
    ("Aimanter à la grille (Ctrl+F8)", "Snap to Grid (Ctrl+F8)"),
    ("Quantifier aux images (Alt+F8)", "Quantize to Frames (Alt+F8)"),
    ("Fondu enchaîné automatique (Ctrl+Maj+X)", "Automatic Crossfades (Ctrl+Shift+X)"),
    ("Lecture en boucle (Q)", "Loop Playback (Q)"),
    ("Raccourcis et à propos…", "Shortcuts and About…"),
    ("Assistant Claude…", "Claude Assistant…"),
    ("Sous-titres automatiques…", "Automatic Subtitles…"),
    ("Coupe automatique (silences et hésitations)…", "Auto Trim (silences and hesitations)…"),
    ("Gérer les plugins…", "Manage Plug-ins…"),
    ("Installer un plugin…", "Install a Plug-in…"),
    ("Plugins OFX détectés…", "Detected OFX Plug-ins…"),
    ("Langue", "Language"),
    // shortcuts shown in menus
    ("Ctrl+Maj+S", "Ctrl+Shift+S"),
    ("Suppr", "Del"),
    // toolbar / transport tooltips
    ("Nouveau (Ctrl+N)", "New (Ctrl+N)"),
    ("Ouvrir (Ctrl+O)", "Open (Ctrl+O)"),
    ("Enregistrer (Ctrl+S)", "Save (Ctrl+S)"),
    ("Rendre en tant que… (Ctrl+R)", "Render As… (Ctrl+R)"),
    ("Propriétés du projet", "Project Properties"),
    ("Annuler (Ctrl+Z)", "Undo (Ctrl+Z)"),
    ("Rétablir (Ctrl+Y)", "Redo (Ctrl+Y)"),
    ("Couper (Ctrl+X)", "Cut (Ctrl+X)"),
    ("Copier (Ctrl+C)", "Copy (Ctrl+C)"),
    ("Coller (Ctrl+V)", "Paste (Ctrl+V)"),
    ("Supprimer (Suppr)", "Delete (Del)"),
    ("Diviser au curseur (S)", "Split at Cursor (S)"),
    ("Insérer un marqueur (M)", "Insert Marker (M)"),
    ("Zoom avant (Haut)", "Zoom In (Up)"),
    ("Zoom arrière (Bas)", "Zoom Out (Down)"),
    ("Claude", "Claude"),
    ("Assistant Claude : demandez une modification du montage", "Claude Assistant: ask for an edit"),
    ("Sous-titres IA", "AI Subtitles"),
    ("Sous-titres automatiques (transcription locale)", "Automatic subtitles (local transcription)"),
    ("Coupe auto", "Auto Trim"),
    ("Couper les silences et les hésitations de l'événement sélectionné", "Cut silences and hesitations from the selected event"),
    ("Gérer les plugins d'effets", "Manage effect plug-ins"),
    ("Enregistrer (non disponible dans cette version)", "Record (not available in this version)"),
    ("Lire depuis le début (Maj+Espace)", "Play From Start (Shift+Space)"),
    ("Lecture (Espace)", "Play (Space)"),
    ("Pause (Entrée)", "Pause (Enter)"),
    ("Arrêt (Échap)", "Stop (Esc)"),
    ("Aller au début (Origine)", "Go to Start (Home)"),
    ("Aller à la fin (Fin)", "Go to End (End)"),
    ("Image précédente (Gauche)", "Previous Frame (Left)"),
    ("Image suivante (Droite)", "Next Frame (Right)"),
    ("Début", "Start"),
    ("Fin", "End"),
    ("Longueur", "Length"),
    // panels
    ("Ajouter au curseur", "Add at Cursor"),
    ("Retirer du projet", "Remove from Project"),
    ("Glissez des fichiers ici ou cliquez sur « Importer un média… »", "Drop files here or click \"Import Media…\""),
    ("Dossier parent", "Parent folder"),
    ("Aucun dossier ni média ici.", "No folders or media here."),
    ("Double-clic : importer et ajouter au curseur", "Double-click: import and add at cursor"),
    ("Glissez une transition sur un événement vidéo (elle s'applique à son entrée en fondu ou au fondu enchaîné).", "Drag a transition onto a video event (it applies to its fade-in or crossfade)."),
    ("Glissez un effet sur un événement vidéo, ou double-cliquez pour l'ajouter à la sélection.", "Drag an effect onto a video event, or double-click to add it to the selection."),
    ("Cliquez pour créer le média et l'ajouter au curseur (piste sélectionnée).", "Click to create the media and add it at the cursor (selected track)."),
    ("Aperçu vidéo", "Video Preview"),
    ("Propriétés du projet vidéo", "Video project properties"),
    ("Grille", "Grid"),
    ("Enregistrer un instantané dans un fichier", "Save snapshot to file"),
    ("Maître", "Master"),
    ("Vu-mètres du bus maître (plancher -60 dB). Cliquez pour remettre les crêtes à zéro.", "Master bus meters (-60 dB floor). Click to reset peaks."),
    ("Ébauche (Auto)", "Draft (Auto)"),
    ("Aperçu (Auto)", "Preview (Auto)"),
    ("Bonne (Auto)", "Good (Auto)"),
    ("Texte (titre)", "Text (title)"),
    ("Couleur unie", "Solid Color"),
    ("Dégradé de couleurs", "Color Gradient"),
    ("Mire de couleurs", "Color Bars"),
    ("Tonalité 440 Hz (audio)", "440 Hz Tone (audio)"),
    ("Musique de démonstration (audio)", "Demo Music (audio)"),
    ("Aa", "Aa"),
    // timeline
    ("Muet", "Mute"),
    ("Solo", "Solo"),
    ("M", "M"),
    ("S", "S"),
    ("Renommer", "Rename"),
    ("Insérer une piste vidéo", "Insert Video Track"),
    ("Insérer une piste audio", "Insert Audio Track"),
    ("Dupliquer la piste", "Duplicate Track"),
    ("Supprimer la piste", "Delete Track"),
    ("Diviser au curseur", "Split at Cursor"),
    ("Type de fondu d'entrée", "Fade In Type"),
    ("Type de fondu de sortie", "Fade Out Type"),
    ("Transition d'entrée", "Fade In Transition"),
    ("Panoramique/Recadrage…", "Pan/Crop…"),
    ("Modifier le média généré…", "Edit Generated Media…"),
    ("Boucle", "Loop"),
    ("Dissocier du groupe", "Remove from Group"),
    ("Sélectionner comme région de boucle", "Select as Loop Region"),
    ("Propriétés…", "Properties…"),
    ("Insérer un marqueur ici", "Insert Marker Here"),
    ("Insérer un texte ici", "Insert Text Here"),
    // windows
    ("Aucun effet. Ajoutez-en un ci-dessous ou glissez-le depuis l'onglet « Effets vidéo ».", "No effect. Add one below or drag it from the \"Video FX\" tab."),
    ("Retirer", "Remove"),
    ("Ajouter :", "Add:"),
    ("Luminosité", "Brightness"),
    ("Contraste", "Contrast"),
    ("Saturation", "Saturation"),
    ("Intensité", "Intensity"),
    ("Rayon", "Radius"),
    ("Seuil", "Threshold"),
    ("Taille des blocs", "Block size"),
    ("Le résultat s'affiche dans l'aperçu vidéo.", "The result shows in the video preview."),
    ("Zoom", "Zoom"),
    ("Position X", "Position X"),
    ("Position Y", "Position Y"),
    ("Rotation (°)", "Rotation (°)"),
    ("Réinitialiser", "Reset"),
    ("Propriétés de l'événement", "Event Properties"),
    ("Nom", "Name"),
    ("Longueur (s)", "Length (s)"),
    ("Décalage dans le média (s)", "Offset in media (s)"),
    ("Opacité", "Opacity"),
    ("Volume", "Volume"),
    ("Fondu d'entrée (s)", "Fade in (s)"),
    ("Courbe d'entrée", "Fade-in curve"),
    ("Fondu de sortie (s)", "Fade out (s)"),
    ("Courbe de sortie", "Fade-out curve"),
    ("Nom du projet", "Project name"),
    ("Modèle", "Template"),
    ("Largeur", "Width"),
    ("Hauteur", "Height"),
    ("Inverser (horizontal / vertical)", "Swap (landscape / portrait)"),
    ("Orientation", "Orientation"),
    ("Vertical", "Portrait"),
    ("Carré", "Square"),
    ("Horizontal", "Landscape"),
    ("Fréquence d'images", "Frame rate"),
    ("Fréquence audio", "Audio sample rate"),
    ("Tempo", "Tempo"),
    ("Rendu", "Render"),
    ("Ouvrir le dossier", "Open Folder"),
    ("Fermer", "Close"),
    ("Rendre en tant que", "Render As"),
    ("Format", "Format"),
    ("Parcourir…", "Browse…"),
    ("Plage", "Range"),
    ("Rendre seulement la région de boucle", "Render loop region only"),
    ("(Utiliser les paramètres du projet)", "(Use project settings)"),
    ("Taille de l'image", "Frame size"),
    ("Celle du projet", "Project's"),
    ("Inverser", "Swap"),
    ("Inverser largeur et hauteur (horizontal / vertical)", "Swap width and height (landscape / portrait)"),
    ("Codec vidéo", "Video codec"),
    ("Encodeur", "Encoder"),
    ("Débit", "Bit rate"),
    ("Débit constant (bps)", "Constant bit rate (bps)"),
    ("Débit variable", "Variable bit rate"),
    ("Maximum (bps) :", "Maximum (bps):"),
    ("Moyenne (bps) :", "Average (bps):"),
    ("10 bits (débit ×1,25, comme les modèles HDR)", "10-bit (bit rate ×1.25, like the HDR templates)"),
    ("Cadrage", "Framing"),
    ("Étirer la vidéo pour remplir l'image (sinon bandes noires)", "Stretch video to fill output frame (otherwise black bars)"),
    ("Audio : fréquence", "Audio: sample rate"),
    ("Audio : débit", "Audio: bit rate"),
    ("Rendre", "Render"),
    ("×", "×"),
    ("Sous-titre :", "Subtitle:"),
    ("Texte :", "Text:"),
    ("Taille (% de la hauteur)", "Size (% of height)"),
    ("Fréquence (Hz)", "Frequency (Hz)"),
    ("Ce générateur n'a pas de réglage.", "This generator has no settings."),
    ("Blue Edit – raccourcis", "Blue Edit – shortcuts"),
    ("Blue Edit (« Blue ») : logiciel de montage vidéo à pistes, écrit en Rust.", "Blue Edit (\"Blue\"): track-based video editor, written in Rust."),
    ("Haut d'un événement : glisser les coins pour les fondus, le milieu pour le gain.", "Top of an event: drag the corners for fades, the middle for gain."),
    // AI and plug-ins
    ("Packs d'effets", "Effect packs"),
    ("Installer un plugin… (.bluefx)", "Install a plug-in… (.bluefx)"),
    ("Ouvrir le dossier des plugins", "Open plug-ins folder"),
    ("Créer un plugin avec les effets de l'événement…", "Create a plug-in from the event's effects…"),
    ("Enregistre la chaîne d'effets de l'événement sélectionné dans un fichier .bluefx à partager ou à installer.", "Saves the selected event's effect chain to a .bluefx file to share or install."),
    ("Afficher ce pack dans l'onglet Effets vidéo", "Show this pack in the Video FX tab"),
    ("Désinstaller", "Uninstall"),
    ("Rechercher à nouveau", "Scan again"),
    ("Aucun plugin OFX trouvé dans C:\\Program Files\\Common Files\\OFX\\Plugins.", "No OFX plug-in found in C:\\Program Files\\Common Files\\OFX\\Plugins."),
    ("Ajoute l'effet intégré le plus proche aux événements vidéo sélectionnés", "Adds the closest built-in effect to the selected video events"),
    ("pas d'équivalent", "no equivalent"),
    ("Aucun plugin OFX trouvé dans les dossiers OpenFX standard.", "No OFX plug-in found in the standard OpenFX folders."),
    ("Modèle :", "Model:"),
    ("Télécharger un modèle de transcription", "Download a transcription model"),
    ("Modèles whisper.cpp officiels (Hugging Face). Ils s'installent dans le dossier de Blue Edit.", "Official whisper.cpp models (Hugging Face), installed in Blue Edit's folder."),
    ("Sous-titres automatiques", "Automatic Subtitles"),
    ("La parole du projet est transcrite sur cet ordinateur (whisper.cpp via ffmpeg) : rien n'est envoyé sur Internet.", "Speech is transcribed on this computer (whisper.cpp via ffmpeg): nothing is sent over the Internet."),
    ("Langue :", "Language:"),
    ("Taille du texte (% de la hauteur)", "Text size (% of height)"),
    ("Seulement la région de boucle", "Loop region only"),
    ("Générer les sous-titres", "Generate subtitles"),
    ("Transcription en cours…", "Transcribing…"),
    ("Exporter en .srt…", "Export as .srt…"),
    ("Choisissez ou téléchargez d'abord un modèle.", "Choose or download a model first."),
    ("Coupe automatique (silences et hésitations)", "Auto Trim (silences and hesitations)"),
    ("Sélectionnez un événement qui contient du son (une prise filmée, une voix…).", "Select an event that contains sound (a filmed take, a voice…)."),
    ("Seuil automatique", "Automatic threshold"),
    ("dB", "dB"),
    ("Silence minimum (s)", "Minimum silence (s)"),
    ("Marge autour de la parole (s)", "Margin around speech (s)"),
    ("Segment gardé minimum (s)", "Minimum kept segment (s)"),
    ("Couper les hésitations (euh, heu, hum, mmh)", "Cut hesitations (um, uh, hmm)"),
    ("Autres mots :", "Other words:"),
    ("ben, bah…", "like, so…"),
    ("Recherche des hésitations (transcription)…", "Finding hesitations (transcription)…"),
    ("Trouver les hésitations (transcription locale)", "Find hesitations (local transcription)"),
    ("modèle requis : voir IA > Sous-titres automatiques", "model needed: see AI > Automatic Subtitles"),
    ("Le son de cet événement n'est pas encore chargé.", "This event's sound is not loaded yet."),
    ("■ gardé", "■ kept"),
    ("■ silence", "■ silence"),
    ("■ hésitation", "■ hesitation"),
    ("Appliquer la coupe", "Apply the cut"),
    ("Assistant Claude", "Claude Assistant"),
    ("Nouvelle conversation", "New conversation"),
    ("Clé d'API", "API key"),
    ("Clé d'API Claude (créée sur console.anthropic.com) :", "Claude API key (created at console.anthropic.com):"),
    ("Clé lue dans la variable ANTHROPIC_API_KEY.", "Key read from the ANTHROPIC_API_KEY variable."),
    ("Claude réfléchit…", "Claude is thinking…"),
    ("Votre demande… (Entrée pour envoyer)", "Your request… (Enter to send)"),
    ("Envoyer", "Send"),
    // labels of curves, transitions, effects, formats
    ("Linéaire", "Linear"),
    ("Rapide", "Fast"),
    ("Lente", "Slow"),
    ("Douce", "Smooth"),
    ("Nette", "Sharp"),
    ("Maintien", "Hold"),
    ("Fondu enchaîné", "Crossfade"),
    ("Volet horizontal", "Horizontal Wipe"),
    ("Volet vertical", "Vertical Wipe"),
    ("Poussée", "Push"),
    ("Iris", "Iris"),
    ("Fondu par le noir", "Dip to Black"),
    ("Stores", "Blinds"),
    ("Volet horloge", "Clock Wipe"),
    ("Flash blanc", "White Flash"),
    ("Luminosité et contraste", "Brightness and Contrast"),
    ("Noir et blanc", "Black and White"),
    ("Sépia", "Sepia"),
    ("Inverser les couleurs", "Invert"),
    ("Flou", "Blur"),
    ("Vignettage", "Vignette"),
    ("Lueur", "Glow"),
    ("Séparation RVB", "RGB Separation"),
    ("Glitch", "Glitch"),
    ("VHS", "VHS"),
    ("Grain de film", "Film Grain"),
    ("Tremblement de caméra", "Camera Shake"),
    ("Bichromie", "Duotone"),
    ("Mosaïque", "Mosaic"),
    ("Noir moderne", "Modern Noir"),
    ("MP4 (vidéo + AAC)", "MP4 (video + AAC)"),
    ("QuickTime MOV", "QuickTime MOV"),
    ("Matroska MKV", "Matroska MKV"),
    ("WebM (VP9 + Opus)", "WebM (VP9 + Opus)"),
    ("Audio seul : WAV 16 bits", "Audio only: 16-bit WAV"),
    ("ProRes 422 (sans débit cible)", "ProRes 422 (no target bit rate)"),
    ("Processeur (logiciel)", "CPU (software)"),
    // status bar
    ("Niveau :", "Level:"),
    ("Vol. :", "Vol:"),
    ("Pan. :", "Pan:"),
    ("Centre", "Center"),
    ("G", "L"),
    ("D", "R"),
    ("Image :", "Frame:"),
    ("Zoom :", "Zoom:"),
    ("Vidéo", "Video"),
    ("Audio", "Audio"),
    ("Projet :", "Project:"),
    ("Aperçu :", "Preview:"),
    ("Affichage :", "Display:"),
    ("Audio : prêt", "Audio: ready"),
    ("Audio : aucun périphérique", "Audio: no device"),
    ("ffmpeg : trouvé", "ffmpeg: found"),
    ("ffmpeg : absent", "ffmpeg: missing"),
    ("pistes", "tracks"),
    ("durée", "length"),
    ("Vitesse :", "Speed:"),
    ("Vidéo + audio", "Video + audio"),
    ("Image", "Image"),
    ("Générateur audio", "Audio generator"),
    ("Générateur vidéo", "Video generator"),
    ("Prêt.", "Ready."),
    ("Sans titre", "Untitled"),
];
