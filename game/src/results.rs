use std::{
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc},
    time::SystemTime,
};

use di::{RefMut, ServiceProvider};
use kson::score_ticks::ScoreTick;
use luals_gen::ToLuaLsType;
use serde::Serialize;

use crate::{
    async_service::AsyncService,
    button_codes::UscButton,
    config::GameConfig,
    game::{
        gauge::{Gauge, GaugeType},
        HitRating, HitWindow,
    },
    game_main::AutoPlay,
    help,
    lua_service::LuaProvider,
    scene::{Scene, SceneData},
    song_provider::{DiffId, ScoreProvider, SongDiffId, SongId},
    songselect::{Difficulty, Song},
    vg_ui::Vgfx,
    ControlMessage,
};
use serde_with::*;
use tealr::{
    mlu::{
        mlua::{Function, Lua, LuaSerdeExt},
        TealData, UserData,
    },
    ToTypename,
};

#[derive(Debug, ToTypename, Clone, Serialize, Default, ToLuaLsType)]
#[serde(rename_all = "camelCase")]
struct HidSud {}

#[serde_as]
#[derive(Debug, Clone, Serialize, Default, luals_gen::ToLuaLsType)]
#[serde(rename_all = "camelCase")]
pub struct SongResultData {
    score: u32,
    gauge_type: u8,    // 0 = normal, 1 = hard. Should be defined in constants sometime
    gauge_option: i32, // type specific, such as difficulty level for the same gauge type if available
    mirror: bool,
    random: bool,
    auto_flags: i32, //bits for autoplay settings, 0 = no autoplay
    gauge: f32,      // value of the gauge at the end of the song
    misses: i32,
    goods: i32,
    perfects: i32,
    max_combo: i32,
    level: u8,
    difficulty: u8,
    title: String,      // With the player name in multiplayer
    real_title: String, // Always without the player name
    artist: String,
    effector: String,
    illustrator: String,
    bpm: String,
    duration: i32, // Length of the chart in milliseconds
    jacket_path: PathBuf,
    median_hit_delta: f64,
    mean_hit_delta: f64,
    median_hit_delta_abs: f64,
    mean_hit_delta_abs: f64,
    earlies: i32,
    lates: i32,
    badge: u8, // same as song wheel badge (except 0 which means the user manually exited)
    gauge_samples: Vec<f32>, // gauge values sampled throughout the song
    grade: String, // "S", "AAA+", "AAA", etc.
    high_scores: Vec<Score>, // Same as song wheel scores
    player_name: String,
    display_index: i32, // Only on multiplayer; which player's score (not necessarily the viewer's) is being shown right not
    #[serde(skip_serializing_if = "Option::is_none")]
    uid: Option<String>, // Only on multiplayer; the UID of the viewer
    hit_window: HitWindow, // Same as gameplay HitWindow
    autoplay: bool,
    playback_speed: f32,
    mission: String,               // Only on practice mode
    retry_count: i32,              // Only on practice mode
    is_self: bool, // Whether this score is viewer's in multiplayer; always true for singleplayer
    speed_mod_type: i32, // Only when isSelf is true; 0 for XMOD, 1 for MMOD, 2 for CMOD
    speed_mod_value: f64, // Only when isSelf is true; HiSpeed for XMOD, ModSpeed for MMOD and CMOD
    note_hit_stats: Vec<HitStat>, // Only when isSelf is true; contains HitStat for notes (excluding hold notes and lasers)
    hold_hit_stats: Vec<HitStat>, // Only when isSelf is true; contains HitStat for holds
    laser_hit_stats: Vec<HitStat>, // Only when isSelf is true; contains HitStat for lasers
    is_local: bool,               // Whether this score was set locally
    song_id: SongDiffId,
}

#[repr(u8)]
pub enum ClearMark {
    None = 0,
    Played,
    Cleared,
    HardCleared,
    FullCombo,
    Perfect,
}

pub fn calculate_clear_mark(hits: &[HitRating], manual: bool, gauge: &Gauge) -> ClearMark {
    if manual {
        return ClearMark::None;
    }

    if !gauge.is_cleared() || gauge.is_dead() {
        return ClearMark::Played;
    }

    if hits.iter().all(|x| x.crit()) {
        return ClearMark::Perfect;
    }

    if hits.iter().all(|x| x.hit()) {
        return ClearMark::FullCombo;
    }

    match gauge {
        Gauge::None => ClearMark::None,
        Gauge::Normal { .. } => ClearMark::Cleared,
        Gauge::Hard { .. } => ClearMark::HardCleared,
    }
}

