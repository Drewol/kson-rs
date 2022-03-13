#![windows_subsystem = "windows"]

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use chart_editor::MainState;
use eframe::egui::{
    self, menu, warn_if_debug_build, Button, Color32, DragValue, Frame, Grid, Key, Label, Layout,
    Pos2, Rect, Response, RichText, Sense, Slider, Ui, Vec2,
};
use eframe::epi::App;
use kson::{BgmInfo, Chart, MetaInfo};
use serde::{Deserialize, Serialize};

mod action_stack;
mod chart_camera;
mod chart_editor;
mod dsp;
mod playback;
mod tools;
mod utils;

pub trait Widget {
    fn ui(self, ui: &mut Ui) -> Response;
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct NewChartOptions {
    audio: String,
    filename: String,
    destination: Option<PathBuf>,
}

impl Widget for &mut kson::MetaInfo {
    fn ui(self, ui: &mut Ui) -> Response {
        let edit_row = |ui: &mut Ui, label: &str, data: &mut String| {
            ui.label(label);
            ui.text_edit_singleline(data);
            ui.end_row();
        };

        egui::Grid::new("metadata_editor")
            .show(ui, |ui| {
                edit_row(ui, "Title", &mut self.title);
                edit_row(ui, "Artist", &mut self.artist);
                edit_row(ui, "Effector", &mut self.chart_author);
                edit_row(ui, "Jacket", &mut self.jacket_filename);
                edit_row(ui, "Jacket Artist", &mut self.jacket_author);

                ui.label("Difficulty:");
                ui.end_row();

                ui.label("Level");
                ui.add(DragValue::new(&mut self.level).clamp_range(1..=20));
                ui.end_row();

                if self.difficulty.name.is_none() {
                    self.difficulty.name = Some(Default::default());
                }
                edit_row(ui, "Name", self.difficulty.name.as_mut().unwrap());

                if self.difficulty.short_name.is_none() {
                    self.difficulty.short_name = Some(Default::default());
                }
                edit_row(
                    ui,
                    "Short Name",
                    self.difficulty.short_name.as_mut().unwrap(),
                );

                self.difficulty.short_name.as_mut().unwrap().truncate(3);

                ui.label("Index");
                ui.add(DragValue::new(&mut self.difficulty.idx));
            })
            .response
    }
}

impl Widget for &mut NewChartOptions {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label("Filename:");
            ui.text_edit_singleline(&mut self.filename);
        });

        ui.separator();
        ui.label("Audio File:");
        ui.label(&self.audio);
        if ui.button("...").clicked() {
            let picked_file =
                nfd::open_file_dialog(Some("mp3,flac,wav,ogg"), None).map(|res| match res {
                    nfd::Response::Okay(s) => Some(s),
                    _ => None,
                });

            if let Ok(Some(picked_file)) = picked_file {
                self.audio = picked_file;
            }
        }

        ui.separator();
        ui.label("Destination folder (audio folder will be used if empty):");
        if ui.button("...").clicked() {
            let picked_folder = nfd::open_pick_folder(None).map(|res| match res {
                nfd::Response::Okay(s) => Some(PathBuf::from_str(&s)),
                _ => None,
            });

            if let Ok(Some(Ok(picked_folder))) = picked_folder {
                self.destination = Some(picked_folder);
            }
        }
        ui.separator();

        ui.add_enabled(
            !self.audio.is_empty() && !self.filename.is_empty(),
            Button::new("Ok"),
        )
    }
}

