use std::cmp::Ordering;

use crate::{ByPulse, Graph, GraphPoint, GraphSectionPoint};
use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CameraInfo {
    pub tilt: TiltInfo,
    pub cam: CamInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct TiltInfo {
    pub scale: ByPulse<f32>,
    pub manual: ByPulse<Vec<GraphSectionPoint>>,
    pub keep: ByPulse<bool>,
}

impl Graph<Option<f64>> for ByPulse<Vec<GraphSectionPoint>> {
    fn value_at(&self, tick: f64) -> Option<f64> {
        let tick_u = tick as u32;
        let index = self
            .binary_search_by(|x| cmp_graph_section(x, tick_u))
            .ok()?;
        let (y, graph) = &self[index];

        graph.value_at(tick - (*y) as f64)
    }

    fn direction_at(&self, tick: f64) -> Option<f64> {
        let tick_u = tick as u32;
        let index = self
            .binary_search_by(|x| cmp_graph_section(x, tick_u))
            .ok()?;
        let (y, graph) = &self[index];

        graph.direction_at(tick - (*y) as f64)
    }

    fn wide_at(&self, _: f64) -> u32 {
        1
    }
}

fn cmp_graph_section((y, graph): &(u32, Vec<GraphSectionPoint>), cmp_y: u32) -> Ordering {
    if cmp_y < *y {
        Ordering::Less
    } else {
        let ry = graph.last().map(|x| x.ry).unwrap_or(0);
        if (cmp_y - *y) <= ry {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamInfo {
    pub body: CamGraphs,
    #[serde(skip_serializing_if = "CamPatternInfo::is_empty")]
    pub pattern: CamPatternInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamPatternInfo {
    #[serde(skip_serializing_if = "CamPatternLaserInfo::is_empty")]
    pub laser: CamPatternLaserInfo,
}

impl CamPatternInfo {
    fn is_empty(&self) -> bool {
        self.laser.slam_event.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamPatternLaserInfo {
    #[serde(skip_serializing_if = "CamPatternLaserInvokeList::is_empty")]
    pub slam_event: CamPatternLaserInvokeList,
}

impl CamPatternLaserInfo {
    fn is_empty(&self) -> bool {
        self.slam_event.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamPatternLaserInvokeList {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spin: Vec<CamPatternInvokeSpin>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub half_spin: Vec<CamPatternInvokeSpin>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub swing: Vec<CamPatternInvokeSwing>,
}

impl CamPatternLaserInvokeList {
    fn is_empty(&self) -> bool {
        self.spin.is_empty() && self.swing.is_empty() && self.half_spin.is_empty()
    }
}

/// (pulse, direction, duration)
#[derive(Debug, Serialize, Deserialize, Copy, Clone, Default)]
pub struct CamPatternInvokeSpin(pub u32, pub i32, pub u32);
#[derive(Debug, Serialize, Deserialize, Copy, Clone, Default)]
pub struct CamPatternInvokeSwing(
    pub u32,
    pub i32,
    pub u32,
    #[serde(default, skip_serializing_if = "IsDefault::is_default")] pub CamPatternInvokeSwingValue,
);

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq)]
pub struct CamPatternInvokeSwingValue {
    pub scale: f32,  // scale
    pub repeat: u32, // number of repetitions
    pub decay_order: u32, // order of the decay that scales camera values (0-2)
                     // (note that this decay is applied even if repeat=1)
                     // - equation: `value * (1.0 - ((l - ry) / l))^decay_order`
                     // - 0: no decay, 1: linear decay, 2: squared decay
}

impl Default for CamPatternInvokeSwingValue {
    fn default() -> Self {
        Self {
            scale: 1.0,
            repeat: 1,
            decay_order: 0,
        }
    }
}

type GraphVec = Vec<GraphPoint>;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamGraphs {
    pub zoom: GraphVec,
    pub shift_x: GraphVec,
    pub rotation_x: GraphVec,
    pub rotation_z: GraphVec,
    #[serde(rename = "rotation_z.highway")]
    pub rotation_z_highway: GraphVec,
    #[serde(rename = "rotation_z.jdgline")]
    pub rotation_z_jdgline: GraphVec,
    pub split: GraphVec,
}

pub trait IsDefault {
    fn is_default(&self) -> bool;
}

impl<T> IsDefault for T
where
    T: Default + PartialEq,
{
    fn is_default(&self) -> bool {
        self.eq(&T::default())
    }
}