impl SongResultData {
    pub fn from_diff(
        song: Arc<Song>,
        diff_idx: usize,
        score: u32,
        hit_ratings: Vec<HitRating>,
        gauge: Gauge,
        hit_window: HitWindow,
        autoplay: AutoPlay,
        max_combo: i32,
        duration: i32,
        manual_exit: bool,
    ) -> anyhow::Result<Self> {
        use itertools::Itertools;
        use statrs::statistics::{Data, Median, Statistics};
        let Difficulty {
            jacket_path,
            level,
            difficulty,
            id: _,
            effector,
            top_badge: _,
            scores,
            hash: _,
            illustrator,
        } = song.difficulties.read().expect("Lock error")[diff_idx].clone();

        let Song {
            title,
            artist,
            bpm,
            id: _,
            difficulties: _,
        } = (*song).clone();

        let grade = match score {
            99_00000.. => "S",
            98_00000.. => "AAA+",
            97_00000.. => "AAA",
            95_00000.. => "AA+",
            93_00000.. => "AA",
            90_00000.. => "A+",
            87_00000.. => "A",
            75_00000.. => "B",
            65_00000.. => "C",
            0.. => "D",
        }
        .to_string();

        let badge = calculate_clear_mark(&hit_ratings, manual_exit, &gauge);

        let stat_times = hit_ratings
            .iter()
            .filter(|x| x.for_stats())
            .map(|x| x.delta())
            .collect_vec();
        let mean_hit_delta = stat_times.clone().mean();

        let stat_times = Data::new(stat_times);

        let median_hit_delta = stat_times.median();

        let (laser_hit_stats, note_hit_stats, hold_hit_stats): (
            Vec<HitStat>,
            Vec<HitStat>,
            Vec<HitStat>,
        ) = hit_ratings.iter().try_fold(
            (vec![], vec![], vec![]),
            |(mut laser, mut note, mut hold), x| -> anyhow::Result<_> {
                let mut rating: HitStat = (*x).try_into()?;
                rating.time_frac = (rating.time as f32 / duration as f32).clamp(0.0, 1.0);

                match x {
                    HitRating::None => {}
                    HitRating::Crit {
                        tick,
                        delta: _,
                        time: _,
                    }
                    | HitRating::Good {
                        tick,
                        delta: _,
                        time: _,
                    }
                    | HitRating::Miss {
                        tick,
                        delta: _,
                        time: _,
                    } => match tick.tick {
                        ScoreTick::Laser { lane: _, pos: _ }
                        | ScoreTick::Slam {
                            lane: _,
                            start: _,
                            end: _,
                        } => laser.push(rating),

                        ScoreTick::Chip { .. } => note.push(rating),
                        ScoreTick::Hold { .. } => hold.push(rating),
                    },
                }
                Ok((laser, note, hold))
            },
        )?;

        Ok(Self {
            score,
            jacket_path,
            artist,
            real_title: title.clone(),
            title,
            effector,
            high_scores: scores,
            level,
            difficulty,
            bpm,
            grade,
            gauge_samples: Vec::from(gauge.get_samples()),
            gauge: gauge.value(),
            goods: hit_ratings
                .iter()
                .filter(|x| {
                    matches!(
                        x,
                        HitRating::Good {
                            tick: _,
                            delta: _,
                            time: _
                        }
                    )
                })
                .count() as i32,
            perfects: hit_ratings
                .iter()
                .filter(|x| {
                    matches!(
                        x,
                        HitRating::Crit {
                            tick: _,
                            delta: _,
                            time: _
                        }
                    )
                })
                .count() as i32,
            misses: hit_ratings
                .iter()
                .filter(|x| {
                    matches!(
                        x,
                        HitRating::Miss {
                            tick: _,
                            delta: _,
                            time: _
                        }
                    )
                })
                .count() as i32,

            earlies: hit_ratings
                .iter()
                .filter(
                    |x| matches!(x, HitRating::Good { tick: _, delta, time: _ } if *delta > 0.0),
                )
                .count() as i32,
            lates: hit_ratings
                .iter()
                .filter(
                    |x| matches!(x, HitRating::Good { tick: _, delta, time: _ } if *delta < 0.0),
                )
                .count() as i32,
            laser_hit_stats,
            note_hit_stats,
            hold_hit_stats,
            song_id: SongDiffId::SongDiff(
                song.id.clone(),
                song.difficulties.read().expect("Lock error")[diff_idx]
                    .hash
                    .as_ref()
                    .map(|h| DiffId(SongId::StringId(h.clone())))
                    .unwrap_or_else(|| {
                        song.difficulties.read().expect("Lock error")[diff_idx]
                            .id
                            .clone()
                    }),
            ),
            gauge_type: GaugeType::try_from(gauge)
                .map(|x| x as u8)
                .unwrap_or_default(),
            hit_window,
            playback_speed: 1.0,
            auto_flags: match autoplay {
                AutoPlay::None => 0,
                AutoPlay::Buttons => 1,
                AutoPlay::Lasers => 2,
                AutoPlay::All => 3,
            },
            autoplay: autoplay.any(),
            gauge_option: 0,
            mirror: false,
            random: false,
            max_combo,
            illustrator,
            duration,
            median_hit_delta,
            mean_hit_delta,
            median_hit_delta_abs: median_hit_delta.abs(),
            mean_hit_delta_abs: mean_hit_delta.abs(),
            badge: badge as u8,
            player_name: String::new(),
            display_index: 0,
            uid: None,
            mission: String::new(),
            retry_count: 0,
            is_self: true,
            speed_mod_type: 0,
            speed_mod_value: GameConfig::get().mod_speed,
            is_local: true,
        })
    }
}

