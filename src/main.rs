use femtovg as vg;
use td::HasContext;
use three_d as td;

fn main() {
    let window = td::Window::new(td::WindowSettings {
        title: "Triangle!".to_string(),
        max_size: Some((1280, 720)),
        ..Default::default()
    })
    .unwrap();

    let mut input = gilrs::GilrsBuilder::default()
        .build()
        .expect("Failed to create input context");

    // Get the graphics context from the window
    let context = window.gl();
    let renderer = unsafe {
        vg::renderer::OpenGl::new_from_context(
            std::mem::transmute_copy(&**context),
            context.version().is_embedded,
        )
        .expect("awd")
    };
    let mut canvas = vg::Canvas::new(renderer).expect("Failed to create canvas");

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

    window.render_loop(move |frame_input| {
        camera.set_viewport(frame_input.viewport);

        // Set the current transformation of the triangle
        model.set_transformation(td::Mat4::from_angle_y(td::radians(
            (frame_input.accumulated_time * 0.005) as f32,
        )));

        for ele in &frame_input.events {
            match *ele {
                td::Event::MouseMotion {
                    button,
                    delta,
                    position,
                    modifiers,
                    handled,
                } => {
                    (mousex, mousey) = position;
                }
                _ => {}
            }
        }

        while let Some(e) = input.next_event() {
            match e.event {
                gilrs::EventType::ButtonPressed(_, _) => todo!(),
                gilrs::EventType::ButtonRepeated(_, _) => todo!(),
                gilrs::EventType::ButtonReleased(_, _) => todo!(),
                gilrs::EventType::ButtonChanged(_, _, _) => todo!(),
                gilrs::EventType::AxisChanged(_, _, _) => todo!(),
                gilrs::EventType::Connected => todo!(),
                gilrs::EventType::Disconnected => todo!(),
                gilrs::EventType::Dropped => todo!(),
            }
        }

        {
            frame_input
                .screen()
                .clear(td::ClearState::color_and_depth(0.0, 0.0, 0.0, 0.0, 1.0))
                .render(&camera, &[&model], &[]);
        }
        canvas.reset();
        canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);

        draw_fills(&mut canvas, mousex as f32, mousey as f32, 0.0, 0.0);
        canvas.flush();

        td::FrameOutput {
            exit: false,
            swap_buffers: true,
            wait_next_event: false,
        }
    });
}

fn draw_fills<T: vg::Renderer>(
    canvas: &mut vg::Canvas<T>,
    x: f32,
    y: f32,
    mousex: f32,
    mousey: f32,
) {
    use vg::{Color, FillRule, Paint, Path};
    canvas.save();
    canvas.translate(x, y);

    let mut evenodd_fill = Paint::color(Color::rgba(220, 220, 220, 120));
    evenodd_fill.set_fill_rule(FillRule::EvenOdd);

    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    if canvas.contains_point(&mut path, mousex, mousey, FillRule::EvenOdd) {
        evenodd_fill.set_color(Color::rgb(220, 220, 220));
    }

    canvas.fill_path(&mut path, &evenodd_fill);

    canvas.translate(100.0, 0.0);

    let mut nonzero_fill = Paint::color(Color::rgba(220, 220, 220, 120));
    nonzero_fill.set_fill_rule(FillRule::NonZero);

    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    if canvas.contains_point(&mut path, mousex, mousey, FillRule::NonZero) {
        nonzero_fill.set_color(Color::rgb(220, 220, 220));
    }

    canvas.fill_path(&mut path, &nonzero_fill);

    canvas.restore();
}