impl Widget for &mut kson::BgmInfo {
    fn ui(self, ui: &mut Ui) -> Response {
        if self.filename.is_none() {
            self.filename = Some(Default::default());
        }

        Grid::new("bgm_info")
            .show(ui, |ui| {
                ui.label("Audio File");
                ui.text_edit_singleline(self.filename.as_mut().unwrap());
                ui.end_row();

                ui.label("Offset");
                ui.add(DragValue::new(&mut self.offset).suffix("ms"));
                ui.end_row();

                ui.label("Volume");
                ui.add(Slider::new(&mut self.vol, 0.0..=1.0).clamp_to_range(true));
                ui.end_row();

                ui.separator();
                ui.end_row();

                ui.label("Preview Offset");
                ui.add(DragValue::new(&mut self.preview_offset).suffix("ms"));
                ui.end_row();

                ui.label("Preview Duration");
                ui.add(DragValue::new(&mut self.preview_duration).suffix("ms"));
                ui.end_row();
            })
            .response
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuiEvent {
    #[serde(skip_serializing)]
    NewChart(NewChartOptions), //(Audio, Filename, Destination)
    New,
    Open,
    Save,
    SaveAs,
    Metadata,
    MusicInfo,
    ToolChanged(ChartTool),
    Play,
    Undo,
    Redo,
    Home,
    End,
    Next,
    Previous,
    ExportKsh,
    Preferences,
}

impl std::fmt::Display for GuiEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let GuiEvent::ToolChanged(tool) = self {
            write!(f, "{:?}", tool)
        } else {
            write!(f, "{:?}", self)
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize, Eq, PartialOrd, Ord)]
pub enum ChartTool {
    None,
    BT,
    FX,
    RLaser,
    LLaser,
    BPM,
    TimeSig,
    Camera,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Clone)]
pub struct KeyCombo {
    key: egui::Key,
    modifiers: Modifiers,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Copy, Clone)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub mac_cmd: bool,
    pub command: bool,
}

struct AppState {
    editor: chart_editor::MainState,
    key_bindings: HashMap<KeyCombo, GuiEvent>,
    show_preferences: bool,
    new_chart: Option<NewChartOptions>,
    meta_edit: Option<MetaInfo>,
    bgm_edit: Option<BgmInfo>,
    exiting: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    key_bindings: HashMap<KeyCombo, GuiEvent>,
    track_width: f32,
    beats_per_column: u32,
}

//TODO: ehhhhhhhhh
impl From<egui::Modifiers> for Modifiers {
    fn from(
        egui::Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd,
            command,
        }: egui::Modifiers,
    ) -> Self {
        Self {
            alt,
            ctrl,
            shift,
            mac_cmd,
            command,
        }
    }
}

impl KeyCombo {
    fn new(key: egui::Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

impl std::fmt::Display for Modifiers {
    #[cfg(not(target_os = "macos"))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys = Vec::new();
        if self.ctrl {
            keys.push("ctrl");
        }
        if self.alt {
            keys.push("alt");
        }
        if self.shift {
            keys.push("shift");
        }

        write!(f, "{}", keys.join(" + "))
    }
    #[cfg(target_os = "macos")]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys = Vec::new();
        if self.ctrl {
            keys.push("ctrl");
        }
        if self.alt {
            keys.push("opt")
        }
        if self.shift {
            keys.push("shift")
        }
        if self.command {
            keys.push("cmd")
        }

        write!(f, "{}", keys.join(" + "))
    }
}

impl std::fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.modifiers.any() {
            write!(f, "{} + {:?}", self.modifiers, self.key)
        } else {
            write!(f, "{:?}", self.key)
        }
    }
}

impl Modifiers {
    fn new() -> Self {
        Self {
            alt: false,
            command: false,
            ctrl: false,
            mac_cmd: false,
            shift: false,
        }
    }

