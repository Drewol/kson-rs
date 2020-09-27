/*
* MIT License
*
* Copyright (c) 2019 Olivia Ifrim
*
* Permission is hereby granted, free of charge, to any person obtaining a copy
* of this software and associated documentation files (the "Software"), to deal
* in the Software without restriction, including without limitation the rights
* to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
* copies of the Software, and to permit persons to whom the Software is
* furnished to do so, subject to the following conditions:
*
* The above copyright notice and this permission notice shall be included in all
* copies or substantial portions of the Software.
*
* THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
* IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
* FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
* AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
* LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
* OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
* SOFTWARE.
*/
extern crate gfx_core;
extern crate gfx_device_gl;
extern crate imgui;
extern crate imgui_gfx_renderer;

use self::gfx_core::{handle::RenderTargetView, memory::Typed};
use self::imgui_gfx_renderer::*;
use crate::MainState;
use ggez::graphics;
use ggez::Context;
use imgui::*;
use std::collections::VecDeque;
use std::error::Error;

use std::time::Instant;

#[derive(PartialEq, Copy, Clone)]
pub enum ChartTool {
    None,
    BT,
    FX,
    RLaser,
    LLaser,
}

pub enum GuiEvent {
    Open,
    Save,
    ToolChanged(ChartTool),
    Undo,
    Redo,
    SaveAs,
    Exit,
}

#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct MouseState {
    pos: (i32, i32),
    pressed: (bool, bool, bool),
    wheel: f32,
}

pub struct ImGuiWrapper {
    pub imgui: imgui::Context,
    pub renderer: Renderer<gfx_core::format::Rgba8, gfx_device_gl::Resources>,
    last_frame: Instant,
    pub event_queue: VecDeque<GuiEvent>,
    tools: [(String, ChartTool); 4],
    pub selected_tool: ChartTool,
}

impl ImGuiWrapper {
    pub fn new(ctx: &mut Context) -> Result<Self, Box<dyn Error>> {
        // Create the imgui object
        let mut imgui = imgui::Context::create();
        let mut imgui_config = crate::get_config_path();
        imgui_config.push("imgui");
        imgui_config.set_extension("ini");
        imgui.set_ini_filename(imgui_config);
        let (factory, gfx_device, _, _, _) = graphics::gfx_objects(ctx);
        // Shaders
        let shaders = {
            let version = gfx_device.get_info().shading_language;
            if version.is_embedded {
                if version.major >= 3 {
                    Shaders::GlSlEs300
                } else {
                    Shaders::GlSlEs100
                }
            } else if version.major >= 4 {
                Shaders::GlSl400
            } else if version.major >= 3 {
                Shaders::GlSl130
            } else {
                Shaders::GlSl110
            }
        };

        // Renderer
        let renderer = Renderer::init(&mut imgui, &mut *factory, shaders)?;

        // Create instace
        Ok(Self {
            imgui,
            renderer,
            last_frame: Instant::now(),
            event_queue: VecDeque::new(),
            tools: [
                (String::from("BT"), ChartTool::BT),
                (String::from("FX"), ChartTool::FX),
                (String::from("LL"), ChartTool::LLaser),
                (String::from("RL"), ChartTool::RLaser),
            ],
            selected_tool: ChartTool::None,
        })
    }

    fn labeled_text_input(ui: &Ui, target: &mut String, label: &ImStr) {
        let mut imstring = ImString::from(target.clone());
        imstring.reserve(512);
        ui.input_text(label, &mut imstring).build();
        *target = String::from(imstring.to_str());
    }

