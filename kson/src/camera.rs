use std::cmp::Ordering;

use crate::{ByPulse, Graph, GraphPoint, GraphSectionPoint};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CameraInfo {
    pub tilt: ByPulse<TiltValue>,
    pub cam: CamInfo,
}

pub enum ResolvedTiltValue {
    Named(NamedTiltValue),
    Manual(f64),
}

impl ResolvedTiltValue {
    pub fn is_keep(&self) -> bool {
        match self {
            ResolvedTiltValue::Named(named_tilt_value) => matches!(
                named_tilt_value,
                NamedTiltValue::KeepNormal
                    | NamedTiltValue::KeepBigger
                    | NamedTiltValue::KeepBiggest
            ),
            ResolvedTiltValue::Manual(_) => false,
        }
    }

    pub fn scale(&self) -> f64 {
        match self {
            ResolvedTiltValue::Named(v) => match v {
                NamedTiltValue::Normal | NamedTiltValue::KeepNormal => 1.0,
                NamedTiltValue::Bigger | NamedTiltValue::KeepBigger => 1.5,
                NamedTiltValue::Biggest | NamedTiltValue::KeepBiggest => 2.0,
                NamedTiltValue::Zero => 0.0,
            },
            ResolvedTiltValue::Manual(_) => 1.0,
        }
    }
}

impl CameraInfo {
    pub fn tilt_at(&self, y: u32) -> ResolvedTiltValue {
        let (a, b) = match self.tilt.binary_search_by_key(&y, |x| x.0) {
            Ok(idx) => (Some(&self.tilt[idx]), self.tilt.get(idx + 1)),
            Err(idx) if idx == 0 => (None, self.tilt.get(idx)),
            Err(idx) => (self.tilt.get(idx.saturating_sub(1)), self.tilt.get(idx)),
        };
        let a = a.unwrap_or(&(0, TiltValue::Named(NamedTiltValue::Normal)));

        if let TiltValue::ManualToNamedInstant(_, named) = a.1 {
            return ResolvedTiltValue::Named(named);
        }

        if let Some(a_g) = a.1.to_graph_point(a.0) {
            if let Some(b_g) = b.and_then(|b| b.1.to_graph_point(b.0)) {
                ResolvedTiltValue::Manual(vec![a_g, b_g].value_at(y as f64))
            } else {
                ResolvedTiltValue::Manual(a_g.vf.unwrap_or(a_g.v))
            }
        } else {
            let TiltValue::Named(v) = a.1 else {
                unreachable!() // All others resolve to graph points
            };

            ResolvedTiltValue::Named(v)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum NamedTiltValue {
    #[default]
    Normal,
    Bigger,
    Biggest,
    KeepNormal,
    KeepBigger,
    KeepBiggest,
    Zero,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(untagged)]
pub enum TiltValue {
    Named(NamedTiltValue),
    ManualPoint(f64),
    ManualInstant(f64, f64),
    ManualToNamedInstant(f64, NamedTiltValue),
    /// v, (a, b)
    ManualCurve(f64, (f64, f64)),
    /// (v, vf), (a, b)
    ManualCurveInstant((f64, f64), (f64, f64)),
}

impl TiltValue {
    fn to_graph_point(self, y: u32) -> Option<GraphPoint> {
        match self {
            TiltValue::Named(_) => None,
            TiltValue::ManualPoint(v) => Some(GraphPoint {
                y,
                v,
                vf: None,
                a: 0.5,
                b: 0.5,
            }),
            TiltValue::ManualInstant(v, vf) => Some(GraphPoint {
                y,
                v,
                vf: Some(vf),
                a: 0.5,
                b: 0.5,
            }),
            TiltValue::ManualToNamedInstant(v, _) => Some(GraphPoint {
                y,
                v,
                vf: None,
                a: 0.5,
                b: 0.5,
            }),
            TiltValue::ManualCurve(v, (a, b)) => Some(GraphPoint {
                y,
                v,
                vf: None,
                a,
                b,
            }),
            TiltValue::ManualCurveInstant((v, vf), (a, b)) => Some(GraphPoint {
                y,
                v,
                vf: Some(vf),
                a,
                b,
            }),
        }
    }
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
    #[serde(default, skip_serializing_if = "crate::IsDefault::is_default")]
    pub  CamPatternInvokeSwingValue,
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
    pub zoom_bottom: GraphVec,
    pub zoom_size: GraphVec,
    pub zoom_top: GraphVec,
    pub rotation_z: GraphVec,
    #[serde(rename = "rotation_z.highway")]
    pub rotation_z_highway: GraphVec,
    #[serde(rename = "rotation_z.jdgline")]
    pub rotation_z_jdgline: GraphVec,
    pub split: GraphVec,
}
