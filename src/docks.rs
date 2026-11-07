//! Dockable windows (egui_dock): every window can be moved, grouped in tabs,
//! split or undocked into a floating window. Layouts: built-in presets
//! (Default / AudioMixing / ColorCorrection / SourceWindowLayout /
//! TimelineTopLayout) and 10 user layouts.

use crate::app::App;
use crate::i18n::tr;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
pub enum Tab {
    Media,
    Explorer,
    Transitions,
    VideoFx,
    Generators,
    Preview,
    Master,
    Trimmer,
    Mixer,
    Scopes,
    Grading,
    EditDetails,
    Notes,
    Loudness,
    History,
    Surround,
    Plugins,
}

impl Tab {
    pub const ALL: [Tab; 17] = [
        Tab::Media,
        Tab::Explorer,
        Tab::Transitions,
        Tab::VideoFx,
        Tab::Generators,
        Tab::Preview,
        Tab::Master,
        Tab::Trimmer,
        Tab::Mixer,
        Tab::Scopes,
        Tab::Grading,
        Tab::EditDetails,
        Tab::Notes,
        Tab::Loudness,
        Tab::History,
        Tab::Surround,
        Tab::Plugins,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Media => tr("Média du projet"),
            Tab::Explorer => tr("Explorateur"),
            Tab::Transitions => tr("Transitions"),
            Tab::VideoFx => tr("Effets vidéo"),
            Tab::Generators => tr("Générateurs de médias"),
            Tab::Preview => tr("Aperçu vidéo"),
            Tab::Master => tr("Maître"),
            Tab::Trimmer => tr("Rogneuse"),
            Tab::Mixer => tr("Table de mixage"),
            Tab::Scopes => tr("Oscilloscopes vidéo"),
            Tab::Grading => tr("Étalonnage"),
            Tab::EditDetails => tr("Détails de montage"),
            Tab::Notes => tr("Notes du projet"),
            Tab::Loudness => tr("Vu-mètres de sonie"),
            Tab::History => tr("Historique"),
            Tab::Surround => tr("Panoramique surround"),
            Tab::Plugins => tr("Gestionnaire de plugins"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Default,
    AudioMixing,
    ColorGrading,
    TimelineTop,
    Source,
}

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Default, Preset::AudioMixing, Preset::ColorGrading, Preset::TimelineTop, Preset::Source];
    pub fn label(self) -> &'static str {
        match self {
            Preset::Default => tr("Disposition par défaut"),
            Preset::AudioMixing => tr("Mixage audio"),
            Preset::ColorGrading => tr("Étalonnage des couleurs"),
            Preset::TimelineTop => tr("Ligne de temps en haut"),
            Preset::Source => tr("Fenêtre source (rogneuse)"),
        }
    }
}

/// Builds a preset layout. Returns the dock state and whether the timeline
/// is on top.
pub fn preset(p: Preset) -> (DockState<Tab>, bool) {
    match p {
        Preset::Default | Preset::TimelineTop => {
            let mut s = DockState::new(vec![Tab::Media, Tab::Explorer, Tab::Transitions, Tab::VideoFx, Tab::Generators, Tab::Trimmer, Tab::Mixer]);
            let surf = s.main_surface_mut();
            let [_, right] = surf.split_right(NodeIndex::root(), 0.42, vec![Tab::Preview, Tab::Scopes, Tab::Loudness]);
            surf.split_right(right, 0.86, vec![Tab::Master]);
            (s, p == Preset::TimelineTop)
        }
        Preset::AudioMixing => {
            let mut s = DockState::new(vec![Tab::Mixer, Tab::Media, Tab::Explorer]);
            let surf = s.main_surface_mut();
            let [_, right] = surf.split_right(NodeIndex::root(), 0.62, vec![Tab::Loudness, Tab::Surround]);
            surf.split_right(right, 0.6, vec![Tab::Master]);
            (s, false)
        }
        Preset::ColorGrading => {
            let mut s = DockState::new(vec![Tab::Grading, Tab::VideoFx]);
            let surf = s.main_surface_mut();
            let [_, right] = surf.split_right(NodeIndex::root(), 0.45, vec![Tab::Preview]);
            surf.split_right(right, 0.55, vec![Tab::Scopes]);
            (s, false)
        }
        Preset::Source => {
            let mut s = DockState::new(vec![Tab::Trimmer, Tab::Media, Tab::Explorer]);
            let surf = s.main_surface_mut();
            let [_, right] = surf.split_right(NodeIndex::root(), 0.5, vec![Tab::Preview]);
            surf.split_right(right, 0.86, vec![Tab::Master]);
            (s, false)
        }
    }
}

