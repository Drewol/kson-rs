extern crate ggez;
extern crate imgui;
extern crate math;
extern crate nfd;
extern crate serde_json;

mod gui;
use crate::gui::{GuiEvent, ImGuiWrapper};
use ggez::event::{self, EventHandler, KeyCode, KeyMods, MouseButton};
use ggez::graphics;
use ggez::nalgebra as na;
use ggez::{Context, GameResult};
use math::round;
use nfd::Response;
mod chart;
use std::fs::File;
use std::io::prelude::*;

struct MainState {
    redraw: bool,
    chart: chart::Chart,
    w: f32,
    h: f32,
    tick_height: f64,
    track_width: f64,
    imgui_wrapper: ImGuiWrapper,
    save_path: Option<String>,
}

impl MainState {
    fn new(mut ctx: &mut Context) -> GameResult<MainState> {
        let s = MainState {
            w: 800.0,
            h: 600.0,
            redraw: false,
            chart: chart::Chart::new(),
            tick_height: 1.0,
            track_width: 72.0,
            imgui_wrapper: ImGuiWrapper::new(ctx),
            save_path: None,
        };
        Ok(s)
    }

    fn tick_to_pos(&self, in_y: u32) -> (f64, f64) {
        let tick = in_y as f64;
        let x = round::floor((tick * self.tick_height) / self.h as f64, 0) * self.track_width * 2.0;
        let y = ((tick * self.tick_height) as f64) % self.h as f64;
        let y = self.h as f64 - y;
        (x, y)
    }
}

impl event::EventHandler for MainState {
    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        loop {
            let event = self.imgui_wrapper.event_queue.pop_front();
            {
                match event {
                    Some(e) => match e {
                        GuiEvent::Open => match open_chart() {
                            Some((new_chart, path)) => {
                                self.chart = new_chart;
                                self.save_path = Some(path);
                            }
                            None => (),
                        },
                        GuiEvent::SaveAs => match save_chart_as(&self.chart) {
                            Some(new_path) => self.save_path = Some(new_path),
                            None => (),
                        },
                        _ => (),
                    },
                    None => break,
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        if !self.redraw {
            ggez::timer::sleep(std::time::Duration::from_millis(10));
        }

        //draw chart
        {
            graphics::clear(ctx, graphics::BLACK);

            //draw track
            let track = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                graphics::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: self.track_width as f32,
                    h: self.h,
                },
                [0.2, 0.2, 0.2, 1.0].into(),
            )?;

            let track_count = 1 + (0.5 * self.w / self.track_width as f32) as u32;
            for i in 0..track_count {
                graphics::draw(
                    ctx,
                    &track,
                    (na::Point2::new(
                        (self.track_width / 2.0 + i as f64 * self.track_width * 2.0) as f32,
                        0.0,
                    ),),
                )?;
            }

            //draw notes
            let mut any_bt = false;
            for bt_lane in self.chart.note.bt.iter() {
                any_bt = !bt_lane.is_empty();
                if any_bt {
                    break;
                }
            }
            if any_bt {
                let note_builder = &mut graphics::MeshBuilder::new();
                let note = graphics::Mesh::new_rectangle(
                    ctx,
                    graphics::DrawMode::fill(),
                    graphics::Rect {
                        x: self.track_width as f32 / 2.0,
                        y: 0.0,
                        w: self.track_width as f32 / 6.0 - 2.0,
                        h: -2.0,
                    },
                    graphics::WHITE,
                )?;
                for i in 0..4 {
                    for n in &self.chart.note.bt[i] {
                        if n.l == 0 {
                            let (x, y) = self.tick_to_pos(n.y);
                            if x > self.w as f64 + self.track_width * 2.0 {
                                break;
                            }
                            let x = (x + (i + 1) as f64 * self.track_width / 6.0) as f32 - 1.0
                                + self.track_width as f32 / 2.0;
                            let y = y as f32;
                            let w = self.track_width as f32 / 6.0 - 2.0;
                            let h = -2.0;

                            note_builder.rectangle(
                                graphics::DrawMode::fill(),
                                [x, y, w, h].into(),
                                graphics::WHITE,
                            );
                        } else {
                        }
                    }
                }

                let note_mesh = note_builder.build(ctx).unwrap();
                graphics::draw(ctx, &note_mesh, (na::Point2::new(0.0, 0.0),))?;
            }
        }

        // Draw ui
        {
            self.imgui_wrapper.render(ctx, 1.0);
        }
        graphics::present(ctx)?;
        println!("{}fps", ggez::timer::fps(ctx));
        self.redraw = false;
        Ok(())
    }
    fn mouse_button_down_event(&mut self, ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        self.redraw = true;
        let data = serde_json::to_string(&self.chart).unwrap();
        println!("Serialized = {}", data);

        //update imgui
        self.imgui_wrapper.update_mouse_down((
            button == MouseButton::Left,
            button == MouseButton::Right,
            button == MouseButton::Middle,
        ));
    }

