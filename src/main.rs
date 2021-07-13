use std::path::PathBuf;

use anyhow::Result;
use chart_editor::MainState;
use eframe::egui::{self, menu, warn_if_debug_build, Color32, Frame, Label, Pos2, Rect, Vec2};
use eframe::epi::App;
use log::debug;
use nalgebra::ComplexField;

mod action_stack;
mod chart_editor;
mod dsp;
mod playback;
mod tools;

struct AppState {
    editor: chart_editor::MainState,
    current_tool: ChartTool,
}

pub enum GuiEvent {
    New(String, String, Option<PathBuf>), //(Audio, Filename, Destination)
    Open,
    Save,
    ToolChanged(ChartTool),
    Undo,
    Redo,
    SaveAs,
    ExportKsh,
}

#[derive(PartialEq, Copy, Clone)]
pub enum ChartTool {
    None,
    BT,
    FX,
    RLaser,
    LLaser,
    BPM,
    TimeSig,
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

impl App for AppState {
    fn setup(
        &mut self,
        _ctx: &egui::CtxRef,
        _frame: &mut eframe::epi::Frame<'_>,
        _storage: Option<&dyn eframe::epi::Storage>,
    ) {
    }

    fn warm_up_enabled(&self) -> bool {
        false
    }

    fn save(&mut self, _storage: &mut dyn eframe::epi::Storage) {}

    fn on_exit(&mut self) {}

    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut eframe::epi::Frame<'_>) {
        //input checking
        for e in &ctx.input().events {
            match e {
                egui::Event::Copy => {}
                egui::Event::Cut => {}
                egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                } => {}
                egui::Event::PointerMoved(pos) => self.editor.mouse_motion_event(*pos),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {}
                _ => {}
            }
        }

        if let Err(e) = self.editor.update(ctx) {
            panic!(e);
        }

        //draw
        let dt = ctx.input().unstable_dt;

        //menu
        {
            egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
                menu::bar(ui, |ui| {
                    menu::menu(ui, "File", |ui| {
                        if ui.button("Open").clicked() {
                            self.editor.gui_event_queue.push_back(GuiEvent::Open);
                        }
                    })
                });
                ui.separator();
                menu::bar(ui, |ui| {
                    for (name, tool) in &TOOLS {
                        if ui
                            .selectable_label(self.current_tool == *tool, name)
                            .clicked()
                        {
                            if *tool == self.current_tool {
                                self.current_tool = ChartTool::None;
                                self.editor
                                    .gui_event_queue
                                    .push_back(GuiEvent::ToolChanged(ChartTool::None))
                            } else {
                                self.current_tool = *tool;
                                self.editor
                                    .gui_event_queue
                                    .push_back(GuiEvent::ToolChanged(*tool));
                            }
                        }
                    }
                })
            });
        }

        //main
        {
            egui::SidePanel::left("leftPanel")
                .default_width(200.0)
                .resizable(true)
                .show(ctx, |ui| {
                    warn_if_debug_build(ui);
                    ui.label(&format!("FPS: {:.1}", 1.0 / &dt));
                });

            let main_frame = Frame {
                margin: Vec2::new(0.0, 0.0),
                fill: Color32::BLACK,
                ..Default::default()
            };

            if let Some(tool) = &mut self.editor.cursor_object {
                tool.draw_ui(ctx, &mut self.editor.actions);
            }

            let main_response = egui::CentralPanel::default()
                .frame(main_frame)
                .show(ctx, |ui| self.editor.draw(ui))
                .inner;

            match main_response {
                Ok(response) => {
                    if response.hovered() && ctx.input().scroll_delta != Vec2::ZERO {
                        self.editor.mouse_wheel_event(ctx.input().scroll_delta.y);
                    }

                    if response.clicked() {
                        let pos = ctx.input().pointer.hover_pos().unwrap_or(Pos2::ZERO);

                        self.editor.primary_clicked(pos)
                    }

                    if response.drag_started() {
                        let pos = ctx.input().pointer.hover_pos().unwrap_or(Pos2::ZERO);
                        self.editor
                            .drag_start(egui::PointerButton::Primary, pos.x, pos.y)
                    }

                    if response.drag_released() {
                        let pos = ctx.input().pointer.hover_pos().unwrap_or(Pos2::ZERO);
                        self.editor
                            .drag_end(egui::PointerButton::Primary, pos.x, pos.y)
                    }
                }
                Err(e) => panic!(e),
            }
        }

        frame.set_window_size(ctx.used_size());
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
        ..Default::default()
    };

    eframe::run_native(
        Box::new(AppState {
            editor: MainState::new()?,
            current_tool: ChartTool::None,
        }),
        options,
    );
}
