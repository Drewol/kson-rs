#![allow(dead_code)]
use diesel::prelude::*;

#[derive(Queryable)]
#[diesel(table_name = Challenges)]
pub struct Challenge {
    rowid: i32,
    title: String,
    charts: String,
    chart_meta: String,
    clear_mark: i32,
    best_score: i32,
    req_text: String,
    path: String,
    hash: String,
    level: i32,
    lwt: i32,
}

#[derive(Queryable)]
#[diesel(table_name = Charts)]
#[diesel(belongs_to(Folder))]
pub struct Chart {
    rowid: i32,
    folderid: i32,
    title: String,
    artist: String,
    title_translit: String,
    artist_translit: String,
    jacket_path: String,
    effector: String,
    illustrator: String,
    diff_name: String,
    diff_shortname: String,
    path: String,
    bpm: String,
    diff_index: i32,
    level: i32,
    preview_offset: i32,
    preview_length: i32,
    lwt: i32,
    hash: String,
    preview_file: String,
    custom_offset: i32,
}

#[derive(Queryable)]
#[diesel(table_name = Collections)]
pub struct Collection {
    collection: String,
    folderid: i32,
    rowid: i32,
}

#[derive(Queryable)]
#[diesel(table_name = Folders)]
pub struct Folder {
    rowid: i32,
    path: String,
}

#[derive(Queryable)]
#[diesel(table_name = PracticeSetups)]
pub struct PracticeSetup {
    chart_id: i32,
    setup_title: String,
    loop_success: i32,
    loop_fail: i32,
    range_begin: i32,
    range_end: i32,
    fail_cond_type: i32,
    fail_cond_value: i32,
    playback_speed: f32,
    inc_speed_on_success: i32,
    inc_speed: f32,
    inc_streak: i32,
    dec_speed_on_fail: i32,
    dec_speed: f32,
    min_playback_speed: f32,
    max_rewind: i32,
    max_rewind_measure: i32,
    rowid: i32,
}

#[derive(Queryable)]
#[diesel(table_name = Scores)]
#[diesel(belongs_to(Chart))]
pub struct Score {
    score: i32,
    crit: i32,
    near: i32,
    miss: i32,
    gauge: f32,
    gauge_type: i32,
    gauge_opt: i32,
    auto_flags: i32,
    mirror: i32,
    random: i32,
    timestamp: i32,
    replay: String,
    user_name: String,
    user_id: String,
    local_score: i32,
    window_perfect: i32,
    window_good: i32,
    window_hold: i32,
    window_miss: i32,
    window_slam: i32,
    chart_hash: String,
    early: i32,
    late: i32,
    combo: i32,
    rowid: i32,
}
