use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fmt::Display,
    time::Duration,
};

use di::{inject, injectable};
use femtovg::rgb::Rgb;
use itertools::Itertools;
use kson::{BtLane, Side};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use tokio::time::timeout;

use crate::config::GameConfig;

#[derive(
    Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, Default, strum::EnumIter,
)]
pub enum Channel {
    #[default]
    R,
    G,
    B,
}

impl Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::R => f.write_str("Red"),
            Channel::G => f.write_str("Green"),
            Channel::B => f.write_str("Blue"),
        }
    }
}

impl Channel {
    fn get<T>(&self, rgb: Rgb<T>) -> T {
        match self {
            Channel::R => rgb.r,
            Channel::G => rgb.g,
            Channel::B => rgb.b,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, strum::EnumIter)]
pub enum LightingTarget {
    Start,
    Bt(BtLane),
    Fx(Side),
    Top(Side, Channel),
    Middle(Side, Channel),
    Bottom(Side, Channel),
    Base(Channel),
}

impl Display for LightingTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LightingTarget::Start => f.write_str("Start"),
            LightingTarget::Bt(bt_lane) => f.write_fmt(format_args!("BT {bt_lane}")),
            LightingTarget::Fx(side) => f.write_fmt(format_args!("FX {side}")),
            LightingTarget::Top(side, channel) => f.write_fmt(format_args!("Top {side} {channel}")),
            LightingTarget::Middle(side, channel) => {
                f.write_fmt(format_args!("Middle {side} {channel}"))
            }
            LightingTarget::Bottom(side, channel) => {
                f.write_fmt(format_args!("Bottom {side} {channel}"))
            }
            LightingTarget::Base(channel) => f.write_fmt(format_args!("Base {channel}")),
        }
    }
}

impl LightingTarget {
    pub fn is_rgb(&self) -> bool {
        match self {
            LightingTarget::Top(..) => true,
            LightingTarget::Middle(..) => true,
            LightingTarget::Bottom(..) => true,
            LightingTarget::Base(..) => true,
            _ => false,
        }
    }

    pub fn iter() -> impl Iterator<Item = Self> {
        // We can avoid allocating vectors by boxing iterators, but wont be called in a hot path anyway
        <Self as strum::IntoEnumIterator>::iter().flat_map(|x| match x {
            LightingTarget::Start => vec![Self::Start],
            LightingTarget::Bt(..) => BtLane::iter().map(|x| Self::Bt(x)).collect_vec(),
            LightingTarget::Fx(..) => Side::iter().map(|x| Self::Fx(x)).collect_vec(),
            LightingTarget::Top(..) => Side::iter()
                .flat_map(|s| Channel::iter().map(move |c| (s, c)))
                .map(|(s, c)| Self::Top(s, c))
                .collect_vec(),
            LightingTarget::Middle(..) => Side::iter()
                .flat_map(|s| Channel::iter().map(move |c| (s, c)))
                .map(|(s, c)| Self::Middle(s, c))
                .collect_vec(),
            LightingTarget::Bottom(..) => Side::iter()
                .flat_map(|s| Channel::iter().map(move |c| (s, c)))
                .map(|(s, c)| Self::Bottom(s, c))
                .collect_vec(),
            LightingTarget::Base(..) => Channel::iter().map(|x| Self::Base(x)).collect_vec(),
        })
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappedTarget {
    pub report_id: u32,
    pub start_bit: u32,
    pub target: LightingTarget,
}

pub type LightingDeviceMap = Vec<MappedTarget>;
pub type LightingMap = HashMap<String, LightingDeviceMap>;

pub struct LightingService {
    worker: tokio::task::JoinHandle<()>,
    tx: tokio::sync::watch::Sender<LightingData>,
    rx: tokio::sync::watch::Receiver<LightingData>,
}

#[injectable]
impl LightingService {
    #[inject]
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::watch::channel(LightingData::default());
        Self {
            worker: tokio::spawn(async {}),
            tx,
            rx,
        }
    }

    pub fn stop(&mut self) {
        self.worker.abort();
    }

    pub fn start(&mut self) {
        self.worker = tokio::spawn(lighting_worker(self.rx.clone()));
    }

    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }

    pub fn update(&self, value: LightingData) {
        _ = self.tx.send(value);
    }
}

async fn lighting_worker(mut rx: tokio::sync::watch::Receiver<LightingData>) {
    let mut config = { GameConfig::get().lighting.clone() };
    let Ok(api) = hidlights::HidLights::new() else {
        return;
    };

    let mut devices = api
        .devices()
        .into_iter()
        .filter_map(|dev| {
            config
                .remove(&dev.path().to_string_lossy().into_owned())
                .map(|conf| (dev, conf))
        })
        .filter_map(|(dev, conf)| dev.open().ok().map(|dev| (dev, conf)))
        .filter_map(|(dev, conf)| dev.reports().ok().map(|x| (dev, conf, x)))
        .map(|(dev, conf, reports)| {
            (
                dev,
                conf.into_iter()
                    .map(|x| ((x.report_id, x.start_bit), x.target))
                    .collect::<HashMap<_, _>>(),
                reports
                    .into_iter()
                    .map(|r| (r.id(), r))
                    .collect::<HashMap<_, _>>(),
            )
        })
        .collect_vec();
    let mut used_reports = HashSet::new();
    loop {
        {
            if let Ok(Err(_)) = timeout(Duration::from_millis(10), rx.changed()).await {
                // channel closed
                return;
            }
        }

        {
            let lighting_data = rx.borrow();
            for (dev, conf, reports) in &mut devices {
                used_reports.clear();
                for ((report_id, start_bit), target) in conf {
                    let Some(rep) = reports.get_mut(report_id) else {
                        continue;
                    };

                    let Ok(out) = rep
                        .outputs
                        .binary_search_by_key(start_bit, |x| x.bits().start)
                    else {
                        continue;
                    };

                    rep.outputs[out].real_value = lighting_data.get(*target);

                    used_reports.insert(*report_id);
                }

                for rep in &used_reports {
                    let Some(rep) = reports.get(rep) else {
                        continue;
                    };

                    dev.write_report(rep);
                }
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct LightingData {
    pub top: [Rgb<f32>; 2],
    pub middle: [Rgb<f32>; 2],
    pub bottom: [Rgb<f32>; 2],
    pub base: Rgb<f32>,
    pub buttons: [bool; 7],
}

impl LightingData {
    fn get(&self, target: LightingTarget) -> f32 {
        match target {
            LightingTarget::Start => self.buttons[0] as u8 as f32,
            LightingTarget::Bt(bt_lane) => self.buttons[1 + bt_lane as usize] as u8 as f32,
            LightingTarget::Fx(side) => self.buttons[5 + side as usize] as u8 as f32,
            LightingTarget::Top(side, channel) => channel.get(self.top[side as usize]),
            LightingTarget::Middle(side, channel) => channel.get(self.middle[side as usize]),
            LightingTarget::Bottom(side, channel) => channel.get(self.bottom[side as usize]),
            LightingTarget::Base(channel) => channel.get(self.base),
        }
    }
}
