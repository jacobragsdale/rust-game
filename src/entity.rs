use ggez::{Context, GameResult};
use ggez::graphics::Canvas;

pub trait Entity {
    fn update(&mut self, ctx: &mut Context);
    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult;
}