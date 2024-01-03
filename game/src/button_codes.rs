use std::{
    borrow::BorrowMut,
    collections::HashMap,
    ops::{AddAssign, SubAssign},
    sync::mpsc::{channel, Receiver, Sender},
    time::{Duration, SystemTime},
};

use game_loop::winit::event::ElementState;
use gilrs::{
    ev::{filter::FilterFn, Code},
    Axis, Button, Event,
};
use kson::{BtLane, Side};

pub struct RuscFilter {
    button_map: HashMap<u32, Button>,
    axis_map: HashMap<u32, (Axis, f32)>,
    offset: Duration,
    offset_neg: bool,
    offset_rx: Receiver<i32>,
}

pub struct CustomBindingFilter;

impl RuscFilter {
    pub fn new(offset: i32) -> (Self, Sender<i32>) {
        let (offset_tx, offset_rx) = channel();
        (
            Self {
                button_map: HashMap::from([
                    (0, Button::Start),
                    (1, Button::South),
                    (2, Button::East),
                    (3, Button::North),
                    (4, Button::West),
                    (5, Button::LeftTrigger),
                    (6, Button::RightTrigger),
                ]),
                axis_map: HashMap::from([
                    //Axis to_u32 are marked with a 1 in the 2^16 bit
                    (1 << 16, (Axis::LeftStickX, 1.0)),
                    (1 << 16 | 1, (Axis::RightStickX, -1.0)),
                ]),
                offset: Duration::from_millis(offset.unsigned_abs() as _),
                offset_neg: offset < 0,
                offset_rx,
            },
            offset_tx,
        )
    }

    pub fn update(&mut self) {
        while let Ok(new_offset) = self.offset_rx.try_recv() {
            self.offset = Duration::from_millis(new_offset.unsigned_abs() as _);
            self.offset_neg = new_offset < 0;
        }
    }
}

impl FilterFn for RuscFilter {
    fn filter(&self, ev: Option<gilrs::Event>, gilrs: &mut gilrs::Gilrs) -> Option<gilrs::Event> {
        match ev {
            Some(mut ev) => {
                let source = gilrs.gamepad(ev.id).mapping_source();
                if self.offset_neg {
                    ev.time.add_assign(self.offset);
                } else {
                    ev.time.sub_assign(self.offset);
                }
                match source {
                    gilrs::MappingSource::SdlMappings => Some(ev),
                    _ => {
                        // apply default mapping
                        // a:b1,b:b2,x:b4,y:b3,start:b0,leftshoulder:b5,rightshoulder:b6,leftx:a0,rightx:a1

                        match ev.event {
                            gilrs::EventType::ButtonPressed(_, code) => {
                                self.button_map.get(&code.into_u32()).map(|b| Event {
                                    id: ev.id,
                                    event: gilrs::EventType::ButtonPressed(*b, code),
                                    time: ev.time,
                                })
                            }
                            gilrs::EventType::ButtonRepeated(_, code) => {
                                self.button_map.get(&code.into_u32()).map(|b| Event {
                                    id: ev.id,
                                    event: gilrs::EventType::ButtonRepeated(*b, code),
                                    time: ev.time,
                                })
                            }
                            gilrs::EventType::ButtonReleased(_, code) => {
                                self.button_map.get(&code.into_u32()).map(|b| Event {
                                    id: ev.id,
                                    event: gilrs::EventType::ButtonReleased(*b, code),
                                    time: ev.time,
                                })
                            }
                            gilrs::EventType::ButtonChanged(_, v, code) => {
                                self.button_map.get(&code.into_u32()).map(|b| Event {
                                    id: ev.id,
                                    event: gilrs::EventType::ButtonChanged(*b, v, code),
                                    time: ev.time,
                                })
                            }
                            gilrs::EventType::AxisChanged(_, v, code) => self
                                .axis_map
                                .get(&code.into_u32())
                                .map(|(axis, sens)| Event {
                                    id: ev.id,
                                    event: gilrs::EventType::AxisChanged(*axis, *sens * v, code),
                                    time: ev.time,
                                }),
                            gilrs::EventType::Connected => Some(ev),
                            gilrs::EventType::Disconnected => Some(ev),
                            gilrs::EventType::Dropped => Some(ev),
                        }
                        .or(Some(ev))
                    }
                }
            }
            _ => ev,
        }
    }
}

use serde::{Deserialize, Serialize};

