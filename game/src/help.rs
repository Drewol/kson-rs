use std::sync::{Arc, Mutex, RwLock};

use di::{transient_factory, RefMut, ServiceCollection};
use rfd::AsyncFileDialog;
use tealr::{
    mlu::{
        mlua::{self, FromLuaMulti, IntoLuaMulti, Lua, Result},
        MaybeSend,
    },
    TealMultiValue, ToTypename,
};

use crate::worker_service::WorkerService;

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

pub(crate) fn add_lua_static_method<'lua, M, A, R, F, T: 'static + Sized + ToTypename>(
    methods: &mut M,
    name: &'static str,
    mut function: F,
) where
    M: Sized + tealr::mlu::TealDataMethods<'lua, T>,
    A: FromLuaMulti<'lua> + TealMultiValue,
    R: IntoLuaMulti<'lua> + TealMultiValue,
    F: 'static + MaybeSend + FnMut(&'lua Lua, &mut T, A) -> Result<R>,
{
    let scope_id = puffin::ThreadProfiler::call(|f| f.register_function_scope(name, "Lua", 0));

    methods.add_function_mut(name, move |lua, p: A| {
        let _profile_scope = if puffin::are_scopes_on() && !name.ends_with("Profile") {
            Some(puffin::ProfilerScope::new(
                scope_id,
                format!(
                    "{}:{}",
                    lua.inspect_stack(1)
                        .and_then(|s| s.source().source.map(|s| s.to_string()))
                        .unwrap_or_default(),
                    lua.inspect_stack(1).map(|s| s.curr_line()).unwrap_or(-1)
                ),
            ))
        } else {
            None
        };

        let mut maybe_data = { lua.app_data_ref::<RefMut<T>>().map(|x| x.clone()) };
        if let Some(data_rc) = maybe_data.take() {
            let data = data_rc.clone();
            drop(data_rc);
            drop(maybe_data);
            let data_lock = data.try_write();
            match data_lock {
                Ok(mut data) => function(lua, &mut data, p),
                Err(e) => Err(mlua::Error::external(format!("{e}"))),
            }
        } else {
            Err(mlua::Error::external("App data not set"))
        }
    })
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
