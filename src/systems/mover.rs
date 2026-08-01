//! Moving platforms, and the riders they carry.
//!
//! A platform is an entity with a [`Position`], a [`Collider`] and a [`Mover`].
//! Nothing in the physics knows it exists: `rebuild_geometry` folds its
//! collider into the world's geometry exactly as it folds a lit fire's, and
//! `resolve_move` sees a rect. The only thing this file adds on top of "an
//! entity moved its collider" is *rider carry* — standing on a platform has to
//! transport you — which is the part that is genuinely difficult, and the
//! source of most platformer physics bugs.
//!
//! Four decisions are made here. Each was a choice, so each is written down.
//!
//! # 1. Where in the tick this runs
//!
//! [`advance`] is the **last** thing in the decide phase, immediately before
//! [`crate::systems::body::rebuild_geometry`] and therefore immediately before
//! [`crate::systems::body::move_bodies`]. That position is load-bearing in
//! three directions at once:
//!
//! - **After `rebuild_geometry` would be wrong.** Geometry is rebuilt at one
//!   point per tick; a platform that moved after it would present last tick's
//!   rect to this tick's collision, so a rider would sink into it by exactly
//!   the platform's speed and be shoved back out by the depenetration
//!   fallback, once per tick, forever. Moving before the rebuild is what makes
//!   a platform ordinary geometry that happens to be somewhere new.
//! - **Before `move_bodies` is the whole ticket.** Riders are carried here,
//!   then integrate their own velocity in `move_bodies`, then resolve — one
//!   resolution covering both the ride and the rider's own motion. Carrying
//!   after `move_bodies` would mean displacing bodies that had already been
//!   resolved, i.e. putting them inside walls with nothing left to push them
//!   out.
//! - **After `avatar::control` and `npc::think`**, so a controller's decision
//!   this tick is visible here. That is how drop-through works on a moving
//!   one-way platform: `control` sets `Body::ignore_one_way` when down is
//!   pressed, and a body that has decided to fall through a platform is no
//!   longer riding it.
//!
//! # 2. Carry adds to position, never to velocity
//!
//! A rider's position gains the platform's displacement directly; the
//! platform's motion never touches `Velocity`. Adding to velocity instead
//! leaks the platform's momentum into the rider's own integration — gravity
//! and jump arcs are computed from velocity — so jump height would depend on
//! which way the platform happened to be going, and the leak would persist
//! after leaving. Position carry is exact: the rider's displacement equals the
//! platform's, bit for bit, every tick.
//!
//! # 3. Leaving does not inherit the platform's velocity
//!
//! Jumping or stepping off gives the same jump you get from static ground.
//! `vel.y < 0.0` — a body that has just launched — is not a rider, so the tick
//! of the jump carries nothing either: a jump from a platform moving left and
//! a jump from the same platform moving right produce identical arcs, and
//! `jumping_off_is_the_same_jump_whichever_way_the_platform_is_going` in
//! `tests/physics_diagnostics.rs` holds that down.
//!
//! Momentum inheritance is the other defensible answer, and it is the more
//! "physical" one. It was rejected because it makes every jump from a platform
//! a different jump, which is untestable by tape (the arc depends on where in
//! the platform's cycle you pressed) and unlearnable by a player, for a gain
//! that only reads as realism on platforms far faster than these.
//!
//! # 4. Crush reuses the depenetration fallback
//!
//! A rider carried into a wall — or a platform driven into a standing body —
//! ends the carry overlapping a solid with no approach direction to infer,
//! which is precisely the case `resolve_move`'s depenetration pass exists for.
//! It pushes out along the shallowest axis, so a body pressed between a moving
//! platform and a solid is squeezed out rather than tunnelling through or
//! sticking. There is deliberately no second escape hatch here: one way to get
//! un-stuck means one thing to get right.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Body, Collider, ColliderKind, Mover, Position, Size, Velocity};
use crate::level::LevelData;
use crate::physics::Aabb;
use crate::sim::TICKS_PER_SECOND;

/// How far a body's feet may sit from a platform's top and still count as
/// standing on it, in pixels.
///
/// Resolution snaps a landing to `top - height` exactly, so a resting body is
/// flush and this could be zero; a pixel of slack is insurance against float
/// residue in that round trip, and is far below the gap between any two
/// surfaces a map can place.
const RIDE_TOLERANCE: f32 = 1.0;

