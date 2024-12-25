mod player;
mod config;
mod entity;

use ggez::{Context, ContextBuilder, GameResult};
use ggez::graphics::{self, Color};
use ggez::event::{self, EventHandler};
use ggez::mint::{Point2, Vector2};
use ggez::winit::event::VirtualKeyCode;
use crate::player::Player;

fn main() {
    let conf = config::Config::new("config.toml");
    // Make a Context.
    let (mut ctx, event_loop) = ContextBuilder::new("jacobs_game", "Jacob Ragsdale")
        .window_setup(ggez::conf::WindowSetup::default().title("Jacob's game"))
        .window_mode(ggez::conf::WindowMode::default().dimensions(1920.0, 1080.0))
        .build().expect("Failed to create game context");

    // Create an instance of your event handler.
    // Usually, you should provide it with the Context object to
    // use when setting your game up.
    let my_game = MyGame::new(&mut ctx);

    // Run!
    event::run(ctx, event_loop, my_game);
}

struct MyGame {
    player: Player
}

impl MyGame {
    pub fn new(_ctx: &mut Context) -> MyGame {
        // Load/create resources such as entities
        MyGame {
            player: Player::new(Point2 { x: 0.0, y: 0.0, }, 200.0, Vector2 { x: 0.0, y: 0.0, })
        }
    }

    fn handle_player_input(&mut self, _ctx: &mut Context) {
        self.player.apply_gravity();
        
        // Jump
        if _ctx.keyboard.is_key_pressed(VirtualKeyCode::Up) && !_ctx.keyboard.is_key_pressed(VirtualKeyCode::Down)  {
            self.player.jump();
        }

        // Move left and right
        if _ctx.keyboard.is_key_pressed(VirtualKeyCode::Left) && !_ctx.keyboard.is_key_pressed(VirtualKeyCode::Right) {
            self.player.move_left();
        }
        else if _ctx.keyboard.is_key_pressed(VirtualKeyCode::Right) && !_ctx.keyboard.is_key_pressed(VirtualKeyCode::Left) {
            self.player.move_right();
        } else {
            self.player.apply_friction();
        }

        // Change colors
        if _ctx.keyboard.is_key_just_pressed(VirtualKeyCode::Space){
            self.player.update_color();
        }
    }
}

impl EventHandler for MyGame {

    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        self.player.update_position(_ctx);
        self.handle_player_input(_ctx);

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);
        self.player.draw(ctx, &mut canvas)?;

        canvas.finish(ctx)
    }
}