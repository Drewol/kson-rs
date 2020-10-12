#![windows_subsystem = "windows"]
mod action_stack;
mod custom_loop;
mod dsp;
mod gui;
mod playback;
mod tools;

use crate::gui::{ChartTool, GuiEvent, ImGuiWrapper};
use ggez::event::{EventHandler, KeyCode, KeyMods, MouseButton};
use ggez::graphics;
use ggez::nalgebra as na;
use ggez::{Context, GameResult};
use std::collections::VecDeque;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;
use tools::{BpmTool, ButtonInterval, CursorObject, LaserTool};

macro_rules! profile_scope {
    ($string:expr) => {
        //let _profile_scope =
        //    thread_profiler::ProfileScope::new(format!("{}: {}", module_path!(), $string));
    };
}

pub struct MainState {
    chart: kson::Chart,
    redraw: bool,
    save_path: Option<String>,
    mouse_x: f32,
    mouse_y: f32,

    cursor_line: u32,
    cursor_object: Option<Box<dyn CursorObject>>,
    actions: action_stack::ActionStack<kson::Chart>,
    pub screen: ScreenState,
    pub audio_playback: playback::AudioPlayback,
    pub gui_event_queue: VecDeque<GuiEvent>,
}

#[derive(Copy, Clone)]
pub struct ScreenState {
    w: f32,
    h: f32,
    tick_height: f32,
    track_width: f32,
    top_margin: f32,
    bottom_margin: f32,
    beats_per_col: u32,
    x_offset: f32,
    x_offset_target: f32,
    beat_res: u32,
}

impl ScreenState {
    fn lane_width(&self) -> f32 {
        self.track_width / 6.0
    }

    fn ticks_per_col(&self) -> u32 {
        self.beats_per_col * self.beat_res
    }

    fn track_spacing(&self) -> f32 {
        self.track_width * 2.0
    }

    fn tick_to_pos(&self, in_y: u32) -> (f32, f32) {
        let h = self.chart_draw_height();
        let x = (in_y / self.ticks_per_col()) as f32 * self.track_spacing();
        let y = (in_y % self.ticks_per_col()) as f32 * self.tick_height;
        let y = h - y + self.top_margin;
        (x - self.x_offset, y)
    }

    fn chart_draw_height(&self) -> f32 {
        self.h - (self.bottom_margin + self.top_margin)
    }

    fn pos_to_tick(&self, in_x: f32, in_y: f32) -> u32 {
        self.pos_to_tick_f(in_x, in_y).floor() as u32
    }

    fn pos_to_tick_f(&self, in_x: f32, in_y: f32) -> f64 {
        let h = self.chart_draw_height() as f64;
        let y: f64 = 1.0 - ((in_y - self.top_margin).max(0.0) / h as f32).min(1.0) as f64;
        let x = (in_x + self.x_offset) as f64;
        let x = math::round::floor(x as f64 / self.track_spacing() as f64, 0);
        ((y + x) * self.beats_per_col as f64 * self.beat_res as f64).max(0.0)
    }

    fn pos_to_lane(&self, in_x: f32) -> f32 {
        let mut x = (in_x + self.x_offset) % self.track_spacing();
        x = ((x - self.track_width as f32 / 2.0).max(0.0) / self.track_width as f32).min(1.0);
        (x * 6.0).min(6.0) as f32
    }

    fn update(&mut self, delta_time: f32) {
        self.x_offset = self.x_offset + (self.x_offset_target - self.x_offset) * delta_time;
    }