    fn alt(mut self) -> Self {
        self.alt = true;
        self
    }
    fn command(mut self) -> Self {
        self.command = true;
        self
    }
    #[cfg(target_os = "macos")]
    fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }
    #[cfg(not(target_os = "macos"))]
    fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self.command = true;
        self
    }
    fn mac_cmd(mut self) -> Self {
        self.mac_cmd = true;
        self
    }
    fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    fn any(self) -> bool {
        self.alt || self.command || self.ctrl || self.mac_cmd || self.shift
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut default_bindings = HashMap::new();
        let nomod = Modifiers::new();

        default_bindings.insert(
            KeyCombo::new(Key::S, Modifiers::new().ctrl()),
            GuiEvent::Save,
        );
        default_bindings.insert(
            KeyCombo::new(Key::N, Modifiers::new().ctrl()),
            GuiEvent::New,
        );
        default_bindings.insert(
            KeyCombo::new(Key::P, Modifiers::new().ctrl()),
            GuiEvent::Preferences,
        );
        default_bindings.insert(
            KeyCombo::new(Key::T, Modifiers::new().ctrl()),
            GuiEvent::Metadata,
        );
        default_bindings.insert(
            KeyCombo::new(Key::M, Modifiers::new().ctrl()),
            GuiEvent::MusicInfo,
        );
        default_bindings.insert(
            KeyCombo::new(Key::S, Modifiers::new().ctrl().shift()),
            GuiEvent::SaveAs,
        );
        default_bindings.insert(
            KeyCombo::new(Key::O, Modifiers::new().ctrl()),
            GuiEvent::Open,
        );
        default_bindings.insert(
            KeyCombo::new(Key::Z, Modifiers::new().ctrl()),
            GuiEvent::Undo,
        );
        default_bindings.insert(
            KeyCombo::new(Key::Y, Modifiers::new().ctrl()),
            GuiEvent::Redo,
        );

        //Tools
        {
            default_bindings.insert(
                KeyCombo::new(Key::Num0, nomod),
                GuiEvent::ToolChanged(ChartTool::None),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num1, nomod),
                GuiEvent::ToolChanged(ChartTool::BT),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num2, nomod),
                GuiEvent::ToolChanged(ChartTool::FX),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num3, nomod),
                GuiEvent::ToolChanged(ChartTool::LLaser),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num4, nomod),
                GuiEvent::ToolChanged(ChartTool::RLaser),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num5, nomod),
                GuiEvent::ToolChanged(ChartTool::BPM),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num6, nomod),
                GuiEvent::ToolChanged(ChartTool::TimeSig),
            );
            default_bindings.insert(
                KeyCombo::new(Key::Num7, nomod),
                GuiEvent::ToolChanged(ChartTool::Camera),
            );
        }

        default_bindings.insert(KeyCombo::new(Key::Space, nomod), GuiEvent::Play);
        default_bindings.insert(KeyCombo::new(Key::Home, nomod), GuiEvent::Home);
        default_bindings.insert(KeyCombo::new(Key::End, nomod), GuiEvent::End);
        default_bindings.insert(KeyCombo::new(Key::PageDown, nomod), GuiEvent::Next);
        default_bindings.insert(KeyCombo::new(Key::PageUp, nomod), GuiEvent::Previous);

        Self {
            key_bindings: default_bindings,
            track_width: 72.0,
            beats_per_column: 16,
        }
    }
}

pub fn rect_xy_wh(rect: [f32; 4]) -> Rect {
    let (mut x, mut y, mut w, mut h) = (rect[0], rect[1], rect[2], rect[3]);
    if w < 0.0 {
        x += w;
        w = w.abs();
    }

    if h < 0.0 {
        y += h;
        h = h.abs();
    }

    Rect::from_x_y_ranges(x..=x + w, y..=y + h)
}

const TOOLS: [(&str, ChartTool); 6] = [
    ("BT", ChartTool::BT),
    ("FX", ChartTool::FX),
    ("LL", ChartTool::LLaser),
    ("RL", ChartTool::RLaser),
    ("BPM", ChartTool::BPM),
    ("TS", ChartTool::TimeSig),
];

