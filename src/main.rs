mod entity;

use ggez::{Context, ContextBuilder, GameResult};
use ggez::graphics::{self, Color};
use ggez::event::{self, EventHandler};
use ggez::mint::{Point2, Vector2};
use crate::entity::Entity;

fn main() {
    // Make a Context.
    let (mut ctx, event_loop) = ContextBuilder::new("my_game", "Cool Game Author")
        .build()
        .expect("aieee, could not create ggez context!");

    // Create an instance of your event handler.
    // Usually, you should provide it with the Context object to
    // use when setting your game up.
    let my_game = MyGame::new(&mut ctx);

    // Run!
    event::run(ctx, event_loop, my_game);
}

struct MyGame {
    // Your state here...
    entity: Entity
}

impl MyGame {
    pub fn new(_ctx: &mut Context) -> MyGame {
        // Load/create resources such as images here.
        MyGame {
            entity: Entity::new(Point2 { x: 0.0, y: 0.0, }, 200.0, Vector2 { x: 2.0, y: 2.0, })
        }
    }

    fn handle_player_input(&mut self, _ctx: &mut Context) {
        let currently_pressed = _ctx.keyboard.pressed_keys();
        if currently_pressed.len() > 0 {
            let x = 1;
        }
        for key in currently_pressed {

        }
    }
}

impl EventHandler for MyGame {
    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        // Update code here...
        self.entity.update_position();
        self.handle_player_input(_ctx);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::WHITE);
        self.entity.draw(&mut canvas).unwrap();

        canvas.finish(ctx)
    }
}