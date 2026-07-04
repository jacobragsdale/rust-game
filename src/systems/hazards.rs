//! Detects lethal conditions (spike balls, falling off the world) and raises
//! a `player_died` event. Killing the player is the level scene's job.

use hecs::World;

use crate::ecs::components::{Dead, Player, Position, Size, SpikeBall};
use crate::ecs::events::Events;
use crate::physics::{circle_intersects_aabb, Aabb};

pub fn check(world: &mut World, events: &mut Events, world_height: f32) {
    let spike_centers: Vec<ggez::glam::Vec2> = world
        .query_mut::<(&Position, &SpikeBall)>()
        .into_iter()
        .map(|(_, (pos, _))| pos.0)
        .collect();

    for (_, (pos, size, _, dead)) in world.query_mut::<(&Position, &Size, &Player, Option<&Dead>)>()
    {
        if dead.is_some() {
            continue;
        }

        let rect = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        let spiked = spike_centers
            .iter()
            .any(|&c| circle_intersects_aabb(c, SpikeBall::TOTAL_RADIUS, rect));
        let fell = rect.bottom() > world_height;

        if spiked || fell {
            events.player_died = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Velocity;
    use ggez::glam::Vec2;

    fn world_with_player(y: f32) -> World {
        let mut world = World::new();
        let pos = Vec2::new(100.0, y);
        world.spawn((
            Player::new(pos),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(Vec2::splat(Player::SIZE)),
        ));
        world
    }

    #[test]
    fn falling_off_the_world_kills() {
        let mut world = world_with_player(1100.0);
        let mut events = Events::default();
        check(&mut world, &mut events, 1060.0);
        assert!(events.player_died);
    }

    #[test]
    fn touching_a_spike_ball_kills() {
        let mut world = world_with_player(500.0);
        world.spawn((Position(Vec2::new(210.0, 550.0)), SpikeBall));
        let mut events = Events::default();
        check(&mut world, &mut events, 2000.0);
        assert!(events.player_died);
    }

    #[test]
    fn safe_when_clear_of_hazards() {
        let mut world = world_with_player(500.0);
        world.spawn((Position(Vec2::new(500.0, 500.0)), SpikeBall));
        let mut events = Events::default();
        check(&mut world, &mut events, 2000.0);
        assert!(!events.player_died);
    }
}