impl FilterFn for CustomBindingFilter {
    fn filter(&self, ev: Option<Event>, gilrs: &mut gilrs::Gilrs) -> Option<Event> {
        match ev {
            Some(mut ev) => {
                let uuid = uuid::Uuid::from_bytes(gilrs.gamepad(ev.id).uuid());
                if let Some(bindings) = GameConfig::get().controller_binds.get(&uuid) {
                    match &mut ev.event {
                        gilrs::EventType::ButtonPressed(button, code)
                        | gilrs::EventType::ButtonRepeated(button, code)
                        | gilrs::EventType::ButtonReleased(button, code)
                        | gilrs::EventType::ButtonChanged(button, _, code) => {
                            *button = bindings
                                .buttons
                                .iter()
                                .find(|x| *x.1 == *code)
                                .map(|x| *x.0)
                                .unwrap_or(*button);
                        }
                        gilrs::EventType::AxisChanged(axis, _, code) => {
                            *axis = bindings
                                .axis
                                .iter()
                                .find(|x| *x.1 == *code)
                                .map(|x| *x.0)
                                .unwrap_or(*axis)
                        }
                        gilrs::EventType::Connected => {}
                        gilrs::EventType::Disconnected => {}
                        gilrs::EventType::Dropped => {}
                    }
                }
                Some(ev)
            }
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum UscButton {
    BT(BtLane),
    FX(Side),
    Start,
    Back,
    /// (Side, Direction)
    Laser(Side, Side),
    Other(Button),
}

impl UscButton {
    pub fn to_gilrs_code_u32(self) -> u32 {
        match self {
            UscButton::BT(bt) => match bt {
                BtLane::A => 1,
                BtLane::B => 2,
                BtLane::C => 3,
                BtLane::D => 4,
            },
            UscButton::FX(side) => match side {
                Side::Left => 5,
                Side::Right => 6,
            },
            UscButton::Start => 0,
            UscButton::Back => 255,
            UscButton::Laser(_, _) => 255,
            UscButton::Other(_) => 255,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            UscButton::BT(bt) => match bt {
                BtLane::A => "BT A",
                BtLane::B => "BT B",
                BtLane::C => "BT C",
                BtLane::D => "BT D",
            },
            UscButton::FX(side) => match side {
                Side::Left => "FX L",
                Side::Right => "FX R",
            },
            UscButton::Start => "Start",
            UscButton::Back => "Back",
            UscButton::Laser(side, dir) => match (side, dir) {
                (Side::Left, Side::Left) => "<--Left Laser",
                (Side::Left, Side::Right) => "Left Laser-->",
                (Side::Right, Side::Left) => "<--Right Laser",
                (Side::Right, Side::Right) => "Right Laser-->",
            },
            UscButton::Other(_) => "Unknown",
        }
    }
}

impl From<u8> for UscButton {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::BT(BtLane::A),
            1 => Self::BT(BtLane::B),
            2 => Self::BT(BtLane::C),
            3 => Self::BT(BtLane::D),
            4 => Self::FX(Side::Left),
            5 => Self::FX(Side::Right),
            6 => Self::Start,
            7 => Self::Back,
            _ => Self::Other(Button::Unknown),
        }
    }
}

impl From<UscButton> for u8 {
    fn from(val: UscButton) -> Self {
        match val {
            UscButton::BT(bt) => match bt {
                BtLane::A => 0,
                BtLane::B => 1,
                BtLane::C => 2,
                BtLane::D => 3,
            },
            UscButton::FX(side) => match side {
                Side::Left => 4,
                Side::Right => 5,
            },
            UscButton::Start => 6,
            UscButton::Back => 7,
            UscButton::Other(_) => 255,
            UscButton::Laser(_, _) => 255,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UscInputEvent {
    Laser(LaserState, SystemTime),
    Button(UscButton, ElementState, SystemTime),
}

impl From<Button> for UscButton {
    fn from(c: Button) -> Self {
        match c {
            Button::South => UscButton::BT(BtLane::A),
            Button::East => UscButton::BT(BtLane::B),
            Button::North => UscButton::BT(BtLane::C),
            Button::West => UscButton::BT(BtLane::D),
            Button::LeftTrigger => UscButton::FX(Side::Left), //Shoulder button
            Button::RightTrigger => UscButton::FX(Side::Right),
            Button::Select => UscButton::Back,
            Button::Start => UscButton::Start,
            other => UscButton::Other(other),
        }
    }
}

impl From<UscButton> for Button {
    fn from(val: UscButton) -> Self {
        match val {
            UscButton::BT(bt) => match bt {
                BtLane::A => Button::South,
                BtLane::B => Button::East,
                BtLane::C => Button::North,
                BtLane::D => Button::West,
            },
            UscButton::FX(side) => match side {
                Side::Left => Button::LeftTrigger,
                Side::Right => Button::RightTrigger,
            },
            UscButton::Start => Button::Start,
            UscButton::Back => Button::Select,
            UscButton::Laser(_, _) => Button::Unknown,
            UscButton::Other(c) => c,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaserAxis {
    pub delta: f32,
    pub pos: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum LaserSideAxis {
    Left(LaserAxis),
    Right(LaserAxis),
}

impl From<LaserSideAxis> for LaserAxis {
    fn from(val: LaserSideAxis) -> Self {
        match val {
            LaserSideAxis::Left(l) => l,
            LaserSideAxis::Right(l) => l,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaserState(LaserAxis, LaserAxis);

impl LaserState {
    pub fn get(&self, side: Side) -> LaserSideAxis {
        match side {
            Side::Left => LaserSideAxis::Left(self.0),
            Side::Right => LaserSideAxis::Right(self.1),
        }
    }

    pub fn get_axis(&self, side: Side) -> LaserAxis {
        match side {
            Side::Left => self.0,
            Side::Right => self.1,
        }
    }

    pub fn update(&mut self, side: Side, new_pos: f32) {
        let new_pos_pi = new_pos * std::f32::consts::PI;

        let state = match side {
            Side::Left => self.0.borrow_mut(),
            Side::Right => self.1.borrow_mut(),
        };

        state.delta = new_pos_pi - state.pos;
        if state.delta.abs() > std::f32::consts::PI {
            state.delta += std::f32::consts::TAU * (state.delta.signum() * -1.0);
        }
        state.pos = new_pos_pi;
    }

    pub fn zero_deltas(&mut self) {
        self.0.delta = 0.0;
        self.1.delta = 0.0;
    }
}

use crate::config::GameConfig;
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CustomControlleMap {
    pub buttons: HashMap<Button, Code>,
    pub axis: HashMap<Axis, Code>, //TODO: Direction?
}

pub type CustomBindings = HashMap<uuid::Uuid, CustomControlleMap>;