    fn mouse_button_up_event(
        &mut self,
        _ctx: &mut Context,
        _button: MouseButton,
        _x: f32,
        _y: f32,
    ) {
        self.imgui_wrapper.update_mouse_down((false, false, false));
    }

    fn resize_event(&mut self, ctx: &mut Context, w: f32, h: f32) {
        self.redraw = true;
        graphics::set_screen_coordinates(
            ctx,
            graphics::Rect {
                x: 0.0,
                y: 0.0,
                w: w,
                h: h,
            },
        );
        self.w = w;
        self.h = h;
        self.tick_height = h as f64 / (self.chart.beat.resolution as f64 * 16.0);
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut Context,
        keycode: KeyCode,
        _keymods: KeyMods,
        _repeat: bool,
    ) {
        match keycode {
            KeyCode::P => {
                self.imgui_wrapper.open_popup();
            }
            _ => (),
        }
    }

    fn mouse_motion_event(&mut self, _ctx: &mut Context, x: f32, y: f32, _dx: f32, _dy: f32) {
        self.imgui_wrapper.update_mouse_pos(x, y);
    }
}

fn open_chart() -> Option<(chart::Chart, String)> {
    let mut chart = chart::Chart::new();
    let mut path = String::new();
    let dialog_result = nfd::dialog().filter("ksh").open().unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = file_path;
            let result = chart::Chart::from_ksh(&path);
            if result.is_err() {
                return None;
            }
            chart = result.unwrap_or_else(|e| {
                panic!(e);
            })
        }
        _ => return None,
    }

    Some((chart, path))
}

fn save_chart_as(chart: &chart::Chart) -> Option<String> {
    let mut path = String::new();
    let dialog_result = nfd::open_save_dialog(Some("kson"), None).unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = file_path;
            let mut file = File::create(&path).unwrap();
            file.write_all(serde_json::to_string(&chart).unwrap().as_bytes());
        }
        _ => return None,
    }

    Some(path)
}

pub fn main() -> GameResult {
    let win_setup = ggez::conf::WindowSetup {
        title: "USC Editor".to_owned(),
        samples: ggez::conf::NumSamples::Zero,
        vsync: true,
        icon: "".to_owned(),
        srgb: true,
    };

    let mode = ggez::conf::WindowMode {
        width: 800.0,
        height: 600.0,
        maximized: false,
        fullscreen_type: ggez::conf::FullscreenType::Windowed,
        borderless: false,
        min_width: 0.0,
        max_width: 0.0,
        min_height: 0.0,
        max_height: 0.0,
        resizable: true,
    };

    let cb = ggez::ContextBuilder::new("usc-editor", "Drewol")
        .window_setup(win_setup)
        .window_mode(mode);

    let (ctx, event_loop) = &mut cb.build()?;

    let state = &mut MainState::new(ctx)?;

    event::run(ctx, event_loop, state)
}
