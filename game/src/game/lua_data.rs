use std;

use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::DurationMilliSecondsWithFrac;
use three_d::vec3;

use three_d::Vec2;

use three_d::Camera;
use three_d_asset::InnerSpace;

use std::time::Duration;

use std::path::PathBuf;

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaGameState {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) jacket_path: PathBuf,
    pub(crate) demo_mode: bool,
    pub(crate) difficulty: u8,
    pub(crate) level: u8,
    pub(crate) progress: f32, // 0.0 at the start of a song, 1.0 at the end
    pub(crate) hispeed: f32,
    pub(crate) hispeed_adjust: u32, // 0 = not adjusting, 1 = coarse (xmod) adjustment, 2 = fine (mmod) adjustment
    pub(crate) bpm: f32,
    pub(crate) gauge: LuaGauge,
    pub(crate) hidden_cutoff: f32,
    pub(crate) sudden_cutoff: f32,
    pub(crate) hidden_fade: f32,
    pub(crate) sudden_fade: f32,
    pub(crate) autoplay: bool,
    pub(crate) combo_state: u32,        // 2 = puc, 1 = uc, 0 = normal
    pub(crate) note_held: [bool; 6], // Array indicating wether a hold note is being held, in order: ABCDLR
    pub(crate) laser_active: [bool; 2], // Array indicating if the laser cursor is on a laser, in order: LR
    pub(crate) score_replays: Vec<ScoreReplay>, //Array of previous scores for the current song
    pub(crate) crit_line: CritLine,     // info about crit line and everything attached to it
    pub(crate) hit_window: HitWindow, // This may be absent (== nil) for the default timing window (46 / 92 / 138 / 250ms)
    pub(crate) multiplayer: bool,
    pub(crate) user_id: String,
    pub(crate) practice_setup: bool, // true: it's the setup, false: practicing n
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaGauge {
    #[serde(rename = "type")]
    pub(crate) gauge_type: i32,
    pub(crate) options: i32,
    pub(crate) value: f32,
    pub(crate) name: String,
}

#[serde_as]
#[derive(Debug, Serialize, Default, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HitWindow {
    #[serde(rename = "type")]
    pub variant: i32,
    #[serde_as(as = "DurationMilliSecondsWithFrac<f64>")]
    pub perfect: Duration,
    #[serde_as(as = "DurationMilliSecondsWithFrac<f64>")]
    pub good: Duration,
    #[serde_as(as = "DurationMilliSecondsWithFrac<f64>")]
    pub hold: Duration,
    #[serde_as(as = "DurationMilliSecondsWithFrac<f64>")]
    pub miss: Duration,
}

impl HitWindow {
    pub fn new(variant: i32, perfect_ms: u64, good_ms: u64, hold_ms: u64, miss_ms: u64) -> Self {
        Self {
            variant,
            perfect: Duration::from_millis(perfect_ms),
            good: Duration::from_millis(good_ms),
            hold: Duration::from_millis(hold_ms),
            miss: Duration::from_millis(miss_ms),
        }
    }
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CritLine {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) rotation: f32,
    pub(crate) cursors: [Cursor; 2],
    pub(crate) line: Line,
    pub(crate) x_offset: f32,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
pub(crate) struct Cursor {
    pub(crate) pos: f32,
    pub(crate) alpha: f32,
    pub(crate) skew: f32,
}

impl Cursor {
    pub fn new(pos: f32, camera: &Camera, alpha: f32) -> Self {
        let pos = (pos - 0.5) * (5.0 / 6.0);

        let crit_pos = Vec2::from(camera.pixel_at_position(vec3(0.0, 0.0, 0.0)));
        let c_pos = Vec2::from(camera.pixel_at_position(vec3(pos, 0.0, 0.0)));
        let c_pos_up = Vec2::from(camera.pixel_at_position(vec3(pos, 0.2, 0.0)));
        let c_pos_down = Vec2::from(camera.pixel_at_position(vec3(pos, -0.2, 0.0)));
        let dist_from_crit_center =
            (crit_pos - c_pos).magnitude() * if pos < 0.0 { -1.0 } else { 1.0 };
        let cursor_angle_vector = c_pos_up - c_pos_down;

        let skew = cursor_angle_vector.y.atan2(cursor_angle_vector.x) + std::f32::consts::FRAC_PI_2;

        Self {
            pos: dist_from_crit_center,
            alpha,
            skew,
        }
    }
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Line {
    pub(crate) x1: f32,
    pub(crate) y1: f32,
    pub(crate) x2: f32,
    pub(crate) y2: f32,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScoreReplay {
    pub(crate) max_score: i32,
    pub(crate) current_score: i32,
}
