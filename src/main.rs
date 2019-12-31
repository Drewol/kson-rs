#![windows_subsystem = "windows"]

extern crate ggez;
extern crate imgui;
extern crate math;
extern crate nfd;
extern crate rfmod;
extern crate serde_json;
extern crate thread_profiler;
extern crate time_calc;

mod chart;
mod custom_loop;
mod gui;
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
use time_calc::{ms_from_ticks, ticks_from_ms};
use tools::{ButtonInterval, CursorObject, LaserTool};

macro_rules! profile_scope {
    ($string:expr) => {
        //let _profile_scope =
        //    thread_profiler::ProfileScope::new(format!("{}: {}", module_path!(), $string));
    };
}

pub struct MainState {
    redraw: bool,
    chart: chart::Chart,
    w: f32,
    h: f32,
    tick_height: f32,
    track_width: f32,
    save_path: Option<String>,
    top_margin: f32,
    bottom_margin: f32,
    beats_per_col: u32,
    mouse_x: f32,
    mouse_y: f32,
    x_offset: f32,
    x_offset_target: f32,
    cursor_object: Option<Box<dyn CursorObject>>,
    fmod_sys: rfmod::Sys,
    fmod_sound: Option<rfmod::Sound>,
    fmod_channel: Option<rfmod::Channel>,
    pub gui_event_queue: VecDeque<GuiEvent>,
}

impl MainState {
    fn new() -> GameResult<MainState> {
        let s = MainState {
            w: 800.0,
            h: 600.0,
            redraw: false,
            chart: chart::Chart::new(),
            tick_height: 1.0,
            track_width: 72.0,
            save_path: None,
            top_margin: 60.0,
            bottom_margin: 10.0,
            beats_per_col: 16,
            mouse_x: 0.0,
            mouse_y: 0.0,
            x_offset: 0.0,
            x_offset_target: 0.0,
            cursor_object: None,
            fmod_sys: rfmod::Sys::new().unwrap(),
            fmod_sound: None,
            fmod_channel: None,
            gui_event_queue: VecDeque::new(),
        };
        Ok(s)
    }

    pub fn get_cursor_ms(&self) -> f64 {
        let tick = self.pos_to_tick(self.mouse_x, self.mouse_y);
        let tick = tick - (tick % (self.chart.beat.resolution / 2));
        self.tick_to_ms(tick)
    }

    fn draw_laser_section(
        &self,
        section: &chart::LaserSection,
        mb: &mut graphics::MeshBuilder,
        color: graphics::Color,
    ) -> GameResult {
        profile_scope!("Section");
        let y_base = section.y;
        let wide = section.wide == 2;
        let slam_height = 6.0 as f32;
        let half_lane = self.lane_width() / 2.0;
        let half_track = self.track_width / 2.0;
        let track_lane_diff = self.track_width - self.lane_width();

        for se in section.v.windows(2) {
            profile_scope!("Window");
            let s = &se[0];
            let e = &se[1];
            let l = e.ry - s.ry;
            let interval = chart::Interval {
                y: s.ry + y_base,
                l: l,
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
                let (x, y) = self.tick_to_pos(interval.y);
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
                value_width = value_width * 2.0;
                start_value = start_value * 2.0 - 0.5;
            }

            for (x, y, h, (sv, ev)) in self.interval_to_ranges(&interval) {
                profile_scope!("Range");
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

                let (x, y) = self.tick_to_pos(l.ry + y_base);
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
            }
        }
        Ok(())
    }

    fn lane_width(&self) -> f32 {
        self.track_width / 6.0
    }

    fn ticks_per_col(&self) -> u32 {
        self.beats_per_col * self.chart.beat.resolution
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
        let h = self.chart_draw_height();
        let y = 1.0 - ((in_y - self.top_margin).max(0.0) / h).min(1.0);
        let x = in_x + self.x_offset;
        let x = math::round::floor(x as f64 / self.track_spacing() as f64, 0);
        math::round::floor(
            (y as f64 + x) * self.beats_per_col as f64 * self.chart.beat.resolution as f64,
            0,
        )
        .max(0.0) as u32
    }

