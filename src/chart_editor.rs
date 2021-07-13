#![windows_subsystem = "windows"]

use crate::tools::*;
use crate::*;
use anyhow::Result;

use directories_next::BaseDirs;
use eframe::egui::{Color32, CtxRef, PointerButton, Pos2, Rect, Shape, Stroke};
use eframe::egui::{Painter, Rgba};

use egui::Ui;
use kson::Ksh;
use log::debug;
use na::point;
use nalgebra as na;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

macro_rules! profile_scope {
    ($string:expr) => {
        //let _profile_scope =
        //    thread_profiler::ProfileScope::new(format!("{}: {}", module_path!(), $string));
    };
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

pub const EGUI_ID: &str = "chart_editor";

pub struct MainState {
    pub chart: kson::Chart,
    pub save_path: Option<PathBuf>,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub gui_event_queue: VecDeque<crate::GuiEvent>,
    pub cursor_line: u32,
    pub cursor_object: Option<Box<dyn CursorObject>>,
    pub actions: action_stack::ActionStack<kson::Chart>,
    pub screen: ScreenState,
    pub audio_playback: playback::AudioPlayback,
}

#[derive(Copy, Clone)]
pub struct ScreenState {
    pub w: f32,
    pub h: f32,
    pub tick_height: f32,
    pub track_width: f32,
    pub top_margin: f32,
    pub left_margin: f32,
    pub bottom_margin: f32,
    pub beats_per_col: u32,
    pub x_offset: f32,
    pub x_offset_target: f32,
    pub beat_res: u32,
}

impl ScreenState {
    pub fn lane_width(&self) -> f32 {
        self.track_width / 6.0
    }

    pub fn ticks_per_col(&self) -> u32 {
        self.beats_per_col * self.beat_res
    }

    pub fn track_spacing(&self) -> f32 {
        self.track_width * 2.0
    }

    pub fn tick_to_pos(&self, in_y: u32) -> (f32, f32) {
        let h = self.chart_draw_height();
        let x = (in_y / self.ticks_per_col()) as f32 * self.track_spacing() + self.left_margin
            - self.x_offset;
        let y = (in_y % self.ticks_per_col()) as f32 * self.tick_height;
        let y = h - y + self.top_margin;
        (x, y)
    }

    pub fn chart_draw_height(&self) -> f32 {
        self.h - (self.bottom_margin + self.top_margin)
    }

    pub fn pos_to_tick(&self, in_x: f32, in_y: f32) -> u32 {
        self.pos_to_tick_f(in_x, in_y).floor() as u32
    }

    pub fn pos_to_tick_f(&self, in_x: f32, in_y: f32) -> f64 {
        let h = self.chart_draw_height() as f64;
        let y: f64 = 1.0 - ((in_y - self.top_margin).max(0.0) / h as f32).min(1.0) as f64;
        let x = (in_x + self.x_offset - self.left_margin) as f64;
        let x = math::round::floor(x as f64 / self.track_spacing() as f64, 0);
        ((y + x) * self.beats_per_col as f64 * self.beat_res as f64).max(0.0)
    }

    pub fn pos_to_lane(&self, in_x: f32) -> f32 {
        let mut x = (in_x + self.x_offset - self.left_margin) % self.track_spacing();
        x = ((x - self.track_width as f32 / 2.0).max(0.0) / self.track_width as f32).min(1.0);
        (x * 6.0).min(6.0) as f32
    }

    pub fn update(&mut self, delta_time: f32) -> bool {
        self.x_offset = self.x_offset + (self.x_offset_target - self.x_offset) * delta_time;
        if (self.x_offset_target - self.x_offset).abs() < 0.5 {
            self.x_offset = self.x_offset_target;
            false
        } else {
            true
        }
    }

    pub fn interval_to_ranges(
        &self,
        in_interval: &kson::Interval,
    ) -> Vec<(f32, f32, f32, (f32, f32))> // (x,y,h, (start,end))
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
    pub fn new() -> Result<MainState> {
        let mut new_chart = kson::Chart::new();
        new_chart.beat.bpm.push(kson::ByPulse { y: 0, v: 120.0 });
        new_chart.beat.time_sig.push(kson::ByMeasureIndex {
            idx: 0,
            v: kson::TimeSignature { d: 4, n: 4 },
        });

        let s = MainState {
            chart: new_chart.clone(),
            screen: ScreenState {
                w: 800.0,
                h: 600.0,
                tick_height: 1.0,
                track_width: 72.0,
                top_margin: 60.0,
                bottom_margin: 10.0,
                left_margin: 0.0,
                beats_per_col: 16,
                x_offset: 0.0,
                x_offset_target: 0.0,
                beat_res: 48,
            },
            gui_event_queue: VecDeque::new(),
            save_path: None,
            mouse_x: 0.0,
            mouse_y: 0.0,

            cursor_object: None,
            audio_playback: playback::AudioPlayback::try_new()?,
            cursor_line: 0,
            actions: action_stack::ActionStack::new(new_chart),
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

    pub fn draw_cursor_line(&self, painter: &Painter, tick: u32, (r, g, b, a): (u8, u8, u8, u8)) {
        let (x, y) = self.screen.tick_to_pos(tick as u32);
        let x = x + self.screen.track_width / 2.0;
        let p1 = egui::pos2(x, y);
        let p2 = egui::pos2(x + self.screen.track_width, y);

        painter.line_segment(
            [p1, p2],
            Stroke {
                color: Color32::from_rgba_unmultiplied(r, g, b, a),
                width: 1.5,
            },
        );
    }

    pub fn draw_laser_section(
        &self,
        section: &kson::LaserSection,
        mb: &mut Vec<Shape>,
        color: Color32,
    ) -> Result<()> {
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

                mb.push(Shape::rect_filled(
                    rect_xy_wh([x, y, w, -slam_height]),
                    0.0,
                    color,
                ));
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
                    let points = vec![
                        [ex - xoff, ey].into(),
                        [ex + xoff, ey].into(),
                        [sx + xoff, sy - syoff].into(),
                        [sx - xoff, sy - syoff].into(),
                    ];

                    let segment = Shape::convex_polygon(points, color, Stroke::none());
                    mb.push(segment);
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
                        let points = vec![
                            [ex - xoff, cey].into(),
                            [ex + xoff, cey].into(),
                            [sx + xoff, csy].into(),
                            [sx - xoff, csy].into(),
                        ];

                        let segment = Shape::convex_polygon(points, color, Stroke::none());
                        mb.push(segment);
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

                mb.push(Shape::rect_filled(
                    rect_xy_wh([x, y, w, -slam_height]),
                    0.0,
                    color,
                ));

                let end_rect_x = if sx > ex {
                    0.0
                } else {
                    self.screen.lane_width()
                };

                mb.push(Shape::rect_filled(
                    rect_xy_wh([
                        x + w - end_rect_x,
                        y - slam_height,
                        self.screen.lane_width(),
                        -slam_height,
                    ]),
                    0.0,
                    color,
                ));
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
                mb.push(Shape::rect_filled(
                    rect_xy_wh([x, y, self.screen.lane_width(), slam_height]),
                    0.0,
                    color,
                ));
            }
        }
        Ok(())
    }

    pub fn update(&mut self, ctx: &CtxRef) -> Result<()> {
        while let Some(e) = self.gui_event_queue.pop_front() {
            use crate::ChartTool;
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
                GuiEvent::SaveAs | GuiEvent::Save => {
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
                GuiEvent::ToolChanged(new_tool) => match new_tool {
                    ChartTool::None => self.cursor_object = None,
                    ChartTool::BT => {
                        self.cursor_object = Some(Box::new(ButtonInterval::new(false)))
                    }
                    ChartTool::FX => self.cursor_object = Some(Box::new(ButtonInterval::new(true))),
                    ChartTool::LLaser => self.cursor_object = Some(Box::new(LaserTool::new(false))),
                    ChartTool::RLaser => self.cursor_object = Some(Box::new(LaserTool::new(true))),
                    ChartTool::BPM => self.cursor_object = Some(Box::new(BpmTool::new())),
                    ChartTool::TimeSig => self.cursor_object = Some(Box::new(TimeSigTool::new())),
                },
                GuiEvent::Undo => self.actions.undo(),
                GuiEvent::Redo => self.actions.redo(),
                GuiEvent::New(audio_file, filename, chart_folder) => {
                    let mut new_chart = kson::Chart::new();
                    new_chart.beat.bpm.push(kson::ByPulse { y: 0, v: 120.0 });
                    new_chart.beat.time_sig.push(kson::ByMeasureIndex {
                        idx: 0,
                        v: kson::TimeSignature { d: 4, n: 4 },
                    });

                    let audio_pathbuf = std::path::PathBuf::from(audio_file);
                    new_chart.audio.bgm = Some(kson::BgmInfo {
                        filename: Some(String::from(
                            audio_pathbuf.file_name().unwrap().to_str().unwrap(),
                        )),
                        offset: 0,
                        vol: 1.0,
                        preview_duration: 15000,
                        preview_filename: None,
                        preview_offset: 0,
                    });
                    self.save_path = if let Some(save_path) = chart_folder {
                        //copy audio file
                        let mut audio_new_path = std::path::PathBuf::from(save_path.clone());
                        audio_new_path.push(audio_pathbuf.file_name().unwrap());
                        if !audio_new_path.exists() {
                            std::fs::copy(audio_pathbuf, audio_new_path).unwrap();
                        }
                        Some(save_path)
                    } else {
                        Some(audio_pathbuf.parent().unwrap().to_path_buf())
                    };

                    let mut kson_path = self.save_path.clone().unwrap();
                    kson_path.push(filename);
                    kson_path.set_extension("kson");
                    self.save_path = Some(kson_path.clone());
                    if let Ok(mut file) = File::create(kson_path) {
                        file.write_all(serde_json::to_string(&new_chart).unwrap().as_bytes())
                            .unwrap();
                    }
                    self.actions.reset(new_chart.clone());
                    self.chart = new_chart;
                }
                GuiEvent::ExportKsh => {
                    if let Ok(mut chart) = self.actions.get_current() {
                        let dialog_result = nfd::open_save_dialog(Some("ksh"), None);

                        if let Ok(nfd::Response::Okay(file_path)) = dialog_result {
                            let mut file = File::create(&file_path).unwrap();
                            profile_scope!("Write KSH");
                            chart.to_ksh(file);
                        }
                    }
                }
            }
        }
        if let Ok(current_chart) = self.actions.get_current() {
            let tempmeta = self.chart.meta.clone(); //metadata editing not covered by action stack
            self.chart = current_chart;
            self.chart.meta = tempmeta;
        }

        let delta_time = (10.0 * ctx.input().unstable_dt).min(1.0);
        if self.screen.update(delta_time) {
            ctx.request_repaint();
        }
        let tick = self.audio_playback.get_tick(&self.chart);
        self.audio_playback.update(tick);
        Ok(())
    }

    pub fn draw(&mut self, ui: &Ui) -> Result<()> {
        ui.make_persistent_id(EGUI_ID);
        self.resize_event(ui.max_rect_finite());

        profile_scope!("Draw Chart");
        //draw notes
        let mut track_line_builder = Vec::new();
        let mut track_measure_builder = Vec::new();
        let mut bt_builder = Vec::new();
        let mut long_bt_builder = Vec::new();
        let mut fx_builder = Vec::new();
        let mut long_fx_builder = Vec::new();
        let mut laser_builder = Vec::new();
        let laser_color = [
            Color32::from_rgba_unmultiplied(0, 115, 144, 255),
            Color32::from_rgba_unmultiplied(194, 6, 140, 255),
        ];
        let min_tick_render = self.screen.pos_to_tick(-100.0, self.screen.h);
        let max_tick_render = self.screen.pos_to_tick(self.screen.w + 50.0, 0.0);

        let chart_draw_height = self.screen.chart_draw_height();
        let lane_width = self.screen.lane_width();
        let track_spacing = self.screen.track_spacing();
        {
            profile_scope!("Build components");
            //draw track
            {
                let track_count = 2 + (self.screen.w / self.screen.track_spacing()) as u32;
                profile_scope!("Track Components");
                let x = self.screen.track_width / 2.0 + lane_width + self.screen.left_margin
                    - (self.screen.x_offset % (self.screen.track_width * 2.0));
                for i in 0..track_count {
                    let x = x + i as f32 * track_spacing;
                    for j in 0..5 {
                        let x = x + j as f32 * lane_width;
                        track_line_builder.push(Shape::rect_filled(
                            rect_xy_wh([x, self.screen.top_margin, 0.5, chart_draw_height]),
                            0.0,
                            Color32::WHITE,
                        ));
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
                    let shade = if is_measure { 255 } else { 127 };
                    track_measure_builder.push(Shape::rect_filled(
                        rect_xy_wh([x, y, w, -0.5]),
                        0.0,
                        Color32::from_gray(shade),
                    ));
                }
            }

            //bt
            {
                profile_scope!("BT Components");
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

                            bt_builder.push(Shape::rect_filled(
                                rect_xy_wh([x, y, w, h]),
                                0.0,
                                Color32::WHITE,
                            ));
                        } else {
                            for (x, y, h, _) in self.screen.interval_to_ranges(n) {
                                let x = x
                                    + i as f32 * self.screen.lane_width()
                                    + 1.0 * i as f32
                                    + self.screen.lane_width()
                                    + self.screen.track_width / 2.0;
                                let w = self.screen.track_width as f32 / 6.0 - 2.0;

                                long_bt_builder.push(Shape::rect_filled(
                                    rect_xy_wh([x, y, w, h]),
                                    0.0,
                                    Color32::WHITE,
                                ));
                            }
                        }
                    }
                }
            }

            //fx
            {
                profile_scope!("FX Components");
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
                            let color = Color32::from_rgb(255, 77, 0);

                            fx_builder.push(Shape::rect_filled(
                                rect_xy_wh([x, y, w, h]),
                                0.0,
                                color,
                            ));
                        } else {
                            for (x, y, h, _) in self.screen.interval_to_ranges(n) {
                                let x = x
                                    + (i as f32 * self.screen.lane_width() * 2.0)
                                    + self.screen.track_width / 2.0
                                    + 2.0 * i as f32
                                    + self.screen.lane_width();
                                let w = self.screen.lane_width() * 2.0 - 1.0;
                                let color = Color32::from_rgba_unmultiplied(255, 77, 0, 180);

                                long_fx_builder.push(Shape::rect_filled(
                                    rect_xy_wh([x, y, w, h]),
                                    0.0,
                                    color,
                                ));
                            }
                        }
                    }
                }
            }

            //laser
            {
                profile_scope!("Laser Components");
                for i in 0..2 {
                    for section in &self.chart.note.laser[i] {
                        let y_base = section.y;
                        if section.v.last().unwrap().ry + y_base < min_tick_render {
                            continue;
                        }
                        if y_base > max_tick_render {
                            break;
                        }

                        self.draw_laser_section(section, &mut laser_builder, laser_color[i])?;
                    }
                }
            }
        }

        debug!("Max Rect: {:?}", ui.max_rect_finite());

        let painter = ui.painter_at(ui.max_rect_finite());

        //meshses
        {
            profile_scope!("Build Meshes");
            let mod_params = (point![-self.screen.x_offset % track_spacing, 0.0],);
            let params = (point![-self.screen.x_offset, 0.0],);
            //draw built meshes
            //track
            {
                profile_scope!("Track Mesh");
                painter.extend(track_line_builder);
                painter.extend(track_measure_builder);
            }
            //long fx
            {
                profile_scope!("Long FX Mesh");
                painter.extend(long_fx_builder);
            }
            //long bt
            {
                profile_scope!("Long BT Mesh");
                painter.extend(long_bt_builder);
            }
            //fx
            {
                profile_scope!("FX Mesh");
                painter.extend(fx_builder);
            }
            //bt
            {
                profile_scope!("BT Mesh");
                painter.extend(bt_builder);
            }
            //laser
            {
                profile_scope!("Laser Mesh");
                painter.extend(laser_builder);
            }
        }

        if let Some(cursor) = &self.cursor_object {
            cursor
                .draw(self, &painter)
                .unwrap_or_else(|e| println!("{}", e));
        }

        {
            let tick = if self.audio_playback.is_playing() {
                self.audio_playback.get_tick(&self.chart) as u32
            } else {
                self.cursor_line
            };

            self.draw_cursor_line(&painter, tick, (255u8, 0u8, 0u8, 255u8));
        }

        //BPM & Time Signatures
        {
            profile_scope!("BPM & Time Signatures");
            let mut changes: Vec<(u32, Vec<(String, (u8, u8, u8, u8))>)> = Vec::new();
            {
                profile_scope!("Build BPM & Time signature change list");
                for bpm_change in &self.chart.beat.bpm {
                    let color = (0, 128, 255, 255).into();
                    let entry = (format!("{:.2}", bpm_change.v), color);
                    match changes.binary_search_by(|c| c.0.cmp(&bpm_change.y)) {
                        Ok(idx) => changes.get_mut(idx).unwrap().1.push(entry),
                        Err(new_idx) => {
                            let mut new_vec = Vec::new();
                            new_vec.push(entry);
                            changes.insert(new_idx, (bpm_change.y, new_vec));
                        }
                    }
                }

                for ts_change in &self.chart.beat.time_sig {
                    let tick = self.chart.measure_to_tick(ts_change.idx);

                    let color = (255, 255, 0, 255);
                    let entry = (
                        format!("{}/{}", ts_change.v.n, ts_change.v.d),
                        color.clone(),
                    );

                    match changes.binary_search_by(|c| c.0.cmp(&tick)) {
                        Ok(idx) => changes.get_mut(idx).unwrap().1.push(entry),
                        Err(new_idx) => {
                            let mut new_vec = Vec::new();
                            new_vec.push(entry);
                            changes.insert(new_idx, (tick, new_vec));
                        }
                    }
                }
            }
            let mut any_texts = false;
            //TODO
            // {
            //     //TODO: Cache text, it renders very slow but it will have to do for now
            //     profile_scope!("Build Text");
            //     for c in changes {
            //         if c.0 < min_tick_render {
            //             continue;
            //         } else if c.0 > max_tick_render {
            //             break;
            //         }
            //         let (x, y) = self.screen.tick_to_pos(c.0);
            //         let line_height = 12.0;

            //         for (i, l) in c.1.iter().enumerate() {
            //             let text = graphics::Text::new(graphics::TextFragment {
            //                 text: l.0.clone(),
            //                 color: Some(l.1),
            //                 font: Some(graphics::Font::default()),
            //                 scale: Some(graphics::Scale::uniform(line_height)),
            //                 ..Default::default()
            //             });
            //             graphics::queue_text(
            //                 ctx,
            //                 &text,
            //                 [x, y - i as f32 * line_height - self.screen.bottom_margin],
            //                 Some(graphics::WHITE),
            //             );
            //             any_texts = true;
            //         }
            //     }
            // }
            // if any_texts {
            //     profile_scope!("Draw Text");
            //     graphics::draw_queued_text(
            //         ctx,
            //         graphics::DrawParam::new().dest([-self.screen.x_offset, 0.0]),
            //         Some(graphics::BlendMode::Alpha),
            //         graphics::FilterMode::Linear,
            //     )?;
            // }
        }

        Ok(())
    }
    fn mouse_button_down_event(&mut self, button: PointerButton, x: f32, y: f32) {
        if let PointerButton::Primary = button {
            let res = self.chart.beat.resolution;
            let lane = self.screen.pos_to_lane(x);
            let tick = self.screen.pos_to_tick(x, y);
            let tick = tick - (tick % (res / 2));
            let tick_f = self.screen.pos_to_tick_f(x, y);
            match self.cursor_object {
                Some(ref mut cursor) => cursor.mouse_down(
                    self.screen,
                    tick,
                    tick_f,
                    lane,
                    &self.chart,
                    &mut self.actions,
                    na::point![x, y],
                ),
                None => self.cursor_line = tick,
            }
        }
    }

    fn mouse_button_up_event(&mut self, button: PointerButton, x: f32, y: f32) {
        if let PointerButton::Primary = button {
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
                    na::point![x, y],
                );
            }
        }
    }

    fn resize_event(&mut self, size: Rect) {
        self.screen.w = size.width();
        self.screen.h = size.height();
        self.screen.top_margin = size.top() + 20.0;
        self.screen.left_margin = size.left();

        self.screen.tick_height = self.screen.chart_draw_height()
            / (self.chart.beat.resolution * self.screen.beats_per_col) as f32;
    }

    fn key_down_event(&mut self, keycode: egui::Key, keymods: egui::Modifiers, pressed: bool) {
        match keycode {
            egui::Key::Home => self.screen.x_offset_target = 0.0,
            egui::Key::PageUp => {
                self.screen.x_offset_target +=
                    self.screen.w - (self.screen.w % self.screen.track_spacing())
            }
            egui::Key::PageDown => {
                self.screen.x_offset_target = (self.screen.x_offset_target
                    - (self.screen.w - (self.screen.w % self.screen.track_spacing())))
                .max(0.0)
            }
            egui::Key::End => {
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
            egui::Key::Space => {
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
                            match self.audio_playback.open(path) {
                                Ok(_) => {
                                    let ms =
                                        self.chart.tick_to_ms(self.cursor_line) + bgm.offset as f64;
                                    let ms = ms.max(0.0);
                                    self.audio_playback.build_effects(&self.chart);
                                    self.audio_playback.set_poistion(ms);
                                    self.audio_playback.play();
                                }
                                Err(msg) => {
                                    println!("{}", msg);
                                }
                            }
                        }
                    }
                }
            }
            egui::Key::Z => {
                if keymods.ctrl {
                    self.actions.undo();
                }
            }
            egui::Key::Y => {
                if keymods.ctrl {
                    self.actions.redo();
                }
            }
            _ => (),
        }
    }

    fn mouse_motion_event(&mut self, pos: Pos2) {
        self.mouse_x = pos.x;
        self.mouse_y = pos.y;

        let lane = self.screen.pos_to_lane(pos.x);
        let tick = self.screen.pos_to_tick(pos.x, pos.y);
        let tick_f: f64 = self.screen.pos_to_tick_f(pos.x, pos.y);
        let tick = tick - (tick % (self.chart.beat.resolution / 2));

        if let Some(cursor) = &mut self.cursor_object {
            cursor.update(tick, tick_f, lane, na::point![pos.x, pos.y]);
        }
    }

    pub fn mouse_wheel_event(&mut self, y: f32) {
        self.screen.x_offset_target += y.signum() * self.screen.track_width * 2.0;
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

fn open_chart_file(path: PathBuf) -> Result<Option<(kson::Chart, PathBuf)>> {
    match path.extension().and_then(OsStr::to_str).unwrap_or_default() {
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

fn open_chart() -> Result<Option<(kson::Chart, PathBuf)>> {
    let dialog_result = nfd::dialog().filter("ksh,kson").open()?;

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            let path = PathBuf::from(&file_path);
            open_chart_file(path)
        }
        _ => Ok(None),
    }
}

fn save_chart_as(chart: &kson::Chart) -> Result<Option<PathBuf>> {
    let dialog_result = nfd::open_save_dialog(Some("kson"), None)?;

    match dialog_result {
        nfd::Response::Okay(file_path) => {
            let mut file = File::create(&file_path).unwrap();
            profile_scope!("Write kson");
            file.write_all(serde_json::to_string(&chart)?.as_bytes())?;
            Ok(Some(PathBuf::from(&file_path)))
        }
        _ => return Ok(None),
    }
}
