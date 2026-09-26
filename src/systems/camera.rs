//! Camera that follows the player, clamped to world bounds.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Position, Size};

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

    /// Centre on `center`, clamped so the view never shows past the edge of
    /// the world — and, on an axis where the world is smaller than the view,
    /// centred on it instead, so a small room sits in the middle of the
    /// screen rather than pinned to its top-left corner.
    fn follow(&mut self, center: Vec2) {
        let desired = center - self.viewport / 2.0;
        let slack = self.world - self.viewport;
        let axis = |desired: f32, slack: f32| {
            if slack < 0.0 {
                slack / 2.0
            } else {
                desired.clamp(0.0, slack)
            }
        };
        self.offset = Vec2::new(axis(desired.x, slack.x), axis(desired.y, slack.y));
    }
}

/// Follow the player.
pub fn follow_avatar(world: &World, camera: &mut Camera) {
    let Some(player) = crate::systems::avatar::player(world) else {
        return;
    };
    if let (Ok(pos), Ok(size)) = (world.get::<&Position>(player), world.get::<&Size>(player)) {
        camera.follow(pos.0 + size.0 / 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_smaller_than_the_view_is_centred_in_it() {
        let mut camera = Camera::new(Vec2::new(640.0, 360.0), Vec2::new(320.0, 160.0));
        camera.follow(Vec2::new(10.0, 10.0));
        assert_eq!(camera.offset(), Vec2::new(-160.0, -100.0));
    }

    #[test]
    fn a_large_world_clamps_at_its_edges() {
        let mut camera = Camera::new(Vec2::new(640.0, 360.0), Vec2::new(2000.0, 400.0));
        camera.follow(Vec2::new(10.0, 10.0));
        assert_eq!(camera.offset(), Vec2::ZERO);
        camera.follow(Vec2::new(1990.0, 390.0));
        assert_eq!(camera.offset(), Vec2::new(1360.0, 40.0));
        camera.follow(Vec2::new(1000.0, 200.0));
        assert_eq!(camera.offset(), Vec2::new(680.0, 20.0));
    }
}