/// Ticks for one leg of a path walked at `speed` px/s, rounded to whole ticks
/// and never zero.
///
/// The rounding is the price of [`Mover`] being exactly periodic: an authored
/// speed becomes the nearest speed that crosses in a whole number of ticks.
/// At 60 Hz that is at worst half a tick over the whole leg, and it buys a
/// platform whose position at tick *t* is the same forever.
pub fn leg_ticks(from: Vec2, to: Vec2, speed: f32) -> u32 {
    let distance = (to - from).length();
    if distance <= 0.0 || speed <= 0.0 {
        return 0;
    }
    let ticks = (distance / speed) * TICKS_PER_SECOND as f32;
    (ticks.round() as u32).max(1)
}

/// Spawn an entity for every moving platform the map places.
///
/// Called from `Sim::new` after the map's entity list and after the fires, so
/// that adding a platform to a map cannot renumber the NPCs a tape addresses
/// by spawn index.
pub fn spawn_movers(world: &mut World, level: &LevelData) {
    for spawn in &level.movers {
        let mover = Mover::new(
            spawn.from,
            spawn.to,
            leg_ticks(spawn.from, spawn.to, spawn.speed),
            spawn.phase,
        );
        let collider = Collider {
            rect_offset: Vec2::ZERO,
            size: spawn.size,
            kind: if spawn.one_way {
                ColliderKind::OneWay
            } else {
                ColliderKind::Solid
            },
        };
        // Placed where tick 0 says it is, not at `from`: `Sim::new` builds the
        // geometry before the first `step`, and a platform authored with a
        // phase has to start where its phase puts it.
        world.spawn((Position(mover.at(0)), collider, mover));
    }
}

/// Put every platform where `tick` says it is, carrying nothing.
///
/// The other half of [`advance`], for the one caller that must not carry: a
/// [`Sim`](crate::sim::Sim) rebuilt from a save has its platforms where
/// [`spawn_movers`] put them, at tick 0, and a clock that says otherwise. The
/// first `advance` after that reads a delta of the platform's *entire* travel
/// — up to the whole path, in one tick — and hands it to whatever is standing
/// on it, which is a rider teleported most of a tile sideways on the tick after
/// a load and permanently out of step with the run it was saved from.
///
/// So a load places instead of advancing. Nothing is riding anything yet at
/// that point — the world has just been built — and the platform is where the
/// restored tick says it is before a single body is asked to move, which is
/// what makes [`crate::save`]'s claim that restoring the tick restores the
/// platforms true rather than merely intended.
pub fn place(world: &mut World, tick: u64) {
    for (_, (pos, mover)) in world.query_mut::<(&mut Position, &Mover)>() {
        pos.0 = mover.at(tick);
    }
}

