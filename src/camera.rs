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

pub fn parse_ksh_zoom_values(data: &str) -> Result<(f64, Option<f64>), Box<dyn Error>> {
    let (v, vf): (f64, Option<f64>) = {
        if data.contains(';') {
            let mut values = data.split(';');
            (
                values.next().unwrap_or("0").parse()?,
                values.next().map(|vf| vf.parse::<f64>().unwrap_or(0.)),
            )
        } else {
            (data.parse()?, None)
        }
    };
    let v = v / 100.0;
    let vf = vf.map(|val| val / 100.0);
    Ok((v, vf))
}