    fn interval_to_ranges(&self, in_interval: &kson::Interval) -> Vec<(f32, f32, f32, (f32, f32))> // (x,y,h, (start,end))
    {
        let mut res: Vec<(f32, f32, f32, (f32, f32))> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let ticks_per_col = self.beats_per_col * self.beat_res;
        let mut start = in_interval.y;
        let end = start + in_interval.l;
        while start / ticks_per_col < end / ticks_per_col {
            ranges.push((start, ticks_per_col * (1 + start / ticks_per_col)));
            start = ticks_per_col * (1 + start / ticks_per_col);
        }
        ranges.push((start, end));

        for (s, e) in ranges {
            let in_l = in_interval.l;
            let prog_s = (s - in_interval.y) as f32 / in_l as f32;
            let prog_e = (e - in_interval.y) as f32 / in_l as f32;
            let start_pos = self.tick_to_pos(s);
            let end_pos = self.tick_to_pos(e);
            if (start_pos.0 - end_pos.0).abs() > f32::EPSILON {
                res.push((
                    start_pos.0,
                    start_pos.1,
                    self.top_margin - start_pos.1,
                    (prog_s, prog_e),
                ));
            } else {
                res.push((
                    start_pos.0,
                    start_pos.1,
                    end_pos.1 - start_pos.1,
                    (prog_s, prog_e),
                ))
            }
        }
        res
    }
}

impl MainState {
    fn new(ctx: &ggez::Context) -> GameResult<MainState> {
        let s = MainState {
            chart: kson::Chart::new(),
            screen: ScreenState {
                w: 800.0,
                h: 600.0,
                tick_height: 1.0,
                track_width: 72.0,
                top_margin: 60.0,
                bottom_margin: 10.0,
                beats_per_col: 16,
                x_offset: 0.0,
                x_offset_target: 0.0,
                beat_res: 240,
            },
            redraw: false,
            save_path: None,
            mouse_x: 0.0,
            mouse_y: 0.0,

            cursor_object: None,
            audio_playback: playback::AudioPlayback::new(ctx),
            gui_event_queue: VecDeque::new(),
            cursor_line: 0,
            actions: action_stack::ActionStack::new(kson::Chart::new()),
        };
        Ok(s)
    }

    pub fn get_cursor_ms(&self) -> f64 {
        let tick = self.screen.pos_to_tick(self.mouse_x, self.mouse_y);
        let tick = tick - (tick % (self.chart.beat.resolution / 2));
        self.chart.tick_to_ms(tick)
    }

    pub fn get_cursor_tick(&self) -> u32 {
        self.screen.pos_to_tick(self.mouse_x, self.mouse_y)
    }

    pub fn get_cursor_tick_f(&self) -> f64 {
        self.screen.pos_to_tick_f(self.mouse_x, self.mouse_y)
    }

    pub fn get_cursor_lane(&self) -> f32 {
        self.screen.pos_to_lane(self.mouse_x)
    }

