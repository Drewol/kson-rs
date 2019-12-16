#![windows_subsystem = "windows"]

extern crate ggez;
extern crate imgui;
extern crate math;
extern crate nfd;
extern crate serde_json;

mod gui;
use crate::gui::{ChartTool, GuiEvent, ImGuiWrapper};
use ggez::event::{self, EventHandler, KeyCode, KeyMods, MouseButton};
use ggez::graphics;
use ggez::nalgebra as na;
use ggez::{Context, GameResult};
use math::round;
use nfd::Response;
mod chart;
use std::fs::File;
use std::io::prelude::*;
use std::{thread, time};

trait CursorObject {
    fn mouse_down(&mut self, tick: u32, lane: f32, chart: &mut chart::Chart);
    fn mouse_up(&mut self, tick: u32, lane: f32, chart: &mut chart::Chart);
    fn update(&mut self, tick: u32, lane: f32);
    fn draw(&self, state: &MainState, ctx: &mut Context);
}

//structs for cursor objects
struct ButtonInterval {
    pressed: bool,
    fx: bool,
    interval: chart::Interval,
    lane: usize,
}

impl ButtonInterval {
    fn new(fx: bool) -> Self {
        ButtonInterval {
            pressed: false,
            fx: fx,
            interval: chart::Interval { y: 0, l: 0 },
            lane: 0,
        }
    }
}

impl CursorObject for ButtonInterval {
    fn mouse_down(&mut self, tick: u32, lane: f32, chart: &mut chart::Chart) {
        self.pressed = true;
        if self.fx {
            self.lane = if lane < 3.0 { 0 } else { 1 };
        } else {
            self.lane = (lane as usize).max(1).min(4) - 1;
        }
        self.interval.y = tick;
    }

    fn mouse_up(&mut self, tick: u32, lane: f32, chart: &mut chart::Chart) {
        if self.interval.y > tick {
            self.interval.l = 0;
        } else {
            self.interval.l = (tick - self.interval.y);
        }
        let v = std::mem::replace(&mut self.interval, chart::Interval { y: 0, l: 0 });
        if self.fx {
            chart.note.fx[self.lane].push(v);
            chart.note.fx[self.lane].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        } else {
            chart.note.bt[self.lane].push(v);
            chart.note.bt[self.lane].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        }
        self.pressed = false;
        self.lane = 0;
    }

    fn update(&mut self, tick: u32, lane: f32) {
        if !self.pressed {
            self.interval.y = tick;
            if self.fx {
                self.lane = if lane < 3.0 { 0 } else { 1 };
            } else {
                self.lane = (lane as usize).max(1).min(4) - 1;
            }
        }
        if self.interval.y > tick {
            self.interval.l = 0;
        } else {
            self.interval.l = (tick - self.interval.y);
        }
    }

    fn draw(&self, state: &MainState, ctx: &mut Context) {
        graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha);
        let color = if self.fx {
            graphics::Color {
                r: 1.0,
                g: 0.3,
                b: 0.0,
                a: 0.5,
            }
        } else {
            graphics::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.5,
            }
        };
        if self.interval.l == 0 {
            let (x, y) = state.tick_to_pos(self.interval.y);

            let x = if self.fx {
                x + self.lane as f32 * state.lane_width() * 2.0
                    + 2.0 * self.lane as f32
                    + state.lane_width()
                    + state.track_width / 2.0
            } else {
                x + self.lane as f32 * state.lane_width()
                    + 1.0 * self.lane as f32
                    + state.lane_width()
                    + state.track_width / 2.0
            };
            let y = y as f32;

            let w = if self.fx {
                state.track_width as f32 / 3.0 - 1.0
            } else {
                state.track_width as f32 / 6.0 - 2.0
            };
            let h = -2.0;

            let m = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                [x, y, w, h].into(),
                color,
            )
            .unwrap();
            graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),));
        } else {
            let mut long_bt_builder = graphics::MeshBuilder::new();
            for (x, y, h, _) in state.interval_to_ranges(&self.interval) {
                let x = if self.fx {
                    x + self.lane as f32 * state.lane_width() * 2.0
                        + 2.0 * self.lane as f32
                        + state.lane_width()
                        + state.track_width / 2.0
                } else {
                    x + self.lane as f32 * state.lane_width()
                        + 1.0 * self.lane as f32
                        + state.lane_width()
                        + state.track_width / 2.0
                };

                let w = if self.fx {
                    state.track_width as f32 / 3.0 - 1.0
                } else {
                    state.track_width as f32 / 6.0 - 2.0
                };

                long_bt_builder.rectangle(graphics::DrawMode::fill(), [x, y, w, h].into(), color);
            }
            let m = long_bt_builder.build(ctx).unwrap();
            graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),));
        }
    }
}

