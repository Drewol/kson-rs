use std::{rc::Rc, sync::RwLock};

use crate::{
    config::GameConfig,
    game_data::{self, ExportGame, LuaPath},
    lua_http::{ExportLuaHttp, LuaHttp},
    util::lua_address,
    vg_ui::{ExportVgfx, Vgfx},
    InnerRuscMixer, LuaArena,
};
use anyhow::Result;
use di::{injectable, Ref, RefMut};
use generational_arena::Index;
use log::info;
use puffin::profile_scope;
use serde_json::json;
use tealr::mlu::mlua::Lua;
use tealr::mlu::mlua::LuaSerdeExt;

//TODO: Used expanded macro because of wrong dependencies, use macro when fixed
#[injectable]
pub struct LuaProvider {
    arena: RefMut<LuaArena>,
    vgfx: RefMut<Vgfx>,
    context: Ref<three_d::core::Context>,
    mixer: Ref<InnerRuscMixer>,
    game_data: RefMut<game_data::GameData>,
    registrerd: Vec<u32>,
}

impl LuaProvider {
    pub fn register_libraries(&self, lua: Rc<Lua>, script_path: impl AsRef<str>) -> Result<Index> {
        //Set path for 'require' (https://stackoverflow.com/questions/4125971/setting-the-global-lua-path-variable-from-c-c?lq=1)
        let mut real_script_path = GameConfig::get().skin_path();
        let arena = self.arena.clone();
        let vgfx = self.vgfx.clone();
        let game_data = self.game_data.clone();

        tealr::mlu::set_global_env(ExportVgfx, &lua)?;
        tealr::mlu::set_global_env(ExportGame, &lua)?;
        tealr::mlu::set_global_env(LuaPath, &lua)?;
        tealr::mlu::set_global_env(ExportLuaHttp, &lua)?;
        lua.globals()
            .set(
                "IRData",
                lua.to_value(&json!({
                    "Active": false
                }))
                .unwrap(),
            )
            .unwrap();
        let idx = arena
            .write()
            .expect("Could not get lock to lua arena")
            .0
            .insert(lua.clone());

        {
            vgfx.write().unwrap().init_asset_scope(lua_address(&lua))
        }

        {
            lua.set_app_data(vgfx.clone());
            lua.set_app_data(game_data.clone());
            lua.set_app_data(self.context.clone());
            lua.set_app_data(self.mixer.clone());
            lua.set_app_data(LuaHttp::default());
            //lua.gc_stop();
        }

        {
            let package: tealr::mlu::mlua::Table = lua.globals().get("package").unwrap();
            let package_path = format!(
                "{0}/scripts/?.lua;{0}/scripts/?",
                real_script_path.as_os_str().to_string_lossy()
            );
            package.set("path", package_path).unwrap();

            lua.globals().set("package", package).unwrap();
        }

        real_script_path.push("scripts");

        real_script_path.push("common.lua");
        if real_script_path.exists() {
            info!("Loading: {:?}", &real_script_path);
            let test_code = std::fs::read_to_string(&real_script_path)?;
            lua.load(&test_code).set_name("common.lua")?.eval::<()>()?;
        }

        real_script_path.pop();

        real_script_path.push(script_path.as_ref());
        info!("Loading: {:?}", &real_script_path);
        let test_code = std::fs::read_to_string(real_script_path)?;
        {
            profile_scope!("evaluate lua file");
            lua.load(&test_code).set_name(script_path)?.eval::<()>()?;
        }
        Ok(idx)
    }
}
