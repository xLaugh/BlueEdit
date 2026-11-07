//! Claude, built into the editor: a chat assistant that reads the project
//! and edits it through tools. Raw HTTPS calls to the Claude Messages API
//! (there is no official Rust SDK). The network call runs on a worker
//! thread; tools run on the UI thread, which owns the project.

use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver};

pub const MODEL: &str = "claude-opus-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
/// Server-side fallback when the model declines (routing by refusal category).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOOL_ROUNDS: usize = 24;

#[derive(Clone, Debug)]
pub enum Line {
    User(String),
    Assistant(String),
    Tool(String),
    Error(String),
}

pub struct Chat {
    pub key: String,
    pub key_from_env: bool,
    pub messages: Vec<Value>,
    pub lines: Vec<Line>,
    pub input: String,
    pub pending: Option<Receiver<Result<Value, String>>>,
    pub rounds: usize,
    pub show_key: bool,
}

impl Chat {
    pub fn new() -> Chat {
        let env = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty());
        let key = env.clone().unwrap_or_else(load_key);
        Chat {
            key,
            key_from_env: env.is_some(),
            messages: vec![],
            lines: vec![],
            input: String::new(),
            pending: None,
            rounds: 0,
            show_key: false,
        }
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn reset(&mut self) {
        self.messages.clear();
        self.lines.clear();
        self.pending = None;
        self.rounds = 0;
    }

    /// Starts a request with the current conversation.
    pub fn send(&mut self, project_context: String) {
        let key = self.key.trim().to_string();
        let body = json!({
            "model": MODEL,
            "max_tokens": 16000,
            "output_config": {"effort": "medium"},
            "fallbacks": "default",
            "system": [
                {"type": "text", "text": SYSTEM_PROMPT, "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": project_context}
            ],
            "tools": tools(),
            "messages": self.messages,
        });
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(request(&key, &body));
        });
        self.pending = Some(rx);
    }
}

fn key_file() -> std::path::PathBuf {
    crate::plugins::data_dir().join("claude_key.txt")
}

pub fn load_key() -> String {
    std::fs::read_to_string(key_file()).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn save_key(key: &str) -> Result<(), String> {
    std::fs::create_dir_all(crate::plugins::data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(key_file(), key.trim()).map_err(|e| e.to_string())
}

fn request(key: &str, body: &Value) -> Result<Value, String> {
    if key.is_empty() {
        return Err("Aucune clé d'API Claude : saisissez-la dans le panneau de l'assistant.".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(600))
        .build();
    let resp = agent
        .post(API_URL)
        .set("x-api-key", key)
        .set("anthropic-version", API_VERSION)
        .set("anthropic-beta", FALLBACK_BETA)
        .set("content-type", "application/json")
        .send_json(body.clone());
    match resp {
        Ok(r) => r.into_json::<Value>().map_err(|e| format!("réponse illisible : {e}")),
        Err(ureq::Error::Status(code, r)) => {
            let v: Value = r.into_json().unwrap_or(Value::Null);
            let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
            Err(match code {
                401 => "Clé d'API refusée (401). Vérifiez la clé.".into(),
                429 => format!("Trop de requêtes (429), réessayez dans un instant. {msg}"),
                c if c >= 500 => format!("Le service Claude est indisponible ({c}). {msg}"),
                c => format!("Erreur {c} : {msg}"),
            })
        }
        Err(e) => Err(format!("Connexion impossible : {e}")),
    }
}

const SYSTEM_PROMPT: &str = "Tu es l'assistant de montage intégré à Blue Edit (« Blue »), un logiciel de montage vidéo à pistes (comme les logiciels de montage professionnels). \
Tu réponds en français, brièvement. Tu peux lire et modifier le projet de la personne avec les outils fournis. \
Les temps sont en secondes. Les pistes sont numérotées à partir de 0, de haut en bas ; la piste la plus haute est au premier plan. \
Avant de modifier, appelle get_project si tu n'as pas l'état actuel. Fais exactement ce qui est demandé ; pour une action destructrice et ambiguë \
(supprimer beaucoup de choses), demande d'abord. Après tes modifications, résume en une ou deux phrases ce que tu as fait. \
Chaque modification peut être annulée avec Ctrl+Z ou l'outil undo.";

fn num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": desc,
        "input_schema": {"type": "object", "properties": props, "required": required}
    })
}

