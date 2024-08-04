use crate::tools::*;
use crate::*;
use anyhow::{anyhow, bail, Result};

use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};
use eframe::egui::{
    pos2, Align2, Color32, Context, PointerButton, Pos2, Rect, Response, Sense, Shape, Stroke,
};
use eframe::egui::{Painter, Rgba};

use eframe::epaint::FontId;
use egui::Ui;
use kson::overlaps::Overlaps;
use kson::{ByPulseOption, GraphPoint, GraphSectionPoint, Interval, Ksh, Vox, KSON_RESOLUTION};
use kson_music_playback as playback;

use puffin::profile_scope;

use rodio::OutputStream;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
pub const EGUI_ID: &str = "chart_editor";

pub struct MainState {
    pub audio_out: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    pub chart: kson::Chart,
    pub save_path: Option<PathBuf>,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub gui_event_queue: VecDeque<crate::GuiEvent>,
    pub cursor_line: u32,
    pub cursor_object: Option<Box<dyn CursorObject>>,
    pub current_tool: ChartTool,
    pub actions: action_stack::ActionStack<kson::Chart>,
    pub screen: ScreenState,
    pub audio_playback: playback::AudioPlayback,
    pub laser_colors: [Color32; 2],
}

#[derive(Copy, Clone)]
pub struct ScreenState {
    pub w: f32,
    pub h: f32,
    pub tick_height: f32,
    pub track_width: f32,
    pub top_margin: f32,
    pub top: f32,
    pub left_margin: f32,
    pub bottom_margin: f32,
    pub beats_per_col: u32,
    pub x_offset: f32,
    pub x_offset_target: f32,
    pub beat_res: u32,
    pub curve_per_tick: f32,
}

type MakeVertFn = Box<dyn Fn(&[f32; 3]) -> Vertex>;

