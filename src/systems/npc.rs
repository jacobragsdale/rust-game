//! NPC controllers. Like the player's, these decide a velocity and hand the
//! rest to [`crate::systems::body`] — which is the whole point of M1: an NPC
//! falls, lands, and collides using exactly the code the player does, so the
//! two can never drift apart.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Body, Patrol, Position, Size, Velocity};
use crate::physics::{Aabb, SolidRect};

/// Walk, and turn around at anything that stops you.
///
/// Two reasons to turn: a wall ahead, or no floor ahead. The second is what
/// keeps a patroller on its ledge instead of marching off it — and it has to
/// be a look-ahead probe rather than a reaction to falling, because by the time
/// the body is airborne it is already too late to not have walked off.
pub fn patrol(world: &mut World, geometry: &[SolidRect]) {
    for (_, (patrol, pos, vel, size, body)) in
        world.query_mut::<(&mut Patrol, &Position, &mut Velocity, &Size, &Body)>()
    {
        if body.frozen {
            continue;
        }

        // Airborne patrollers keep their horizontal speed and do not steer:
        // turning mid-air would let one walk off a ledge and immediately
        // scuttle back onto it.
        if body.grounded
            && (wall_ahead(pos.0, size.0, patrol.dir, geometry)
                || !floor_ahead(pos.0, size.0, patrol.dir, geometry))
        {
            patrol.dir = -patrol.dir;
        }

        vel.0.x = patrol.dir * patrol.speed;
    }
}

/// Is there a solid directly in front of the body, at body height?
fn wall_ahead(pos: Vec2, size: Vec2, dir: f32, geometry: &[SolidRect]) -> bool {
    let x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - Patrol::LOOKAHEAD
    };
    // Inset vertically so the floor being stood on is not read as a wall.
    let probe = Aabb::new(x, pos.y + 2.0, Patrol::LOOKAHEAD, size.y - 4.0);
    geometry
        .iter()
        .any(|s| !s.one_way && probe.overlaps(&s.rect))
}

