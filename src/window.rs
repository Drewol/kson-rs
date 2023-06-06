use std::num::NonZeroU32;

use femtovg::{renderer::OpenGl, Canvas};
use game_loop::winit::{
    self,
    event_loop::{EventLoop, EventLoopBuilder},
    window::WindowBuilder,
};
use glow::Context;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;

use crate::button_codes::UscInputEvent;

/// Mostly borrowed code from femtovg/examples
pub fn create_window() -> (
    winit::window::Window,
    glutin::surface::Surface<WindowSurface>,
    Canvas<OpenGl>,
    Context,
    EventLoop<UscInputEvent>,
    PossiblyCurrentContext,
) {
    let event_loop = EventLoopBuilder::<UscInputEvent>::with_user_event().build();

    let window_builder = WindowBuilder::new()
        .with_inner_size(winit::dpi::PhysicalSize::new(1280, 720))
        .with_resizable(true)
        .with_title("Test");

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_multisampling(4);

    let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));

    let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
            // Find the config with the maximum number of samples, so our triangle will
            // be smooth.
            configs
                .reduce(|accum, config| {
                    let transparency_check = config.supports_transparency().unwrap_or(false)
                        & !accum.supports_transparency().unwrap_or(false);

                    if transparency_check || config.num_samples() < accum.num_samples() {
                        config
                    } else {
                        accum
                    }
                })
                .unwrap()
        })
        .unwrap();

    let window = window.unwrap();

    let raw_window_handle = Some(window.raw_window_handle());

    let gl_display = gl_config.display();

    let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);
    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(glutin::context::Version {
            major: 3,
            minor: 3,
        })))
        .build(raw_window_handle);
    let mut not_current_gl_context = Some(unsafe {
        gl_display
            .create_context(&gl_config, &context_attributes)
            .unwrap_or_else(|_| {
                gl_display
                    .create_context(&gl_config, &fallback_context_attributes)
                    .expect("failed to create context")
            })
    });

    let (width, height): (u32, u32) = window.inner_size().into();
    let raw_window_handle = window.raw_window_handle();
    let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(width).unwrap(),
        NonZeroU32::new(height).unwrap(),
    );

    let surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &attrs)
            .unwrap()
    };

    let gl_context = not_current_gl_context
        .take()
        .unwrap()
        .make_current(&surface)
        .unwrap();

    let renderer =
        unsafe { OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s) as *const _) }
            .expect("Cannot create renderer");
    let context = unsafe {
        glow::Context::from_loader_function_cstr(|symbol| {
            gl_display.get_proc_address(symbol) as *const _
        })
    };

    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    let scale_factor = window.scale_factor();
    canvas.set_size(width, height, scale_factor as f32);

    (window, surface, canvas, context, event_loop, gl_context)
}
