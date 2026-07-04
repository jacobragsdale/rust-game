//! Draws the world. Everything is still flat-colored geometry; all shapes are
//! packed into a single mesh per frame (one draw call) — the sprite/atlas
//! renderer replaces this in Phase 2.

use std::f32::consts::PI;

use ggez::glam::Vec2;
use ggez::graphics::{self, Canvas, Color, DrawParam, Mesh, MeshBuilder, Rect};
use ggez::{Context, GameResult};
use hecs::World;

use crate::ecs::components::{Player, Position, Size, Solid, SpikeBall};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub player: Color,
    pub platform: Color,
}

pub fn draw_world(
    ctx: &mut Context,
    canvas: &mut Canvas,
    world: &mut World,
    camera_offset: Vec2,
    palette: Palette,
) -> GameResult {
    let mut mb = MeshBuilder::new();

    for (_, (pos, size, _)) in world.query_mut::<(&Position, &Size, &Solid)>() {
        let rect = Rect::new(
            pos.0.x - camera_offset.x,
            pos.0.y - camera_offset.y,
            size.0.x,
            size.0.y,
        );
        mb.rectangle(graphics::DrawMode::fill(), rect, palette.platform)?;
    }

    for (_, (pos, _)) in world.query_mut::<(&Position, &SpikeBall)>() {
        build_spike_ball(&mut mb, pos.0 - camera_offset)?;
    }

    for (_, (pos, size, _)) in world.query_mut::<(&Position, &Size, &Player)>() {
        let rect = Rect::new(
            pos.0.x - camera_offset.x,
            pos.0.y - camera_offset.y,
            size.0.x,
            size.0.y,
        );
        mb.rectangle(graphics::DrawMode::fill(), rect, palette.player)?;
    }

    let mesh = Mesh::from_data(ctx, mb.build());
    canvas.draw(&mesh, DrawParam::default());
    Ok(())
}

fn build_spike_ball(mb: &mut MeshBuilder, center: Vec2) -> GameResult {
    let color = Color::WHITE;
    let half_base = PI / SpikeBall::NUM_SPIKES as f32;

    for i in 0..SpikeBall::NUM_SPIKES {
        let angle = 2.0 * PI * i as f32 / SpikeBall::NUM_SPIKES as f32;
        let tip = center + Vec2::from_angle(angle) * SpikeBall::TOTAL_RADIUS;
        let base1 = center + Vec2::from_angle(angle - half_base) * SpikeBall::BODY_RADIUS;
        let base2 = center + Vec2::from_angle(angle + half_base) * SpikeBall::BODY_RADIUS;
        mb.polygon(graphics::DrawMode::fill(), &[tip, base1, base2], color)?;
    }
    mb.circle(
        graphics::DrawMode::fill(),
        center,
        SpikeBall::BODY_RADIUS,
        0.5,
        color,
    )?;
    Ok(())
}