/// Is there anything to stand on just beyond the leading edge?
fn floor_ahead(pos: Vec2, size: Vec2, dir: f32, geometry: &[SolidRect]) -> bool {
    let x = if dir > 0.0 {
        pos.x + size.x
    } else {
        pos.x - Patrol::LOOKAHEAD
    };
    // One-way platforms count: they hold a body up from above, which is all
    // that matters for deciding whether the next step lands on something.
    let probe = Aabb::new(x, pos.y + size.y, Patrol::LOOKAHEAD, Patrol::FLOOR_PROBE);
    geometry.iter().any(|s| probe.overlaps(&s.rect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Avatar;
    use crate::sim::{Sim, TICK};
    use crate::systems::body;
    use crate::systems::input::PlayerInput;

    const SIZE: Vec2 = Vec2::new(20.0, 30.0);
    const SPEED: f32 = 60.0;

    fn spawn(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        world.spawn((
            Patrol::new(dir, SPEED),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Body::new(pos, Avatar::GRAVITY, Avatar::MAX_FALL),
        ))
    }

    fn tick(world: &mut World, geo: &[SolidRect]) {
        patrol(world, geo);
        body::move_bodies(world, geo, TICK);
    }

    fn x_of(world: &World, e: hecs::Entity) -> f32 {
        world.get::<&Position>(e).unwrap().0.x
    }

    fn dir_of(world: &World, e: hecs::Entity) -> f32 {
        world.get::<&Patrol>(e).unwrap().dir
    }

    /// A ledge with open air at both ends. The patroller must stay on it.
    fn ledge() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(100.0, 400.0, 200.0, 32.0))]
    }

    #[test]
    fn turns_around_at_a_ledge_instead_of_walking_off() {
        let mut world = World::new();
        let geo = ledge();
        let e = spawn(&mut world, Vec2::new(150.0, 400.0 - SIZE.y), 1.0);

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for _ in 0..1200 {
            tick(&mut world, &geo);
            let x = x_of(&world, e);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            assert!(
                world.get::<&Body>(e).unwrap().grounded,
                "walked off the ledge at x={x}"
            );
        }

        assert!(min_x >= 100.0, "went off the left end: {min_x}");
        assert!(max_x + SIZE.x <= 300.0, "went off the right end: {max_x}");
        assert!(
            max_x - min_x > 100.0,
            "barely moved: patrolled {min_x}..{max_x}"
        );
    }

    #[test]
    fn turns_around_at_a_wall() {
        let mut world = World::new();
        let geo = vec![
            SolidRect::solid(Aabb::new(0.0, 400.0, 400.0, 32.0)), // floor
            SolidRect::solid(Aabb::new(250.0, 300.0, 32.0, 100.0)), // wall
        ];
        let e = spawn(&mut world, Vec2::new(150.0, 400.0 - SIZE.y), 1.0);

        for _ in 0..300 {
            tick(&mut world, &geo);
            assert!(
                x_of(&world, e) + SIZE.x <= 250.0 + 0.01,
                "walked into the wall"
            );
        }
        assert_eq!(dir_of(&world, e), -1.0, "turned back from the wall");
    }

    /// The route is decided by geometry, so mirroring the world must mirror
    /// the patrol exactly. Catches a look-ahead probe that is right-biased.
    #[test]
    fn patrolling_is_left_right_symmetric() {
        let run = |dir: f32, mirror: bool| -> Vec<f32> {
            let geo = if mirror {
                vec![SolidRect::solid(Aabb::new(-300.0, 400.0, 200.0, 32.0))]
            } else {
                ledge()
            };
            let start = if mirror {
                Vec2::new(-150.0 - SIZE.x, 400.0 - SIZE.y)
            } else {
                Vec2::new(150.0, 400.0 - SIZE.y)
            };
            let mut world = World::new();
            let e = spawn(&mut world, start, dir);
            (0..600)
                .map(|_| {
                    tick(&mut world, &geo);
                    let x = x_of(&world, e);
                    if mirror {
                        -x - SIZE.x
                    } else {
                        x
                    }
                })
                .collect()
        };

        let normal = run(1.0, false);
        let mirrored = run(-1.0, true);
        for (t, (a, b)) in normal.iter().zip(&mirrored).enumerate() {
            assert!((a - b).abs() < 1e-3, "tick {t}: {a} vs mirrored {b}");
        }
    }

    /// A patroller is just a body with a controller, so it obeys gravity and
    /// lands on whatever is beneath it like anything else.
    #[test]
    fn a_patroller_dropped_in_midair_falls_and_then_patrols() {
        let mut world = World::new();
        let geo = vec![SolidRect::solid(Aabb::new(0.0, 400.0, 400.0, 32.0))];
        let e = spawn(&mut world, Vec2::new(200.0, 100.0), 1.0);

        for _ in 0..180 {
            tick(&mut world, &geo);
        }
        let body = *world.get::<&Body>(e).unwrap();
        assert!(body.grounded, "never landed");
        assert_eq!(
            world.get::<&Position>(e).unwrap().0.y,
            400.0 - SIZE.y,
            "came to rest on the floor"
        );
    }

    /// End to end through the real `Sim`: a `K` in a fixture grid becomes a
    /// knight that walks its platform and stays on it.
    ///
    /// The assertion is "never stops being grounded" rather than an exact x
    /// range. A patroller turns when its leading edge reaches the drop, so it
    /// can overhang by up to one tick of movement — which is not falling off,
    /// and pinning the exact overhang would just encode the walk speed.
    #[test]
    fn a_knight_placed_in_a_map_patrols_its_platform() {
        let mut sim = Sim::fixture(&["..............", "..P.......K...", "..############"]);

        let knight = sim.npcs()[0];
        let start = sim.npc_positions()[0];
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let mut reversals = 0;
        let mut last_dir = sim.world.get::<&Patrol>(knight).unwrap().dir;

        for tick in 0..900 {
            sim.step(PlayerInput::default());

            let pos = sim.npc_positions()[0];
            min_x = min_x.min(pos.x);
            max_x = max_x.max(pos.x);

            assert!(
                sim.world.get::<&Body>(knight).unwrap().grounded,
                "tick {tick}: knight left the platform at x={:.2}, y={:.2}",
                pos.x,
                pos.y
            );

            let dir = sim.world.get::<&Patrol>(knight).unwrap().dir;
            if dir != last_dir {
                reversals += 1;
                last_dir = dir;
            }
        }

        assert!(
            reversals >= 2,
            "should have turned around repeatedly, got {reversals}"
        );
        assert!(max_x - min_x > 50.0, "barely moved: {min_x:.1}..{max_x:.1}");
        assert_eq!(
            sim.npc_positions()[0].y,
            start.y,
            "stayed at platform height throughout"
        );
    }
}
