//! NPC controllers. Like the player's, these decide a velocity and hand the
//! rest to [`crate::systems::body`] — which is the whole point of M1: an NPC
//! falls, lands, and collides using exactly the code the player does, so the
//! two can never drift apart.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{
    Attacking, Body, Health, Hostile, Patrol, Position, Size, Stance, Team, Velocity,
};
use crate::physics::{Aabb, SolidRect};
use crate::sim::event::GameEvent;

/// Decide what every NPC does this tick.
///
/// Entities with only a [`Patrol`] walk their route. Entities that also have a
/// [`Hostile`] run the full state machine on top of it: patrol until the player
/// is in front of them, chase, swing when close enough, and walk home when the
/// player gets away.
///
/// Runs in phase 1 alongside `avatar::control`, and like it only sets a
/// velocity — an NPC moves through exactly the same `move_bodies` the player
/// does, which is the point of the M1 split.
pub fn think(world: &mut World, geometry: &[SolidRect], events: &mut Vec<GameEvent>) {
    let target = player_position(world);

    let mut swings: Vec<(hecs::Entity, String)> = Vec::new();

    for (entity, (patrol, hostile, pos, vel, size, body, health, attacking)) in world
        .query::<(
            &mut Patrol,
            Option<&mut Hostile>,
            &Position,
            &mut Velocity,
            &Size,
            &Body,
            &Health,
            &Attacking,
        )>()
        .iter()
    {
        if body.frozen || health.dead() {
            continue;
        }
        // Knocked about: keep flying, no steering. Same rule as the player.
        if health.hitstun > 0 {
            continue;
        }

        let Some(hostile) = hostile else {
            walk(patrol, pos.0, size.0, vel, body, geometry, patrol.speed);
            continue;
        };
        hostile.cooldown = hostile.cooldown.saturating_sub(1);

        // A swing in progress owns the entity until it finishes.
        if attacking.busy() {
            hostile.stance = Stance::Attack;
            vel.0.x = 0.0;
            continue;
        }

        let seen = target.and_then(|t| sighted(pos.0, size.0, patrol.dir, t));
        let reachable = target.and_then(|t| within(pos.0, size.0, t, Hostile::REACH));

        hostile.stance = match hostile.stance {
            Stance::Patrol | Stance::Return if seen.is_some() => Stance::Chase,
            Stance::Patrol => Stance::Patrol,
            // A finished swing drops back to chasing, and re-evaluates from
            // there — otherwise a killed player leaves the knight swinging at
            // the spot they died on.
            Stance::Chase | Stance::Attack => {
                match target.and_then(|t| within(pos.0, size.0, t, Hostile::LOSE)) {
                    Some(_) => Stance::Chase,
                    None => Stance::Return,
                }
            }
            Stance::Return => Stance::Return,
        };

        match hostile.stance {
            Stance::Patrol => walk(patrol, pos.0, size.0, vel, body, geometry, patrol.speed),
            Stance::Chase => {
                if reachable.is_some() && hostile.cooldown == 0 {
                    hostile.stance = Stance::Attack;
                    hostile.cooldown = Hostile::COOLDOWN;
                    vel.0.x = 0.0;
                    swings.push((entity, hostile.attack.clone()));
                } else if let Some(toward) = seen.or(reachable) {
                    // Face and close, but still refuse to walk off a ledge.
                    patrol.dir = toward;
                    let speed = patrol.speed * Hostile::CHASE_MULTIPLIER;
                    if body.grounded && !floor_ahead(pos.0, size.0, toward, geometry) {
                        vel.0.x = 0.0;
                    } else {
                        vel.0.x = toward * speed;
                    }
                } else {
                    vel.0.x = 0.0;
                }
            }
            Stance::Attack => vel.0.x = 0.0,
            Stance::Return => {
                let toward = (hostile.home - pos.0.x).signum();
                if (hostile.home - pos.0.x).abs() <= Hostile::HOME_SLACK {
                    hostile.stance = Stance::Patrol;
                    vel.0.x = 0.0;
                } else {
                    patrol.dir = toward;
                    walk(patrol, pos.0, size.0, vel, body, geometry, patrol.speed);
                }
            }
        }
    }

    for (entity, attack) in swings {
        if let Ok(mut attacking) = world.get::<&mut Attacking>(entity) {
            attacking.start(&attack);
            events.push(GameEvent::Attacked { attack });
        }
    }
}

