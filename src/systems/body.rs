//! Moving physical bodies through the world: gravity, integration, and
//! collision against the level, for every entity that has a [`Body`].
//!
//! This is the half of movement that has nothing to do with *who* is moving.
//! It used to live inside the avatar's update loop, which meant an NPC could
//! not fall without duplicating it. Now a controller decides a velocity and
//! sets a few knobs on the body; this runs once per tick over all of them.
//!
//! Running as one pass over the world, rather than as a helper each controller
//! calls, buys two things. Bodies with no controller at all — dropped items,
//! projectiles, gibs — still move. And every body moves at the same point in
//! the tick, which is what keeps the simulation reasonable to think about when
//! there are thirty of them, and what moving platforms will need in order to
//! carry their riders.

use hecs::World;

use crate::ecs::components::{Body, Position, Size, Velocity};
use crate::physics::{self, SolidRect};

/// Advance every body one tick: gravity, integrate, resolve, record contact.
pub fn move_bodies(world: &mut World, geometry: &[SolidRect], dt: f32) {
    for (_, (pos, vel, size, body)) in
        world.query_mut::<(&mut Position, &mut Velocity, &Size, &mut Body)>()
    {
        if body.frozen {
            continue;
        }

        // --- gravity, then any tighter cap the controller asked for ---
        vel.0.y = (vel.0.y + body.gravity * dt).min(body.max_fall);
        if let Some(cap) = body.fall_cap {
            vel.0.y = vel.0.y.min(cap);
        }

        // --- integrate ---
        body.prev_pos = pos.0;
        pos.0 += vel.0 * dt;

        // --- collide ---
        let contact = if body.ignore_one_way {
            let solids_only: Vec<SolidRect> =
                geometry.iter().copied().filter(|s| !s.one_way).collect();
            physics::resolve_move(&mut pos.0, &mut vel.0, body.prev_pos, size.0, &solids_only)
        } else {
            physics::resolve_move(&mut pos.0, &mut vel.0, body.prev_pos, size.0, geometry)
        };

        body.landed = contact.grounded && !body.grounded;
        body.grounded = contact.grounded;
        body.on_solid = contact.on_solid;
        body.on_one_way = contact.on_one_way;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Avatar;
    use crate::physics::Aabb;
    use crate::sim::TICK;
    use ggez::glam::Vec2;

    const SIZE: Vec2 = Vec2::new(20.0, 34.0);

    fn floor() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(0.0, 400.0, 400.0, 32.0))]
    }

    /// Spawn a bare body — no `Avatar`, nothing player-specific. If this works,
    /// an NPC works.
    fn spawn_body(world: &mut World, pos: Vec2) -> hecs::Entity {
        world.spawn((
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Body::new(pos, Avatar::GRAVITY, Avatar::MAX_FALL),
        ))
    }

    fn body_of(world: &World, entity: hecs::Entity) -> Body {
        *world.get::<&Body>(entity).unwrap()
    }

    #[test]
    fn a_body_with_no_controller_falls_and_lands() {
        let mut world = World::new();
        let entity = spawn_body(&mut world, Vec2::new(100.0, 200.0));
        let geo = floor();

        for _ in 0..120 {
            move_bodies(&mut world, &geo, TICK);
            if body_of(&world, entity).grounded {
                break;
            }
        }

        let body = body_of(&world, entity);
        assert!(body.grounded, "never reached the floor");
        assert!(body.on_solid && !body.on_one_way);
        assert_eq!(
            world.get::<&Position>(entity).unwrap().0.y,
            400.0 - SIZE.y,
            "came to rest on top of the floor"
        );
    }

    /// `landed` marks the transition, not the state — otherwise a controller
    /// cannot tell touching down from having been there all along.
    #[test]
    fn landed_is_true_for_exactly_one_tick() {
        let mut world = World::new();
        let entity = spawn_body(&mut world, Vec2::new(100.0, 380.0));
        let geo = floor();

        let mut landed_ticks = 0;
        for _ in 0..60 {
            move_bodies(&mut world, &geo, TICK);
            if body_of(&world, entity).landed {
                landed_ticks += 1;
            }
        }

        assert_eq!(landed_ticks, 1, "one landing, not one per grounded tick");
        assert!(body_of(&world, entity).grounded, "and it stayed grounded");
    }

    #[test]
    fn a_frozen_body_does_not_move_or_accumulate_gravity() {
        let mut world = World::new();
        let entity = spawn_body(&mut world, Vec2::new(100.0, 100.0));
        world.get::<&mut Body>(entity).unwrap().frozen = true;
        let geo = floor();

        for _ in 0..60 {
            move_bodies(&mut world, &geo, TICK);
        }

        assert_eq!(world.get::<&Position>(entity).unwrap().0.y, 100.0);
        assert_eq!(
            world.get::<&Velocity>(entity).unwrap().0.y,
            0.0,
            "gravity must not accumulate while frozen, or unfreezing teleports"
        );
    }

    #[test]
    fn fall_cap_limits_speed_below_terminal_velocity() {
        let mut world = World::new();
        let entity = spawn_body(&mut world, Vec2::new(100.0, 0.0));
        world.get::<&mut Body>(entity).unwrap().fall_cap = Some(70.0);

        for _ in 0..60 {
            move_bodies(&mut world, &[], TICK);
            assert!(world.get::<&Velocity>(entity).unwrap().0.y <= 70.0);
        }
    }

    #[test]
    fn ignore_one_way_falls_through_platforms_but_not_solids() {
        let mut world = World::new();
        let entity = spawn_body(&mut world, Vec2::new(100.0, 300.0));
        world.get::<&mut Body>(entity).unwrap().ignore_one_way = true;

        let mut geo = floor();
        geo.push(SolidRect::one_way(Aabb::new(0.0, 350.0, 400.0, 8.0)));

        for _ in 0..120 {
            move_bodies(&mut world, &geo, TICK);
        }

        let pos = world.get::<&Position>(entity).unwrap().0;
        assert_eq!(
            pos.y,
            400.0 - SIZE.y,
            "through the platform, onto the floor"
        );
    }

    /// Every body moves in one pass, so a world full of them stays coherent.
    #[test]
    fn many_bodies_all_advance_in_one_pass() {
        let mut world = World::new();
        let geo = floor();
        let entities: Vec<hecs::Entity> = (0..8)
            .map(|i| spawn_body(&mut world, Vec2::new(i as f32 * 30.0, 100.0)))
            .collect();

        for _ in 0..120 {
            move_bodies(&mut world, &geo, TICK);
        }

        for entity in entities {
            assert!(body_of(&world, entity).grounded, "every body landed");
        }
    }
}
