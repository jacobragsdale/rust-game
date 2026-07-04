use crate::entity::Entity;
use ggez::graphics::{self, Canvas, Color};
use ggez::mint::Vector2;
use ggez::{Context, GameResult};

pub struct Platform {
    rect: graphics::Rect,
    color: Color,
    speed: f32,
}

impl Platform {
    pub fn new(x: f32, y: f32, width: f32, height: f32, speed: f32, color: Color) -> Self {
        let rect = graphics::Rect::new(x, y, width, height);

        Platform { rect, color, speed }
    }

    pub fn rect(&self) -> graphics::Rect {
        self.rect
    }

    pub fn right_edge(&self) -> f32 {
        self.rect.x + self.rect.w
    }

    pub fn is_offscreen(&self, margin: f32) -> bool {
        self.right_edge() < -margin
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }
}

impl Entity for Platform {
    fn update(&mut self, ctx: &mut Context) {
        let delta = ctx.time.delta().as_secs_f32();
        self.rect.x -= self.speed * delta;
    }

    fn draw(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        camera_offset: Vector2<f32>,
    ) -> GameResult {
        let draw_rect = graphics::Rect::new(
            self.rect.x - camera_offset.x,
            self.rect.y - camera_offset.y,
            self.rect.w,
            self.rect.h,
        );
        let mesh =
            graphics::Mesh::new_rectangle(ctx, graphics::DrawMode::fill(), draw_rect, self.color)?;

        canvas.draw(&mesh, graphics::DrawParam::default());
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
