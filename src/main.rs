use std::{
    io::Write,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
};

use crate::vg_ui::ExportVgfx;
use femtovg as vg;
use generational_arena::Arena;
use songselect::SongSelect;
use td::egui;
use td::HasContext;
use tealr::mlu::{
    mlua::{Function, Lua},
    UserDataProxy,
};
use three_d as td;

mod game_data;
mod songselect;
mod vg_ui;
fn main() -> anyhow::Result<()> {
    let window = td::Window::new(td::WindowSettings {
        title: "Test".to_string(),
        max_size: Some((1280, 720)),
        ..Default::default()
    })
    .unwrap();

    let mut input = gilrs::GilrsBuilder::default()
        .build()
        .expect("Failed to create input context");

    // Get the graphics context from the window
    let lua_arena: Arena<Lua> = Arena::new();

    let context = window.gl();
    let renderer = unsafe {
        vg::renderer::OpenGl::new_from_context(
            std::mem::transmute_copy(&**context),
            context.version().is_embedded,
        )
        .expect("awd")
    };

    let canvas = Arc::new(Mutex::new(
        vg::Canvas::new(renderer).expect("Failed to create canvas"),
    ));
    let mut vgfx = vg_ui::Vgfx::new(canvas.clone(), std::env::current_dir()?);

    // Create a CPU-side mesh consisting of a single colored triangle
    let positions = vec![
        td::vec3(0.5, -0.5, 0.0),  // bottom right
        td::vec3(-0.5, -0.5, 0.0), // bottom left
        td::vec3(0.0, 0.5, 0.0),   // top
    ];
    let colors = vec![
        td::Color::new(255, 0, 0, 255), // bottom right
        td::Color::new(0, 255, 0, 255), // bottom left
        td::Color::new(0, 0, 255, 255), // top
    ];
    let cpu_mesh = td::CpuMesh {
        positions: td::Positions::F32(positions),
        colors: Some(colors),
        ..Default::default()
    };

    // Construct a model, with a default color material, thereby transferring the mesh data to the GPU
    let mut model = td::Gm::new(
        td::Mesh::new(&context, &cpu_mesh),
        td::ColorMaterial::default(),
    );

    let mut camera = td::Camera::new_perspective(
        window.viewport(),
        td::vec3(0.0, 0.0, 2.0),
        td::vec3(0.0, 0.0, 0.0),
        td::vec3(0.0, 1.0, 0.0),
        td::degrees(45.0),
        0.1,
        10.0,
    );

    let mut mousex = 0.0;
    let mut mousey = 0.0;

    let songs_folder = loop {
        if let Some(f) = rfd::FileDialog::new().pick_folder() {
            break f;
        }
    };

    let songsel = Arc::new(Mutex::new(songselect::SongSelect::new(songs_folder)));

    let typedef_folder = Path::new("types");
    if !typedef_folder.exists() {
        std::fs::create_dir_all(typedef_folder)?;
    }

    let gfx_typedef = tealr::TypeWalker::new()
        .process_type_inline::<vg_ui::Vgfx>()
        .generate_global("gfx")?;

    let game_typedef = tealr::TypeWalker::new()
        .process_type_inline::<game_data::GameData>()
        .generate_global("game")?;

    let songwheel_typedef = tealr::TypeWalker::new()
        .process_type::<songselect::Song>()
        .process_type::<songselect::Difficulty>()
        .process_type_inline::<songselect::SongSelect>()
        .generate_global("songwheel")?;

    let mut typedef_file_path = typedef_folder.to_path_buf();
    typedef_file_path.push("rusc.d.tl");
    let mut typedef_file = std::fs::File::create(typedef_file_path).expect("Failed to create");
    let file_content = format!("{}\n{}\n{}", gfx_typedef, game_typedef, songwheel_typedef)
        .lines()
        .filter(|l| !l.starts_with("return"))
        .collect::<Vec<_>>()
        .join("\n");

    write!(typedef_file, "{}", file_content)?;
    typedef_file.flush()?;
    drop(typedef_file);
    let mut gui = three_d::GUI::new(&context);

    let mut lua = tealr::mlu::mlua::Lua::new();
    tealr::mlu::set_global_env(ExportVgfx::default(), &lua).unwrap();
    lua.globals().set("songwheel", songsel.clone())?;
    lua.globals().set("gfx", vgfx.clone())?;
    lua.globals().set(
        "game",
        game_data::GameData {
            mouse_pos: (0.0, 0.0),
            resolution: (800, 600),
        },
    )?;

    let test_code = std::fs::read_to_string("scripts/songwheel.lua")?;
    lua.load_from_std_lib(tealr::mlu::mlua::StdLib::ALL_SAFE)?;
    if let Err(e) = lua.load(&test_code).set_name("SongWheel")?.eval::<()>() {
        println!("{:?}", e);
    }

    window.render_loop(move |mut frame_input| {
        camera.set_viewport(frame_input.viewport);

        // Set the current transformation of the triangle
        model.set_transformation(td::Mat4::from_angle_y(td::radians(
            (frame_input.accumulated_time * 0.005) as f32,
        )));

        for ele in &frame_input.events {
            if let td::Event::MouseMotion {
                button: _,
                delta: _,
                position,
                modifiers: _,
                handled: _,
            } = *ele
            {
                (mousex, mousey) = position;
            }
        }

        while let Some(e) = input.next_event() {
            match e.event {
                gilrs::EventType::ButtonPressed(_, _) => {}
                gilrs::EventType::ButtonRepeated(_, _) => {}
                gilrs::EventType::ButtonReleased(_, _) => {}
                gilrs::EventType::ButtonChanged(_, _, _) => {}
                gilrs::EventType::AxisChanged(_, _, _) => {}
                gilrs::EventType::Connected => {}
                gilrs::EventType::Disconnected => {}
                gilrs::EventType::Dropped => {}
            }
        }

        {
            frame_input
                .screen()
                .clear(td::ClearState::color_and_depth(0.0, 0.0, 0.0, 0.0, 1.0));
            // .render(&camera, [&model], &[]);
        }
        if let Err(e) = lua.globals().set(
            "game",
            game_data::GameData {
                mouse_pos: (mousex, mousey),
                resolution: (frame_input.viewport.width, frame_input.viewport.height),
            },
        ) {
            println!("{:?}", e);
        }

        {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
                canvas.flush();
            }
        }

        let render: Function = lua.globals().get("render").expect("no render function");

        if let Err(e) = render.call::<_, ()>(frame_input.elapsed_time as f32 / 1000.0) {
            panic!("{:?}", e);
        }

        {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
                canvas.flush();
            }
        }

        {
            let mut songsel_handle = songsel.lock().expect("Songsel busy idk");
            gui.update(
                &mut frame_input.events,
                frame_input.accumulated_time,
                frame_input.viewport,
                frame_input.device_pixel_ratio,
                |gui_context| songsel_handle.debug_ui(gui_context, &lua),
            );

            frame_input.screen().write(|| gui.render());
        }

        td::FrameOutput {
            exit: false,
            swap_buffers: true,
            wait_next_event: false,
        }
    });

    Ok(())
}
