use gilrs::{ev::Code, Button};
use kson::{BtLane, Side};

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
