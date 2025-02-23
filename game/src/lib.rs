mod animation;
mod app;
mod async_service;
mod audio;
mod audio_test;
mod button_codes;
mod companion_interface;
mod config;
mod game;
mod game_data;
mod game_main;
mod help;
mod input_state;
mod lua_http;
mod lua_service;
mod main_menu;
mod results;
mod scene;
mod settings_dialog;
mod settings_screen;
mod shaded_mesh;
mod skin_settings;
mod song_provider;
mod songselect;
mod take_duration_fade;
mod test_scenes;
mod touch;
mod transition;
mod util;
mod vg_ui;
mod window;
mod worker_service;
pub use app::*;
use button_codes::UscInputEvent;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(aapp: AndroidApp) {
    use log::LevelFilter;
    use winit::event_loop::{self, EventLoopBuilder};
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default().with_max_level(LevelFilter::Info));
    app::INSTALL_DIR_OVERRIDE.set(aapp.internal_data_path().expect("No internal data path"));
    app::GAME_DIR_OVERRIDE.set(aapp.internal_data_path().expect("No external data path"));
    let event_loop = winit::event_loop::EventLoop::<UscInputEvent>::with_user_event()
        .with_android_app(aapp)
        .build()
        .unwrap();

    crate::run(event_loop).expect("Game error");
}
