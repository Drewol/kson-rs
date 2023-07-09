use std::borrow::BorrowMut;

use game_loop::winit::event::ElementState;
use gilrs::Button;
use kson::{BtLane, Side};

#[derive(Debug, Clone, Copy)]
pub enum UscButton {
    BT(BtLane),
    FX(Side),
    Start,
    Back,
    Other(Button),
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
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UscInputEvent {
    Laser(LaserState),
    Button(UscButton, ElementState),
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

        log::info!("{:?}", self);
    }

    pub fn zero_deltas(&mut self) {
        self.0.delta = 0.0;
        self.1.delta = 0.0;
    }
}
