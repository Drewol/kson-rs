use crate::{ByPulse, GraphPoint, GraphSectionPoint};
use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CameraInfo {
    pub tilt: TiltInfo,
    pub cam: CamInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TiltInfo {
    pub manual: Vec<ByPulse<Vec<GraphSectionPoint>>>,
    pub keep: Vec<ByPulse<bool>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CamInfo {
    pub body: CamGraphs,
    pub tilt_assign: Option<CamGraphs>,
    pub pattern: CamPatternInfo,
}

#[derive(Serialize, Deserialize, Copy, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CamPatternInfo;

type GraphVec = Vec<GraphPoint>;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CamGraphs {
    pub zoom: GraphVec,
    pub shift_x: GraphVec,
    pub rotation_x: GraphVec,
    pub rotation_z: GraphVec,
    pub rotation_z_lane: GraphVec,
    pub rotation_z_jdgline: GraphVec,
}
