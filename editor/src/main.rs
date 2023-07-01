#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use chart_editor::MainState;
use eframe::egui::style::Selection;
use eframe::egui::{
    self, menu, warn_if_debug_build, Button, Color32, ComboBox, DragValue, Frame, Grid, Key, Label,
    Layout, Pos2, Rect, Response, RichText, Sense, Slider, Ui, Vec2, Visuals,
};
use eframe::App;
use i18n_embed::unic_langid::LanguageIdentifier;
use kson::{BgmInfo, Chart, MetaInfo};
use puffin::profile_scope;
use serde::{Deserialize, Serialize};

mod action_stack;
mod assets;
mod camera_widget;
mod chart_camera;
mod chart_editor;
mod i18n;
mod tools;
mod utils;

pub trait Widget {
    fn ui(self, ui: &mut Ui) -> Response;
}

use i18n::fl;
use tracing::info;

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
                edit_row(ui, &i18n::fl!("title"), &mut self.title);
                edit_row(ui, &i18n::fl!("artist"), &mut self.artist);
                edit_row(ui, &i18n::fl!("effector"), &mut self.chart_author);
                edit_row(ui, &i18n::fl!("jacket"), &mut self.jacket_filename);
                edit_row(ui, &i18n::fl!("jacket_artist"), &mut self.jacket_author);

                ui.label(i18n::fl!("difficulty"));
                ui.end_row();

                ui.label(i18n::fl!("level"));
                ui.add(DragValue::new(&mut self.level).clamp_range(1..=20));
                ui.end_row();

                ui.label(i18n::fl!("index"));
                ui.add(DragValue::new(&mut self.difficulty));
            })
            .response
    }
}