    fn pos_to_lane(&self, in_x: f32) -> f32 {
        let mut x = (in_x + self.x_offset) % self.track_spacing();
        x = ((x - self.track_width as f32 / 2.0).max(0.0) / self.track_width as f32).min(1.0);
        (x * 6.0).min(5.0) as f32
    }

    fn ms_to_tick(&self, ms: f64) -> u32 {
        let mut remaining = ms;
        let mut ret: u32 = 0;
        let mut prev = self
            .chart
            .beat
            .bpm
            .first()
            .unwrap_or(&chart::ByPulse { y: 0, v: 120.0 });

        for b in &self.chart.beat.bpm {
            let new_ms = self.tick_to_ms(b.y);
            if new_ms > ms {
                break;
            }
            ret = b.y;
            remaining = ms - new_ms;
            prev = b;
        }
        ret + ticks_from_ms(remaining, prev.v, self.chart.beat.resolution) as u32
    }

    fn tick_to_ms(&self, tick: u32) -> f64 {
        let mut ret: f64 = 0.0;
        let mut prev = self
            .chart
            .beat
            .bpm
            .first()
            .unwrap_or(&chart::ByPulse { y: 0, v: 120.0 });

        for b in &self.chart.beat.bpm {
            if b.y > tick {
                break;
            }
            ret = ret + ms_from_ticks((b.y - prev.y) as i64, prev.v, self.chart.beat.resolution);
            prev = b;
        }
        ret + ms_from_ticks((tick - prev.y) as i64, prev.v, self.chart.beat.resolution)
    }