impl ScreenState {
    pub fn draw_laser_section(
        &self,
        section: &kson::LaserSection,
        mb: &mut Vec<Mesh>,
        color: Color32,
        with_uv: bool,
        slam_height_override: f32,
    ) {
        //TODO: Draw sections as a single `Mesh`
        profile_scope!("Section");
        let y_base = section.tick();
        let slam_uv = Rect {
            min: pos2(0.0, 0.0),
            max: pos2(1.0, 1.0),
        };
        let wide = section.wide() == 2;
        let slam_height = if slam_height_override.is_nan() {
            6.0_f32 * self.note_height_mult()
        } else {
            slam_height_override
        };
        let half_lane = self.lane_width() / 2.0;
        let half_track = self.track_width / 2.0;
        let track_lane_diff = self.track_width - self.lane_width();

        let mut mesh = Mesh::with_texture(Default::default());
        let make_vert: MakeVertFn = if with_uv {
            Box::new(move |p: &[f32; 3]| Vertex {
                pos: [p[0], p[1]].into(),
                color,
                uv: pos2(p[2], 0.5),
            })
        } else {
            Box::new(move |p: &[f32; 3]| Vertex {
                pos: [p[0], p[1]].into(),
                color,
                uv: WHITE_UV,
            })
        };

        let add_slam_rect = |mesh: &mut Mesh, slam_rect: Rect| {
            let i_off = mesh.vertices.len() as u32;
            mesh.add_triangle(i_off, i_off + 1, i_off + 2);
            mesh.add_triangle(i_off + 2, i_off + 1, i_off + 3);
            mesh.reserve_vertices(4);
            mesh.vertices.push(Vertex {
                pos: slam_rect.left_top(),
                uv: [0.0, 0.5].into(),
                color,
            });
            mesh.vertices.push(Vertex {
                pos: slam_rect.right_top(),
                uv: [0.0, 0.5].into(),
                color,
            });
            mesh.vertices.push(Vertex {
                pos: slam_rect.left_bottom(),
                uv: [1.0, 0.5].into(),
                color,
            });
            mesh.vertices.push(Vertex {
                pos: slam_rect.right_bottom(),
                uv: [1.0, 0.5].into(),
                color,
            });
        };

        for se in section.segments() {
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
            let mut syoff = 0.0_f32;

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
                let (pos_x, pos_y) = self.tick_to_pos(interval.y);

                let sx = pos_x + sv * track_lane_diff + half_track + half_lane;
                let ex = pos_x + ev * track_lane_diff + half_track + half_lane;

                let (x, width): (f32, f32) = if sx > ex {
                    (sx + half_lane, (ex - half_lane) - (sx + half_lane))
                } else {
                    (sx - half_lane, (ex + half_lane) - (sx - half_lane))
                };

                if with_uv {
                    add_slam_rect(&mut mesh, rect_xy_wh([x, pos_y, width, -slam_height]));
                } else {
                    mesh.add_colored_rect(rect_xy_wh([x, pos_y, width, -slam_height]), color);
                }
            }

            let mut value_width = e.v as f32 - start_value;
            if wide {
                value_width *= 2.0;
                start_value = start_value * 2.0 - 0.5;
            }

            let curve_points = (s.a, s.b);

            for (x, y, h, (sv, ev)) in self.interval_to_ranges(&interval) {
                if (curve_points.0 - curve_points.1).abs() < f64::EPSILON {
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
                    let mut points = [
                        [ex - xoff, ey, 0.0],
                        [ex + xoff, ey, 1.0],
                        [sx + xoff, sy - syoff, 1.0],
                        [sx - xoff, sy - syoff, 0.0],
                    ]
                    .iter()
                    .map(&make_vert)
                    .collect();

                    let i_off = mesh.vertices.len() as u32;
                    mesh.vertices.append(&mut points);
                    mesh.indices.append(&mut vec![
                        i_off,
                        1 + i_off,
                        2 + i_off,
                        i_off,
                        2 + i_off,
                        3 + i_off,
                    ]);
                } else {
                    profile_scope!("Range - Curved");
                    let sy = y - syoff;
                    syoff = 0.0; //only first section after slam needs this
                    let ey = y + h;
                    let curve_segments =
                        ((ey - sy).abs() / (self.tick_height.abs() / self.curve_per_tick)) as i32;
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
                        let cey = sy + curve_segment_h * i as f32 + curve_segment_h;

                        let xoff = half_lane;
                        let i_off = mesh.vertices.len() as u32;

                        let mut points: Vec<Vertex> = [
                            [ex - xoff, cey, 0.0],
                            [ex + xoff, cey, 1.0],
                            [sx + xoff, csy, 1.0],
                            [sx - xoff, csy, 0.0],
                        ]
                        .iter()
                        .map(&make_vert)
                        .collect();
                        mesh.vertices.append(&mut points);
                        mesh.indices.append(&mut vec![
                            i_off,
                            1 + i_off,
                            2 + i_off,
                            i_off,
                            2 + i_off,
                            3 + i_off,
                        ]);
                    }
                }
            }
        }

        if let Some(l) = section.last() {
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
                let ex = x + ev * track_lane_diff + half_track + half_lane;

                let (x, w): (f32, f32) = if sx > ex {
                    (sx + half_lane, (ex - half_lane) - (sx + half_lane))
                } else {
                    (sx - half_lane, (ex + half_lane) - (sx - half_lane))
                };
                let end_rect_x = if sx > ex { 0.0 } else { self.lane_width() };

                if with_uv {
                    add_slam_rect(&mut mesh, rect_xy_wh([x, y, w, -slam_height]));

                    mesh.add_rect_with_uv(
                        rect_xy_wh([
                            x + w - end_rect_x,
                            y - slam_height * self.tick_height.signum(),
                            self.lane_width(),
                            -slam_height,
                        ]),
                        slam_uv,
                        color,
                    );
                } else {
                    mesh.add_colored_rect(rect_xy_wh([x, y, w, -slam_height]), color);

                    mesh.add_colored_rect(
                        rect_xy_wh([
                            x + w - end_rect_x,
                            y - slam_height,
                            self.lane_width(),
                            -slam_height,
                        ]),
                        color,
                    );
                }
            }
        }

        if let Some(l) = section.first() {
            if l.vf.is_some() {
                let mut sv: f32 = l.v as f32;
                if wide {
                    sv = sv * 2.0 - 0.5;
                }

                let (x, y) = self.tick_to_pos(l.ry + y_base);
                let x = x + sv * track_lane_diff + half_track;
                if with_uv {
                    let slam_rect = rect_xy_wh([
                        x,
                        y - slam_height,
                        self.lane_width(),
                        slam_height * self.tick_height.signum(),
                    ]);
                    mesh.add_rect_with_uv(slam_rect, slam_uv, color);
                } else {
                    mesh.add_colored_rect(
                        rect_xy_wh([x, y, self.lane_width(), slam_height]),
                        color,
                    );
                }
            }
        }

        let segment = Mesh {
            indices: mesh.indices,
            vertices: mesh.vertices,
            ..Default::default()
        };
        mb.push(segment);
    }

    pub fn lane_width(&self) -> f32 {
        self.track_width / 6.0
    }

    pub fn ticks_per_col(&self) -> u32 {
        self.beats_per_col.saturating_mul(self.beat_res)
    }

    pub fn track_spacing(&self) -> f32 {
        self.track_width * 2.0
    }

    pub fn note_height_mult(&self) -> f32 {
        self.track_width / 72.0
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
        self.h - (self.bottom_margin + self.top_margin) + self.top
    }

    pub fn pos_to_tick(&self, in_x: f32, in_y: f32) -> u32 {
        self.pos_to_tick_f(in_x, in_y).floor() as u32
    }

    pub fn pos_to_tick_f(&self, in_x: f32, in_y: f32) -> f64 {
        let h = self.chart_draw_height() as f64;
        let y: f64 = 1.0 - ((in_y - self.top_margin).max(0.0) / h as f32).min(1.0) as f64;
        let x = (in_x + self.x_offset - self.left_margin) as f64;
        let x = math::round::floor(x / self.track_spacing() as f64, 0);
        ((y + x) * self.beats_per_col as f64 * self.beat_res as f64).max(0.0)
    }

    pub fn pos_to_lane(&self, in_x: f32) -> f32 {
        let mut x = (in_x + self.x_offset + self.left_margin) % self.track_spacing();
        x = ((x - self.track_width / 2.0).max(0.0) / self.track_width).min(1.0);
        (x * 6.0).min(6.0)
    }

    pub fn update(&mut self, delta_time: f32, beat_res: u32) -> bool {
        self.beat_res = beat_res;
        self.x_offset = self.x_offset + (self.x_offset_target - self.x_offset) * delta_time;
        if (self.x_offset_target - self.x_offset).abs() < 0.5 {
            self.x_offset = self.x_offset_target;
            false
        } else {
            true
        }
    }

    pub fn get_control_point_pos_section(
        &self,
        points: &[GraphSectionPoint],
        start_y: u32,
        bounds: (f32, f32),
        track_bounds: Option<(f32, f32)>,
    ) -> Option<Pos2> {
        self.get_control_point_pos(
            &points
                .iter()
                .map(|p| GraphPoint {
                    y: p.ry + start_y,
                    v: p.v,
                    vf: p.vf,
                    a: p.a,
                    b: p.b,
                })
                .collect::<Vec<_>>(),
            bounds,
            track_bounds,
        )
    }

    pub fn get_control_point_pos(
        &self,
        points: &[GraphPoint],
        bounds: (f32, f32),
        track_bounds: Option<(f32, f32)>,
    ) -> Option<Pos2> {
        if let (None, None) = (points.first(), points.get(1)) {
            return None;
        }

        let track_bounds = track_bounds.unwrap_or((0.0, 1.0));

        let start = points.first()?;

        let (a, b) = (start.a, start.b);

        let transform_value = |v: f64| (v - bounds.0 as f64) / (bounds.1 - bounds.0) as f64;

        let start_value = if let Some(vf) = start.vf {
            transform_value(vf)
        } else {
            transform_value(start.v)
        };
        let end = points.get(1)?;
        let start_tick = start.y;
        let end_tick = end.y;
        match start_tick.cmp(&end_tick) {
            std::cmp::Ordering::Greater => panic!("Laser section start later than end."),
            std::cmp::Ordering::Equal => return None,
            _ => {}
        };
        let intervals = self.interval_to_ranges(&Interval {
            y: start_tick,
            l: end_tick - start_tick,
        });

        if let Some(&(interval_x, interval_y, interval_h, (interval_start, interval_end))) =
            intervals.iter().find(|&&v| {
                let s = (v.3).0 as f64;
                let e = (v.3).1 as f64;
                a >= s && a <= e
            })
        {
            let value_width = transform_value(end.v) - start_value;
            let x = (start_value + b * value_width) as f32;
            let x = track_bounds.0 + x * (track_bounds.1 - track_bounds.0);
            let x = x * self.track_width + interval_x + self.track_width / 2.0;
            let y = interval_y
                + interval_h * (a as f32 - interval_start) / (interval_end - interval_start);
            Some(Pos2::new(x, y))
        } else {
            panic!("Curve `a` was not in any interval");
        }
    }
    /// Returns (x,y,h, (start,end))
    pub fn interval_to_ranges(
        &self,
        in_interval: &kson::Interval,
    ) -> Vec<(f32, f32, f32, (f32, f32))> {
        let mut res: Vec<(f32, f32, f32, (f32, f32))> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let ticks_per_col = self.beats_per_col.saturating_mul(self.beat_res);
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
    pub fn new() -> MainState {
        let (new_chart, save_path) = if let Some(Ok(Some((chart, path)))) = std::env::args()
            .nth(1)
            .map(|p| open_chart_file(PathBuf::from(p)))
        {
            (chart, Some(path))
        } else {
            let mut c = kson::Chart::new();
            c.beat.bpm.push((0, 120.0));
            c.beat.time_sig.push((0, kson::TimeSignature(4, 4)));

            (c, None)
        };

        MainState {
            chart: new_chart.clone(),
            screen: ScreenState {
                top: 0.0,
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
                curve_per_tick: 1.5,
            },
            gui_event_queue: VecDeque::new(),
            save_path,
            mouse_x: 0.0,
            mouse_y: 0.0,
            current_tool: ChartTool::None,

            cursor_object: None,
            audio_playback: playback::AudioPlayback::new(),
            cursor_line: 0,
            actions: action_stack::ActionStack::new(new_chart),
            laser_colors: [
                Color32::from_rgba_unmultiplied(0, 115, 144, 127),
                Color32::from_rgba_unmultiplied(194, 6, 140, 127),
            ],
            audio_out: None,
        }
    }

    #[allow(unused)]
    pub fn get_cursor_ms_from_mouse(&self) -> f64 {
        let tick = self.screen.pos_to_tick(self.mouse_x, self.mouse_y);
        let tick = tick - (tick % (KSON_RESOLUTION / 2));
        self.chart.tick_to_ms(tick)
    }

    #[allow(unused)]
    pub fn get_cursor_tick_from_mouse(&self) -> u32 {
        self.screen.pos_to_tick(self.mouse_x, self.mouse_y)
    }

    #[allow(unused)]
    pub fn get_cursor_tick_from_mouse_f(&self) -> f64 {
        self.screen.pos_to_tick_f(self.mouse_x, self.mouse_y)
    }

    #[allow(unused)]
    pub fn get_cursor_lane_from_mouse(&self) -> f32 {
        self.screen.pos_to_lane(self.mouse_x)
    }

    pub fn get_current_cursor_tick(&self) -> f32 {
        if self.audio_playback.is_playing() {
            self.audio_playback.get_tick(&self.chart) as f32
        } else {
            self.cursor_line as f32
        }
    }

    pub fn draw_cursor_line(&self, painter: &Painter, tick: u32, color: Color32) {
        let (x, y) = self.screen.tick_to_pos(tick);
        let x = x + self.screen.track_width / 2.0;
        let p1 = egui::pos2(x, y);
        let p2 = egui::pos2(x + self.screen.track_width, y);

        painter.line_segment([p1, p2], Stroke { color, width: 1.5 });
    }

    pub fn draw_graph(
        &self,
        graph: &impl kson::Graph<f64>,
        painter: &Painter,
        bounds: (f32, f32),
        stroke: Stroke,
    ) {
        let transform_value = |v: f32| (v - bounds.0) / (bounds.1 - bounds.0);

        let ticks_per_col = self.screen.beats_per_col * KSON_RESOLUTION;
        let min_tick_render = self.screen.pos_to_tick(-100.0, self.screen.h);
        let max_tick_render = self.screen.pos_to_tick(self.screen.w + 50.0, 0.0);

        let min_tick_render = min_tick_render - min_tick_render % ticks_per_col;

        let max_tick_render = max_tick_render - max_tick_render % ticks_per_col;

        let resolution = 3;
        for col in (min_tick_render..max_tick_render)
            .collect::<Vec<_>>()
            .chunks(ticks_per_col as usize)
        {
            for segment_ticks in col.windows(resolution).step_by(resolution - 1) {
                //could miss end of column with bad resolutions
                let s = segment_ticks[0];
                let e = segment_ticks[resolution - 1];
                let sv = transform_value(graph.value_at(s as f64) as f32);
                let ev = transform_value(graph.value_at(e as f64) as f32);

                let (sx, sy) = self.screen.tick_to_pos(s);
                let (ex, ey) = self.screen.tick_to_pos(e);

                let sx = sx + sv * self.screen.track_width + self.screen.track_width / 2.0;
                let ex = ex + ev * self.screen.track_width + self.screen.track_width / 2.0;

                painter.line_segment([pos2(sx, sy), pos2(ex, ey)], stroke);
            }
        }
    }
    //TODO: Shares most code with draw_graph, combine somehow?
    pub fn draw_graph_segmented(
        &self,
        graph: &impl kson::Graph<Option<f64>>,
        painter: &Painter,
        bounds: (f32, f32),
        stroke: Stroke,
    ) {
        let transform_value = |v: f32| (v - bounds.0) / (bounds.1 - bounds.0);

        let ticks_per_col = self.screen.beats_per_col * KSON_RESOLUTION;
        let min_tick_render = self.screen.pos_to_tick(-100.0, self.screen.h);
        let max_tick_render = self.screen.pos_to_tick(self.screen.w + 50.0, 0.0);

        let min_tick_render = min_tick_render - min_tick_render % ticks_per_col;

        let max_tick_render = max_tick_render - max_tick_render % ticks_per_col;

        let resolution = 3;
        for col in (min_tick_render..max_tick_render)
            .collect::<Vec<_>>()
            .chunks(ticks_per_col as usize)
        {
            for segment_ticks in col.windows(resolution).step_by(resolution - 1) {
                //could miss end of column with bad resolutions
                let s = segment_ticks[0];
                let e = segment_ticks[resolution - 1];

                let sv = graph.value_at(s as f64);
                let ev = graph.value_at(e as f64);

                let (sv, ev) = match (sv, ev) {
                    (Some(sv), Some(ev)) => (sv, ev),
                    _ => continue,
                };

                let sv = transform_value(sv as f32);
                let ev = transform_value(ev as f32);

                let (sx, sy) = self.screen.tick_to_pos(s);
                let (ex, ey) = self.screen.tick_to_pos(e);

                let sx = sx + sv * self.screen.track_width + self.screen.track_width / 2.0;
                let ex = ex + ev * self.screen.track_width + self.screen.track_width / 2.0;

                painter.line_segment([pos2(sx, sy), pos2(ex, ey)], stroke);
            }
        }
    }

    pub fn save(&mut self) -> Result<bool> {
        match (&self.save_path, self.actions.get_current()) {
            (None, Ok(chart)) => {
                if let Some(new_path) = save_chart_as(&chart).unwrap_or_else(|e| {
                    println!("Failed to save chart:");
                    println!("\t{}", e);
                    None
                }) {
                    self.save_path = Some(new_path);
                    self.actions.save();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            (Some(path), Ok(chart)) => {
                let mut file = File::create(path)?;
                profile_scope!("Write kson");
                file.write_all(serde_json::to_string(&chart)?.as_bytes())?;
                self.actions.save();
                Ok(true)
            }
            _ => bail!("Could not save chart."),
        }
    }

    pub fn update(&mut self, ctx: &Context) -> Result<()> {
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
                GuiEvent::Save => {
                    self.save()?;
                }
                GuiEvent::SaveAs => {
                    if let Ok(chart) = self.actions.get_current() {
                        if let Some(new_path) = save_chart_as(&chart).unwrap_or_else(|e| {
                            println!("Failed to save chart:");
                            println!("\t{}", e);
                            None
                        }) {
                            self.save_path = Some(new_path);
                            self.actions.save();
                        }
                    }
                }
                GuiEvent::ToolChanged(new_tool) => {
                    if self.current_tool != new_tool {
                        self.cursor_object = match new_tool {
                            ChartTool::None => None,
                            ChartTool::BT => Some(Box::new(ButtonInterval::new(false))),
                            ChartTool::FX => Some(Box::new(ButtonInterval::new(true))),
                            ChartTool::LLaser => Some(Box::new(LaserTool::new(false))),
                            ChartTool::RLaser => Some(Box::new(LaserTool::new(true))),
                            ChartTool::BPM => Some(Box::new(BpmTool::new())),
                            ChartTool::TimeSig => Some(Box::new(TimeSigTool::new())),
                            ChartTool::Camera => Some(Box::<CameraTool>::default()),
                        };
                        self.current_tool = new_tool;
                        ctx.request_repaint();
                    }
                }
                GuiEvent::Undo => self.actions.undo(),
                GuiEvent::Redo => self.actions.redo(),
                GuiEvent::NewChart(new_chart_opts) => {
                    let mut new_chart = kson::Chart::new();
                    new_chart.beat.bpm.push((0, 120.0));
                    new_chart.beat.time_sig.push((0, kson::TimeSignature(4, 4)));

                    let audio_pathbuf = std::path::PathBuf::from(new_chart_opts.audio);
                    new_chart.audio.bgm = kson::BgmInfo {
                        filename: String::from(
                            audio_pathbuf
                                .file_name()
                                .ok_or(anyhow!("Invalid filename"))?
                                .to_str()
                                .ok_or(anyhow!("Failed to convert filename to string"))?,
                        ),
                        offset: 0,
                        vol: 1.0,
                        preview: {
                            kson::PreviewInfo {
                                offset: 0,
                                duration: 15000,
                                preview_filename: None,
                            }
                        },
                        legacy: kson::LegacyBgmInfo {
                            fp_filenames: vec![],
                        },
                    };
                    self.save_path = if let Some(save_path) = new_chart_opts.destination {
                        //copy audio file
                        let mut audio_new_path = save_path.clone();
                        audio_new_path.push(
                            audio_pathbuf
                                .file_name()
                                .ok_or(anyhow!("Invalid filename"))?,
                        );
                        if !audio_new_path.exists() {
                            std::fs::copy(audio_pathbuf, audio_new_path)?;
                        }
                        Some(save_path)
                    } else {
                        Some(
                            audio_pathbuf
                                .parent()
                                .ok_or(anyhow!("Invalid path"))?
                                .to_path_buf(),
                        )
                    };

                    let mut kson_path =
                        self.save_path.clone().ok_or(anyhow!("Invalid save path"))?;
                    kson_path.push(new_chart_opts.filename);
                    kson_path.set_extension("kson");
                    self.save_path = Some(kson_path.clone());
                    if let Ok(mut file) = File::create(kson_path) {
                        file.write_all(serde_json::to_string(&new_chart)?.as_bytes())?;
                    }
                    self.actions.reset(new_chart.clone());
                    self.chart = new_chart;
                }
                GuiEvent::ExportKsh => {
                    if let Ok(chart) = self.actions.get_current() {
                        let dialog_result = nfd::open_save_dialog(Some("ksh"), None);

                        if let Ok(nfd::Response::Okay(file_path)) = dialog_result {
                            let mut path = PathBuf::from(file_path);
                            path.set_extension("ksh");
                            let file = File::create(&path)?;
                            profile_scope!("Write KSH");
                            chart.to_ksh(file)?;
                        }
                    }
                }
                GuiEvent::Play => {
                    if self.audio_playback.is_playing() {
                        self.audio_playback.stop();
                        drop(self.audio_out.take());
                    } else if let Some(path) = &self.save_path {
                        let path = Path::new(path)
                            .parent()
                            .ok_or(anyhow!("Invalid audio path"))?;
                        let bgm = &self.chart.audio.bgm;
                        let filename = &bgm.filename;
                        let filename = &filename
                            .split(';')
                            .next()
                            .ok_or(anyhow!("Invalid audio filename"))?;
                        let path = path.join(Path::new(filename));
                        info!("Playing file: {}", path.display());
                        let path = path.to_str().ok_or(anyhow!("Invalid audio path"))?;
                        match self.audio_playback.open_path(path) {
                            Ok(_) => {
                                let ms =
                                    self.chart.tick_to_ms(self.cursor_line) + bgm.offset as f64;
                                let ms = ms.max(0.0);
                                self.audio_playback.build_effects(&self.chart);
                                self.audio_playback.play();
                                drop(self.audio_out.take());
                                let audio_out = OutputStream::try_default()?;
                                use rodio::source::Source;
                                let audio_file = self
                                    .audio_playback
                                    .get_source()
                                    .expect("Source not available");

                                self.audio_playback.set_fx_enable(true, true);

                                self.audio_playback.play();
                                audio_out.1.play_raw(
                                    audio_file.skip_duration(Duration::from_millis(ms as _)),
                                )?;
                                self.audio_out = Some(audio_out);
                            }
                            Err(msg) => {
                                println!("{}", msg);
                            }
                        }
                    }
                }
                GuiEvent::Home => self.screen.x_offset_target = 0.0,
                GuiEvent::End => {
                    let mut target: f32 = 0.0;

                    //check pos of last bt
                    for i in 0..4 {
                        if let Some(note) = self.chart.note.bt[i].last() {
                            target = target.max(
                                self.screen.tick_to_pos(note.y + note.l).0 + self.screen.x_offset,
                            )
                        }
                    }

                    //check pos of last fx
                    for i in 0..2 {
                        if let Some(note) = self.chart.note.fx[i].last() {
                            target = target.max(
                                self.screen.tick_to_pos(note.y + note.l).0 + self.screen.x_offset,
                            )
                        }
                    }

                    //check pos of last lasers
                    for i in 0..2 {
                        if let Some(section) = self.chart.note.laser[i].last() {
                            if let Some(segment) = section.last() {
                                target = target.max(
                                    self.screen.tick_to_pos(segment.ry + section.tick()).0
                                        + self.screen.x_offset,
                                )
                            }
                        }
                    }

                    self.screen.x_offset_target = target - (target % self.screen.track_spacing())
                }
                GuiEvent::Next => {
                    self.screen.x_offset_target = (self.screen.x_offset_target
                        - (self.screen.w - (self.screen.w % self.screen.track_spacing())))
                    .max(0.0)
                }
                GuiEvent::Previous => {
                    self.screen.x_offset_target +=
                        self.screen.w - (self.screen.w % self.screen.track_spacing())
                }
                _ => (),
            }
        }
        if let Ok(current_chart) = self.actions.get_current() {
            self.chart = current_chart;
        }

        let delta_time = (10.0 * ctx.input(|x| x.unstable_dt)).min(1.0);
        if self.screen.update(delta_time, KSON_RESOLUTION) || self.audio_playback.is_playing() {
            ctx.request_repaint();
        }

        Ok(())
    }

    pub fn draw(&mut self, ui: &Ui) -> Result<Response> {
        puffin::profile_function!();

        ui.make_persistent_id(EGUI_ID);
        self.resize_event(ui.max_rect());

        let painter = ui.painter_at(ui.max_rect());
        let interact = ui.interact(ui.max_rect(), ui.id(), Sense::click_and_drag());

        //draw notes
        let mut track_line_builder = Vec::new();
        let mut track_measure_builder = Vec::new();
        let mut bt_builder = Vec::new();
        let mut long_bt_builder = Vec::new();
        let mut fx_builder = Vec::new();
        let mut long_fx_builder = Vec::new();
        let mut laser_builder = Vec::new();
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
                            rect_xy_wh([x, self.screen.top_margin, 1.0, chart_draw_height]),
                            0.0,
                            Color32::GRAY,
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
                    let color = if is_measure {
                        Rgba::from_rgb(1.0, 1.0, 0.0)
                    } else {
                        Rgba::from_gray(0.5)
                    };
                    track_measure_builder.push(Shape::rect_filled(
                        rect_xy_wh([x, painter.round_to_pixel(y), w, -1.0]),
                        0.0,
                        color,
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
                            let w = self.screen.track_width / 6.0 - 2.0;
                            let h = -2.0 * self.screen.note_height_mult();

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
                                let w = self.screen.track_width / 6.0 - 2.0;

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
                            let h = -2.0 * self.screen.note_height_mult();
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
                for (lane, color) in self.chart.note.laser.iter().zip(self.laser_colors.iter()) {
                    for section in lane {
                        let y_base = section.tick();
                        if section
                            .last()
                            .ok_or(anyhow!("Tried to draw an empty laser section"))?
                            .ry
                            + y_base
                            < min_tick_render
                        {
                            continue;
                        }
                        if y_base > max_tick_render {
                            break;
                        }

                        self.screen.draw_laser_section(
                            section,
                            &mut laser_builder,
                            *color,
                            false,
                            f32::NAN,
                        );
                    }
                }
            }
        }

        //meshses
        {
            profile_scope!("Build Meshes");
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
                painter.extend(laser_builder.into_iter().map(Shape::mesh));
            }
        }

        if let Some(cursor) = &self.cursor_object {
            profile_scope!("Tool");
            cursor
                .draw(self, &painter)
                .unwrap_or_else(|e| println!("{}", e));
        }

        {
            self.draw_cursor_line(
                &painter,
                self.get_current_cursor_tick() as u32,
                Color32::from_rgb(255u8, 0u8, 0u8),
            );
        }

        //BPM & Time Signatures
        {
            profile_scope!("BPM & Time Signatures");
            let mut changes: Vec<(u32, Vec<(String, Color32)>)> = Vec::new();
            {
                profile_scope!("Build BPM & Time signature change list");
                for bpm_change in &self.chart.beat.bpm {
                    let color = Color32::from_rgba_unmultiplied(0, 128, 255, 255);

                    let entry = (
                        emath::format_with_decimals_in_range(bpm_change.1, 0..=3),
                        color,
                    );
                    match changes.binary_search_by(|c| c.0.cmp(&bpm_change.0)) {
                        Ok(idx) => changes[idx].1.push(entry),
                        Err(new_idx) => {
                            let new_vec = vec![entry];
                            changes.insert(new_idx, (bpm_change.0, new_vec));
                        }
                    }
                }

                for ts_change in &self.chart.beat.time_sig {
                    let tick = self.chart.measure_to_tick(ts_change.0);

                    let color = Color32::from_rgba_premultiplied(255, 255, 0, 255);
                    let entry = (format!("{}/{}", ts_change.1 .0, ts_change.1 .1), color);

                    match changes.binary_search_by(|c| c.0.cmp(&tick)) {
                        Ok(idx) => changes[idx].1.push(entry),
                        Err(new_idx) => {
                            let new_vec = vec![entry];
                            changes.insert(new_idx, (tick, new_vec));
                        }
                    }
                }
            }

            {
                //TODO: Cache text, it renders very slow but it will have to do for now
                profile_scope!("Build Text");
                for c in changes {
                    if c.0 < min_tick_render {
                        continue;
                    } else if c.0 > max_tick_render {
                        break;
                    }
                    let (x, y) = self.screen.tick_to_pos(c.0);
                    let x = x + self.screen.track_width * 1.5;
                    let line_height = 12.0;

                    for (i, (text, color)) in c.1.iter().enumerate() {
                        painter.text(
                            pos2(x, y - i as f32 * line_height),
                            Align2::RIGHT_BOTTOM,
                            text,
                            FontId::monospace(12.0),
                            *color,
                        );
                    }
                }
            }
        }

        Ok(interact)
    }

    pub fn drag_start(&mut self, button: PointerButton, x: f32, y: f32, modifiers: &Modifiers) {
        if let PointerButton::Primary = button {
            let res = KSON_RESOLUTION;
            let lane = self.screen.pos_to_lane(x);
            let tick = self.screen.pos_to_tick(x, y);
            let tick = tick - (tick % (res / 2));
            let tick_f = self.screen.pos_to_tick_f(x, y);
            if let Some(ref mut cursor) = self.cursor_object {
                cursor.drag_start(
                    self.screen,
                    tick,
                    tick_f,
                    lane,
                    &self.chart,
                    &mut self.actions,
                    pos2(x, y),
                    modifiers,
                )
            }
        }
    }

    pub fn drag_end(&mut self, button: PointerButton, x: f32, y: f32) {
        if let PointerButton::Primary = button {
            let lane = self.screen.pos_to_lane(x);
            let tick = self.screen.pos_to_tick(x, y);
            let tick_f = self.screen.pos_to_tick_f(x, y);
            let tick = tick - (tick % (KSON_RESOLUTION / 2));
            if let Some(cursor) = &mut self.cursor_object {
                cursor.drag_end(
                    self.screen,
                    tick,
                    tick_f,
                    lane,
                    &self.chart,
                    &mut self.actions,
                    pos2(x, y),
                );
            }
        }
    }

    fn resize_event(&mut self, size: Rect) {
        self.screen.w = size.width();
        self.screen.h = size.height();
        self.screen.top = size.top();
        self.screen.top_margin = size.top() + 20.0;
        self.screen.left_margin = size.left();

        self.screen.tick_height =
            self.screen.chart_draw_height() / (KSON_RESOLUTION * self.screen.beats_per_col) as f32;
    }

    fn get_clicked_data(&self, pos: Pos2) -> (f32, u32, f64) {
        let lane = self.screen.pos_to_lane(pos.x);
        let tick = self.screen.pos_to_tick(pos.x, pos.y);
        let tick_f: f64 = self.screen.pos_to_tick_f(pos.x, pos.y);
        let tick = tick - (tick % (KSON_RESOLUTION / 2));

        (lane, tick, tick_f)
    }

    pub fn primary_clicked(&mut self, pos: Pos2) {
        self.mouse_x = pos.x;
        self.mouse_y = pos.y;
        let (lane, tick, tick_f) = self.get_clicked_data(pos);
        self.cursor_line = tick;

        if let Some(cursor) = &mut self.cursor_object {
            cursor.primary_click(
                self.screen,
                tick,
                tick_f,
                lane,
                &self.chart,
                &mut self.actions,
                pos2(pos.x, pos.y),
            );
        }
    }

    pub fn middle_clicked(&mut self, pos: Pos2) {
        self.mouse_x = pos.x;
        self.mouse_y = pos.y;
        let (lane, tick, tick_f) = self.get_clicked_data(pos);

        if let Some(cursor) = &mut self.cursor_object {
            cursor.middle_click(
                self.screen,
                tick,
                tick_f,
                lane,
                &self.chart,
                &mut self.actions,
                pos2(pos.x, pos.y),
            )
        }
    }

    pub fn mouse_motion_event(&mut self, pos: Pos2) {
        self.mouse_x = pos.x;
        self.mouse_y = pos.y;
        let (lane, tick, tick_f) = self.get_clicked_data(pos);

        if let Some(cursor) = &mut self.cursor_object {
            cursor.update(tick, tick_f, lane, pos2(pos.x, pos.y), &self.chart);
        }
    }

    pub fn mouse_wheel_event(&mut self, y: f32) {
        self.screen.x_offset_target += y.signum() * self.screen.track_width * 2.0;
        self.screen.x_offset_target = self.screen.x_offset_target.max(0.0);
    }

    pub(crate) fn context_menu(&mut self, ui: &mut Ui, pos: Pos2) {
        let (lane, tick, _tick_f) = self.get_clicked_data(pos);

        let index = if lane < 3.0 { 0 } else { 1 };

        let mut fx = self.chart.note.fx[index].iter();

        if let Some(fx) = fx.find(|x| x.contains(tick)) {
            let Some(effects) = self.chart.audio.audio_effect.as_ref() else {
                return;
            };
            let mut effect_keys: Vec<&String> = effects.fx.def.keys().collect();
            effect_keys.sort();

            for effect_key in effect_keys {
                let mut checked = effects
                    .fx
                    .long_event
                    .get(effect_key)
                    .map(|x| &x[index])
                    .is_some_and(|x| x.iter().any(|x| x.tick() == fx.y));

                if ui.checkbox(&mut checked, effect_key).changed() {
                    let effect_key = effect_key.clone();
                    let y = fx.y;
                    if checked {
                        self.actions.new_action(
                            fl!("insert_fx_effect", effect = effect_key.clone()),
                            move |c| {
                                let Some(effects) = c.audio.audio_effect.as_mut() else {
                                    bail!("No effects")
                                };

                                let events =
                                    effects.fx.long_event.entry(effect_key.clone()).or_default();

                                events[index].push(ByPulseOption::new(y, None));

                                Ok(())
                            },
                        )
                    } else {
                        self.actions.new_action(
                            fl!("remove_fx_effect", effect = effect_key.clone()),
                            move |c| {
                                let Some(effects) = c.audio.audio_effect.as_mut() else {
                                    bail!("No effects")
                                };

                                let Some(events) = effects.fx.long_event.get_mut(&effect_key)
                                else {
                                    bail!("No events")
                                };

                                events[index].retain(|v| v.tick() != y);

                                Ok(())
                            },
                        )
                    }
                };
            }
        } else {
            ui.close_menu();
        }
    }
}
#[allow(unused)]
fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

//https://github.com/m4saka/ksh2kson/issues/4#issuecomment-573343229
pub fn do_curve(x: f64, a: f64, b: f64) -> f64 {
    let t = if x < f64::EPSILON || a < f64::EPSILON {
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
            File::open(&path)?.read_to_string(&mut data)?;
            Ok(Some((kson::Chart::from_ksh(&data)?, path)))
        }
        "kson" => {
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            profile_scope!("kson parse");
            Ok(Some((serde_json::from_reader(reader)?, path)))
        }
        "vox" => {
            let mut data = String::from("");
            File::open(&path)?.read_to_string(&mut data)?;
            Ok(Some((kson::Chart::from_vox(&data)?, path)))
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
            let mut path = PathBuf::from(&file_path);
            path.set_extension("kson");
            let mut file = File::create(&path)?;
            profile_scope!("Write kson");
            file.write_all(serde_json::to_string(&chart)?.as_bytes())?;
            Ok(Some(path))
        }
        _ => Ok(None),
    }
}
