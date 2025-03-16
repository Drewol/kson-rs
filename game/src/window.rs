use std::num::NonZeroU32;

use anyhow::anyhow;
use femtovg::{renderer::OpenGl, Canvas};
use glow::Context;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use winit::{
    self,
    event_loop::ActiveEventLoop,
    raw_window_handle::HasWindowHandle,
};
use winit::{dpi::PhysicalPosition, monitor::MonitorHandle};

use crate::config::GameConfig;

pub fn find_monitor(
    mut monitors: impl Iterator<Item = MonitorHandle>,
    pos: PhysicalPosition<i32>,
) -> Option<MonitorHandle> {
    monitors.find(|x| x.position() == pos)
}

type WindowCreation = (
    winit::window::Window,
    glutin::surface::Surface<WindowSurface>,
    Canvas<OpenGl>,
    Context,
    PossiblyCurrentContext,
);

/// Mostly borrowed code from femtovg/examples
pub fn create_window(event_loop: &ActiveEventLoop) -> anyhow::Result<WindowCreation> {
    let settings = &GameConfig::get().graphics;

    let window_builder = winit::window::Window::default_attributes()
        .with_resizable(true)
        .with_title("USC Game");

    #[cfg(not(target_os = "android"))]
    let window_builder = match settings.fullscreen {
        crate::config::Fullscreen::Windowed { pos, size } => {
            window_builder.with_position(pos).with_inner_size(size)
        }
        crate::config::Fullscreen::Borderless { monitor } => {
            window_builder.with_fullscreen(Some(winit::window::Fullscreen::Borderless(
                find_monitor(event_loop.available_monitors(), monitor),
            )))
        }
        crate::config::Fullscreen::Exclusive {
            monitor,
            resolution,
        } => {
            if let Some(mode) =
                find_monitor(event_loop.available_monitors(), monitor).and_then(|monitor| {
                    monitor
                        .video_modes()
                        .filter(|x| x.size() == resolution)
                        .max_by_key(|x| x.refresh_rate_millihertz())
                })
            {
                window_builder.with_fullscreen(Some(winit::window::Fullscreen::Exclusive(mode)))
            } else {
                window_builder
            }
        }
    };

    let template = ConfigTemplateBuilder::new();

    let display_builder = DisplayBuilder::new()
        .with_preference(glutin_winit::ApiPreference::PreferEgl)
        .with_window_attributes(Some(window_builder));

    let (window, gl_config) = display_builder
        .build(event_loop, template, |configs| {
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
                .expect("No config available")
        })
        .map_err(|e| {
            log::error!("{e}");
            anyhow!("Failed to build window")
        })?;

    let window = window.ok_or(anyhow!("No window"))?;

    let gl_display = gl_config.display();
    let raw_window_handle = window.window_handle().ok().map(|x| x.as_raw());
    let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);
    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(Some(glutin::context::Version {
            major: 3,
            minor: 1,
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
    let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle.ok_or(anyhow!("No window handle"))?,
        NonZeroU32::new(width).ok_or(anyhow!("Zero width"))?,
        NonZeroU32::new(height).ok_or(anyhow!("Zero height"))?,
    );

    let surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &attrs)?
    };

    let gl_context = not_current_gl_context
        .take()
        .ok_or(anyhow!("No GL context"))?
        .make_current(&surface)?;

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
    surface.set_swap_interval(
        &gl_context,
        if settings.vsync {
            glutin::surface::SwapInterval::Wait(NonZeroU32::new(1).expect("Bad value"))
        } else {
            glutin::surface::SwapInterval::DontWait
        },
    )?;

    Ok((window, surface, canvas, context, gl_context))
}