    fn draw_laser_section(
        &self,
        section: &kson::LaserSection,
        mb: &mut graphics::MeshBuilder,
        color: graphics::Color,
    ) -> GameResult {
        profile_scope!("Section");
        let y_base = section.y;
        let wide = section.wide == 2;
        let slam_height = 6.0 as f32;
        let half_lane = self.screen.lane_width() / 2.0;
        let half_track = self.screen.track_width / 2.0;
        let track_lane_diff = self.screen.track_width - self.screen.lane_width();

        for se in section.v.windows(2) {
            profile_scope!("Window");
            let s = &se[0];
            let e = &se[1];
            let l = e.ry - s.ry;
            let interval = kson::Interval {
                y: s.ry + y_base,
                l,
            };

            if interval.l == 0 {
                continue;
            }

            let mut start_value = s.v as f32;
            let mut syoff = 0.0 as f32;

            if let Some(value) = s.vf {
                profile_scope!("Slam");
                start_value = value as f32;
                syoff = slam_height;
                let mut sv: f32 = s.v as f32;
                let mut ev: f32 = value as f32;
                if wide {
                    ev = ev * 2.0 - 0.5;
                    sv = sv * 2.0 - 0.5;
                }

                //draw slam
                let (x, y) = self.screen.tick_to_pos(interval.y);
                let sx = x + sv * track_lane_diff + half_track + half_lane;
                let ex = x + ev * track_lane_diff + half_track + half_lane;

                let (x, w): (f32, f32) = if sx > ex {
                    (sx + half_lane, (ex - half_lane) - (sx + half_lane))
                } else {
                    (sx - half_lane, (ex + half_lane) - (sx - half_lane))
                };
                mb.rectangle(
                    graphics::DrawMode::fill(),
                    [x, y, w, -slam_height].into(),
                    color,
                );
            }

            let mut value_width = (e.v as f32 - start_value) as f32;
            if wide {
                value_width *= 2.0;
                start_value = start_value * 2.0 - 0.5;
            }

            let curve_points = (s.a.unwrap_or(0.5), s.b.unwrap_or(0.5));

            for (x, y, h, (sv, ev)) in self.screen.interval_to_ranges(&interval) {
                if (curve_points.0 - curve_points.1).abs() < std::f64::EPSILON {
                    profile_scope!("Range - Linear");
                    let sx = x
                        + (start_value + (sv * value_width)) * track_lane_diff
                        + half_track
                        + half_lane;
                    let ex = x
                        + (start_value + (ev * value_width)) * track_lane_diff
                        + half_track
                        + half_lane;

                    let sy = y;
                    let ey = y + h;

                    let xoff = half_lane;
                    let (tr, tl, br, bl): (
                        na::Point2<f32>,
                        na::Point2<f32>,
                        na::Point2<f32>,
                        na::Point2<f32>,
                    ) = (
                        [ex - xoff, ey].into(),
                        [ex + xoff, ey].into(),
                        [sx - xoff, sy - syoff].into(),
                        [sx + xoff, sy - syoff].into(),
                    );
                    syoff = 0.0; //only first section after slam needs this
                    let points = [tl, tr, br, br, bl, tl];
                    mb.triangles(&points, color)?;
                } else {
                    profile_scope!("Range - Curved");
                    let sy = y - syoff;
                    syoff = 0.0; //only first section after slam needs this
                    let ey = y + h;
                    let curve_segments = ((ey - sy).abs() / 3.0) as i32;
                    let curve_segment_h = (ey - sy) / curve_segments as f32;
                    let curve_segment_progress_h = (ev - sv) / curve_segments as f32;
                    // let interval_start_value = start_value + sv * value_width;
                    // let interval_value_width =
                    //     (start_value + ev * value_width) - interval_start_value;
                    for i in 0..curve_segments {
                        let cssv = sv + curve_segment_progress_h * i as f32;
                        let csev = sv + curve_segment_progress_h * (i + 1) as f32;
                        let csv = do_curve(cssv as f64, curve_points.0, curve_points.1) as f32;
                        let cev = do_curve(csev as f64, curve_points.0, curve_points.1) as f32;

                        let sx = x
                            + (start_value + (csv * value_width)) * track_lane_diff
                            + half_track
                            + half_lane;
                        let ex = x
                            + (start_value + (cev * value_width)) * track_lane_diff
                            + half_track
                            + half_lane;

                        let csy = sy + curve_segment_h * i as f32;
                        let cey = sy + curve_segment_h * (i + 1) as f32;

                        let xoff = half_lane;
                        let (tr, tl, br, bl): (
                            na::Point2<f32>,
                            na::Point2<f32>,
                            na::Point2<f32>,
                            na::Point2<f32>,
                        ) = (
                            [ex - xoff, cey].into(),
                            [ex + xoff, cey].into(),
                            [sx - xoff, csy].into(),
                            [sx + xoff, csy].into(),
                        );
                        let points = [tl, tr, br, br, bl, tl];
                        mb.triangles(&points, color)?;
                    }
                }
            }
        }

        if let Some(l) = section.v.last() {
            if let Some(vf) = l.vf {
                profile_scope!("End Slam");
                //draw slam
                let mut sv: f32 = l.v as f32;
                let mut ev: f32 = vf as f32;
                if wide {
                    sv = sv * 2.0 - 0.5;
                    ev = ev * 2.0 - 0.5;
                }

                let (x, y) = self.screen.tick_to_pos(l.ry + y_base);
                let sx = x + sv * track_lane_diff + half_track + half_lane;
                let ex = x + ev as f32 * track_lane_diff + half_track + half_lane;

                let (x, w): (f32, f32) = if sx > ex {
                    (sx + half_lane, (ex - half_lane) - (sx + half_lane))
                } else {
                    (sx - half_lane, (ex + half_lane) - (sx - half_lane))
                };

                mb.rectangle(
                    graphics::DrawMode::fill(),
                    [x, y, w, -slam_height].into(),
                    color,
                );
                let end_rect_x = if sx > ex {
                    0.0
                } else {
                    self.screen.lane_width()
                };
                mb.rectangle(
                    graphics::DrawMode::fill(),
                    [
                        x + w - end_rect_x,
                        y - slam_height,
                        self.screen.lane_width(),
                        -slam_height,
                    ]
                    .into(),
                    color,
                );
            }
        }

        if let Some(l) = section.v.first() {
            if l.vf.is_some() {
                let mut sv: f32 = l.v as f32;
                if wide {
                    sv = sv * 2.0 - 0.5;
                }

                let (x, y) = self.screen.tick_to_pos(l.ry + y_base);
                let x = x + sv * track_lane_diff + half_track;
                mb.rectangle(
                    graphics::DrawMode::fill(),
                    [x, y, self.screen.lane_width(), slam_height].into(),
                    color,
                );
            }
        }
        Ok(())
    }
}

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        while let Some(e) = self.gui_event_queue.pop_front() {
            match e {
                GuiEvent::Open => {
                    if let Some(new_chart) = open_chart().unwrap_or_else(|e| {
                        println!("Failed to open chart:");
                        println!("\t{}", e);
                        None
                    }) {
                        self.chart = new_chart.0.clone();
                        self.actions.reset(new_chart.0);
                        self.save_path = Some(new_chart.1);
                    }
                }
                GuiEvent::SaveAs => {
                    if let Ok(mut chart) = self.actions.get_current() {
                        chart.meta = self.chart.meta.clone();
                        if let Some(new_path) = save_chart_as(&chart).unwrap_or_else(|e| {
                            println!("Failed to save chart:");
                            println!("\t{}", e);
                            None
                        }) {
                            self.save_path = Some(new_path);
                        }
                    }
                }
                GuiEvent::Exit => ctx.continuing = false,
                GuiEvent::ToolChanged(new_tool) => match new_tool {
                    ChartTool::BT => {
                        self.cursor_object = Some(Box::new(ButtonInterval::new(false)))
                    }
                    ChartTool::FX => self.cursor_object = Some(Box::new(ButtonInterval::new(true))),
                    ChartTool::LLaser => self.cursor_object = Some(Box::new(LaserTool::new(false))),
                    ChartTool::RLaser => self.cursor_object = Some(Box::new(LaserTool::new(true))),
                    ChartTool::BPM => self.cursor_object = Some(Box::new(BpmTool::new())),
                    _ => self.cursor_object = None,
                },
                GuiEvent::Undo => self.actions.undo(),
                GuiEvent::Redo => self.actions.redo(),
                _ => (),
            }
        }
        if let Ok(current_chart) = self.actions.get_current() {
            let tempmeta = self.chart.meta.clone(); //metadata editing not covered by action stack
            self.chart = current_chart;
            self.chart.meta = tempmeta;
        }
        let delta_time = (10.0 * ggez::timer::delta(ctx).as_secs_f32()).min(1.0);
        self.screen.update(delta_time);
        let tick = self.audio_playback.get_tick(&self.chart);
        self.audio_playback.update(tick);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        profile_scope!("Draw");
        //draw chart
        {
            profile_scope!("Chart");
            //draw notes
            let track_builder = &mut graphics::MeshBuilder::new();
            let bt_builder = &mut graphics::MeshBuilder::new();
            let long_bt_builder = &mut graphics::MeshBuilder::new();
            let fx_builder = &mut graphics::MeshBuilder::new();
            let long_fx_builder = &mut graphics::MeshBuilder::new();
            let laser_builder = &mut graphics::MeshBuilder::new();
            let laser_color: [graphics::Color; 2] = [
                graphics::Color::from_rgba(0, 115, 144, 255),
                graphics::Color::from_rgba(194, 6, 140, 255),
            ];
            let min_tick_render = self.screen.pos_to_tick(-100.0, self.screen.h);
            let max_tick_render = self.screen.pos_to_tick(self.screen.w + 50.0, 0.0);
            graphics::clear(ctx, graphics::BLACK);
            let chart_draw_height = self.screen.chart_draw_height();
            let lane_width = self.screen.lane_width();
            let track_spacing = self.screen.track_spacing();
            //draw track
            {
                profile_scope!("Track");
                let track_count = 2 + (self.screen.w / self.screen.track_spacing()) as u32;
                let x = self.screen.track_width / 2.0 - (self.screen.x_offset % track_spacing)
                    + lane_width;
                for i in 0..track_count {
                    let x = x + i as f32 * track_spacing;
                    for j in 0..5 {
                        let x = x + j as f32 * lane_width;
                        track_builder.rectangle(
                            graphics::DrawMode::fill(),
                            [x, self.screen.top_margin, 0.5, chart_draw_height].into(),
                            graphics::WHITE,
                        );
                    }
                }

                //measure & beat lines
                let x = self.screen.track_width / 2.0 + self.screen.lane_width();
                let w = self.screen.lane_width() * 4.0;
                for (tick, is_measure) in self.chart.beat_line_iter() {
                    if tick < min_tick_render {
                        continue;
                    } else if tick > max_tick_render {
                        break;
                    }

                    let (tx, y) = self.screen.tick_to_pos(tick);
                    let x = tx + x;
                    let color = if is_measure {
                        ggez::graphics::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 0.0,
                            a: 1.0,
                        }
                    } else {
                        ggez::graphics::Color {
                            r: 0.5,
                            g: 0.5,
                            b: 0.5,
                            a: 1.0,
                        }
                    };
                    track_builder.rectangle(
                        graphics::DrawMode::fill(),
                        [x, y, w, -0.5].into(),
                        color,
                    );
                }
            }

            {
                profile_scope!("BT");
                for i in 0..4 {
                    for n in &self.chart.note.bt[i] {
                        if n.y + n.l < min_tick_render {
                            continue;
                        }
                        if n.y > max_tick_render {
                            break;
                        }

                        if n.l == 0 {
                            let (x, y) = self.screen.tick_to_pos(n.y);

                            let x = x
                                + i as f32 * self.screen.lane_width()
                                + 1.0 * i as f32
                                + self.screen.lane_width()
                                + self.screen.track_width / 2.0;
                            let y = y as f32;
                            let w = self.screen.track_width as f32 / 6.0 - 2.0;
                            let h = -2.0;

                            bt_builder.rectangle(
                                graphics::DrawMode::fill(),
                                [x, y, w, h].into(),
                                graphics::WHITE,
                            );
                        } else {
                            for (x, y, h, _) in self.screen.interval_to_ranges(n) {
                                let x = x
                                    + i as f32 * self.screen.lane_width()
                                    + 1.0 * i as f32
                                    + self.screen.lane_width()
                                    + self.screen.track_width / 2.0;
                                let w = self.screen.track_width as f32 / 6.0 - 2.0;

                                long_bt_builder.rectangle(
                                    graphics::DrawMode::fill(),
                                    [x, y, w, h].into(),
                                    graphics::WHITE,
                                );
                            }
                        }
                    }
                }
            }

            //fx
            {
                profile_scope!("FX");
                for i in 0..2 {
                    for n in &self.chart.note.fx[i] {
                        if n.y + n.l < min_tick_render {
                            continue;
                        }
                        if n.y > max_tick_render {
                            break;
                        }

                        if n.l == 0 {
                            let (x, y) = self.screen.tick_to_pos(n.y);

                            let x = x
                                + (i as f32 * self.screen.lane_width() * 2.0)
                                + self.screen.track_width / 2.0
                                + 2.0 * i as f32
                                + self.screen.lane_width();
                            let w = self.screen.lane_width() * 2.0 - 1.0;
                            let h = -2.0;

                            fx_builder.rectangle(
                                graphics::DrawMode::fill(),
                                [x, y, w, h].into(),
                                [1.0, 0.3, 0.0, 1.0].into(),
                            );
                        } else {
                            for (x, y, h, _) in self.screen.interval_to_ranges(n) {
                                let x = x
                                    + (i as f32 * self.screen.lane_width() * 2.0)
                                    + self.screen.track_width / 2.0
                                    + 2.0 * i as f32
                                    + self.screen.lane_width();
                                let w = self.screen.lane_width() * 2.0 - 1.0;

                                long_fx_builder.rectangle(
                                    graphics::DrawMode::fill(),
                                    [x, y, w, h].into(),
                                    [1.0, 0.3, 0.0, 0.7].into(),
                                );
                            }
                        }
                    }
                }
            }

            //laser
            {
                profile_scope!("Lasers");
                for i in 0..2 {
                    for section in &self.chart.note.laser[i] {
                        let y_base = section.y;
                        if section.v.last().unwrap().ry + y_base < min_tick_render {
                            continue;
                        }
                        if y_base > max_tick_render {
                            break;
                        }

                        self.draw_laser_section(section, laser_builder, laser_color[i])?;
                    }
                }
            }

            {
                profile_scope!("Build Meshes");
                graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha)?;
                //draw built meshes
                //track
                let track_mesh = track_builder.build(ctx)?;
                graphics::draw(ctx, &track_mesh, (na::Point2::new(0.0, 0.0),))?;

                //long fx
                let note_mesh = long_fx_builder.build(ctx);
                if let Ok(mesh) = note_mesh {
                    graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?;
                }
                //long bt
                let note_mesh = long_bt_builder.build(ctx);
                if let Ok(mesh) = note_mesh {
                    graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?;
                }
                //fx
                let note_mesh = fx_builder.build(ctx);
                if let Ok(mesh) = note_mesh {
                    graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?;
                }
                //bt
                let note_mesh = bt_builder.build(ctx);
                if let Ok(mesh) = note_mesh {
                    graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?;
                }
                //laser
                graphics::set_blend_mode(ctx, graphics::BlendMode::Add)?;
                let note_mesh = laser_builder.build(ctx);
                if let Ok(mesh) = note_mesh {
                    graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?;
                }
            }

            if let Some(cursor) = &self.cursor_object {
                cursor.draw(self, ctx).unwrap_or_else(|e| println!("{}", e));
            }

            {
                //cursor line
                graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha)?;
                let (x, y) = if self.audio_playback.is_playing() {
                    let tick = self.audio_playback.get_tick(&self.chart);

                    //let delta = ms - self.tick_to_ms(tick);
                    self.screen.tick_to_pos(tick as u32)
                } else {
                    self.screen.tick_to_pos(self.cursor_line)
                };

                let x = x + self.screen.track_width / 2.0;
                let p1: na::Point2<f32> = [x, y].into();
                let p2: na::Point2<f32> = [x + self.screen.track_width, y].into();
                let m =
                    graphics::Mesh::new_line(ctx, &[p1, p2], 1.5, (255u8, 0u8, 0u8, 255u8).into())?;
                graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))?;
            }
        }

        self.redraw = false;
        Ok(())
    }
    fn mouse_button_down_event(&mut self, _ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        if button == MouseButton::Left {
            let res = self.chart.beat.resolution;
            let lane = self.screen.pos_to_lane(x);
            let tick = self.screen.pos_to_tick(x, y);
            let tick = tick - (tick % (self.chart.beat.resolution / 2));
            let tick_f = self.screen.pos_to_tick_f(x, y);
            match self.cursor_object {
                Some(ref mut cursor) => cursor.mouse_down(
                    self.screen,
                    tick,
                    tick_f,
                    lane,
                    &self.chart,
                    &mut self.actions,
                    na::Point2::new(x, y),
                ),
                None => self.cursor_line = tick,
            }
        }
    }

    fn mouse_button_up_event(&mut self, _ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        if button == MouseButton::Left {
            let lane = self.screen.pos_to_lane(x);
            let tick = self.screen.pos_to_tick(x, y);
            let tick_f = self.screen.pos_to_tick_f(x, y);
            let tick = tick - (tick % (self.chart.beat.resolution / 2));
            if let Some(cursor) = &mut self.cursor_object {
                cursor.mouse_up(
                    self.screen,
                    tick,
                    tick_f,
                    lane,
                    &self.chart,
                    &mut self.actions,
                    na::Point2::new(x, y),
                );
            }
        }
    }

    fn resize_event(&mut self, ctx: &mut Context, w: f32, h: f32) {
        self.redraw = true;
        graphics::set_screen_coordinates(
            ctx,
            graphics::Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        )
        .unwrap_or_else(|e| println!("{}", e));
        self.screen.w = w;
        self.screen.h = h;
        self.screen.tick_height = self.screen.chart_draw_height()
            / (self.chart.beat.resolution * self.screen.beats_per_col) as f32;
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut Context,
        keycode: KeyCode,
        keymods: KeyMods,
        _repeat: bool,
    ) {
        match keycode {
            KeyCode::Home => self.screen.x_offset_target = 0.0,
            KeyCode::PageUp => {
                self.screen.x_offset_target +=
                    self.screen.w - (self.screen.w % self.screen.track_spacing())
            }
            KeyCode::PageDown => {
                self.screen.x_offset_target = (self.screen.x_offset_target
                    - (self.screen.w - (self.screen.w % self.screen.track_spacing())))
                .max(0.0)
            }
            KeyCode::End => {
                let mut target: f32 = 0.0;

                //check pos of last bt
                for i in 0..4 {
                    if let Some(note) = self.chart.note.bt[i].last() {
                        target = target
                            .max(self.screen.tick_to_pos(note.y + note.l).0 + self.screen.x_offset)
                    }
                }

                //check pos of last fx
                for i in 0..2 {
                    if let Some(note) = self.chart.note.fx[i].last() {
                        target = target
                            .max(self.screen.tick_to_pos(note.y + note.l).0 + self.screen.x_offset)
                    }
                }

                //check pos of last lasers
                for i in 0..2 {
                    if let Some(section) = self.chart.note.laser[i].last() {
                        if let Some(segment) = section.v.last() {
                            target = target.max(
                                self.screen.tick_to_pos(segment.ry + section.y).0
                                    + self.screen.x_offset,
                            )
                        }
                    }
                }

                self.screen.x_offset_target = target - (target % self.screen.track_spacing())
            }
            KeyCode::Space => {
                if self.audio_playback.is_playing() {
                    self.audio_playback.stop()
                } else if let Some(path) = &self.save_path {
                    let path = Path::new(path).parent().unwrap();
                    if let Some(bgm) = &self.chart.audio.bgm {
                        if let Some(filename) = &bgm.filename {
                            let filename = &filename.split(';').next().unwrap();
                            let path = path.join(Path::new(filename));
                            println!("Playing file: {}", path.display());
                            let path = path.to_str().unwrap();
                            if self.audio_playback.open(path).is_ok() {
                                let ms =
                                    self.chart.tick_to_ms(self.cursor_line) + bgm.offset as f64;
                                let ms = ms.max(0.0);
                                self.audio_playback.build_effects(&self.chart);
                                self.audio_playback.set_poistion(ms);
                                self.audio_playback.play();
                            }
                        }
                    }
                }
            }
            KeyCode::Z => {
                if keymods & KeyMods::CTRL != KeyMods::NONE {
                    self.actions.undo();
                }
            }
            KeyCode::Y => {
                if keymods & KeyMods::CTRL != KeyMods::NONE {
                    self.actions.redo();
                }
            }
            _ => (),
        }
    }

    fn mouse_motion_event(&mut self, _ctx: &mut Context, x: f32, y: f32, _dx: f32, _dy: f32) {
        self.mouse_x = x;
        self.mouse_y = y;

        let lane = self.screen.pos_to_lane(x);
        let tick = self.screen.pos_to_tick(x, y);
        let tick_f: f64 = self.screen.pos_to_tick_f(x, y);
        let tick = tick - (tick % (self.chart.beat.resolution / 2));
        if let Some(cursor) = &mut self.cursor_object {
            cursor.update(tick, tick_f, lane, na::Point2::new(x, y));
        }
    }

    fn mouse_wheel_event(&mut self, _ctx: &mut Context, _x: f32, y: f32) {
        self.screen.x_offset_target += y * self.screen.track_width;
        self.screen.x_offset_target = self.screen.x_offset_target.max(0.0);
    }
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

