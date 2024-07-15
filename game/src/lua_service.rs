use std::rc::Rc;

use crate::{
    config::GameConfig,
    game_data::{self, ExportGame, LuaPath},
    inox::Inox,
    lua_http::{ExportLuaHttp, LuaHttp},
    util::lua_address,
    vg_ui::{ExportVgfx, Vgfx},
    InnerRuscMixer, LuaArena,
};
use anyhow::Result;
use di::{injectable, Ref, RefMut};
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
    inox: Ref<Inox>,
    context: Ref<three_d::core::Context>,
    mixer: Ref<InnerRuscMixer>,
    game_data: RefMut<game_data::GameData>,
}

impl LuaProvider {
    pub fn new_lua() -> Rc<Lua> {
        Rc::new(Lua::new())
    }

    pub fn register_libraries(&self, lua: Rc<Lua>, script_path: impl AsRef<str>) -> Result<()> {
        //Set path for 'require' (https://stackoverflow.com/questions/4125971/setting-the-global-lua-path-variable-from-c-c?lq=1)
        let mut real_script_path = GameConfig::get().skin_path();
        let arena = self.arena.clone();
        let vgfx = self.vgfx.clone();
        let game_data = self.game_data.clone();

        tealr::mlu::set_global_env(ExportVgfx, &lua)?;
        tealr::mlu::set_global_env(ExportGame, &lua)?;
        tealr::mlu::set_global_env(LuaPath, &lua)?;
        tealr::mlu::set_global_env(ExportLuaHttp, &lua)?;
        lua.globals().set(
            "IRData",
            lua.to_value(&json!({
                "Active": false
            }))?,
        )?;
        lua.globals().set("inox", self.inox.clone())?;

        arena
            .write()
            .expect("Could not get lock to lua arena")
            .0
            .push(lua.clone());

        {
            vgfx.write()
                .expect("Lock error")
                .init_asset_scope(lua_address(&lua))
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
            let package: tealr::mlu::mlua::Table = lua.globals().get("package")?;
            let old_path: String = package.get("path")?;

            let package_path = format!(
                "{0};{1}/scripts/?.lua;{1}/scripts/?",
                old_path,
                real_script_path.as_os_str().to_string_lossy()
            );
            package.set("path", package_path)?;

            lua.globals().set("package", package)?;
        }

        real_script_path.push("scripts");

        real_script_path.push("common.lua");
        if real_script_path.exists() {
            info!("Loading: {:?}", &real_script_path);
            let test_code = std::fs::read_to_string(&real_script_path)?;
            lua.load(&test_code).set_name("common.lua").eval::<()>()?;
        }

        real_script_path.pop();

        real_script_path.push(script_path.as_ref());
        info!("Loading: {:?}", &real_script_path);
        let test_code = std::fs::read_to_string(real_script_path)?;
        {
            profile_scope!("evaluate lua file");
            lua.load(&test_code)
                .set_name(script_path.as_ref())
                .eval::<()>()?;
        }
        Ok(())
    }
}
