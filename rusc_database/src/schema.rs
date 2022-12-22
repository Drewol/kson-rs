// @generated automatically by Diesel CLI.

diesel::table! {
    Challenges (rowid) {
        rowid -> Integer,
        title -> Text,
        charts -> Text,
        chart_meta -> Text,
        clear_mark -> Integer,
        best_score -> Integer,
        req_text -> Text,
        path -> Text,
        hash -> Text,
        level -> Integer,
        lwt -> Integer,
    }
}

diesel::table! {
    Charts (rowid) {
        rowid -> Integer,
        folderid -> Integer,
        title -> Text,
        artist -> Text,
        title_translit -> Text,
        artist_translit -> Text,
        jacket_path -> Text,
        effector -> Text,
        illustrator -> Text,
        diff_name -> Text,
        diff_shortname -> Text,
        path -> Text,
        bpm -> Text,
        diff_index -> Integer,
        level -> Integer,
        preview_offset -> Integer,
        preview_length -> Integer,
        lwt -> Integer,
        hash -> Text,
        preview_file -> Text,
        custom_offset -> Integer,
    }
}

diesel::table! {
    Collections (rowid) {
        collection -> Text,
        folderid -> Integer,
        rowid -> Integer,
    }
}

diesel::table! {
    Folders (rowid) {
        rowid -> Integer,
        path -> Text,
    }
}

diesel::table! {
    PracticeSetups (rowid) {
        chart_id -> Integer,
        setup_title -> Text,
        loop_success -> Integer,
        loop_fail -> Integer,
        range_begin -> Integer,
        range_end -> Integer,
        fail_cond_type -> Integer,
        fail_cond_value -> Integer,
        playback_speed -> Float,
        inc_speed_on_success -> Integer,
        inc_speed -> Float,
        inc_streak -> Integer,
        dec_speed_on_fail -> Integer,
        dec_speed -> Float,
        min_playback_speed -> Float,
        max_rewind -> Integer,
        max_rewind_measure -> Integer,
        rowid -> Integer,
    }
}

diesel::table! {
    Scores (rowid) {
        score -> Integer,
        crit -> Integer,
        near -> Integer,
        miss -> Integer,
        gauge -> Float,
        gauge_type -> Integer,
        gauge_opt -> Integer,
        auto_flags -> Integer,
        mirror -> Integer,
        random -> Integer,
        timestamp -> Integer,
        replay -> Text,
        user_name -> Text,
        user_id -> Text,
        local_score -> Integer,
        window_perfect -> Integer,
        window_good -> Integer,
        window_hold -> Integer,
        window_miss -> Integer,
        window_slam -> Integer,
        chart_hash -> Text,
        early -> Integer,
        late -> Integer,
        combo -> Integer,
        rowid -> Integer,
    }
}

diesel::joinable!(Collections -> Folders (folderid));
diesel::joinable!(PracticeSetups -> Charts (chart_id));

diesel::allow_tables_to_appear_in_same_query!(
    Challenges,
    Charts,
    Collections,
    Folders,
    PracticeSetups,
    Scores,
);