//https://github.com/m4saka/ksh2kson/issues/4#issuecomment-573343229
pub fn do_curve(x: f64, a: f64, b: f64) -> f64 {
    let t = if x < std::f64::EPSILON || a < std::f64::EPSILON {
        (a - (a * a + x - 2.0 * a * x).sqrt()) / (-1.0 + 2.0 * a)
    } else {
        x / (a + (a * a + (1.0 - 2.0 * a) * x).sqrt())
    };
    2.0 * (1.0 - t) * t * b + t * t
}

fn open_chart_file(path: String) -> Result<Option<(kson::Chart, String)>, Box<dyn Error>> {
    match get_extension_from_filename(&path)
        .unwrap_or("")
        .to_lowercase()
        .as_ref()
    {
        "ksh" => {
            let mut data = String::from("");
            File::open(&path).unwrap().read_to_string(&mut data)?;
            Ok(Some((kson::Chart::from_ksh(&data)?, path)))
        }
        "kson" => {
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            profile_scope!("kson parse");
            Ok(Some((serde_json::from_reader(reader)?, path)))
        }

        _ => Ok(None),
    }
}

fn open_chart() -> Result<Option<(kson::Chart, String)>, Box<dyn Error>> {
    let path: String;
    let dialog_result = nfd::dialog().filter("ksh,kson").open()?;

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = String::from(&file_path);
            open_chart_file(path)
        }
        _ => Ok(None),
    }
}

