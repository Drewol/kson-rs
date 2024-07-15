use std::sync::{Arc, RwLock};

use di::{transient_factory, RefMut, ServiceCollection};
use tealr::{
    mlu::{
        self,
        mlua::{self, ExternalError, FromLuaMulti, IntoLuaMulti, Lua, Result},
        MaybeSend,
    },
    TealMultiValue, ToTypename,
};

use crate::{game_data::GameData, vg_ui::Vgfx, worker_service::WorkerService};

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

pub trait ToLuaResult<T> {
    fn to_lua(self) -> std::result::Result<T, mlua::Error>;
}

impl<T, E> ToLuaResult<T> for std::result::Result<T, E>
where
    E: ExternalError,
{
    fn to_lua(self) -> std::result::Result<T, mlua::Error> {
        self.map_err(|x| x.into_lua_err())
    }
}

pub trait LuaHelpers {
    fn vgfx(&self) -> mlua::Result<Arc<RwLock<Vgfx>>>;
    fn game_data(&self) -> mlua::Result<Arc<RwLock<GameData>>>;
}

impl LuaHelpers for Lua {
    fn vgfx(&self) -> mlua::Result<Arc<RwLock<Vgfx>>> {
        self.app_data_ref::<Arc<RwLock<Vgfx>>>()
            .map(|x| x.clone())
            .ok_or(mlua::Error::external("VGFX app data not set"))
    }

    fn game_data(&self) -> mlua::Result<Arc<RwLock<GameData>>> {
        self.app_data_ref::<Arc<RwLock<GameData>>>()
            .map(|x| x.clone())
            .ok_or(mlua::Error::external("VGFX app data not set"))
    }
}
