use ggez::{self, GameResult, graphics};
use ggez::graphics::{Canvas,Color};
use ggez::mint::{Point2, Vector2};

pub(super) struct Entity{
    position: Point2<f32>,
    size: f32,
    entity: graphics::Rect,
    velocity: Vector2<f32>,
}

impl Entity{
    pub fn new(position: Point2<f32>, size: f32, velocity: Vector2<f32>) -> Self{
        Entity{
            position,
            size,
            velocity,
            entity: graphics::Rect::new(position.x, position.y, size, size),
        }
    }

    pub	fn update_position(&mut self){
        self.position.x += self.velocity.x;
        self.position.y += self.velocity.y;
    }

    pub fn draw(&self, canvas: &mut Canvas) -> GameResult<()> {
        canvas.draw(
            &graphics::Quad,
            graphics::DrawParam::new()
                .dest(self.position)
                .scale(self.entity.size())
                .color(Color::BLUE),
        );
        Ok(())
    }
}