impl Widget for &mut NewChartOptions {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label(i18n::fl!("filename"));
            ui.text_edit_singleline(&mut self.filename);
        });

        ui.separator();
        ui.label(i18n::fl!("audio_file"));
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
        ui.label(i18n::fl!("destination_folder"));
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
            Button::new(i18n::fl!("ok")),
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
                ui.label(i18n::fl!("audio_file"));
                ui.text_edit_singleline(self.filename.as_mut().unwrap());
                ui.end_row();

                ui.label(i18n::fl!("offset"));
                ui.add(DragValue::new(&mut self.offset).suffix("ms"));
                ui.end_row();

                ui.label(i18n::fl!("volume"));
                ui.add(Slider::new(&mut self.vol, 0.0..=1.0).clamp_to_range(true));
                ui.end_row();

                ui.separator();
                ui.end_row();

                ui.label(i18n::fl!("preview_offset"));
                ui.add(DragValue::new(&mut self.preview.offset).suffix("ms"));
                ui.end_row();

                ui.label(i18n::fl!("preview_duration"));
                ui.add(DragValue::new(&mut self.preview.duration).suffix("ms"));
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
    language: LanguageIdentifier,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    key_bindings: HashMap<KeyCombo, GuiEvent>,
    track_width: f32,
    beats_per_column: u32,
    language: LanguageIdentifier,
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
            language: "en".parse().unwrap(),
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
                .text(i18n::fl!("track_width")),
        );

        ui.add(
            Slider::new(&mut self.editor.screen.beats_per_col, 4..=32)
                .clamp_to_range(true)
                .text(i18n::fl!("beats_per_col")),
        );

        let selected = ComboBox::new("lang_select", "Language")
            .selected_text(&self.language.language.to_string())
            .show_ui(ui, |ui| {
                [
                    ui.selectable_value(
                        &mut self.language,
                        "en".parse::<LanguageIdentifier>().unwrap(),
                        "en",
                    ),
                    ui.selectable_value(
                        &mut self.language,
                        "sv".parse::<LanguageIdentifier>().unwrap(),
                        "sv",
                    ),
                ]
            });

        if let Some(inner) = selected.inner {
            if inner.iter().any(|r| r.clicked()) {
                i18n::localizer().select(&[self.language.clone()]).unwrap();
            }
        }

        let mut binding_vec: Vec<(&KeyCombo, &GuiEvent)> = self.key_bindings.iter().collect();
        binding_vec.sort_by_key(|f| f.1);
        ui.separator();
        ui.label(i18n::fl!("hotkeys"));
        Grid::new("hotkey_grid").striped(true).show(ui, |ui| {
            for (key, event) in binding_vec {
                ui.label(format!("{}", event));
                ui.add(Label::new(format!("{}", key)).wrap(false));
                ui.end_row();
            }
        });

        if ui.button(i18n::fl!("reset_to_default")).clicked() {
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

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let new_config = Config {
            key_bindings: self.key_bindings.clone(),
            beats_per_column: self.editor.screen.beats_per_col,
            track_width: self.editor.screen.track_width,
            language: self.language.clone(),
        };

        eframe::set_value(storage, CONFIG_KEY, &new_config)
    }

    fn on_exit(&mut self, _ctx: &eframe::glow::Context) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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
                    menu_ui(ui, i18n::fl!("file"), 100.0, |ui| {
                        if ui.button(i18n::fl!("new")).clicked() {
                            self.new_chart = Some(Default::default());
                        }
                        if ui.button(i18n::fl!("open")).clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::Open);
                        }
                        if ui.button(i18n::fl!("save")).clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::Save)
                        }
                        if ui.button(i18n::fl!("save_as")).clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::SaveAs)
                        }
                        if ui.button(i18n::fl!("export_ksh")).clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::ExportKsh)
                        }
                        ui.separator();
                        if ui.button(i18n::fl!("preferences")).clicked() {
                            self.show_preferences = true;
                        }
                        ui.separator();
                        if ui.button(i18n::fl!("exit")).clicked() {
                            frame.quit();
                        }
                    });
                    menu_ui(ui, i18n::fl!("edit"), 70.0, |ui| {
                        let undo_desc = self.editor.actions.prev_action_desc();
                        let redo_desc = self.editor.actions.next_action_desc();

                        if ui
                            .add_enabled(
                                undo_desc.is_some(),
                                Button::new(i18n::fl!(
                                    "undo",
                                    action = undo_desc.as_ref().unwrap_or(&String::new()).clone()
                                )),
                            )
                            .clicked()
                        {
                            self.editor.gui_event_queue.push_back(GuiEvent::Undo);
                        }
                        if ui
                            .add_enabled(
                                redo_desc.is_some(),
                                Button::new(i18n::fl!(
                                    "redo",
                                    action = redo_desc.as_ref().unwrap_or(&String::new()).clone()
                                )),
                            )
                            .clicked()
                        {
                            self.editor.gui_event_queue.push_back(GuiEvent::Redo);
                        }

                        ui.separator();
                        if ui.button(i18n::fl!("metadata")).clicked() && self.meta_edit.is_none() {
                            self.meta_edit = Some(self.editor.chart.meta.clone());
                        }
                        if ui.button(i18n::fl!("music_info")).clicked() && self.meta_edit.is_none()
                        {
                            i18n::localizer().select(&vec!["sv".parse().unwrap()]);
                            self.bgm_edit =
                                Some(self.editor.chart.audio.bgm.clone().unwrap_or_default());
                        }
                    });

                    if !self.editor.actions.saved() {
                        ui.with_layout(Layout::right_to_left(), |ui| {
                            ui.add(egui::Label::new(RichText::new("*").color(Color32::RED)))
                                .on_hover_text(i18n::fl!("unsaved_changes"))
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
            egui::Window::new(i18n::fl!("preferences"))
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
                egui::Window::new(i18n::fl!("new"))
                    .open(&mut open)
                    .show(ctx, |ui| {
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
                egui::Window::new(i18n::fl!("metadata"))
                    .open(&mut open)
                    .show(ctx, |ui| {
                        self.meta_edit.as_mut().unwrap().ui(ui);
                        ui.add_space(10.0);
                        if ui.button(i18n::fl!("ok")).clicked() {
                            let new_action = self.editor.actions.new_action();
                            let new_meta = self.meta_edit.take().unwrap();
                            new_action.action = Box::new(move |chart: &mut Chart| {
                                chart.meta = new_meta.clone();
                                Ok(())
                            });
                            new_action.description = String::from(i18n::fl!("update_metadata"));
                        }
                    });
                if !open {
                    self.meta_edit = None;
                }
            }

            //Music data dialog
            if self.bgm_edit.is_some() {
                let mut open = true;
                egui::Window::new(i18n::fl!("music_info"))
                    .open(&mut open)
                    .show(ctx, |ui| {
                        self.bgm_edit.as_mut().unwrap().ui(ui);
                        ui.add_space(10.0);
                        if ui.button(i18n::fl!("ok")).clicked() {
                            let new_action = self.editor.actions.new_action();
                            let new_bgm = self.bgm_edit.take().unwrap();
                            new_action.description = i18n::fl!("update_music_info").into();
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
                outer_margin: 0.0.into(),
                inner_margin: 0.0.into(),
                fill: Color32::BLACK,
                ..Default::default()
            };
            {
                // Move the tool out of the editor state so it can't modify itself in unexpected ways. Pleases borrow checker.
                let mut borrowed_tool = self.editor.cursor_object.take();
                if let Some(tool) = borrowed_tool.as_mut() {
                    profile_scope!("Tool UI");
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
                egui::Window::new(i18n::fl!("unsaved_changes_alert"))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button(i18n::fl!("yes")).clicked() {
                                self.exiting = false;
                                if matches!(self.editor.save(), Ok(true)) {
                                    frame.quit();
                                }
                            }
                            if ui.button(i18n::fl!("no")).clicked() {
                                self.exiting = false;
                                self.editor.actions.save(); //marks as saved but doesn't actually save
                                frame.quit();
                            }
                            if ui.button(i18n::fl!("cancel")).clicked() {
                                self.exiting = false;
                            }
                        });
                    });
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();
    #[cfg(feature = "profiling")]
    {
        start_puffin_server();
    }

    let options = eframe::NativeOptions {
        drag_and_drop_support: false,
        multisampling: 4,
        vsync: true,
        ..Default::default()
    };

    eframe::run_native(
        "KSON Editor",
        options,
        Box::new(|cc| {
            let config = if let Some(storage) = cc.storage {
                let c: Option<Config> = eframe::get_value(storage, CONFIG_KEY);
                c.unwrap_or_default()
            } else {
                Config::default()
            };

            let mut app = AppState {
                editor: MainState::new().unwrap_or_else(|_| todo!()),
                key_bindings: HashMap::new(),
                show_preferences: false,
                new_chart: None,
                meta_edit: None,
                bgm_edit: None,
                exiting: false,
                language: config.language,
            };

            app.key_bindings = config.key_bindings;
            app.editor.screen.track_width = config.track_width;
            app.editor.screen.beats_per_col = config.beats_per_column;
            cc.egui_ctx.set_visuals(Visuals::dark());

            Box::new(app)
        }),
    );
}

//https://github.com/emilk/egui/blob/master/examples/puffin_profiler/src/main.rs
#[cfg(feature = "profiling")]
fn start_puffin_server() {
    puffin::set_scopes_on(true); // tell puffin to collect data

    match puffin_http::Server::new("0.0.0.0:8585") {
        Ok(puffin_server) => {
            log::info!("Run:  cargo install puffin_viewer && puffin_viewer --url 127.0.0.1:8585");

            // We can store the server if we want, but in this case we just want
            // it to keep running. Dropping it closes the server, so let's not drop it!
            #[allow(clippy::mem_forget)]
            std::mem::forget(puffin_server);
        }
        Err(err) => {
            log::error!("Failed to start puffin server: {}", err);
        }
    };
}
