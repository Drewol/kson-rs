use std::rc::Rc;

use log::warn;
use mlua::{Function, Lua, LuaSerdeExt};
use serde::{Deserialize, Serialize};
use winit::{
    event::{DeviceEvent, KeyEvent, WindowEvent},
    keyboard::{Key, NamedKey},
};

use crate::{
    input_state::InputState,
    util::{self, Warn},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub name: String,
    pub exists: bool,
}

impl Collection {
    pub fn new(name: String, exists: bool) -> Self {
        Self { name, exists }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDialog {
    title: String,
    collections: Vec<Collection>,
    is_text_entry: bool,
    new_name: String,
    closing: bool,
    #[serde(skip)]
    rx: std::sync::mpsc::Receiver<MenuOption>,
    #[serde(skip)]
    lua: Rc<Lua>,
    #[serde(skip)]
    save: Option<Box<dyn FnOnce(String, bool) -> ()>>,
    #[serde(skip)]
    input: InputState,
}

impl CollectionDialog {
    pub fn new<T: FnOnce(String, bool) -> () + 'static>(
        collections: Vec<Collection>,
        lua: Rc<Lua>,
        title: String,
        input: InputState,
        save: T,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let new_self = CollectionDialog {
            title,
            collections,
            is_text_entry: false,
            new_name: String::new(),
            closing: false,
            rx,
            lua: lua.clone(),
            save: Some(Box::new(save)),
            input,
        };

        lua.set_app_data(tx);
        new_self.update_lua();

        lua.globals()
            .set("menu", CollectionMenu)
            .expect("Could not set menu global");

        lua.globals()
            .get::<Function>("open")
            .warn("No open function")
            .map(|f| f.call::<()>(()).warn("Error in open function"));

        new_self
    }
    pub fn tick(&mut self) {
        let Ok(v) = self.rx.try_recv() else {
            return;
        };

        match v {
            MenuOption::Confirm(name) => self.save(name),
            MenuOption::Cancel => self.closing = true,
            MenuOption::ChangeState => {
                self.is_text_entry = !self.is_text_entry;
                self.input.set_text_input_active(self.is_text_entry);
            }
        }

        self.update_lua();
    }

    pub fn save(&mut self, name: String) {
        let exists = self.collections.iter().any(|c| c.name == name && c.exists);
        if let Some(save) = self.save.take() {
            save(name, exists)
        }
        self.closing = true;
        self.update_lua();
    }

    /// Return false on fully closed
    pub fn render(&self, dt: f64) -> bool {
        let Ok(render) = self.lua.globals().get::<Function>("render") else {
            return false;
        };

        render
            .call(dt)
            .warn("Collection dialog lua")
            .unwrap_or(false)
    }

    pub fn on_button_pressed(&self, button: crate::button_codes::UscButton) {
        if self.closing {
            return;
        }

        if let Some(button_pressed) = self
            .lua
            .globals()
            .get::<Function>("button_pressed")
            .warn("No button_pressed function")
        {
            button_pressed
                .call::<()>(u8::from(button))
                .warn("Error in button_pressed");
        }
    }

    pub fn advance_selection(&self, advance_steps: i32) {
        if self.closing {
            return;
        }

        if let Some(advance_selection) = self
            .lua
            .globals()
            .get::<Function>("advance_selection")
            .warn("No advance_selection function")
        {
            advance_selection
                .call::<()>(advance_steps)
                .warn("Error in advance_selection");
        }
    }

    pub fn on_input(&mut self, event: &winit::event::Event<crate::button_codes::UscInputEvent>) {
        if !self.is_text_entry || self.closing {
            return;
        }

        match event {
            winit::event::Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: KeyEvent { logical_key, .. },
                        ..
                    },
                ..
            } => {
                if *logical_key == Key::Named(NamedKey::Enter) && !self.new_name.is_empty() {
                    let name = std::mem::take(&mut self.new_name);
                    self.save(name);
                }

                if *logical_key == Key::Named(NamedKey::Escape) {
                    self.closing = true;
                }

                self.update_lua();
            }
            _ => {}
        }

        if util::do_text_event(&mut self.new_name, event) {
            self.update_lua();
        }
    }

    fn update_lua(&self) {
        self.lua
            .globals()
            .set(
                "dialog",
                self.lua
                    .to_value(self)
                    .expect("Failed to convert dialog to lua"),
            )
            .warn("Failed to set dialog global");
    }
}

struct CollectionMenu;

enum MenuOption {
    Confirm(String),
    Cancel,
    ChangeState,
}

#[mlua_bridge::mlua_bridge(rename_funcs = "PascalCase")]
impl CollectionMenu {
    pub fn cancel(tx: &std::sync::mpsc::Sender<MenuOption>) {
        tx.send(MenuOption::Cancel);
    }

    pub fn confirm(name: String, tx: &std::sync::mpsc::Sender<MenuOption>) {
        tx.send(MenuOption::Confirm(name));
    }

    pub fn change_state(tx: &std::sync::mpsc::Sender<MenuOption>) {
        tx.send(MenuOption::ChangeState);
    }
}

impl Drop for CollectionDialog {
    fn drop(&mut self) {
        self.input.set_text_input_active(false);
    }
}
