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

use crate::ecs::components::{Body, Collider, Position, Size, Velocity};
use crate::physics::{self, Geometry, SolidQuery, SolidRect, SolidsOnly};

/// Refresh the entity-owned half of the world's geometry.
///
/// Called from one place, [`crate::sim::Sim::step`], immediately before
/// [`move_bodies`]: every collider is read at the position its owner decided
/// on this tick, and the geometry then holds still for the whole movement
/// phase. Fire that blinks on a schedule, a platform that moves, a hazard on a
/// rope — all of them are "an entity moved its collider" and all of them land
/// here rather than in a special case inside collision.
///
/// Sorted by entity id, not taken in query order. hecs iterates archetypes in
/// creation order, so adding a component to one entity mid-run would otherwise
/// reshuffle the list that `resolve_move` walks — the same trap
/// [`crate::sim::Sim::npcs`] guards against, with the same fix.
pub fn rebuild_geometry(geometry: &mut Geometry, world: &World) {
    let mut owned: Vec<(u32, SolidRect)> = world
        .query::<(&Position, &Collider)>()
        .iter()
        .filter_map(|(entity, (pos, collider))| Some((entity.id(), collider.solid_rect(pos.0)?)))
        .collect();
    owned.sort_by_key(|(id, _)| *id);
    geometry.set_entity_rects(owned.into_iter().map(|(_, rect)| rect));
}

/// Advance every body one tick: gravity, integrate, resolve, record contact.
pub fn move_bodies<Q: SolidQuery + ?Sized>(world: &mut World, geometry: &Q, dt: f32) {
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
            physics::resolve_move(
                &mut pos.0,
                &mut vel.0,
                body.prev_pos,
                size.0,
                &SolidsOnly(geometry),
            )
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

    // --- entity-owned geometry ------------------------------------------

    /// The rebuild reads a collider at its owner's position, so a collider
    /// that moves is geometry that moves — no special case anywhere in
    /// collision, which is the whole point of the ticket.
    #[test]
    fn a_collider_follows_its_owner() {
        let mut world = World::new();
        let platform = world.spawn((
            Position(Vec2::new(100.0, 200.0)),
            Collider::solid(Vec2::new(64.0, 16.0)),
        ));
        let mut geometry = Geometry::default();

        rebuild_geometry(&mut geometry, &world);
        assert_eq!(
            geometry.rects()[0].rect,
            Aabb::new(100.0, 200.0, 64.0, 16.0)
        );

        world.get::<&mut Position>(platform).unwrap().0.x = 180.0;
        rebuild_geometry(&mut geometry, &world);
        assert_eq!(
            geometry.rects()[0].rect,
            Aabb::new(180.0, 200.0, 64.0, 16.0)
        );
    }

    /// Entity-owned rects arrive in entity-id order however hecs feels like
    /// iterating. Adding a component moves an entity to another archetype,
    /// which is exactly what reshuffles a raw query.
    #[test]
    fn entity_rects_are_assembled_in_entity_id_order() {
        #[derive(Clone, Copy, Debug)]
        struct Stunned;

        let mut world = World::new();
        let ids: Vec<hecs::Entity> = (0..6)
            .map(|i| {
                world.spawn((
                    Position(Vec2::new(i as f32 * 40.0, 0.0)),
                    Collider::solid(Vec2::new(8.0, 8.0)),
                ))
            })
            .collect();
        let mut geometry = Geometry::default();

        rebuild_geometry(&mut geometry, &world);
        let xs: Vec<f32> = geometry.rects().iter().map(|s| s.rect.x).collect();
        assert_eq!(xs, vec![0.0, 40.0, 80.0, 120.0, 160.0, 200.0]);

        // Shuffle the archetypes underneath it.
        world.insert_one(ids[3], Stunned).unwrap();
        world.insert_one(ids[0], Stunned).unwrap();
        world.remove_one::<Stunned>(ids[3]).unwrap();

        rebuild_geometry(&mut geometry, &world);
        let xs: Vec<f32> = geometry.rects().iter().map(|s| s.rect.x).collect();
        assert_eq!(
            xs,
            vec![0.0, 40.0, 80.0, 120.0, 160.0, 200.0],
            "id order held"
        );
    }

    /// A hazard is geometry, but it is not an obstruction: you walk into fire
    /// rather than bumping off it. L-2 gives it a query of its own.
    #[test]
    fn a_hazard_collider_contributes_nothing_to_collision() {
        let mut world = World::new();
        world.spawn((
            Position(Vec2::new(0.0, 0.0)),
            Collider::hazard(Vec2::new(32.0, 32.0)),
        ));
        world.spawn((
            Position(Vec2::new(64.0, 0.0)),
            Collider::one_way(Vec2::new(32.0, 8.0)),
        ));
        let mut geometry = Geometry::default();

        rebuild_geometry(&mut geometry, &world);
        assert_eq!(geometry.entity_rect_count(), 1, "only the platform");
        assert!(geometry.rects()[0].one_way);
    }

    /// A body lands on an entity's collider exactly as it lands on a tile, and
    /// falls again the moment the entity is gone.
    #[test]
    fn a_body_lands_on_an_entity_collider_and_falls_when_it_goes_away() {
        let mut world = World::new();
        let body = spawn_body(&mut world, Vec2::new(100.0, 100.0));
        let ledge = world.spawn((
            Position(Vec2::new(80.0, 300.0)),
            Collider::solid(Vec2::new(96.0, 16.0)),
        ));
        let mut geometry = Geometry::from_level(&[Aabb::new(0.0, 400.0, 400.0, 32.0)], &[]);

        for _ in 0..120 {
            rebuild_geometry(&mut geometry, &world);
            move_bodies(&mut world, &geometry, TICK);
            if body_of(&world, body).grounded {
                break;
            }
        }
        assert_eq!(
            world.get::<&Position>(body).unwrap().0.y,
            300.0 - SIZE.y,
            "came to rest on the entity's collider, not the floor"
        );

        world.despawn(ledge).unwrap();
        for _ in 0..120 {
            rebuild_geometry(&mut geometry, &world);
            move_bodies(&mut world, &geometry, TICK);
        }
        assert_eq!(
            world.get::<&Position>(body).unwrap().0.y,
            400.0 - SIZE.y,
            "dropped to the floor once the collider stopped existing"
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