struct MainState {
    redraw: bool,
    chart: chart::Chart,
    w: f32,
    h: f32,
    tick_height: f32,
    track_width: f32,
    imgui_wrapper: ImGuiWrapper,
    save_path: Option<String>,
    top_margin: f32,
    bottom_margin: f32,
    beats_per_col: u32,
    mouse_x: f32,
    mouse_y: f32,
    x_offset: f32,
    x_offset_target: f32,
    cursor_object: Option<Box<dyn CursorObject>>,
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
            top_margin: 60.0,
            bottom_margin: 10.0,
            beats_per_col: 16,
            mouse_x: 0.0,
            mouse_y: 0.0,
            x_offset: 0.0,
            x_offset_target: 0.0,
            cursor_object: None,
        };
        Ok(s)
    }

    fn lane_width(&self) -> f32 {
        self.track_width / 6.0
    }

    fn ticks_per_col(&self) -> u32 {
        self.beats_per_col * self.chart.beat.resolution
    }

    fn tick_to_pos(&self, in_y: u32) -> (f32, f32) {
        let h = self.chart_draw_height();
        let x = (in_y / self.ticks_per_col()) as f32 * self.track_width * 2.0;
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
        let x = math::round::floor(x as f64 / (self.track_width * 2.0) as f64, 0);
        math::round::floor(
            (y as f64 + x) * self.beats_per_col as f64 * self.chart.beat.resolution as f64,
            0,
        )
        .max(0.0) as u32
    }

    fn pos_to_lane(&self, in_x: f32) -> f32 {
        let mut x = (in_x + self.x_offset) % (self.track_width as f32 * 2.0);
        x = ((x - self.track_width as f32 / 2.0).max(0.0) / self.track_width as f32).min(1.0);
        (x * 6.0).min(5.0) as f32
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
            let prog_s = (s - in_interval.y) as f32 / in_interval.l as f32;
            let prog_e = (e - in_interval.y) as f32 / in_interval.l as f32;
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
                        GuiEvent::Exit => _ctx.continuing = false,
                        GuiEvent::ToolChanged(new_tool) => match new_tool {
                            ChartTool::BT => {
                                self.cursor_object = Some(Box::new(ButtonInterval::new(false)))
                            }
                            ChartTool::FX => {
                                self.cursor_object = Some(Box::new(ButtonInterval::new(true)))
                            }
                            _ => self.cursor_object = None,
                        },
                        _ => (),
                    },
                    None => break,
                }
            }
        }

        let deltaTime = (10.0 * ggez::timer::delta(_ctx).as_secs_f32()).min(1.0);
        self.x_offset = self.x_offset + (self.x_offset_target - self.x_offset) * deltaTime;
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        //draw chart
        {
            graphics::clear(ctx, graphics::BLACK);
            let chart_draw_height = self.chart_draw_height();
            //draw track
            let track = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                graphics::Rect {
                    x: 0.0,
                    y: self.top_margin,
                    w: self.track_width as f32,
                    h: chart_draw_height,
                },
                [0.2, 0.2, 0.2, 1.0].into(),
            )?;

            let track_count = 2 + (0.5 * self.w / self.track_width as f32) as u32;
            for i in 0..track_count {
                graphics::draw(
                    ctx,
                    &track,
                    (na::Point2::new(
                        (self.track_width / 2.0 + i as f32 * self.track_width * 2.0) as f32
                            - (self.x_offset % (self.track_width * 2.0)),
                        0.0,
                    ),),
                )?;
            }

            //draw notes
            let bt_builder = &mut graphics::MeshBuilder::new();
            let long_bt_builder = &mut graphics::MeshBuilder::new();
            let fx_builder = &mut graphics::MeshBuilder::new();
            let long_fx_builder = &mut graphics::MeshBuilder::new();
            let ll_builder = &mut graphics::MeshBuilder::new();
            let rl_builder = &mut graphics::MeshBuilder::new();
            let laser_builder = &mut graphics::MeshBuilder::new();
            let laser_color: [graphics::Color; 2] = [
                graphics::Color::from_rgba(0, 115, 144, 255),
                graphics::Color::from_rgba(194, 6, 140, 255),
            ];
            let slam_height = 6.0 as f32;
            let min_tick_render = self.pos_to_tick(-100.0, self.h);
            let max_tick_render = self.pos_to_tick(self.w + 50.0, 0.0);
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

            //fx
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

            //laser
            for i in 0..2 {
                for section in &self.chart.note.laser[i] {
                    let y_base = section.y;
                    if section.v.last().unwrap().ry + y_base < min_tick_render {
                        continue;
                    }
                    if y_base > max_tick_render {
                        break;
                    }

                    let wide = section.wide == 2;
                    for se in section.v.windows(2) {
                        let s = &se[0];
                        let e = &se[1];
                        let interval = chart::Interval {
                            y: s.ry + y_base,
                            l: e.ry - s.ry,
                        };
                        let mut start_value = s.v as f32;
                        let mut syoff = 0.0 as f32;

                        match s.vf {
                            Some(value) => {
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
                                let sx = x
                                    + sv * (self.track_width - self.lane_width())
                                    + (self.track_width / 2.0)
                                    + self.lane_width() / 2.0;
                                let ex = x
                                    + ev * (self.track_width - self.lane_width())
                                    + (self.track_width / 2.0)
                                    + self.lane_width() / 2.0;

                                let mut w: f32 = 0.0;
                                let mut x: f32 = 0.0;
                                if sx > ex {
                                    x = sx + self.lane_width() / 2.0;
                                    w = (ex - self.lane_width() / 2.0) - x;
                                } else {
                                    x = sx - self.lane_width() / 2.0;
                                    w = (ex + self.lane_width() / 2.0) - x;
                                }
                                laser_builder.rectangle(
                                    graphics::DrawMode::fill(),
                                    [x, y, w, -slam_height].into(),
                                    laser_color[i],
                                );
                            }
                            _ => (),
                        };
                        let mut value_width = (e.v as f32 - start_value) as f32;
                        if wide {
                            value_width = value_width * 2.0;
                            start_value = start_value * 2.0 - 0.5;
                        }

                        for (x, y, h, (sv, ev)) in self.interval_to_ranges(&interval) {
                            let sx = x
                                + (start_value + (sv * value_width))
                                    * (self.track_width - self.lane_width())
                                + (self.track_width / 2.0)
                                + self.lane_width() / 2.0;
                            let ex = x
                                + (start_value + (ev * value_width))
                                    * (self.track_width - self.lane_width())
                                + (self.track_width / 2.0)
                                + self.lane_width() / 2.0;

                            let sy = y;
                            let ey = y + h;

                            let xoff = self.lane_width() / 2.0;
                            let points = [
                                na::Point2 {
                                    coords: [sx - xoff, sy - syoff].into(),
                                },
                                na::Point2 {
                                    coords: [sx + xoff, sy - syoff].into(),
                                },
                                na::Point2 {
                                    coords: [ex + xoff, ey].into(),
                                },
                                na::Point2 {
                                    coords: [ex - xoff, ey].into(),
                                },
                            ];

                            laser_builder.polygon(
                                graphics::DrawMode::fill(),
                                &points,
                                laser_color[i],
                            )?;
                        }
                    }

                    let last = section.v.last();
                    match last {
                        Some(l) => {
                            match l.vf {
                                Some(vf) => {
                                    //draw slam
                                    let mut sv: f32 = l.v as f32;
                                    let mut ev: f32 = vf as f32;
                                    if wide {
                                        sv = sv * 2.0 - 0.5;
                                        ev = ev * 2.0 - 0.5;
                                    }

                                    let (x, y) = self.tick_to_pos(l.ry + y_base);
                                    let sx = x
                                        + sv * (self.track_width - self.lane_width())
                                        + (self.track_width / 2.0)
                                        + self.lane_width() / 2.0;
                                    let ex = x
                                        + ev as f32 * (self.track_width - self.lane_width())
                                        + (self.track_width / 2.0)
                                        + self.lane_width() / 2.0;

                                    let mut w: f32 = 0.0;
                                    let mut x: f32 = 0.0;
                                    if sx > ex {
                                        x = sx + self.lane_width() / 2.0;
                                        w = (ex - self.lane_width() / 2.0) - x;
                                    } else {
                                        x = sx - self.lane_width() / 2.0;
                                        w = (ex + self.lane_width() / 2.0) - x;
                                    }
                                    laser_builder.rectangle(
                                        graphics::DrawMode::fill(),
                                        [x, y, w, -slam_height].into(),
                                        laser_color[i],
                                    );
                                }
                                None => (),
                            }
                        }
                        None => (),
                    };
                }
            }

            {
                graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha)?;
                //draw built meshes
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
                Some(ref cursor) => cursor.draw(self, ctx),
                None => (),
            }
        }

        // Draw ui
        {
            self.imgui_wrapper.render(ctx, 1.0);
        }
        graphics::present(ctx)?;
        self.redraw = false;
        ggez::timer::yield_now();
        Ok(())
    }
    fn mouse_button_down_event(&mut self, ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        //update imgui
        self.imgui_wrapper.update_mouse_down((
            button == MouseButton::Left,
            button == MouseButton::Right,
            button == MouseButton::Middle,
        ));

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

    fn mouse_button_up_event(&mut self, ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        self.imgui_wrapper.update_mouse_down((false, false, false));

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
        );
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
            KeyCode::P => {
                self.imgui_wrapper.open_popup();
            }
            KeyCode::Home => self.x_offset_target = 0.0,
            KeyCode::PageUp => {
                self.x_offset_target =
                    self.x_offset_target + (self.w - (self.w % (self.track_width * 2.0)))
            }
            KeyCode::PageDown => {
                self.x_offset_target =
                    (self.x_offset_target - (self.w - (self.w % (self.track_width * 2.0)))).max(0.0)
            }
            KeyCode::End => {
                let mut target: f32 = 0.0;

                //check pos of last bt
                for i in 0..4 {
                    let last = self.chart.note.bt[i].last();
                    match last {
                        Some(note) => {
                            target = target.max(self.tick_to_pos(note.y + note.l).0 + self.x_offset)
                        }
                        None => (),
                    }
                }

                //check pos of last fx
                for i in 0..2 {
                    let last = self.chart.note.fx[i].last();
                    match last {
                        Some(note) => {
                            target = target.max(self.tick_to_pos(note.y + note.l).0 + self.x_offset)
                        }
                        None => (),
                    }
                }

                //check pos of last lasers
                for i in 0..2 {
                    let last_section = self.chart.note.laser[i].last();
                    match last_section {
                        Some(section) => {
                            let last_segment = section.v.last();
                            match last_segment {
                                Some(segment) => {
                                    target = target.max(
                                        self.tick_to_pos(segment.ry + section.y).0 + self.x_offset,
                                    )
                                }
                                None => (),
                            }
                        }
                        None => (),
                    }
                }

                self.x_offset_target = target - (target % (self.track_width * 2.0))
            }
            _ => (),
        }
    }

    fn mouse_motion_event(&mut self, _ctx: &mut Context, x: f32, y: f32, _dx: f32, _dy: f32) {
        self.imgui_wrapper.update_mouse_pos(x, y);
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

    fn mouse_wheel_event(&mut self, _ctx: &mut Context, x: f32, y: f32) {
        self.x_offset_target = self.x_offset_target + y * self.track_width;
        self.x_offset_target = self.x_offset_target.max(0.0);
    }
}

fn open_chart() -> Option<(chart::Chart, String)> {
    let chart: chart::Chart;
    let path: String;
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
    let path: String;
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
        samples: ggez::conf::NumSamples::Four,
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

    let modules = ggez::conf::ModuleConf {
        gamepad: false,
        audio: false,
    };

    let cb = ggez::ContextBuilder::new("usc-editor", "Drewol")
        .window_setup(win_setup)
        .window_mode(mode)
        .modules(modules);

    let (ctx, event_loop) = &mut cb.build()?;

    let state = &mut MainState::new(ctx)?;

    event::run(ctx, event_loop, state)
}