impl AppState {
    fn preferences(&mut self, ui: &mut Ui) {
        warn_if_debug_build(ui);

        ui.add(
            Slider::new(&mut self.editor.screen.track_width, 50.0..=300.0)
                .clamp_to_range(true)
                .text("Track Width"),
        );

        ui.add(
            Slider::new(&mut self.editor.screen.beats_per_col, 4..=32)
                .clamp_to_range(true)
                .text("Beats per column"),
        );

        let mut binding_vec: Vec<(&KeyCombo, &GuiEvent)> = self.key_bindings.iter().collect();
        binding_vec.sort_by_key(|f| f.1);
        ui.separator();
        ui.label("Hotkeys");
        Grid::new("hotkey_grid").striped(true).show(ui, |ui| {
            for (key, event) in binding_vec {
                ui.label(format!("{}", event));
                ui.add(Label::new(format!("{}", key)).wrap(false));
                ui.end_row();
            }
        });

        if ui.button("Reset to defaults").clicked() {
            self.key_bindings = Config::default().key_bindings;
        }
    }
}

const CONFIG_KEY: &str = "CONFIG_2";

fn menu_ui(ui: &mut Ui, title: impl ToString, min_width: f32, add_contents: impl FnOnce(&mut Ui)) {
    menu::menu_button(ui, title.to_string(), |ui| {
        ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.allocate_exact_size(Vec2::new(min_width, 0.0), Sense::hover());
            add_contents(ui);
        });
    });
}

impl App for AppState {
    fn setup(
        &mut self,
        _ctx: &egui::Context,
        _frame: &eframe::epi::Frame,
        storage: Option<&dyn eframe::epi::Storage>,
    ) {
        let config = if let Some(storage) = storage {
            let c: Option<Config> = eframe::epi::get_value(storage, CONFIG_KEY);
            c.unwrap_or_default()
        } else {
            Config::default()
        };
        self.key_bindings = config.key_bindings;
        self.editor.screen.track_width = config.track_width;
        self.editor.screen.beats_per_col = config.beats_per_column;
    }

    fn on_exit_event(&mut self) -> bool {
        let at_save = self.editor.actions.saved();
        if !at_save {
            self.exiting = true;
        }

        at_save
    }

    fn warm_up_enabled(&self) -> bool {
        false
    }

    fn save(&mut self, storage: &mut dyn eframe::epi::Storage) {
        let new_config = Config {
            key_bindings: self.key_bindings.clone(),
            beats_per_column: self.editor.screen.beats_per_col,
            track_width: self.editor.screen.track_width,
        };

        eframe::epi::set_value(storage, CONFIG_KEY, &new_config)
    }

    fn on_exit(&mut self) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    fn update(&mut self, ctx: &egui::Context, frame: &eframe::epi::Frame) {
        //input checking
        //TODO: Block events when exiting?
        let events = { ctx.input().events.clone() };
        for e in events {
            match e {
                egui::Event::Copy => {}
                egui::Event::Cut => {}
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                } => {
                    if pressed && !ctx.wants_keyboard_input() {
                        let key_combo = KeyCombo {
                            key,
                            modifiers: modifiers.into(),
                        };

                        match self.key_bindings.get(&key_combo) {
                            Some(GuiEvent::New) => {
                                if self.new_chart.is_none() {
                                    self.new_chart = Some(Default::default())
                                }
                            }
                            Some(GuiEvent::Preferences) => self.show_preferences = true,
                            Some(GuiEvent::Metadata) => {
                                self.meta_edit = Some(self.editor.chart.meta.clone())
                            }
                            Some(GuiEvent::MusicInfo) => {
                                self.bgm_edit =
                                    Some(self.editor.chart.audio.bgm.clone().unwrap_or_default())
                            }

                            Some(action) => self.editor.gui_event_queue.push_back(action.clone()),
                            None => (),
                        }
                    }
                }
                egui::Event::PointerMoved(pos) => self.editor.mouse_motion_event(pos),

                _ => {}
            }
        }

        if let Err(e) = self.editor.update(ctx) {
            panic!("{}", e);
        }

