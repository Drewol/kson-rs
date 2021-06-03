use std::error::Error;

use crate::{ByPulse, GraphPoint, GraphSectionPoint, Interval};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CameraInfo {
    pub tilt: TiltInfo,
    pub cam: CamInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct TiltInfo {
    pub manual: Vec<ByPulse<Vec<GraphSectionPoint>>>,
    pub keep: Vec<Interval>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamInfo {
    pub body: CamGraphs,
    pub tilt_assign: Option<CamGraphs>,
    pub pattern: CamPatternInfo,
}

#[derive(Serialize, Deserialize, Copy, Clone, Default)]
pub struct CamPatternInfo;

type GraphVec = Vec<GraphPoint>;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CamGraphs {
    pub zoom: GraphVec,
    pub shift_x: GraphVec,
    pub rotation_x: GraphVec,
    pub rotation_z: GraphVec,
    pub rotation_z_lane: GraphVec,
    pub rotation_z_jdgline: GraphVec,
}