pub fn tools() -> Value {
    let effects: Vec<&str> = crate::model::VideoFx::presets()
        .into_iter()
        .chain(crate::model::VideoFx::pack())
        .map(|f| f.key())
        .collect();
    let transitions = ["dissolve", "wipe_left", "wipe_down", "push", "iris", "through_black", "blinds", "clock_wipe", "flash"];
    let curves = ["linear", "fast", "slow", "smooth", "sharp", "hold"];
    json!([
        tool("get_project", "Renvoie l'état complet du projet : format, curseur, pistes, événements (id, début, fin, fondus, effets), médias et marqueurs.", json!({}), &[]),
        tool("set_cursor", "Place le curseur de lecture.", json!({"time": num("Temps en secondes")}), &["time"]),
        tool("split_events", "Divise des événements à un instant donné. Sans event_ids, divise tous les événements sous cet instant.", json!({"time": num("Temps en secondes"), "event_ids": {"type": "array", "items": {"type": "integer"}}}), &["time"]),
        tool("delete_events", "Supprime des événements. ripple=true décale ensuite les événements suivants pour fermer le trou.", json!({"event_ids": {"type": "array", "items": {"type": "integer"}}, "ripple": {"type": "boolean"}}), &["event_ids"]),
        tool("move_event", "Déplace un événement (et son groupe) à un nouveau début, éventuellement sur une autre piste du même type.", json!({"event_id": {"type": "integer"}, "start": num("Nouveau début en secondes"), "track_index": {"type": "integer"}}), &["event_id", "start"]),
        tool("trim_event", "Change le début et/ou la fin d'un événement sur la ligne de temps (rognage).", json!({"event_id": {"type": "integer"}, "start": num("Nouveau début"), "end": num("Nouvelle fin")}), &["event_id"]),
        tool("set_fades", "Règle les fondus d'entrée et de sortie d'un événement (secondes) et leurs courbes.", json!({"event_id": {"type": "integer"}, "fade_in": num("Durée du fondu d'entrée"), "fade_out": num("Durée du fondu de sortie"), "curve_in": {"type": "string", "enum": curves}, "curve_out": {"type": "string", "enum": curves}}), &["event_id"]),
        tool("set_event_gain", "Règle l'opacité (vidéo) ou le volume (audio) d'un événement, entre 0 et 1.", json!({"event_id": {"type": "integer"}, "gain": num("0 à 1")}), &["event_id", "gain"]),
        tool("add_effect", "Ajoute un effet vidéo à un événement vidéo. intensity (0 à 1) règle la force quand l'effet en a une.", json!({"event_id": {"type": "integer"}, "effect": {"type": "string", "enum": effects}, "intensity": num("0 à 1, optionnel")}), &["event_id", "effect"]),
        tool("clear_effects", "Retire tous les effets d'un événement.", json!({"event_id": {"type": "integer"}}), &["event_id"]),
        tool("set_transition", "Choisit la transition utilisée à l'entrée d'un événement vidéo (fondu enchaîné avec l'événement précédent qui le chevauche, ou fondu d'entrée).", json!({"event_id": {"type": "integer"}, "transition": {"type": "string", "enum": transitions}}), &["event_id", "transition"]),
        tool("add_text", "Ajoute un titre (ou un sous-titre si subtitle=true) sur une piste vidéo.", json!({"text": {"type": "string"}, "start": num("Début"), "duration": num("Durée"), "track_index": {"type": "integer"}, "subtitle": {"type": "boolean"}, "size_pct": num("Taille en % de la hauteur de l'image")}), &["text", "start", "duration"]),
        tool("add_marker", "Ajoute un marqueur.", json!({"time": num("Temps"), "name": {"type": "string"}}), &["time"]),
        tool("set_loop_region", "Définit la région de boucle (sélection de temps).", json!({"start": num("Début"), "end": num("Fin")}), &["start", "end"]),
        tool("set_track", "Modifie une piste : muet, solo, niveau (0 à 1, ou jusqu'à 2 pour l'audio), nom.", json!({"track_index": {"type": "integer"}, "mute": {"type": "boolean"}, "solo": {"type": "boolean"}, "level": num("Niveau"), "name": {"type": "string"}}), &["track_index"]),
        tool("add_track", "Ajoute une piste vidéo ou audio.", json!({"kind": {"type": "string", "enum": ["video", "audio"]}}), &["kind"]),
        tool("insert_media", "Place un média du projet sur la ligne de temps.", json!({"media_id": {"type": "integer"}, "start": num("Début"), "track_index": {"type": "integer"}}), &["media_id", "start"]),
        tool("auto_trim", "Coupe automatiquement les silences d'un événement audio ou vidéo avec son (méthode CrispTrim) et resserre le montage. Les hésitations ne sont coupées que via la fenêtre Coupe automatique (transcription).", json!({"event_id": {"type": "integer"}, "min_silence": num("Silence minimum à couper, secondes (défaut 0,5)"), "margin": num("Marge gardée autour de la parole (défaut 0,15)"), "threshold_db": num("Seuil en dB ; omis = automatique")}), &["event_id"]),
        tool("generate_subtitles", "Lance la transcription automatique de l'audio du projet et ajoute des sous-titres (tâche en arrière-plan).", json!({"language": {"type": "string", "description": "Code langue, ex. fr, en, auto"}}), &[]),
        tool("undo", "Annule la dernière modification.", json!({}), &[]),
    ])
}
