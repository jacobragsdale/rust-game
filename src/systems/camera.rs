//! Camera that follows the player, clamped to world bounds. With the viewport
//! equal to the world (the endless runner) the offset stays at zero; it earns
//! its keep in Phase 2 when Tiled maps are larger than the screen.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Dead, Player, Position, Size};

pub struct Camera {
    offset: Vec2,
    viewport: Vec2,
    world: Vec2,
}

impl Camera {
    pub fn new(viewport: Vec2, world: Vec2) -> Self {
        Camera {
            offset: Vec2::ZERO,
            viewport,
            world,
        }
    }

    pub fn offset(&self) -> Vec2 {
        self.offset
    }

    pub fn world_height(&self) -> f32 {
        self.world.y
    }

    pub fn reset(&mut self) {
        self.offset = Vec2::ZERO;
    }

    fn follow(&mut self, center: Vec2) {
        let max_offset = (self.world - self.viewport).max(Vec2::ZERO);
        let desired = center - self.viewport / 2.0;
        self.offset = desired.clamp(Vec2::ZERO, max_offset);
    }
}

/// Follow the (living) player.
pub fn follow_player(world: &mut World, camera: &mut Camera) {
    for (_, (pos, size, _, dead)) in world.query_mut::<(&Position, &Size, &Player, Option<&Dead>)>()
    {
        if dead.is_none() {
            camera.follow(pos.0 + size.0 / 2.0);
        }
    }
}