fn layout_file(n: usize) -> std::path::PathBuf {
    crate::plugins::data_dir().join(format!("disposition_{n}.json"))
}

#[derive(Serialize, Deserialize)]
struct SavedLayout {
    dock: DockState<Tab>,
    timeline_top: bool,
    dock_height: f32,
}

struct Viewer<'a> {
    app: &'a mut App,
    ctx: egui::Context,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;
    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.label().into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        let ctx = self.ctx.clone();
        let a = &mut *self.app;
        match tab {
            Tab::Media => a.media_tab(ui, &ctx),
            Tab::Explorer => a.explorer_tab(ui, &ctx),
            Tab::Transitions => a.transitions_tab(ui),
            Tab::VideoFx => a.fx_tab(ui),
            Tab::Generators => a.generators_tab(ui, &ctx),
            Tab::Preview => a.preview(ui, &ctx),
            Tab::Master => a.master_bus(ui),
            Tab::Trimmer => a.trimmer_tab(ui, &ctx),
            Tab::Mixer => a.mixer_tab(ui),
            Tab::Scopes => a.scopes_tab(ui),
            Tab::Grading => a.grading_tab(ui),
            Tab::EditDetails => a.edit_details_tab(ui),
            Tab::Notes => a.notes_tab(ui),
            Tab::Loudness => a.loudness_tab(ui),
            Tab::History => a.history_tab(ui),
            Tab::Surround => a.surround_tab(ui),
            Tab::Plugins => a.plugins_tab(ui),
        }
    }
    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Preview | Tab::Master | Tab::Scopes | Tab::Media | Tab::Trimmer => [false, false],
            _ => [false, true],
        }
    }
    fn allowed_in_windows(&self, _tab: &mut Tab) -> bool {
        true
    }
}

impl App {
    /// Shows the dock area (all docked windows).
    pub fn dock_area(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut dock = std::mem::replace(&mut self.dock, DockState::new(vec![]));
        let mut style = Style::from_egui(ui.style());
        style.tab_bar.bg_fill = crate::theme::BAR();
        if self.prefs.hide_tab_labels {
            style.tab_bar.height = 6.0;
        }
        DockArea::new(&mut dock)
            .id(egui::Id::new("blue_dock"))
            .style(style)
            .show_leaf_collapse_buttons(false)
            .show_inside(ui, &mut Viewer { app: self, ctx: ctx.clone() });
        self.dock = dock;
    }

    /// Opens (or focuses) a docked window.
    pub fn show_tab(&mut self, tab: Tab) {
        if let Some(loc) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab(loc);
            if let Some((s, n, _)) = Some(loc) {
                self.dock.set_focused_node_and_surface((s, n));
            }
        } else {
            self.dock.push_to_focused_leaf(tab);
        }
        self.dock_hidden = false;
    }

    pub fn tab_open(&self, tab: Tab) -> bool {
        self.dock.find_tab(&tab).is_some()
    }

    pub fn toggle_tab(&mut self, tab: Tab) {
        if let Some(loc) = self.dock.find_tab(&tab) {
            self.dock.remove_tab(loc);
        } else {
            self.show_tab(tab);
        }
    }

    pub fn load_preset(&mut self, p: Preset) {
        let (d, top) = preset(p);
        self.dock = d;
        self.timeline_top = top;
        self.dock_hidden = false;
        self.status = format!("{} {}", tr("Disposition :"), p.label());
    }

    pub fn save_layout(&mut self, n: usize) {
        let l = SavedLayout { dock: self.dock.clone(), timeline_top: self.timeline_top, dock_height: self.dock_height };
        let _ = std::fs::create_dir_all(crate::plugins::data_dir());
        self.status = match serde_json::to_string(&l).map_err(|e| e.to_string()).and_then(|s| std::fs::write(layout_file(n), s).map_err(|e| e.to_string())) {
            Ok(()) => format!("{} {}", tr("Disposition enregistrée :"), n),
            Err(e) => e,
        };
    }

    pub fn load_layout(&mut self, n: usize) {
        match std::fs::read_to_string(layout_file(n)).ok().and_then(|s| serde_json::from_str::<SavedLayout>(&s).ok()) {
            Some(l) => {
                self.dock = l.dock;
                self.timeline_top = l.timeline_top;
                self.dock_height = l.dock_height;
                self.dock_hidden = false;
                self.status = format!("{} {}", tr("Disposition chargée :"), n);
            }
            None => self.status = format!("{} {}", tr("Aucune disposition enregistrée sous le numéro"), n),
        }
    }

    pub fn layout_exists(n: usize) -> bool {
        layout_file(n).exists()
    }
}
