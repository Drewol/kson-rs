use std::borrow::BorrowMut;

use gilrs::{ev::Code, Button};
use kson::{BtLane, Side};
use tealr::mlu::generics::A;

#[derive(Debug, Clone, Copy)]
pub enum UscButton {
    BT(BtLane),
    FX(Side),
    Start,
    Back,
    Other(Button),
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
