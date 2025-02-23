use std::{collections::HashMap, time::SystemTime};

use egui::accesskit::{Point, Rect, Vec2};
use winit::{dpi::PhysicalPosition, event::TouchPhase};

use crate::button_codes::{UscButton, UscInputEvent};

#[derive(Debug)]
pub struct TouchHelper {
    screen_size: Vec2,
    button_areas: HashMap<UscButton, Rect>,
    held_buttons: HashMap<u64, UscButton>,
    tracked: HashMap<u64, TouchTracker>,
}
#[derive(Debug)]
struct TouchTracker {
    start_time: SystemTime,
    start_pos: PhysicalPosition<f64>,
    current_pos: PhysicalPosition<f64>,
}

impl TouchTracker {
    fn current_point(&self) -> Point {
        Point::new(self.current_pos.x, self.current_pos.y)
    }

    fn new(pos: PhysicalPosition<f64>) -> Self {
        Self {
            start_time: SystemTime::now(),
            start_pos: pos,
            current_pos: pos,
        }
    }

    fn update(&mut self, pos: PhysicalPosition<f64>) {
        self.current_pos = pos;
    }
}

impl TouchHelper {
    pub fn update(
        &mut self,
        ev: &winit::event::Touch,
    ) -> Option<(UscInputEvent, Option<UscInputEvent>)> {
        if matches!(ev.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
            self.tracked.remove(&ev.id);
            let was_held = self.held_buttons.remove(&ev.id)?;
            if self.held_buttons.iter().any(|x| *x.1 == was_held) {
                None //Button is still held
            } else {
                // Button was released
                Some((
                    UscInputEvent::Button(
                        was_held,
                        winit::event::ElementState::Released,
                        SystemTime::now(),
                    ),
                    None,
                ))
            }
        } else {
            let updated = self
                .tracked
                .entry(ev.id)
                .and_modify(|x| x.update(ev.location))
                .or_insert(TouchTracker::new(ev.location));

            let new_button = self
                .button_areas
                .iter()
                .find(|x| x.1.contains(updated.current_point()))?
                .0
                .clone();

            let is_held_by_other = self
                .held_buttons
                .iter()
                .any(|x| *x.0 != ev.id && *x.1 == new_button);

            let previous_held = self.held_buttons.insert(ev.id, new_button);

            let optional_event = previous_held
                .filter(|x| *x != new_button && !self.held_buttons.values().any(|v| *v == *x))
                .map(|x| {
                    UscInputEvent::Button(
                        x,
                        winit::event::ElementState::Released,
                        SystemTime::now(),
                    )
                });

            if is_held_by_other {
                Some((optional_event?, None))
            } else {
                Some((
                    UscInputEvent::Button(
                        new_button,
                        winit::event::ElementState::Pressed,
                        SystemTime::now(),
                    ),
                    optional_event,
                ))
            }
        }
    }
    pub fn new(screen_size: Vec2) -> Self {
        /*
           -----------------
           |  |  back?  |  |
           |  |         |  |
           |LL|---------|RL|
           |  |  start  |  |
           |  |         |  |
           |--|---------|--|
           |  |a  b c  d|  |
           |LR|         |RR|
           |  |---------|  |
           |  | fx | fx |  |
           |  |  L |  R |  |
           -----------------
        */

        let col_width = screen_size.x / 6.0;
        let row_height = screen_size.y / 4.0;

        let mut button_areas: HashMap<UscButton, Rect> = HashMap::new();

        button_areas.insert(
            UscButton::Laser(kson::Side::Left, kson::Side::Left),
            Rect::new(0.0, 0.0, col_width, row_height * 2.0),
        );

        button_areas.insert(
            UscButton::Laser(kson::Side::Left, kson::Side::Right),
            Rect::new(0.0, row_height * 2.0, col_width, row_height * 4.0),
        );

        button_areas.insert(
            UscButton::Laser(kson::Side::Right, kson::Side::Left),
            Rect::new(col_width * 5.0, 0.0, col_width * 6.0, row_height * 2.0),
        );

        button_areas.insert(
            UscButton::Laser(kson::Side::Right, kson::Side::Right),
            Rect::new(
                col_width * 5.0,
                row_height * 2.0,
                col_width * 6.0,
                row_height * 4.0,
            ),
        );

        button_areas.insert(
            UscButton::Back,
            Rect::new(col_width, 0.0, col_width * 5.0, row_height),
        );

        button_areas.insert(
            UscButton::Start,
            Rect::new(col_width, row_height, col_width * 4.0, row_height * 2.0),
        );

        for i in 0..4usize {
            button_areas.insert(
                UscButton::BT(i.try_into().unwrap()),
                Rect::new(
                    col_width + col_width * i as f64,
                    row_height * 2.0,
                    col_width * 2.0 + col_width * i as f64,
                    row_height * 3.0,
                ),
            );
        }

        button_areas.insert(
            UscButton::FX(kson::Side::Left),
            Rect::new(
                col_width,
                row_height * 3.0,
                col_width * 3.0,
                row_height * 4.0,
            ),
        );
        button_areas.insert(
            UscButton::FX(kson::Side::Right),
            Rect::new(
                col_width * 3.0,
                row_height * 3.0,
                col_width * 5.0,
                row_height * 4.0,
            ),
        );

        Self {
            screen_size,
            button_areas,
            held_buttons: HashMap::new(),
            tracked: HashMap::new(),
        }
    }
}