/// Walk in `patrol.dir`, turning at anything that stops you.
///
/// Two reasons to turn: a wall ahead, or no floor ahead. The second is what
/// keeps a walker on its ledge instead of marching off it — and it has to be a
/// look-ahead probe rather than a reaction to falling, because by the time the
/// body is airborne it is already too late to not have walked off.
fn walk(
    patrol: &mut Patrol,
    pos: Vec2,
    size: Vec2,
    vel: &mut Velocity,
    body: &Body,
    geometry: &[SolidRect],
    speed: f32,
) {
    // Airborne walkers keep their horizontal speed and do not steer: turning
    // mid-air would let one walk off a ledge and immediately scuttle back on.
    if body.grounded
        && (wall_ahead(pos, size, patrol.dir, geometry)
            || !floor_ahead(pos, size, patrol.dir, geometry))
    {
        patrol.dir = -patrol.dir;
    }
    vel.0.x = patrol.dir * speed;
}

/// The player's collider, if there is one to look for.
fn player_position(world: &World) -> Option<(Vec2, Vec2)> {
    world
        .query::<(&Team, &Position, &Size, &Health)>()
        .iter()
        .find(|(_, (team, _, _, health))| **team == Team::Player && !health.dead())
        .map(|(_, (_, pos, size, _))| (pos.0, size.0))
}

/// Is the player in the box in front of this NPC? Returns the direction to
/// them if so.
///
/// In *front* rather than in a radius, so that walking up behind a patrolling
/// knight goes unnoticed — which is the difference between an enemy and a
/// proximity trigger.
fn sighted(pos: Vec2, size: Vec2, facing: f32, target: (Vec2, Vec2)) -> Option<f32> {
    let (their_pos, their_size) = target;
    let dy = (their_pos.y + their_size.y / 2.0) - (pos.y + size.y / 2.0);
    if dy.abs() > Hostile::SIGHT_HEIGHT {
        return None;
    }
    let dx = (their_pos.x + their_size.x / 2.0) - (pos.x + size.x / 2.0);
    if dx.abs() > Hostile::SIGHT || dx.signum() != facing.signum() {
        return None;
    }
    Some(dx.signum())
}

