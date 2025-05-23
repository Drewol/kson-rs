use std::rc::Rc;

use crate::{
    config::GameConfig,
    game_data::{self, GameDataLua, LuaPath},
    ir::{InternetRanking, InternetRankingLua},
    lua_http::{ExportLuaHttp, LuaHttp},
    util::lua_address,
    vg_ui::{Vgfx, VgfxLua},
    InnerRuscMixer, LuaArena,
};
use anyhow::Result;
use di::{injectable, Ref, RefMut};
use log::info;
use mlua::LuaSerdeExt;
use mlua::{Lua, UserData};
use puffin::profile_scope;
use serde_json::json;

//TODO: Used expanded macro because of wrong dependencies, use macro when fixed
#[injectable]
pub struct LuaProvider {
    arena: RefMut<LuaArena>,
    vgfx: RefMut<Vgfx>,
    context: Ref<three_d::core::Context>,
    mixer: Ref<InnerRuscMixer>,
    game_data: RefMut<game_data::GameData>,
}

pub struct LuaKey(usize);

impl LuaKey {
    pub fn new(l: &Lua) -> Self {
        Self(lua_address(l))
    }

    pub fn key(&self) -> usize {
        self.0
    }
}

pub fn set_global_env<T: UserData + 'static>(
    v: T,
    name: &str,
    l: &Lua,
) -> std::result::Result<(), mlua::Error> {
    l.globals().set(name, v)
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

        set_global_env(VgfxLua, "gfx", &lua)?;
        set_global_env(GameDataLua, "game", &lua)?;
        set_global_env(LuaPath, "path", &lua)?;
        set_global_env(ExportLuaHttp, "http", &lua)?;
        set_global_env(InternetRankingLua, "IRData", &lua)?;
        set_global_env(InternetRankingLua, "IR", &lua)?;

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
            lua.set_app_data(InternetRanking::new());
            lua.set_app_data(LuaKey::new(&lua));
            //lua.gc_stop();
        }

        {
            let package: mlua::Table = lua.globals().get("package")?;
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
