use ggez::graphics::Canvas;
use ggez::mint::Vector2;
use ggez::{Context, GameResult};
use std::any::Any;

pub trait Entity: Any {
    fn update(&mut self, ctx: &mut Context);
    fn draw(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        camera_offset: Vector2<f32>,
    ) -> GameResult;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
