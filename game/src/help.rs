use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use di::{transient_factory, ServiceCollection};
use femtovg::rgb::ComponentSlice;
use itertools::Itertools;
use rfd::AsyncFileDialog;
use winit::event::ElementState;

use crate::{
    button_codes::{UscButton, UscInputEvent},
    config::GameConfig,
    vg_ui::Vgfx,
    worker_service::WorkerService,
};

pub async fn await_task<T: Send + 'static>(mut t: poll_promise::Promise<T>) -> T {
    loop {
        t = match t.try_take() {
            Ok(t) => break t,
            Err(t) => {
                tokio::task::yield_now().await;
                t
            }
        };
    }
}

pub fn button_click_event(b: UscButton) -> Vec<UscInputEvent> {
    vec![
        UscInputEvent::Button(
            b,
            ElementState::Pressed,
            SystemTime::now() - Duration::from_millis(10),
        ),
        UscInputEvent::Button(
            b,
            ElementState::Released,
            SystemTime::now() - Duration::from_millis(10),
        ),
    ]
}

pub trait ServiceHelper {
    fn add_worker<T: WorkerService + 'static>(&mut self) -> &mut Self;
}

impl ServiceHelper for ServiceCollection {
    fn add_worker<T: WorkerService + 'static>(&mut self) -> &mut Self {
        self.add(transient_factory::<RwLock<dyn WorkerService>, _>(|sp| {
            sp.get_required_mut::<T>()
        }))
    }
}

pub struct AsyncPicker(rfd::AsyncFileDialog, bool);

#[allow(unused)]
impl AsyncPicker {
    pub fn new() -> Self {
        Self(AsyncFileDialog::new(), false)
    }

    pub fn set_can_create_directories(mut self, can: bool) -> Self {
        Self(self.0.set_can_create_directories(can), self.1)
    }

    pub fn set_directory<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        Self(self.0.set_directory(path), self.1)
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        Self(self.0.set_title(title), self.1)
    }

    pub fn set_file_name(mut self, file_name: impl Into<String>) -> Self {
        Self(self.0.set_file_name(file_name), self.1)
    }

    pub fn folder(mut self) -> Self {
        self.1 = false;
        self
    }
    pub fn file(mut self) -> Self {
        self.1 = true;
        self
    }

    pub fn show<S: egui::widgets::text_edit::TextBuffer>(
        self,
        id: egui::Id,
        s: &mut S,
        ui: &mut egui::Ui,
    ) {
        type Dialog = Arc<Mutex<poll_promise::Promise<Option<rfd::FileHandle>>>>;
        let task = ui
            .data_mut(|x| x.remove_temp::<Option<Dialog>>(id))
            .flatten();
        ui.text_edit_singleline(s);
        if ui
            .add_enabled(task.is_none(), egui::Button::new("..."))
            .clicked()
        {
            ui.data_mut(|x| {
                x.insert_temp::<Option<Dialog>>(
                    id,
                    Some(Arc::new(Mutex::new(poll_promise::Promise::spawn_async(
                        async move {
                            if self.1 {
                                self.0.pick_file().await
                            } else {
                                self.0.pick_folder().await
                            }
                        },
                    )))),
                )
            })
        }

        let completed = if let Some(task) = task.clone() {
            let mut task = task.lock().unwrap();
            match task.poll_mut() {
                std::task::Poll::Ready(x) => {
                    if let Some(f) = x.take() {
                        log::info!("Picked file/folder: {:?}", f.path());
                        s.replace_with(f.path().to_str().unwrap_or(""))
                    }
                    true
                }
                std::task::Poll::Pending => false,
            }
        } else {
            false
        };

        if !completed && task.is_some() {
            ui.data_mut(|x| x.insert_temp(id, task))
        }
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        Self(self.0.add_filter(name, extensions), self.1)
    }
}

pub fn transform_shader(s: String) -> String {
    s.lines()
        .filter(|x| !x.starts_with("#version"))
        .filter(|x| !x.starts_with("#extension"))
        .map(|x| {
            if x.starts_with("layout") {
                let i = x[5..]
                    .find(" in ")
                    .or_else(|| x[5..].find(" out "))
                    .unwrap_or(0);
                x[(i + 5)..].trim()
            } else {
                x
            }
        })
        .join("\n")
}

pub fn take_screenshot(
    vgfx: &Vgfx,
    area: Option<((usize, usize), (usize, usize))>,
) -> anyhow::Result<PathBuf> {
    let img = vgfx
        .canvas
        .try_lock()
        .map_err(|_| anyhow!("Failed to lock vgfx"))?
        .screenshot()?;

    let img = if let Some(((x, y), (w, h))) = area {
        img.sub_image(x, y, w, h).to_owned()
    } else {
        img.as_ref()
    };

    let (buf, width, height) = img.to_contiguous_buf();

    let config = GameConfig::get();
    let mut path = config.game_folder.clone();

    if config.screenshot_path.is_absolute() {
        path = config.screenshot_path.clone();
    } else {
        path.push(&config.screenshot_path);
    }

    std::fs::create_dir_all(&path)?;

    let timestamp = chrono::Local::now();

    path.push(timestamp.format("%Y-%m-%d_%H-%M-%S.png").to_string());

    image::save_buffer(
        &path,
        buf.as_slice(),
        width as _,
        height as _,
        image::ColorType::Rgba8,
    )?;

    Ok(path
        .strip_prefix(&config.game_folder)
        .map(|x| x.to_path_buf())
        .unwrap_or(path))
}

pub fn wait_until(frame_end: SystemTime) {
    let mut now = SystemTime::now();
    if now > frame_end {
        return;
    }
    let ms = Duration::from_millis(1);
    while now < frame_end {
        let wait = frame_end.duration_since(now).unwrap_or(Duration::ZERO);
        if wait > ms {
            std::thread::sleep(wait - ms);
        }
        now = SystemTime::now();
    }
}
