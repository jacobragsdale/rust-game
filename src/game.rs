use crate::player::Player;
use ggez::event::EventHandler;
use ggez::graphics::{self, Color};
use ggez::{Context, GameResult};


pub struct Game {
    player: Player
}

impl Game {
    pub fn new(_ctx: &mut Context) -> Game {
        Game {
            player: Player::new()
        }
    }
}

impl EventHandler for Game {
    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        self.player.update(_ctx);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);
        self.player.draw(ctx, &mut canvas)?;

        canvas.finish(ctx)
    }
}