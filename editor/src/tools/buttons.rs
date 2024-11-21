use crate::i18n;
use crate::tools::CursorObject;
use crate::Modifiers;
use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
    rect_xy_wh,
};
use anyhow::Result;
use eframe::egui::{Painter, Pos2, Rgba, Shape};
use kson::overlaps::Overlaps;
use kson::{Chart, Interval};

//structs for cursor objects
pub struct ButtonInterval {
    pressed: bool,
    fx: bool,
    interval: Interval,
    lane: usize,
}

impl ButtonInterval {
    pub const fn new(fx: bool) -> Self {
        ButtonInterval {
            pressed: false,
            fx,
            interval: Interval { y: 0, l: 0 },
            lane: 0,
        }
    }
}

impl CursorObject for ButtonInterval {
    fn drag_start(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
        _modifiers: &Modifiers,
    ) {
        self.pressed = true;
        if self.fx {
            self.lane = if lane < 3.0 { 0 } else { 1 };
        } else {
            self.lane = (lane as usize).clamp(1, 4) - 1;
        }
        self.interval.y = tick;
    }

    fn middle_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        if self.pressed {
            return;
        }

        let lane = if self.fx {
            if lane < 3.0 {
                0
            } else {
                1
            }
        } else {
            (lane as usize).clamp(1, 4) - 1
        };

        //hit test
        let lane_data = if self.fx {
            &chart.note.fx[lane]
        } else {
            &chart.note.bt[lane]
        };

        let index = lane_data
            .iter()
            .enumerate()
            .find(|(_, n)| n.contains(tick))
            .map(|(i, _)| i);

        if let Some(index) = index {
            // remove found index
            let fx = self.fx;
            actions.new_action(
                i18n::fl!("remove_note", lane = if fx { "FX" } else { "BT" }),
                move |chart: &mut Chart| {
                    if fx {
                        chart.note.fx[lane].remove(index);
                    } else {
                        chart.note.bt[lane].remove(index);
                    }

                    Ok(())
                },
            );
        }
    }

    fn drag_end(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        if !self.pressed {
            return;
        }

        if self.interval.y >= tick {
            self.interval.l = 0;
        } else {
            self.interval.l = tick - self.interval.y;
        }
        let v = std::mem::replace(&mut self.interval, Interval { y: 0, l: 0 });
        if self.fx {
            let l = self.lane;

            actions.new_action(
                i18n::fl!(
                    "add_fx",
                    side = if self.lane == 0 {
                        i18n::fl!("left")
                    } else {
                        i18n::fl!("right")
                    }
                ),
                move |edit_chart: &mut Chart| {
                    edit_chart.note.fx[l].push(v);
                    edit_chart.note.fx[l].sort_by(|a, b| a.y.cmp(&b.y));
                    Ok(())
                },
            );
        } else {
            let l = self.lane;

            actions.new_action(
                i18n::fl!(
                    "add_bt",
                    lane = std::char::from_u32('A' as u32 + self.lane as u32)
                        .unwrap_or_default()
                        .to_string()
                ),
                move |edit_chart: &mut Chart| {
                    edit_chart.note.bt[l].push(v);
                    edit_chart.note.bt[l].sort_by(|a, b| a.y.cmp(&b.y));
                    Ok(())
                },
            );
        }
        self.pressed = false;
        self.lane = 0;
    }

    fn update(&mut self, tick: u32, _tick_f: f64, lane: f32, _pos: Pos2, _chart: &Chart) {
        if !self.pressed {
            self.interval.y = tick;
            if self.fx {
                self.lane = if lane < 3.0 { 0 } else { 1 };
            } else {
                self.lane = (lane as usize).clamp(1, 4) - 1;
            }
        }
        if self.interval.y >= tick {
            self.interval.l = 0;
        } else {
            self.interval.l = tick - self.interval.y;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        let color = if self.fx {
            Rgba::from_rgba_premultiplied(1.0, 0.3, 0.0, 0.5)
        } else {
            Rgba::from_rgba_premultiplied(1.0, 1.0, 1.0, 0.5)
        };
        if self.interval.l == 0 {
            let (x, y) = state.screen.tick_to_pos(self.interval.y);

            let x = if self.fx {
                2.0f32.mul_add(
                    self.lane as f32,
                    (self.lane as f32 * state.screen.lane_width()).mul_add(2.0, x),
                ) + state.screen.lane_width()
                    + state.screen.track_width / 2.0
            } else {
                1.0f32.mul_add(
                    self.lane as f32,
                    (self.lane as f32).mul_add(state.screen.lane_width(), x),
                ) + state.screen.lane_width()
                    + state.screen.track_width / 2.0
            };

            let w = if self.fx {
                state.screen.track_width / 3.0 - 1.0
            } else {
                state.screen.track_width / 6.0 - 2.0
            };
            let h = -2.0;

            painter.rect_filled(rect_xy_wh([x, y, w, h]), 0.0, color);
            Ok(())
        } else {
            let mut long_bt_builder = Vec::<Shape>::new();
            for (x, y, h, _) in state.screen.interval_to_ranges(&self.interval) {
                let x = if self.fx {
                    2.0f32.mul_add(
                        self.lane as f32,
                        (self.lane as f32 * state.screen.lane_width()).mul_add(2.0, x),
                    ) + state.screen.lane_width()
                        + state.screen.track_width / 2.0
                } else {
                    1.0f32.mul_add(
                        self.lane as f32,
                        (self.lane as f32).mul_add(state.screen.lane_width(), x),
                    ) + state.screen.lane_width()
                        + state.screen.track_width / 2.0
                };

                let w = if self.fx {
                    state.screen.track_width / 3.0 - 1.0
                } else {
                    state.screen.track_width / 6.0 - 2.0
                };

                long_bt_builder.push(Shape::rect_filled(rect_xy_wh([x, y, w, h]), 0.0, color));
            }

            painter.extend(long_bt_builder);
            Ok(())
        }
    }
}
