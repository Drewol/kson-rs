use crate::game::HitWindow;

use super::GameConfig;

pub fn migrate_config() {
    let mut conf = GameConfig::get_mut();
    float_hit_windows(&mut conf);
}

pub fn float_hit_windows(conf: &mut GameConfig) {
    // When config moved from float millis to int nanos
    // check if windows are unreasonably small and set to normal
    if conf.hit_window.perfect.as_millis() < 2 {
        conf.hit_window = HitWindow::NORMAL
    }
}
