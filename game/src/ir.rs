use crate::{config::GameConfig, game::HitWindow};
use log::warn;
use luals_gen::ToLuaLsType;
use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Value};
use mlua_bridge::mlua_bridge;
use poll_promise::Promise;
use reqwest::{
    header::{HeaderMap, HeaderValue},
    Method, RequestBuilder,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, ToLuaLsType)]
#[serde(rename_all = "camelCase")]
pub struct ServerScore {
    score: i32,
    lamp: u8,
    timestamp: u64,
    crit: i32,
    near: i32,
    error: i32,
    ranking: i32,
    gauge_mod: String,
    note_mod: String,
    username: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum IrResponseBody {
    Heartbeat {
        server_time: u64,
        server_name: String,
        ir_version: String,
    },
    Record {
        record: ServerScore,
    },
    Leaderboard {
        scores: Vec<ServerScore>,
    },
    ScoreSubmit {
        score: ServerScore,
        server_record: ServerScore,
        adjacent_above: Vec<ServerScore>,
        adjacent_below: Vec<ServerScore>,
        is_p_b: bool,
        is_server_record: bool,
        send_replay: bool,
    },
    None {},
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrServerResponse {
    pub status_code: i32,
    pub description: String,
    #[serde(default)]
    pub body: Option<IrResponseBody>,
}

pub struct InternetRanking {
    requests: Vec<(RegistryKey, Promise<anyhow::Result<IrServerResponse>>)>,
}

pub struct InternetRankingLua;

#[mlua_bridge(rename_funcs = "PascalCase", rename_fields = "PascalCase")]
impl InternetRankingLua {
    fn get_active() -> bool {
        !GameConfig::get().ir_endpoint.is_empty()
    }

    fn get_states(lua: &Lua) -> Value {
        lua.to_value(&serde_json::json!(
                {
                    "Unused": 0,
                    "Pending": 10,
                    "Success": 20,
                    "Accepted": 22,
                    "BadRequest": 40,
                    "Unauthorized": 41,
                    "ChartRefused": 42,
                    "Forbidden": 43,
                    "NotFound": 44,
                    "ServerError": 50,
                    "RequestFailure": 60,
                }
        ))
        .expect("Failed to convert to lua")
    }

    fn heartbeat(lua: &Lua, cb: Function, ir: &mut InternetRanking) {
        ir.send(lua, cb, Method::GET, "heartbeat", |r| r);
    }

    fn chart_tracked(lua: &Lua, hash: String, cb: Function, ir: &mut InternetRanking) {
        ir.send(lua, cb, Method::GET, &format!("charts/{hash}/"), |r| r);
    }

    fn record(lua: &Lua, hash: String, cb: Function, ir: &mut InternetRanking) {
        ir.send(
            lua,
            cb,
            Method::GET,
            &format!("charts/{hash}/record"),
            |r| r,
        );
    }

    fn leaderboard(
        lua: &Lua,
        hash: String,
        mode: String,
        n: u32,
        cb: Function,
        ir: &mut InternetRanking,
    ) {
        ir.send(
            lua,
            cb,
            Method::GET,
            &format!("charts/{hash}/leaderboard"),
            |r| r.query(&[("mode", mode), ("n", n.to_string())]),
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitOptions {
    gauge_type: u8,
    gauge_opt: i32,
    mirror: bool,
    random: bool,
    auto_flags: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerChart {
    chart_hash: String,
    artist: String,
    title: String,
    level: u8,
    difficulty: u8,
    effector: String,
    illustrator: String,
    bpm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScoreSubmissionScore {
    score: u32,
    gauge: f32,
    timestamp: u64,
    crit: i32,
    near: i32,
    early: i32,
    late: i32,
    combo: i32,
    error: i32,
    options: SubmitOptions,
    windows: HitWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSubmission {
    chart: ServerChart,
    score: ScoreSubmissionScore,
}

impl From<&crate::results::SongResultData> for ScoreSubmission {
    fn from(value: &crate::results::SongResultData) -> Self {
        Self {
            chart: ServerChart {
                chart_hash: value.chart_hash.clone(),
                artist: value.artist.clone(),
                title: value.title.clone(),
                level: value.level,
                difficulty: value.difficulty,
                effector: value.effector.clone(),
                illustrator: value.illustrator.clone(),
                bpm: value.bpm.clone(),
            },
            score: ScoreSubmissionScore {
                score: value.score,
                gauge: value.gauge,
                timestamp: 0,
                crit: value.perfects,
                near: value.goods,
                early: value.earlies,
                late: value.lates,
                combo: value.max_combo,
                error: value.misses,
                options: SubmitOptions {
                    gauge_type: value.gauge_type,
                    gauge_opt: value.gauge_option,
                    mirror: value.mirror,
                    random: value.random,
                    auto_flags: value.auto_flags,
                },
                windows: value.hit_window,
            },
        }
    }
}

impl InternetRanking {
    pub fn new() -> Self {
        Self { requests: vec![] }
    }

    pub fn enabled() -> bool {
        !GameConfig::get().ir_endpoint.is_empty()
    }

    pub async fn submit(hash: String, score: ScoreSubmission) -> anyhow::Result<IrServerResponse> {
        let client = Self::client()?;
        let url = format!(
            "{}/scores",
            GameConfig::get().ir_endpoint.trim_end_matches('/'),
        );

        Ok(client.post(url).json(&score).send().await?.json().await?)
    }

    fn client() -> anyhow::Result<reqwest::Client> {
        let mut headers = HeaderMap::new();
        headers.append(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", GameConfig::get().ir_api_token))?,
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(client)
    }

    fn send(
        &mut self,
        lua: &Lua,
        cb: Function,
        method: Method,
        path: &str,
        r: impl FnOnce(RequestBuilder) -> RequestBuilder,
    ) -> Option<()> {
        let key = lua.create_registry_value(cb).ok()?;
        let client = Self::client().ok()?;
        let url = format!(
            "{}/{}",
            GameConfig::get().ir_endpoint.trim_end_matches('/'),
            path
        );

        let r = r(client.request(method, url));
        let fut = async move {
            let response = r.send().await?;
            let result = response.json().await?;
            Ok(result)
        };
        let promise = poll_promise::Promise::spawn_async(fut);
        self.requests.push((key, promise));

        None
    }

    pub fn poll(lua: &Lua) {
        let Some(mut ir) = lua.app_data_mut::<InternetRanking>() else {
            return;
        };
        ir.requests
            .retain_mut(|(key, promise)| match promise.poll() {
                std::task::Poll::Ready(result) => {
                    match result {
                        Ok(response) => {
                            let function = lua.registry_value::<Function>(key);
                            if let Ok(function) = function {
                                _ = function.call::<()>(lua.to_value(response).unwrap_or_default());
                            }
                        }
                        Err(e) => {
                            warn!("IR Server request error: {e}");
                        }
                    }
                    false
                }
                std::task::Poll::Pending => true,
            });
    }
}
