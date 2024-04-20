use std::cmp::Ordering;

use crate::{overlaps::Overlaps, ByPulse, Graph, GraphPoint, GraphSectionPoint};
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
    pub manual: ByPulse<Vec<GraphSectionPoint>>,
    pub keep: Vec<ByPulse<bool>>,
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
    #[serde(rename = "rotation_z.highway")]
    pub rotation_z_highway: GraphVec,
    #[serde(rename = "rotation_z.jdgline")]
    pub rotation_z_jdgline: GraphVec,
    pub split: GraphVec,
}