fn save_chart_as(chart: &kson::Chart) -> Result<Option<String>, Box<dyn Error>> {
    let path: String;
    let dialog_result = nfd::open_save_dialog(Some("kson"), None)?;

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = file_path;
            let mut file = File::create(&path).unwrap();
            profile_scope!("Write kson");
            file.write_all(serde_json::to_string(&chart)?.as_bytes())?;
        }
        _ => return Ok(None),
    }

    Ok(Some(path))
}

fn get_config_path() -> std::path::PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
    dir.push("Drewol");
    dir.push("kson-editor");
    dir
}

pub fn main() {
    thread_profiler::register_thread_with_profiler();
    std::fs::create_dir_all(get_config_path()).unwrap();

    let win_setup = ggez::conf::WindowSetup {
        title: "KSON Editor".to_owned(),
        samples: ggez::conf::NumSamples::Four,
        vsync: false,
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

    let modules = ggez::conf::ModuleConf {
        gamepad: false,
        audio: false,
    };

    let cb = ggez::ContextBuilder::new("usc-editor", "Drewol")
        .window_setup(win_setup)
        .window_mode(mode)
        .modules(modules);

    let (ctx, event_loop) = &mut cb.build().unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    let state = &mut MainState::new(&ctx).unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    let mut args = std::env::args();
    if args.len() > 1 {
        args.next();
        if let Some(input_filename) = args.next() {
            if let Ok(load_result) = open_chart_file(input_filename) {
                if let Some(loaded_chart) = load_result {
                    state.chart = loaded_chart.0;
                    state.actions.reset(state.chart.clone());
                }
            }
        }
    }

    let imgui_wrapper = &mut ImGuiWrapper::new(ctx).unwrap_or_else(|e| {
        println!("{}", e);
        panic!();
    });

    match custom_loop::run(ctx, event_loop, state, imgui_wrapper) {
        Ok(_) => (),
        Err(e) => println!("Program exited with error: {}", e),
    }
    state.audio_playback.release();
    let mut profiling_path = get_config_path();
    profiling_path.push("profiling");
    profiling_path.set_extension("json");
    thread_profiler::write_profile(profiling_path.to_str().unwrap());
}
