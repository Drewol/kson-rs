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
mod transition;
mod util;
mod vg_ui;
mod window;
mod worker_service;
pub use app::*;
use button_codes::UscInputEvent;

fn main() -> anyhow::Result<()> {
    let eventloop = winit::event_loop::EventLoop::<UscInputEvent>::with_user_event().build()?;
    app::run(eventloop)
}
