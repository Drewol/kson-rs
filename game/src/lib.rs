include!("main.rs");

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(aapp: AndroidApp) {
    use log::LevelFilter;
    use winit::event_loop::{self, EventLoopBuilder};
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default().with_max_level(LevelFilter::Info));
    INSTALL_DIR_OVERRIDE.set(aapp.internal_data_path().expect("No internal data path"));
    GAME_DIR_OVERRIDE.set(aapp.internal_data_path().expect("No external data path"));
    let event_loop = winit::event_loop::EventLoop::<UscInputEvent>::with_user_event()
        .with_android_app(aapp)
        .build()
        .unwrap();

    run(event_loop).expect("Game error");
}