/// Direction to the player if they are within `range` in any direction.
fn within(pos: Vec2, size: Vec2, target: (Vec2, Vec2), range: f32) -> Option<f32> {
    let (their_pos, their_size) = target;
    let dy = (their_pos.y + their_size.y / 2.0) - (pos.y + size.y / 2.0);
    if dy.abs() > Hostile::SIGHT_HEIGHT {
        return None;
    }
    // Gap between the two colliders, not between their centres, so reach does
    // not depend on how wide the bodies happen to be.
    let gap = if their_pos.x > pos.x {
        their_pos.x - (pos.x + size.x)
    } else {
        pos.x - (their_pos.x + their_size.x)
    };
    if gap > range {
        return None;
    }
    let dx = (their_pos.x + their_size.x / 2.0) - (pos.x + size.x / 2.0);
    Some(if dx == 0.0 { 1.0 } else { dx.signum() })
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

    /// A plain walker: paces, and does not care about the player.
    fn spawn(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        world.spawn((
            Patrol::new(dir, SPEED),
            Team::Enemy,
            Health::new(3, Health::ENEMY_IFRAMES),
            Attacking::default(),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
            Body::new(pos, Avatar::GRAVITY, Avatar::MAX_FALL),
        ))
    }

    /// A walker that will come after you.
    fn spawn_hostile(world: &mut World, pos: Vec2, dir: f32) -> hecs::Entity {
        let entity = spawn(world, pos, dir);
        world
            .insert_one(entity, Hostile::new(pos.x, "knight_slash"))
            .unwrap();
        entity
    }

    /// A stand-in player for the AI to notice.
    fn spawn_target(world: &mut World, pos: Vec2) -> hecs::Entity {
        world.spawn((
            Team::Player,
            Health::new(5, Health::PLAYER_IFRAMES),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(SIZE),
        ))
    }

    fn tick(world: &mut World, geo: &[SolidRect]) {
        think(world, geo, &mut Vec::new());
        body::move_bodies(world, geo, TICK);
    }

    fn stance_of(world: &World, e: hecs::Entity) -> Stance {
        world.get::<&Hostile>(e).unwrap().stance
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

    // --- the fight brain -------------------------------------------------

    fn flat_floor() -> Vec<SolidRect> {
        vec![SolidRect::solid(Aabb::new(0.0, 400.0, 600.0, 32.0))]
    }

    /// Sight is a box in front, so a player behind a patrolling knight is not
    /// noticed. Walking up behind something ought to work.
    #[test]
    fn a_player_behind_the_knight_goes_unnoticed() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(300.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(260.0, 400.0 - SIZE.y));

        // one tick, before the patrol can turn it around
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Patrol);
    }

    #[test]
    fn a_player_in_front_is_chased() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(300.0, 400.0 - SIZE.y));

        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Chase);

        let before = world.get::<&Position>(knight).unwrap().0.x;
        for _ in 0..20 {
            tick(&mut world, &geo);
        }
        let after = world.get::<&Position>(knight).unwrap().0.x;
        assert!(after > before, "closed the distance: {before} -> {after}");
    }

    /// A player directly above on a platform is not "in front of" anything.
    #[test]
    fn a_player_far_above_is_not_in_the_sight_box() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(240.0, 400.0 - SIZE.y - 120.0));

        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Patrol);
    }

    #[test]
    fn closing_to_reach_produces_a_swing() {
        let mut world = World::new();
        let geo = flat_floor();
        let knight = spawn_hostile(&mut world, Vec2::new(200.0, 400.0 - SIZE.y), 1.0);
        spawn_target(&mut world, Vec2::new(232.0, 400.0 - SIZE.y));

        let mut events = Vec::new();
        for _ in 0..30 {
            think(&mut world, &geo, &mut events);
            body::move_bodies(&mut world, &geo, TICK);
            if world.get::<&Attacking>(knight).unwrap().busy() {
                break;
            }
        }

        assert!(world.get::<&Attacking>(knight).unwrap().busy(), "swung");
        assert_eq!(stance_of(&world, knight), Stance::Attack);
        assert!(events
            .iter()
            .any(|e| matches!(e, GameEvent::Attacked { .. })));
        assert_eq!(
            world.get::<&Velocity>(knight).unwrap().0.x,
            0.0,
            "committed to the swing rather than walking through it"
        );
    }

    /// Losing the player sends it home, not wherever the chase ended.
    #[test]
    fn losing_the_player_walks_back_home() {
        let mut world = World::new();
        let geo = flat_floor();
        let home = 200.0;
        let knight = spawn_hostile(&mut world, Vec2::new(home, 400.0 - SIZE.y), 1.0);
        let target = spawn_target(&mut world, Vec2::new(300.0, 400.0 - SIZE.y));

        for _ in 0..30 {
            tick(&mut world, &geo);
        }
        assert_eq!(stance_of(&world, knight), Stance::Chase);
        let chased_to = world.get::<&Position>(knight).unwrap().0.x;
        assert!(chased_to > home, "actually left home");

        // the player vanishes over the horizon
        world.get::<&mut Position>(target).unwrap().0.x = 5000.0;
        tick(&mut world, &geo);
        assert_eq!(stance_of(&world, knight), Stance::Return);

        for _ in 0..600 {
            tick(&mut world, &geo);
            if stance_of(&world, knight) == Stance::Patrol {
                break;
            }
        }
        assert_eq!(stance_of(&world, knight), Stance::Patrol, "settled back");
        let back = world.get::<&Position>(knight).unwrap().0.x;
        assert!(
            (back - home).abs() <= Hostile::HOME_SLACK + 2.0,
            "came home to {home}, ended at {back}"
        );
    }

    /// Chasing must not override the ledge check — an enemy that walks off a
    /// cliff after you is a bug, not tenacity.
    #[test]
    fn a_chase_still_stops_at_a_ledge() {
        let mut world = World::new();
        // ledge ends at x=300; the player stands past it in mid-air
        let geo = vec![SolidRect::solid(Aabb::new(100.0, 400.0, 200.0, 32.0))];
        let knight = spawn_hostile(&mut world, Vec2::new(240.0, 400.0 - SIZE.y), 1.0);
        // Far enough past the drop to be out of reach, close enough to be seen.
        spawn_target(&mut world, Vec2::new(390.0, 400.0 - SIZE.y));

        for _ in 0..120 {
            tick(&mut world, &geo);
            assert!(
                world.get::<&Body>(knight).unwrap().grounded,
                "chased off the ledge at x={}",
                world.get::<&Position>(knight).unwrap().0.x
            );
        }
        assert_eq!(stance_of(&world, knight), Stance::Chase, "still wants to");
        assert!(
            world.get::<&Position>(knight).unwrap().0.x + SIZE.x <= 300.0 + 1.0,
            "stopped at the edge rather than teetering over it"
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