impl SceneData for SongResultData {
    fn make_scene(self: Box<Self>, services: ServiceProvider) -> anyhow::Result<Box<dyn Scene>> {
        services
            .get_required_mut::<AsyncService>()
            .read()
            .expect("Lock error")
            .save_config(); // Save config in case of changed hispeed

        Ok(Box::new(SongResult {
            score_service: services.get_required(),
            close: false,
            control_tx: None,
            data: *self,
            lua: LuaProvider::new_lua(),
            services,
            screenshot_state: ScreenshotState::NotRendered,
        }))
    }
}

#[derive(Debug, ToTypename, Clone, Serialize, Default, ToLuaLsType)]
#[serde(rename_all = "camelCase")]
struct HitStat {
    rating: i32,    // 0 for miss, 1 for near, 2 for crit
    lane: i32,      // 0-3 btn, 4-5 fx, 6-7 lasers
    time: i32,      // In milliseconds
    time_frac: f32, // Between 0 and 1 (time / duration)
    delta: i32,
    hold: i32, // 0 for chip or laser, otherwise # of ticks in hold
}

impl TryFrom<HitRating> for HitStat {
    type Error = anyhow::Error;

    fn try_from(value: HitRating) -> Result<Self, Self::Error> {
        let mut ret = match value {
            HitRating::None => return Err(anyhow::anyhow!("HitRating was None")),
            HitRating::Crit { tick, delta, time }
            | HitRating::Good { tick, delta, time }
            | HitRating::Miss { tick, delta, time } => Self {
                rating: 0,
                lane: tick.tick.global_lane() as i32,
                time: time as i32,
                time_frac: 0.0,
                delta: delta as i32,
                hold: match tick.tick {
                    kson::score_ticks::ScoreTick::Laser { lane: _, pos: _ } => 1,
                    kson::score_ticks::ScoreTick::Slam {
                        lane: _,
                        start: _,
                        end: _,
                    } => 0,
                    kson::score_ticks::ScoreTick::Chip { .. } => 0,
                    kson::score_ticks::ScoreTick::Hold { .. } => 1,
                },
            },
        };

        ret.rating = match value {
            HitRating::None => unreachable!(),
            HitRating::Crit { .. } => 2,
            HitRating::Good { .. } => 1,
            HitRating::Miss { .. } => 0,
        };

        Ok(ret)
    }
}

#[derive(Debug, ToTypename, Clone, Serialize, UserData, Default, ToLuaLsType)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    ///range 0.0 -> 1.0
    pub gauge: f32,
    /// 0 = normal, 1 = hard. Should be defined in constants sometime
    pub gauge_type: u8,
    /// type specific, such as difficulty level for the same gauge type if available
    pub gauge_option: i32,
    pub mirror: bool,
    pub random: bool,
    /// bits for autoplay settings, 0 = no autoplay
    pub auto_flags: i32,
    pub score: i32,
    pub perfects: i32,
    pub goods: i32,
    pub misses: i32,
    pub badge: u8,
    ///timestamp in POSIX time (seconds since Jan 1 1970 00:00:00 UTC)
    pub timestamp: i32,
    pub player_name: String,
    /// Whether this score was set locally
    pub is_local: bool,
    pub hit_window: HitWindow,
    pub earlies: i32,
    pub lates: i32,
    pub combo: u32,
}

