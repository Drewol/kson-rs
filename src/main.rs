extern crate ggez;
extern crate nfd;
extern crate imgui;
extern crate serde_json;

use ggez::event;
use ggez::graphics;
use ggez::nalgebra as na;
use ggez::event::MouseButton;
use ggez::{Context, GameResult};
mod chart;

struct MainState {
    redraw : bool,
    pos_x : f32,
    pos_y : f32,
    chart : chart::Chart
}

impl MainState {
    fn new() -> GameResult<MainState> {
        let s = MainState { pos_x: 0.0, pos_y: 0.0, redraw: false, chart: chart::Chart::new() };
        Ok(s)
    }
}

impl event::EventHandler for MainState {
    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        if !self.redraw
        {
            ggez::timer::sleep(std::time::Duration::from_millis(15));
            return GameResult::Ok(());
        }
        let fps = ggez::timer::fps;
        let (w,h) = graphics::size(ctx);
        println!("{}x{}",w,h);
        println!("{} FPS", fps(ctx));
        let t:[f32; 16] = graphics::transform(ctx).into();
        println!("t:");
        for x in t.iter() {
            print!("{},", x);            
        }
        println!("", );
        graphics::clear(ctx, [0.1, 0.2, 0.3, 1.0].into());
        let circle = graphics::Mesh::new_circle(
            ctx,
            graphics::DrawMode::fill(),
            na::Point2::new(0.0, 0.0),
            100.0,
            22.1,
            graphics::WHITE,
        )?;
        graphics::draw(ctx, &circle, (na::Point2::new(self.pos_x, self.pos_y),))?;

        graphics::present(ctx)?;
        self.redraw = false;
        Ok(())
    }
    fn mouse_button_down_event(&mut self, _ctx: &mut Context, _button: MouseButton, _x: f32, _y: f32) {
        self.redraw = true;
        self.pos_x = _x;
        self.pos_y = _y;

        let data = serde_json::to_string_pretty(&self.chart).unwrap();
        println!("Serialized = {}", data);

    }

    fn resize_event(&mut self, ctx: &mut Context, w: f32, h: f32) {
        self.redraw = true;
        graphics::set_screen_coordinates(ctx, graphics::Rect {x:0.0,y:0.0,w:w,h:h});
    }
}

pub fn main() -> GameResult {

    let win_setup = ggez::conf::WindowSetup {
        title: "USC Editor".to_owned(),
        samples: ggez::conf::NumSamples::Zero,
        vsync: true,
        icon: "".to_owned(),
        srgb: true,
    };

    let mode = ggez::conf::WindowMode {
        width: 800.0,
        height: 600.0,
        maximized: false,
        fullscreen_type: ggez::conf::FullscreenType::Windowed,
        borderless: false,
        min_width: 0.0,
        max_width: 0.0,
        min_height: 0.0,
        max_height: 0.0,
        resizable: true,
    };

    let cb = ggez::ContextBuilder::new("usc-editor", "Drewol").window_setup(win_setup).window_mode(mode);

    let (ctx, event_loop) = &mut cb.build()?;

    let state = &mut MainState::new()?;
    event::run(ctx, event_loop, state)
}