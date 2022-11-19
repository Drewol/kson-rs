use std::sync::{Arc, Mutex};

use tealr::{
    mlu::{
        mlua::{self, FromLuaMulti, Lua, Result, ToLuaMulti},
        MaybeSend,
    },
    TealMultiValue, TypeName,
};

pub(crate) fn add_lua_static_method<'lua, M, S, A, R, F, T: 'static + Sized + TypeName>(
    methods: &mut M,
    name: &S,
    mut function: F,
) where
    M: Sized + tealr::mlu::TealDataMethods<'lua, T>,
    S: ?Sized + AsRef<[u8]>,
    A: FromLuaMulti<'lua> + TealMultiValue,
    R: ToLuaMulti<'lua> + TealMultiValue,
    F: 'static + MaybeSend + FnMut(&'lua Lua, &mut T, A) -> Result<R>,
{
    methods.add_function_mut(name, move |lua, p: A| {
        let data = lua.app_data_mut::<Arc<Mutex<T>>>();
        if let Some(data) = data {
            let data_lock = data.lock();
            if let Ok(mut data) = data_lock {
                function(lua, &mut data, p)
            } else {
                Err(mlua::Error::external("App data not set"))
            }
        } else {
            Err(mlua::Error::external("App data not set"))
        }
    })
}