impl From<&SongResultData> for Score {
    fn from(val: &SongResultData) -> Self {
        let SongResultData {
            score,
            gauge_type,
            gauge_option,
            mirror,
            random,
            auto_flags,
            gauge,
            misses,
            goods,
            perfects,
            earlies,
            lates,
            badge,
            player_name,
            hit_window,
            is_local,
            max_combo,
            ..
        } = val;
        Score {
            gauge: *gauge,
            gauge_type: *gauge_type,
            gauge_option: *gauge_option,
            mirror: *mirror,
            random: *random,
            auto_flags: *auto_flags,
            score: *score as _,
            perfects: *perfects,
            goods: *goods,
            misses: *misses,
            badge: *badge,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time before epoch")
                .as_secs() as _,
            player_name: player_name.clone(),
            is_local: *is_local,
            hit_window: *hit_window,
            earlies: *earlies,
            lates: *lates,
            combo: *max_combo as _,
        }
    }
}

impl TealData for Score {}

enum ScreenshotState {
    NotRendered,
    Rendered,
    Finished,
}

pub struct SongResult {
    data: SongResultData,
    lua: Rc<Lua>,
    services: ServiceProvider,
    control_tx: Option<Sender<ControlMessage>>,
    close: bool,
    score_service: RefMut<dyn ScoreProvider>,
    screenshot_state: ScreenshotState,
}

impl Scene for SongResult {
    fn init(&mut self, app_control_tx: Sender<ControlMessage>) -> anyhow::Result<()> {
        self.score_service
            .write()
            .expect("Lock error")
            .insert_score(&self.data.song_id, Score::from(&self.data))?;

        self.services
            .get_required::<LuaProvider>()
            .register_libraries(self.lua.clone(), "result.lua")?;

        self.lua
            .globals()
            .set("result", self.lua.to_value(&self.data)?)?;

        if let Ok(result_set) = self.lua.globals().get::<_, Function>("result_set") {
            result_set.call::<_, ()>(())?;
        }
        self.control_tx = Some(app_control_tx);
        Ok(())
    }

    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        let render_fn: Function = self.lua.globals().get("render")?;
        render_fn.call(dt / 1000.0)?;

        self.screenshot_state = match self.screenshot_state {
            ScreenshotState::NotRendered => ScreenshotState::Rendered,
            ScreenshotState::Rendered => {
                let screenshot_logic = GameConfig::get().score_screenshots;
                let is_top_score = !self
                    .data
                    .high_scores
                    .iter()
                    .any(|s| s.score > self.data.score as i32);

                let take_screenshot = match screenshot_logic {
                    crate::config::ScoreScreenshot::Always => true,
                    crate::config::ScoreScreenshot::Never => false,
                    crate::config::ScoreScreenshot::Highscores => is_top_score,
                };

                if take_screenshot {
                    let get_capture_rect: Option<Function> =
                        self.lua.globals().get("get_capture_rect").ok();

                    let capture_rect = get_capture_rect
                        .map(|f| f.call::<_, (usize, usize, usize, usize)>(()).ok())
                        .flatten()
                        .map(|(x, y, w, h)| ((x, y), (w, h)));

                    let vgfx = self.lua.app_data_ref::<RefMut<Vgfx>>().unwrap();
                    let screenshot = help::take_screenshot(&vgfx.read().unwrap(), capture_rect);
                    match screenshot {
                        Ok(p) => {
                            log::info!("Saved screenshot to: {:?}", &p);
                            let screenshot_captured: Option<Function> =
                                self.lua.globals().get("screenshot_captured").ok();

                            screenshot_captured.and_then(|x| {
                                x.call::<_, ()>(p.as_os_str().to_string_lossy()).ok()
                            });
                        }
                        Err(e) => log::warn!("Failed to save screenshot: {e}"),
                    }
                }

                ScreenshotState::Finished
            }
            ScreenshotState::Finished => ScreenshotState::Finished,
        };

        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton, _time: SystemTime) {
        if let UscButton::Start = button {
            self.close = true;
        }
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        egui::Window::new("Song Results").show(ctx, |ui| {
            if ui.button("Close").clicked() {
                self.close = true;
            }
        });

        Ok(())
    }

    fn closed(&self) -> bool {
        self.close
    }

    fn name(&self) -> &str {
        "Song Result"
    }
}
