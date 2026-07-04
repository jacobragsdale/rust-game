use crate::entity::Entity;
use ggez::graphics::{self, Canvas, Color};
use ggez::mint::{Point2, Vector2};
use ggez::{Context, GameResult};
use std::f32::consts::PI;

pub struct SpikedBall {
    x: f32,
    y: f32,
    speed: f32,
}

impl SpikedBall {
    const BODY_RADIUS: f32 = 18.0;
    const SPIKE_LENGTH: f32 = 14.0;
    pub const TOTAL_RADIUS: f32 = Self::BODY_RADIUS + Self::SPIKE_LENGTH;
    const NUM_SPIKES: usize = 8;

    pub fn new(x: f32, y: f32, speed: f32) -> Self {
        SpikedBall { x, y, speed }
    }

    pub fn x(&self) -> f32 {
        self.x
    }

    pub fn y(&self) -> f32 {
        self.y
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    pub fn is_offscreen(&self, margin: f32) -> bool {
        self.x + Self::TOTAL_RADIUS < -margin
    }
}

impl Entity for SpikedBall {
    fn update(&mut self, ctx: &mut Context) {
        let delta = ctx.time.delta().as_secs_f32();
        self.x -= self.speed * delta;
    }

    fn draw(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        camera_offset: Vector2<f32>,
    ) -> GameResult {
        let draw_x = self.x - camera_offset.x;
        let draw_y = self.y - camera_offset.y;
        let color = Color::WHITE;
        let half_base = PI / Self::NUM_SPIKES as f32;

        let mut builder = graphics::MeshBuilder::new();

        for i in 0..Self::NUM_SPIKES {
            let angle = 2.0 * PI * i as f32 / Self::NUM_SPIKES as f32;
            let tip = Point2 {
                x: draw_x + (Self::BODY_RADIUS + Self::SPIKE_LENGTH) * angle.cos(),
                y: draw_y + (Self::BODY_RADIUS + Self::SPIKE_LENGTH) * angle.sin(),
            };
            let base1 = Point2 {
                x: draw_x + Self::BODY_RADIUS * (angle - half_base).cos(),
                y: draw_y + Self::BODY_RADIUS * (angle - half_base).sin(),
            };
            let base2 = Point2 {
                x: draw_x + Self::BODY_RADIUS * (angle + half_base).cos(),
                y: draw_y + Self::BODY_RADIUS * (angle + half_base).sin(),
            };
            builder.polygon(graphics::DrawMode::fill(), &[tip, base1, base2], color)?;
        }

        builder.circle(
            graphics::DrawMode::fill(),
            Point2 {
                x: draw_x,
                y: draw_y,
            },
            Self::BODY_RADIUS,
            0.5,
            color,
        )?;

        let mesh = graphics::Mesh::from_data(ctx, builder.build());
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
