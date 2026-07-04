//! Camera that follows the player, clamped to world bounds.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Avatar, Position, Size};

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

    fn follow(&mut self, center: Vec2) {
        let max_offset = (self.world - self.viewport).max(Vec2::ZERO);
        let desired = center - self.viewport / 2.0;
        self.offset = desired.clamp(Vec2::ZERO, max_offset);
    }
}

/// Follow the player.
pub fn follow_avatar(world: &mut World, camera: &mut Camera) {
    for (_, (pos, size, _)) in world.query_mut::<(&Position, &Size, &Avatar)>() {
        camera.follow(pos.0 + size.0 / 2.0);
    }
}
