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
use ggez::graphics;
use ggez::Context;
use imgui::*;
use std::collections::VecDeque;

use std::time::Instant;

pub enum GuiEvent {
    Open,
    Save,
    SaveAs,
    Exit,
}

#[derive(PartialEq, Copy, Clone)]
pub enum ChartTool {
    BT,
    FX,
    RLaser,
    LLaser,
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
    mouse_state: MouseState,
    show_popup: bool,
    pub event_queue: VecDeque<GuiEvent>,
    tools: [(String, ChartTool); 4],
    pub selected_tool: ChartTool,
}

impl ImGuiWrapper {
    pub fn new(ctx: &mut Context) -> Self {
        // Create the imgui object
        let mut imgui = imgui::Context::create();
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
        let renderer = Renderer::init(&mut imgui, &mut *factory, shaders).unwrap();

        // Create instace
        Self {
            imgui,
            renderer,
            last_frame: Instant::now(),
            mouse_state: MouseState::default(),
            show_popup: false,
            event_queue: VecDeque::new(),
            tools: [
                (String::from("BT"), ChartTool::BT),
                (String::from("FX"), ChartTool::FX),
                (String::from("LL"), ChartTool::LLaser),
                (String::from("RL"), ChartTool::RLaser),
            ],
            selected_tool: ChartTool::BT,
        }
    }

    pub fn render(&mut self, ctx: &mut Context, hidpi_factor: f32) {
        // Update mouse
        self.update_mouse();

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
        let mut event_queue = &mut self.event_queue;

        // Various ui things
        {
            let mut file_menu = || {
                if ui.menu_item(im_str!("Open")).build() {
                    event_queue.push_back(GuiEvent::Open);
                }

                if ui.menu_item(im_str!("Save")).build() {
                    event_queue.push_back(GuiEvent::Save);
                }

                if ui.menu_item(im_str!("Save as")).build() {
                    event_queue.push_back(GuiEvent::SaveAs);
                }

                if ui.menu_item(im_str!("Exit")).build() {
                    event_queue.push_back(GuiEvent::Exit);
                }
            };

            // Menu bar
            ui.main_menu_bar(|| {
                ui.menu(im_str!("File")).build(file_menu);
            });

            ui.window(im_str!("Hello world"))
                .size([300.0, 600.0], imgui::Condition::FirstUseEver)
                .position([100.0, 100.0], imgui::Condition::FirstUseEver)
                .build(|| {
                    let fps = ggez::timer::fps(ctx);
                    ui.text(im_str!("FPS: {:.1}", fps));
                });

            // Toolbar
            let tools = &self.tools;
            let mut selected_tool = self.selected_tool;
            ui.window(im_str!("Toolbar"))
                .size([draw_width, 0.0], Condition::Always)
                .position([0.0, 20.0], Condition::Always)
                .movable(false)
                .resizable(false)
                .title_bar(false)
                .scroll_bar(false)
                .build(|| {
                    let mut i = 1.25;
                    for (name, value) in tools {
                        if ui.selectable(
                            ImString::new(name).as_ref(),
                            selected_tool == *value,
                            ImGuiSelectableFlags::empty(),
                            [20.0, 20.0],
                        ) {
                            selected_tool = *value; //seems unsafe(?)
                        }
                        ui.same_line(i * 40.0);
                        i = i + 1.0;
                    }
                });
            self.selected_tool = selected_tool; //will selected tool always be updated before here (?)
        }

        // Render
        let (factory, _, encoder, _, render_target) = graphics::gfx_objects(ctx);
        let draw_data = ui.render();
        self.renderer
            .render(
                &mut *factory,
                encoder,
                &mut RenderTargetView::new(render_target.clone()),
                draw_data,
            )
            .unwrap();
    }

    fn update_mouse(&mut self) {
        self.imgui.io_mut().mouse_pos =
            [self.mouse_state.pos.0 as f32, self.mouse_state.pos.1 as f32];

        self.imgui.io_mut().mouse_down = [
            self.mouse_state.pressed.0,
            self.mouse_state.pressed.1,
            self.mouse_state.pressed.2,
            false,
            false,
        ];

        self.imgui.io_mut().mouse_wheel = self.mouse_state.wheel;
        self.mouse_state.wheel = 0.0;
    }

    pub fn update_mouse_pos(&mut self, x: f32, y: f32) {
        self.mouse_state.pos = (x as i32, y as i32);
    }

    pub fn update_mouse_down(&mut self, pressed: (bool, bool, bool)) {
        self.mouse_state.pressed = pressed;

        if pressed.0 {
            self.show_popup = false;
        }
    }

    pub fn open_popup(&mut self) {
        self.show_popup = true;
    }

    pub fn captures_mouse(&self) -> bool {
        self.imgui.io().want_capture_mouse
    }

    pub fn captures_key(&self) -> bool {
        self.imgui.io().want_capture_keyboard
    }
}