        //draw
        //menu
        {
            egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
                menu::bar(ui, |ui| {
                    menu_ui(ui, "File", 100.0, |ui| {
                        if ui.button("New").clicked() {
                            self.new_chart = Some(Default::default());
                        }
                        if ui.button("Open").clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::Open);
                        }
                        if ui.button("Save").clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::Save)
                        }
                        if ui.button("Save As").clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::SaveAs)
                        }
                        if ui.button("Export Ksh").clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::ExportKsh)
                        }
                        ui.separator();
                        if ui.button("Preferences").clicked() {
                            self.show_preferences = true;
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            frame.quit();
                        }
                    });
                    menu_ui(ui, "Edit", 70.0, |ui| {
                        let undo_desc = self.editor.actions.prev_action_desc();
                        let redo_desc = self.editor.actions.next_action_desc();

                        if ui
                            .add_enabled(
                                undo_desc.is_some(),
                                Button::new(format!(
                                    "Undo: {}",
                                    undo_desc.as_ref().unwrap_or(&String::new())
                                )),
                            )
                            .clicked()
                        {
                            self.editor.gui_event_queue.push_back(GuiEvent::Undo);
                        }
                        if ui
                            .add_enabled(
                                redo_desc.is_some(),
                                Button::new(format!(
                                    "Redo: {}",
                                    redo_desc.as_ref().unwrap_or(&String::new())
                                )),
                            )
                            .clicked()
                        {
                            self.editor.gui_event_queue.push_back(GuiEvent::Redo);
                        }

                        ui.separator();
                        if ui.button("Metadata").clicked() && self.meta_edit.is_none() {
                            self.meta_edit = Some(self.editor.chart.meta.clone());
                        }
                        if ui.button("Music Info").clicked() && self.meta_edit.is_none() {
                            self.bgm_edit =
                                Some(self.editor.chart.audio.bgm.clone().unwrap_or_default());
                        }
                    });

                    if !self.editor.actions.saved() {
                        ui.with_layout(Layout::right_to_left(), |ui| {
                            ui.add(egui::Label::new(RichText::new("*").color(Color32::RED)))
                                .on_hover_text("Unsaved Changes")
                        });
                    }
                });
                ui.separator();
                menu::bar(ui, |ui| {
                    for (name, tool) in &TOOLS {
                        if ui
                            .selectable_label(self.editor.current_tool == *tool, *name)
                            .clicked()
                        {
                            if *tool == self.editor.current_tool {
                                self.editor
                                    .gui_event_queue
                                    .push_back(GuiEvent::ToolChanged(ChartTool::None))
                            } else {
                                self.editor
                                    .gui_event_queue
                                    .push_back(GuiEvent::ToolChanged(*tool));
                            }
                        }
                    }
                })
            });
        }

        //stuff
        {
            let mut open = self.show_preferences;
            egui::Window::new("Preferences")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.with_layout(Layout::top_down_justified(egui::Align::Min), |ui| {
                        self.preferences(ui);
                    });
                });
            self.show_preferences = open;

            //New chart dialog
            if let Some(new_chart) = &mut self.new_chart {
                let mut open = true;
                let mut event = None;
                egui::Window::new("New").open(&mut open).show(ctx, |ui| {
                    if new_chart.ui(ui).clicked() {
                        event = Some(GuiEvent::NewChart(new_chart.clone()));
                    }
                });

                if let Some(event) = event {
                    self.editor.gui_event_queue.push_back(event);
                    self.new_chart = None;
                }

                if !open {
                    self.new_chart = None;
                }
            }

            //Metadata dialog
            if self.meta_edit.is_some() {
                let mut open = true;
                egui::Window::new("Metadata")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        self.meta_edit.as_mut().unwrap().ui(ui);
                        ui.add_space(10.0);
                        if ui.button("Ok").clicked() {
                            let new_action = self.editor.actions.new_action();
                            let new_meta = self.meta_edit.take().unwrap();
                            new_action.action = Box::new(move |chart: &mut Chart| {
                                chart.meta = new_meta.clone();
                                Ok(())
                            });
                            new_action.description = String::from("Update Metadata");
                        }
                    });
                if !open {
                    self.meta_edit = None;
                }
            }

            //Music data dialog
            if self.bgm_edit.is_some() {
                let mut open = true;
                egui::Window::new("Music Info")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        self.bgm_edit.as_mut().unwrap().ui(ui);
                        ui.add_space(10.0);
                        if ui.button("Ok").clicked() {
                            let new_action = self.editor.actions.new_action();
                            let new_bgm = self.bgm_edit.take().unwrap();
                            new_action.description = "Update Music Info".into();
                            new_action.action = Box::new(move |chart: &mut Chart| {
                                chart.audio.bgm = Some(new_bgm.clone());
                                Ok(())
                            });
                        }
                    });
                if !open {
                    self.bgm_edit = None;
                }
            }
        }

        //main
        {
            let main_frame = Frame {
                margin: Vec2::new(0.0, 0.0),
                fill: Color32::BLACK,
                ..Default::default()
            };
            {
                // Move the tool out of the editor state so it can't modify itself in unexpected ways. Pleases borrow checker.
                let mut borrowed_tool = self.editor.cursor_object.take();
                if let Some(tool) = borrowed_tool.as_mut() {
                    tool.draw_ui(&mut self.editor, ctx);
                }
                self.editor.cursor_object = borrowed_tool;
            }

            let main_response = egui::CentralPanel::default()
                .frame(main_frame)
                .show(ctx, |ui| self.editor.draw(ui))
                .inner;

            match main_response {
                Ok(response) => {
                    let pos = ctx.input().pointer.hover_pos().unwrap_or(Pos2::ZERO);
                    if response.hovered() && ctx.input().scroll_delta != Vec2::ZERO {
                        self.editor.mouse_wheel_event(ctx.input().scroll_delta.y);
                    }

                    if response.clicked() {
                        self.editor.primary_clicked(pos)
                    }

                    if response.middle_clicked() {
                        self.editor.middle_clicked(pos)
                    }

                    if response.drag_started()
                        && ctx
                            .input()
                            .pointer
                            .button_down(egui::PointerButton::Primary)
                    {
                        self.editor.drag_start(
                            egui::PointerButton::Primary,
                            pos.x,
                            pos.y,
                            &Modifiers::from(ctx.input().modifiers),
                        )
                    }

                    if response.drag_released() {
                        self.editor
                            .drag_end(egui::PointerButton::Primary, pos.x, pos.y)
                    }
                }
                Err(e) => panic!("{}", e),
            }
        }
        //exiting
        {
            if self.exiting {
                egui::Window::new("There are unsaved changes, save changes before closing?")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Yes").clicked() {
                                self.exiting = false;
                                if matches!(self.editor.save(), Ok(true)) {
                                    frame.quit();
                                }
                            }
                            if ui.button("No").clicked() {
                                self.exiting = false;
                                self.editor.actions.save(); //marks as saved but doesn't actually save
                                frame.quit();
                            }
                            if ui.button("Cancel").clicked() {
                                self.exiting = false;
                            }
                        });
                    });
            }
        }
    }

    fn name(&self) -> &str {
        "KSON Editor"
    }
}

fn main() -> Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;
    let options = eframe::NativeOptions {
        drag_and_drop_support: false,
        multisample: 8,
        ..Default::default()
    };

    eframe::run_native(
        Box::new(AppState {
            editor: MainState::new()?,
            key_bindings: HashMap::new(),
            show_preferences: false,
            new_chart: None,
            meta_edit: None,
            bgm_edit: None,
            exiting: false,
        }),
        options,
    );
}