/// Move every platform to where this tick says it is, and carry its riders.
///
/// See the module docs for where in the tick this belongs and why. In short:
/// last thing before the geometry is rebuilt, so the collider is current for
/// this tick's collision, and before any body integrates, so a ride and the
/// rider's own motion are resolved together.
///
/// The platform is *placed* rather than nudged — its position comes from
/// [`Mover::at`], a closed form — so the displacement applied here is whatever
/// that placement worked out to. Usually that is one tick's worth. Across
/// hitstop, during which `Sim::step` returns before this runs at all, it is
/// several ticks' worth in one step: the platform snaps to where it should be,
/// exactly as a scheduled fire snaps to lit or unlit, and its riders snap with
/// it rather than being left behind.
pub fn advance(world: &mut World, tick: u64) {
    /// A platform that moved this tick: where its box was *before* it did, and
    /// by how much. The box has to be the old one — riders are still standing
    /// where the start of the tick left them, so that is the box they are
    /// standing on.
    struct Moved {
        /// Owner's entity id, for a stable order. See below.
        id: u32,
        was: Aabb,
        delta: Vec2,
        one_way: bool,
    }

    // Sorted by entity id for the same reason `rebuild_geometry` sorts: hecs
    // iterates archetypes in creation order, and a body straddling two
    // platforms rides the first in this list. Without the sort, which platform
    // that is would depend on which entity last gained a component.
    let mut moved: Vec<Moved> = Vec::new();

    for (entity, (pos, mover, collider)) in world.query_mut::<(&mut Position, &Mover, &Collider)>()
    {
        let next = mover.at(tick);
        let delta = next - pos.0;
        if delta != Vec2::ZERO {
            moved.push(Moved {
                id: entity.id(),
                was: collider.aabb(pos.0),
                delta,
                one_way: collider.kind == ColliderKind::OneWay,
            });
        }
        pos.0 = next;
    }
    if moved.is_empty() {
        return;
    }
    moved.sort_by_key(|m| m.id);

    for (_, (pos, vel, size, body)) in world.query_mut::<(&mut Position, &Velocity, &Size, &Body)>()
    {
        // A frozen body did not move and must not be moved; a body that is not
        // grounded is not standing on anything; a body with upward velocity has
        // jumped and is no longer a passenger (see decision 3 above).
        if body.frozen || !body.grounded || vel.0.y < 0.0 {
            continue;
        }

        let feet = pos.0.y + size.0.y;
        let riding = moved.iter().find(|platform| {
            // Dropping through a one-way platform is not riding it. This is
            // why `advance` runs after the controllers: `ignore_one_way` is
            // this tick's decision.
            if platform.one_way && body.ignore_one_way {
                return false;
            }
            // Feet resting on the platform's top, both measured at the start of
            // the tick. `Body::prev_pos` answers the same question about the
            // tick *before* this one, which is one tick too late — `Position`
            // has not been touched yet, so it is the start-of-tick value here.
            (feet - platform.was.y).abs() <= RIDE_TOLERANCE
                && pos.0.x < platform.was.right()
                && pos.0.x + size.0.x > platform.was.x
        });

        if let Some(platform) = riding {
            pos.0 += platform.delta;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{Collider, Mover};
    use crate::physics::{Aabb, SolidQuery};
    use crate::sim::{Sim, TICK};
    use crate::systems::body;
    use crate::systems::input::{Action, PlayerInput};

    /// The path a mover is *at*, evaluated from the tick and nothing else.
    #[test]
    fn a_path_is_a_closed_form_function_of_the_tick() {
        let mover = Mover::new(Vec2::new(0.0, 100.0), Vec2::new(120.0, 100.0), 60, 0);

        assert_eq!(mover.at(0), Vec2::new(0.0, 100.0));
        assert_eq!(mover.at(30), Vec2::new(60.0, 100.0));
        assert_eq!(mover.at(60), Vec2::new(120.0, 100.0), "far end");
        assert_eq!(mover.at(90), Vec2::new(60.0, 100.0), "on the way back");
        assert_eq!(mover.at(120), Vec2::new(0.0, 100.0), "home again");

        // Periodic forever, with no accumulated error, because nothing
        // accumulates.
        assert_eq!(mover.period(), 120);
        for tick in 0..500u64 {
            assert_eq!(mover.at(tick), mover.at(tick + mover.period() as u64));
        }
        // ...including a very long way out, where an integrated position would
        // have drifted into a different pixel entirely.
        assert_eq!(mover.at(1_000_000 * 120 + 30), Vec2::new(60.0, 100.0));
    }

    /// The turn is a reflection, so the step onto and off the far end are the
    /// same size as every other step. A platform that dwelt a tick at each end
    /// would show up as a rider stuttering twice per cycle.
    #[test]
    fn reversing_keeps_the_step_size_constant() {
        let mover = Mover::new(Vec2::ZERO, Vec2::new(90.0, 0.0), 30, 0);
        let step = 90.0 / 30.0;
        for tick in 0..300u64 {
            let delta = mover.at(tick + 1) - mover.at(tick);
            assert!(
                (delta.x.abs() - step).abs() < 1e-4,
                "tick {tick}: step was {delta:?}, expected +/-{step}"
            );
        }
    }

    #[test]
    fn phase_puts_two_platforms_out_of_step() {
        let a = Mover::new(Vec2::ZERO, Vec2::new(100.0, 0.0), 60, 0);
        let b = Mover::new(Vec2::ZERO, Vec2::new(100.0, 0.0), 60, 60);
        assert_eq!(a.at(0), Vec2::ZERO);
        assert_eq!(b.at(0), Vec2::new(100.0, 0.0), "starts at the far end");
        for tick in 0..240u64 {
            assert_eq!(a.at(tick), b.at(tick + 60));
        }
    }

    /// A degenerate map is a parked platform, not a panic.
    #[test]
    fn a_zero_length_path_is_a_parked_platform() {
        let parked = Mover::new(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0), 0, 0);
        assert_eq!(parked.at(9_999), Vec2::new(5.0, 5.0));
        assert_eq!(leg_ticks(Vec2::ZERO, Vec2::ZERO, 100.0), 0);
        assert_eq!(leg_ticks(Vec2::ZERO, Vec2::new(100.0, 0.0), 0.0), 0);
    }

    #[test]
    fn an_authored_speed_becomes_a_whole_number_of_ticks() {
        // 120px at 120px/s is one second.
        assert_eq!(
            leg_ticks(Vec2::ZERO, Vec2::new(120.0, 0.0), 120.0),
            TICKS_PER_SECOND
        );
        // ...and a speed that does not divide evenly rounds rather than
        // truncating to zero.
        assert_eq!(leg_ticks(Vec2::ZERO, Vec2::new(1.0, 0.0), 10_000.0), 1);
    }

    // --- inside a running Sim -------------------------------------------

    fn platform(sim: &mut Sim, mover: Mover, size: Vec2, one_way: bool) -> hecs::Entity {
        let collider = Collider {
            rect_offset: Vec2::ZERO,
            size,
            kind: if one_way {
                ColliderKind::OneWay
            } else {
                ColliderKind::Solid
            },
        };
        sim.world.spawn((Position(mover.at(0)), collider, mover))
    }

    fn settle(sim: &mut Sim) {
        for _ in 0..20 {
            sim.step(PlayerInput::default());
        }
        assert!(sim.probe().grounded, "player never settled");
    }

    /// The ordering claim in the module docs, as a test: the rect the geometry
    /// holds at the end of a tick is the rect `Mover::at` names for that tick,
    /// not the previous one. Move `mover::advance` after `rebuild_geometry` in
    /// `Sim::step` and this fails.
    #[test]
    fn a_platforms_collider_is_current_within_the_tick_it_moves() {
        let mut sim = Sim::fixture(&["............", "..P.........", "############"]);
        let mover = Mover::new(Vec2::new(200.0, 100.0), Vec2::new(300.0, 100.0), 50, 0);
        platform(&mut sim, mover, Vec2::new(64.0, 16.0), false);

        for _ in 0..80 {
            sim.step(PlayerInput::default());
            let expected = mover.at(sim.tick - 1);
            let mut found = Vec::new();
            sim.geometry
                .overlapping(Aabb::new(expected.x, expected.y, 64.0, 16.0), &mut found);
            assert!(
                found.iter().any(|s| s.rect.x == expected.x),
                "tick {}: geometry has no platform at {expected:?}",
                sim.tick - 1
            );
        }
    }

    /// Where the platform's box is now, straight out of the world.
    fn platform_pos(sim: &Sim, entity: hecs::Entity) -> Vec2 {
        sim.world.get::<&Position>(entity).expect("a platform").0
    }

    /// The headline behaviour, through the real `Sim::step`: standing still on
    /// a horizontally moving platform transports you, and the transport is
    /// *exactly* the platform's — same float, not "about the same".
    #[test]
    fn a_standing_player_is_carried_by_a_horizontal_platform() {
        // Spawn cell is row 2, so its floor — and the top of the platform
        // below — is at y = 96; the player settles straight onto the platform.
        let mut sim = Sim::fixture(&[
            "................",
            "................",
            "..P.............",
            "................",
            "................",
            "################",
        ]);
        let mover = Mover::new(Vec2::new(32.0, 96.0), Vec2::new(288.0, 96.0), 120, 0);
        let entity = platform(&mut sim, mover, Vec2::new(96.0, 16.0), false);

        settle(&mut sim);
        assert_eq!(sim.probe().y, 96.0 - 34.0, "settled onto the platform");

        for _ in 0..100 {
            let (rider, plat) = (sim.probe().x, platform_pos(&sim, entity).x);
            sim.step(PlayerInput::default());
            // Bit-exact, and deliberately written this way round: the rider's
            // new position must be its old one plus the platform's
            // displacement and *nothing else*. Comparing the two measured
            // deltas instead would be a weaker claim that also failed on the
            // last bit, since `a + d` rounds and `(a + d) - a` need not give
            // back `d`.
            assert_eq!(
                sim.probe().x,
                rider + (platform_pos(&sim, entity).x - plat),
                "tick {}: rider and platform parted company",
                sim.tick
            );
            assert!(sim.probe().grounded, "tick {}: went airborne", sim.tick);
        }
        assert!(sim.probe().x > 200.0, "and it went somewhere");
    }

    /// Down on a moving one-way platform still drops you through it. This is
    /// the case that depends on `advance` running after `avatar::control`:
    /// `ignore_one_way` is set by the controller on the tick down is pressed,
    /// and a body dropping through a platform is not riding it.
    #[test]
    fn a_moving_one_way_platform_can_still_be_dropped_through() {
        let mut sim = Sim::fixture(&[
            "................",
            "................",
            "................",
            "..P.............",
            "################",
        ]);
        settle(&mut sim);
        let floor_y = sim.probe().y;

        // A one-way strip two tiles above the floor, drifting right slowly
        // enough to stay under the player for the whole jump.
        let mover = Mover::new(Vec2::new(64.0, 64.0), Vec2::new(192.0, 64.0), 300, 0);
        platform(&mut sim, mover, Vec2::new(96.0, 8.0), true);

        // Jump onto it.
        let mut landed = false;
        for tick in 0..60 {
            let input = if tick == 0 {
                PlayerInput::from_actions(&[Action::Jump])
            } else {
                PlayerInput::holding(&[Action::Jump])
            };
            sim.step(input);
            if sim.probe().on_one_way_only {
                landed = true;
                break;
            }
        }
        assert!(landed, "never landed on the moving platform");

        // ...and it carries the player while they stand on it.
        let before = sim.probe().x;
        sim.step(PlayerInput::default());
        assert!(sim.probe().x > before, "not being carried");

        // Down alone drops through.
        sim.step(PlayerInput::from_actions(&[Action::Down]));
        assert!(!sim.probe().grounded, "down did not drop through");
        for _ in 0..90 {
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.probe().y, floor_y, "landed back on the floor");
    }

    /// A platform driven into a standing body squeezes it out rather than
    /// swallowing it — the depenetration fallback doing its job, with no
    /// second escape hatch here.
    #[test]
    fn a_platform_driven_into_a_body_pushes_it_out_rather_than_engulfing_it() {
        let mut world = World::new();
        let stats = crate::assets::StatTable::shipped().get("player").unwrap();
        let size = stats.size();
        let start = Vec2::new(200.0, 400.0 - size.y);
        world.spawn((
            Position(start),
            Velocity(Vec2::ZERO),
            Size(size),
            Body::new(start, stats.gravity, stats.max_fall),
        ));
        // A tall block sweeping right through where the body stands.
        let mover = Mover::new(Vec2::new(120.0, 340.0), Vec2::new(320.0, 340.0), 100, 0);
        world.spawn((
            Position(mover.at(0)),
            Collider::solid(Vec2::new(48.0, 60.0)),
            mover,
        ));

        let mut geometry =
            crate::physics::Geometry::from_level(&[Aabb::new(0.0, 400.0, 640.0, 32.0)], &[], &[]);
        for tick in 0..200u64 {
            advance(&mut world, tick);
            body::rebuild_geometry(&mut geometry, &world);
            body::move_bodies(&mut world, &geometry, TICK);

            let pos = world
                .query_mut::<&Position>()
                .into_iter()
                .next()
                .unwrap()
                .1
                 .0;
            let block = mover.at(tick);
            let body_box = Aabb::new(pos.x, pos.y, size.x, size.y);
            let block_box = Aabb::new(block.x, block.y, 48.0, 60.0);
            let overlap_x =
                (body_box.right().min(block_box.right()) - body_box.x.max(block_box.x)).max(0.0);
            let overlap_y =
                (body_box.bottom().min(block_box.bottom()) - body_box.y.max(block_box.y)).max(0.0);
            assert!(
                overlap_x.min(overlap_y) < 1.0,
                "tick {tick}: body {body_box:?} is inside the platform {block_box:?}"
            );
        }
    }

    /// A rider is anything with a [`Body`]. Nothing here is player-specific,
    /// so an NPC parked on a platform travels with it too.
    #[test]
    fn a_rider_needs_no_avatar() {
        let mut world = World::new();
        let stats = crate::assets::StatTable::shipped().get("knight").unwrap();
        let size = stats.size();
        let start = Vec2::new(64.0, 200.0 - size.y);
        world.spawn((
            Position(start),
            Velocity(Vec2::ZERO),
            Size(size),
            Body::new(start, stats.gravity, stats.max_fall),
        ));
        let mover = Mover::new(Vec2::new(32.0, 200.0), Vec2::new(232.0, 200.0), 100, 0);
        world.spawn((
            Position(mover.at(0)),
            Collider::solid(Vec2::new(96.0, 16.0)),
            mover,
        ));
        let mut geometry = crate::physics::Geometry::default();

        for tick in 0..60u64 {
            advance(&mut world, tick);
            body::rebuild_geometry(&mut geometry, &world);
            body::move_bodies(&mut world, &geometry, TICK);
        }
        let pos = world
            .query_mut::<&Position>()
            .into_iter()
            .next()
            .unwrap()
            .1
             .0;
        assert!(pos.x > 100.0, "the knight rode along, ending at {pos:?}");
    }
}
