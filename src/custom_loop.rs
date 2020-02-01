use self::winit_event::*;
use ggez::event::*;
use ggez::*;

use crate::gui::ImGuiWrapper;
use crate::MainState;
/// Modified ggez event loop to forward events to imgui as well

/// Runs the game's main loop, calling event callbacks on the given state
/// object as events occur.
///
/// It does not try to do any type of framerate limiting.  See the
/// documentation for the [`timer`](../timer/index.html) module for more info.
pub fn run(
    ctx: &mut Context,
    events_loop: &mut EventsLoop,
    state: &mut MainState,
    imgui_wrapper: &mut ImGuiWrapper,
) -> GameResult {
    use ggez::input::{keyboard, mouse};

    let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui_wrapper.imgui);

    while ctx.continuing {
        // If you are writing your own event loop, make sure
        // you include `timer_context.tick()` and
        // `ctx.process_event()` calls.  These update ggez's
        // internal state however necessary.
        ctx.timer_context.tick();
        events_loop.poll_events(|event| {
            ctx.process_event(&event);
            platform.handle_event(
                imgui_wrapper.imgui.io_mut(),
                ggez::graphics::window(ctx),
                &event,
            );

            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::Resized(logical_size) => {
                        // let actual_size = logical_size;
                        state.resize_event(
                            ctx,
                            logical_size.width as f32,
                            logical_size.height as f32,
                        );
                    }
                    WindowEvent::CloseRequested => {
                        if !state.quit_event(ctx) {
                            quit(ctx);
                        }
                    }
                    WindowEvent::Focused(gained) => {
                        state.focus_event(ctx, gained);
                    }
                    WindowEvent::ReceivedCharacter(ch) => {
                        if !imgui_wrapper.captures_key() {
                            state.text_input_event(ctx, ch);
                        }
                    }
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(keycode),
                                modifiers,
                                ..
                            },
                        ..
                    } => {
                        if !imgui_wrapper.captures_key() {
                            let repeat = keyboard::is_key_repeated(ctx);
                            state.key_down_event(ctx, keycode, modifiers.into(), repeat);
                        }
                    }
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state: ElementState::Released,
                                virtual_keycode: Some(keycode),
                                modifiers,
                                ..
                            },
                        ..
                    } => {
                        if !imgui_wrapper.captures_key() {
                            state.key_up_event(ctx, keycode, modifiers.into());
                        }
                    }
                    WindowEvent::MouseWheel { delta, .. } => {
                        if !imgui_wrapper.captures_mouse() {
                            let (x, y) = match delta {
                                MouseScrollDelta::LineDelta(x, y) => (x, y),
                                MouseScrollDelta::PixelDelta(winit::dpi::LogicalPosition {
                                    x,
                                    y,
                                }) => (x as f32, y as f32),
                            };
                            state.mouse_wheel_event(ctx, x, y);
                        }
                    }
                    WindowEvent::MouseInput {
                        state: element_state,
                        button,
                        ..
                    } => {
                        if !imgui_wrapper.captures_mouse() {
                            let position = mouse::position(ctx);
                            match element_state {
                                ElementState::Pressed => state
                                    .mouse_button_down_event(ctx, button, position.x, position.y),
                                ElementState::Released => {
                                    state.mouse_button_up_event(ctx, button, position.x, position.y)
                                }
                            }
                        }
                    }
                    WindowEvent::CursorMoved { .. } => {
                        if !imgui_wrapper.captures_mouse() {
                            let position = mouse::position(ctx);
                            let delta = mouse::delta(ctx);
                            state.mouse_motion_event(ctx, position.x, position.y, delta.x, delta.y);
                        }
                    }
                    _x => {
                        // trace!("ignoring window event {:?}", x);
                    }
                },
                Event::DeviceEvent { event, .. } => match event {
                    _ => (),
                },
                Event::Awakened => (),
                Event::Suspended(_) => (),
            }
        });
        // no gamepad
        // Handle gamepad events if necessary.
        // if ctx.conf.modules.gamepad {
        //     while let Some(gilrs::Event { id, event, .. }) = ctx.gamepad_context.next_event() {
        //         match event {
        //             gilrs::EventType::ButtonPressed(button, _) => {
        //                 state.gamepad_button_down_event(ctx, button, GamepadId(id));
        //             }
        //             gilrs::EventType::ButtonReleased(button, _) => {
        //                 state.gamepad_button_up_event(ctx, button, GamepadId(id));
        //             }
        //             gilrs::EventType::AxisChanged(axis, value, _) => {
        //                 state.gamepad_axis_event(ctx, axis, value, GamepadId(id));
        //             }
        //             _ => {}
        //         }
        //     }
        // }
        {
            state.update(ctx)?;
            state.draw(ctx)?;
        }
        imgui_wrapper.render(ctx, state, 1.0);
        ggez::graphics::present(ctx)?;
    }

    Ok(())
}