    fn interval_to_ranges(
        &self,
        in_interval: &chart::Interval,
    ) -> Vec<(f32, f32, f32, (f32, f32))> // (x,y,h, (start,end))
    {
        let mut res: Vec<(f32, f32, f32, (f32, f32))> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let ticks_per_col = self.beats_per_col * self.chart.beat.resolution;
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
            if start_pos.0 != end_pos.0 {
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

impl EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        while let Some(e) = self.gui_event_queue.pop_front() {
            match e {
                GuiEvent::Open => match open_chart().unwrap_or_else(|e| {
                    println!("Failed to open chart:");
                    println!("\t{}", e);
                    None
                }) {
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
                GuiEvent::Exit => ctx.continuing = false,
                GuiEvent::ToolChanged(new_tool) => match new_tool {
                    ChartTool::BT => {
                        self.cursor_object = Some(Box::new(ButtonInterval::new(false)))
                    }
                    ChartTool::FX => self.cursor_object = Some(Box::new(ButtonInterval::new(true))),
                    ChartTool::LLaser => self.cursor_object = Some(Box::new(LaserTool::new(false))),
                    ChartTool::RLaser => self.cursor_object = Some(Box::new(LaserTool::new(true))),
                    _ => self.cursor_object = None,
                },
                _ => (),
            }
        }

        let delta_time = (10.0 * ggez::timer::delta(ctx).as_secs_f32()).min(1.0);
        self.x_offset = self.x_offset + (self.x_offset_target - self.x_offset) * delta_time;
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
            let min_tick_render = self.pos_to_tick(-100.0, self.h);
            let max_tick_render = self.pos_to_tick(self.w + 50.0, 0.0);
            graphics::clear(ctx, graphics::BLACK);
            let chart_draw_height = self.chart_draw_height();
            let lane_width = self.lane_width();
            let track_spacing = self.track_spacing();
            //draw track
            {
                profile_scope!("Track");
                let track_count = 2 + (self.w / self.track_spacing()) as u32;
                let x = self.track_width / 2.0 - (self.x_offset % track_spacing) + lane_width;
                for i in 0..track_count {
                    let x = x + i as f32 * track_spacing;
                    for j in 0..5 {
                        let x = x + j as f32 * lane_width;
                        track_builder.rectangle(
                            graphics::DrawMode::fill(),
                            [x, self.top_margin, 0.5, chart_draw_height].into(),
                            graphics::WHITE,
                        );
                    }
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
                            let (x, y) = self.tick_to_pos(n.y);

                            let x = x
                                + i as f32 * self.lane_width()
                                + 1.0 * i as f32
                                + self.lane_width()
                                + self.track_width / 2.0;
                            let y = y as f32;
                            let w = self.track_width as f32 / 6.0 - 2.0;
                            let h = -2.0;

                            bt_builder.rectangle(
                                graphics::DrawMode::fill(),
                                [x, y, w, h].into(),
                                graphics::WHITE,
                            );
                        } else {
                            for (x, y, h, _) in self.interval_to_ranges(n) {
                                let x = x
                                    + i as f32 * self.lane_width()
                                    + 1.0 * i as f32
                                    + self.lane_width()
                                    + self.track_width / 2.0;
                                let w = self.track_width as f32 / 6.0 - 2.0;

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
                            let (x, y) = self.tick_to_pos(n.y);

                            let x = x
                                + (i as f32 * self.lane_width() * 2.0)
                                + self.track_width / 2.0
                                + 2.0 * i as f32
                                + self.lane_width();
                            let w = self.lane_width() * 2.0 - 1.0;
                            let h = -2.0;

                            fx_builder.rectangle(
                                graphics::DrawMode::fill(),
                                [x, y, w, h].into(),
                                [1.0, 0.3, 0.0, 1.0].into(),
                            );
                        } else {
                            for (x, y, h, _) in self.interval_to_ranges(n) {
                                let x = x
                                    + (i as f32 * self.lane_width() * 2.0)
                                    + self.track_width / 2.0
                                    + 2.0 * i as f32
                                    + self.lane_width();
                                let w = self.lane_width() * 2.0 - 1.0;

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
                match note_mesh {
                    Ok(mesh) => graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?,
                    _ => (),
                }
                //long bt
                let note_mesh = long_bt_builder.build(ctx);
                match note_mesh {
                    Ok(mesh) => graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?,
                    _ => (),
                }
                //fx
                let note_mesh = fx_builder.build(ctx);
                match note_mesh {
                    Ok(mesh) => graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?,
                    _ => (),
                }
                //bt
                let note_mesh = bt_builder.build(ctx);
                match note_mesh {
                    Ok(mesh) => graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?,
                    _ => (),
                }
                //laser
                graphics::set_blend_mode(ctx, graphics::BlendMode::Add)?;
                let note_mesh = laser_builder.build(ctx);
                match note_mesh {
                    Ok(mesh) => graphics::draw(ctx, &mesh, (na::Point2::new(0.0, 0.0),))?,
                    _ => (),
                }
            }

            match self.cursor_object {
                Some(ref cursor) => cursor.draw(self, ctx).unwrap_or_else(|e| println!("{}", e)),
                None => (),
            }
        }

        self.redraw = false;
        Ok(())
    }
    fn mouse_button_down_event(&mut self, _ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        if button == MouseButton::Left {
            let lane = self.pos_to_lane(x);
            let tick = self.pos_to_tick(x, y);
            let tick = tick - (tick % (self.chart.beat.resolution / 2));
            match self.cursor_object {
                Some(ref mut cursor) => cursor.mouse_down(tick, lane, &mut self.chart),
                None => (),
            }
        }
    }

    fn mouse_button_up_event(&mut self, _ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        if button == MouseButton::Left {
            let lane = self.pos_to_lane(x);
            let tick = self.pos_to_tick(x, y);
            let tick = tick - (tick % (self.chart.beat.resolution / 2));
            match self.cursor_object {
                Some(ref mut cursor) => cursor.mouse_up(tick, lane, &mut self.chart),
                None => (),
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
                w: w,
                h: h,
            },
        )
        .unwrap_or_else(|e| println!("{}", e));
        self.w = w;
        self.h = h;
        self.tick_height =
            self.chart_draw_height() / (self.chart.beat.resolution * self.beats_per_col) as f32;
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut Context,
        keycode: KeyCode,
        _keymods: KeyMods,
        _repeat: bool,
    ) {
        match keycode {
            KeyCode::Home => self.x_offset_target = 0.0,
            KeyCode::PageUp => {
                self.x_offset_target =
                    self.x_offset_target + (self.w - (self.w % self.track_spacing()))
            }
            KeyCode::PageDown => {
                self.x_offset_target =
                    (self.x_offset_target - (self.w - (self.w % self.track_spacing()))).max(0.0)
            }
            KeyCode::End => {
                let mut target: f32 = 0.0;

                //check pos of last bt
                for i in 0..4 {
                    if let Some(note) = self.chart.note.bt[i].last() {
                        target = target.max(self.tick_to_pos(note.y + note.l).0 + self.x_offset)
                    }
                }

                //check pos of last fx
                for i in 0..2 {
                    if let Some(note) = self.chart.note.fx[i].last() {
                        target = target.max(self.tick_to_pos(note.y + note.l).0 + self.x_offset)
                    }
                }

                //check pos of last lasers
                for i in 0..2 {
                    if let Some(section) = self.chart.note.laser[i].last() {
                        if let Some(segment) = section.v.last() {
                            target = target
                                .max(self.tick_to_pos(segment.ry + section.y).0 + self.x_offset)
                        }
                    }
                }

                self.x_offset_target = target - (target % self.track_spacing())
            }
            KeyCode::Space => {
                if let Some(path) = &self.save_path {
                    let path = Path::new(path).parent().unwrap();
                    if let Some(bgm) = &self.chart.audio.bgm {
                        if let Some(filename) = &bgm.filename {
                            let filename = &filename.split(";").next().unwrap();
                            let path = path.join(Path::new(filename));
                            println!("Playing file: {}", path.display());
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn mouse_motion_event(&mut self, _ctx: &mut Context, x: f32, y: f32, _dx: f32, _dy: f32) {
        self.mouse_x = x;
        self.mouse_y = y;

        let lane = self.pos_to_lane(x);
        let tick = self.pos_to_tick(x, y);
        let tick = tick - (tick % (self.chart.beat.resolution / 2));
        match self.cursor_object {
            Some(ref mut cursor) => cursor.update(tick, lane),
            None => (),
        }
    }

    fn mouse_wheel_event(&mut self, _ctx: &mut Context, _x: f32, y: f32) {
        self.x_offset_target = self.x_offset_target + y * self.track_width;
        self.x_offset_target = self.x_offset_target.max(0.0);
    }
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

fn open_chart() -> Result<Option<(chart::Chart, String)>, Box<dyn Error>> {
    let path: String;
    let dialog_result = nfd::dialog().filter("ksh,kson").open().unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = String::from(&file_path);
            match get_extension_from_filename(&file_path)
                .unwrap()
                .to_lowercase()
                .as_ref()
            {
                "ksh" => {
                    return Ok(Some((chart::Chart::from_ksh(&path)?, path)));
                }
                "kson" => {
                    let file = File::open(&path)?;
                    let reader = BufReader::new(file);
                    profile_scope!("kson parse");
                    return Ok(Some((serde_json::from_reader(reader)?, path)));
                }

                _ => (),
            }
        }
        _ => return Ok(None),
    }

    Ok(None)
}

fn save_chart_as(chart: &chart::Chart) -> Option<String> {
    let path: String;
    let dialog_result = nfd::open_save_dialog(Some("kson"), None).unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            path = file_path;
            let mut file = File::create(&path).unwrap();
            profile_scope!("Write kson");
            file.write_all(serde_json::to_string(&chart).unwrap().as_bytes())
                .unwrap_or_else(|e| println!("{}", e));
        }
        _ => return None,
    }

    Some(path)
}

pub fn main() {
    thread_profiler::register_thread_with_profiler();

    let win_setup = ggez::conf::WindowSetup {
        title: "USC Editor".to_owned(),
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

    let state = &mut MainState::new().unwrap_or_else(|e| {
        println!("{}", e);
        panic!(e);
    });

    match state.fmod_sys.init() {
        rfmod::Status::Ok => {}
        e => {
            panic!("FmodSys.init failed : {:?}", e);
        }
    };

    let imgui_wrapper = &mut ImGuiWrapper::new(ctx);

    match custom_loop::run(ctx, event_loop, state, imgui_wrapper) {
        Ok(_) => (),
        Err(e) => println!("Program exited with error: {}", e),
    }

    thread_profiler::write_profile("profiling.json");
}