    pub fn render(&mut self, ctx: &mut Context, state: &mut MainState, hidpi_factor: f32) {
        // Create new frame
        let now = Instant::now();
        let delta = now - self.last_frame;
        let delta_s = delta.as_secs() as f32 + delta.subsec_nanos() as f32 / 1_000_000_000.0;
        self.last_frame = now;

        let (draw_width, draw_height) = graphics::drawable_size(ctx);
        self.imgui.io_mut().display_size = [draw_width, draw_height];
        self.imgui.io_mut().display_framebuffer_scale = [hidpi_factor, hidpi_factor];
        self.imgui.io_mut().delta_time = delta_s;

        let ui = self.imgui.frame();
        // Various ui things
        {
            let file_menu_items = |state: &mut MainState| {
                if MenuItem::new(im_str!("Open")).build(&ui) {
                    state.gui_event_queue.push_back(GuiEvent::Open);
                }

                if MenuItem::new(im_str!("Save")).build(&ui) {
                    state.gui_event_queue.push_back(GuiEvent::Save);
                }

                if MenuItem::new(im_str!("Save as")).build(&ui) {
                    state.gui_event_queue.push_back(GuiEvent::SaveAs);
                }

                if MenuItem::new(im_str!("Exit")).build(&ui) {
                    state.gui_event_queue.push_back(GuiEvent::Exit);
                }
            };

            let edit_menu_items = |state: &mut MainState| {
                if let Some(undo_desc) = state.actions.prev_action_desc() {
                    if MenuItem::new(im_str!("Undo: {}", undo_desc).as_ref()).build(&ui) {
                        state.gui_event_queue.push_back(GuiEvent::Undo);
                    }
                }
                if let Some(undo_desc) = state.actions.next_action_desc() {
                    if MenuItem::new(im_str!("Redo: {}", undo_desc).as_ref()).build(&ui) {
                        state.gui_event_queue.push_back(GuiEvent::Redo);
                    }
                }
            };

            // Menu bar
            let main_menu = ui.begin_main_menu_bar();
            if let Some(main_menu) = main_menu {
                let file_menu = ui.begin_menu(im_str!("File"), true);
                if let Some(file_menu) = file_menu {
                    file_menu_items(state);
                    file_menu.end(&ui);
                }
                let edit_menu = ui.begin_menu(im_str!("Edit"), true);
                if let Some(edit_menu) = edit_menu {
                    edit_menu_items(state);
                    edit_menu.end(&ui);
                }
                main_menu.end(&ui);
            }

            let cursor_ms = state.get_cursor_ms();
            let cursor_tick = state.get_cursor_tick();
            let cursor_tick_f = state.get_cursor_tick_f();
            let cursor_lane = state.get_cursor_lane();
            let (lval, rval) = state.audio_playback.get_laser_values();
            Window::new(im_str!("Stats"))
                .size([300.0, 600.0], imgui::Condition::FirstUseEver)
                .position([100.0, 100.0], imgui::Condition::FirstUseEver)
                .build(&ui, || {
                    let fps = ggez::timer::fps(ctx);
                    ui.text(im_str!("FPS: {:.1}", fps));
                    ui.text(im_str!("Cursor: {:.1}ms", cursor_ms));
                    ui.text(im_str!("Cursor tick: {}", cursor_tick));
                    ui.text(im_str!("Cursor tick_f: {:.2}", cursor_tick_f));
                    ui.text(im_str!("Cursor lane: {}", cursor_lane));
                    ui.text(im_str!("Lasers: ({:.2?},{:.2?})", lval, rval))
                });

            // Meta info

            Window::new(im_str!("Meta"))
                .size([300.0, 600.0], imgui::Condition::FirstUseEver)
                .position([100.0, 100.0], imgui::Condition::FirstUseEver)
                .build(&ui, || {
                    ImGuiWrapper::labeled_text_input(
                        &ui,
                        &mut state.chart.meta.title,
                        im_str!("Title"),
                    );

                    ImGuiWrapper::labeled_text_input(
                        &ui,
                        &mut state.chart.meta.artist,
                        im_str!("Artist"),
                    );
                });

            // Toolbar
            let tools = &self.tools;
            let current_tool = self.selected_tool;
            let mut new_tool = ChartTool::None;
            Window::new(im_str!("Toolbar"))
                .size([draw_width, 0.0], Condition::Always)
                .position([0.0, 20.0], Condition::Always)
                .movable(false)
                .resizable(false)
                .title_bar(false)
                .scroll_bar(false)
                .build(&ui, || {
                    let mut i = 1.25;
                    for (name, value) in tools {
                        if Selectable::new(ImString::new(name).as_ref())
                            .selected(current_tool == *value)
                            .flags(SelectableFlags::empty())
                            .size([20.0, 20.0])
                            .build(&ui)
                        {
                            new_tool = *value; //seems unsafe(?)
                        }
                        ui.same_line(i * 40.0);
                        i += 1.0;
                    }
                });
            if new_tool != ChartTool::None && new_tool != self.selected_tool {
                state
                    .gui_event_queue
                    .push_back(GuiEvent::ToolChanged(new_tool));
                self.selected_tool = new_tool;
            } else if self.selected_tool != ChartTool::None && new_tool == self.selected_tool {
                state
                    .gui_event_queue
                    .push_back(GuiEvent::ToolChanged(ChartTool::None));
                self.selected_tool = ChartTool::None;
            }
        }

        // Render
        let (factory, _, encoder, _, render_target) = graphics::gfx_objects(ctx);
        let draw_data = ui.render();
        self.renderer
            .render(
                &mut *factory,
                encoder,
                &mut RenderTargetView::new(render_target),
                draw_data,
            )
            .unwrap();
    }

    pub fn captures_mouse(&self) -> bool {
        self.imgui.io().want_capture_mouse
    }

    pub fn captures_key(&self) -> bool {
        self.imgui.io().want_capture_keyboard
    }
}
