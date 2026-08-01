//! Swinging hazards: the spiked ball on a chain.
//!
//! The third user of entity-owned geometry, and the simplest one since fire.
//! A ball is an entity with a [`Position`], a [`Collider`] of kind
//! [`ColliderKind::Hazard`], and a [`Pendulum`] that says where that position
//! is for a given tick. Nothing in the physics knows it exists:
//! [`crate::systems::body::rebuild_geometry`] folds the collider into the
//! world's hazards exactly as it folds a lit fire's, and the death check in
//! [`crate::systems::avatar::after_move`] asks one question — "is anything
//! lethal overlapping me?" — that a spike, a fire and a wrecking ball all
//! answer the same way. No line of this ticket touches the death path.
//!
//! # Closed form, not integration
//!
//! `theta(t) = amplitude * cos(2*pi * (t + phase) / period)`, and the ball
//! centre is `anchor + length * (sin theta, cos theta)`. That is the linearised
//! pendulum — simple harmonic motion in the angle — evaluated at `t`, never
//! stepped toward it.
//!
//! A stepped pendulum would be worse here than a stepped platform, and a
//! stepped platform was already bad enough to be argued out of the codebase in
//! [`crate::systems::mover`]. Explicit Euler on `theta'' = -k sin(theta)` does
//! not just drift in phase; it gains energy every cycle, so the amplitude an
//! author wrote in the map is not the amplitude the ball reaches, and the
//! amplitude at tick 100 is not the amplitude at tick 100_000. Golden traces
//! would then encode not the swing but the integrator, and every re-record
//! would move.
//!
//! The closed form buys three concrete things:
//!
//! - **Exactly periodic.** `at(t) == at(t + period)` bit for bit, forever,
//!   because the tick reaches the cosine only as an integer remainder. Two
//!   balls authored with the same period stay in lockstep however many ticks
//!   hitstop swallows.
//! - **Reversible and seekable.** "Where was the ball at tick 900?" is a call,
//!   not a replay — which is what makes the arc checkable in `tests/levels.rs`
//!   without running the game.
//! - **Immune to skipped ticks.** [`advance`] *places* the ball rather than
//!   nudging it, so across the ticks hitstop eats it snaps to where it belongs,
//!   exactly as a scheduled fire snaps to lit or unlit.
//!
//! # Where in the tick this runs
//!
//! In the decide phase, beside [`crate::systems::hazard::tick_schedules`] and
//! before [`crate::systems::body::rebuild_geometry`]: a thing that owns a
//! collider deciding where that collider is this tick. So a ball that swings
//! into the player on tick *t* is lethal on tick *t*.
//!
//! It does **not** need [`crate::systems::mover::advance`]'s hard-won position
//! as the last decision before the rebuild. That ordering exists entirely for
//! rider carry, and a hazard has no riders: nothing stands on a ball, because
//! nothing collides with one.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Collider, ColliderKind, Pendulum, Position};
use crate::level::LevelData;

/// The drawn radius of a ball a map does not size itself, in pixels.
///
/// A little under a tile across: big enough to read as a wrecking ball at
/// 640x360, small enough that a corridor one tile taller than the player is
/// still passable beside it.
pub const BALL_RADIUS: f32 = 12.0;

/// The lethal box for a ball of radius `radius`: the square inscribed in the
/// drawn circle.
///
/// Deliberately smaller than the circle's bounding box, for the reason
/// [`crate::systems::hazard::FIRE_SIZE`] is narrower than its cell. A
/// circumscribing square is lethal in four corners where there is visibly
/// nothing, and "I wasn't even touching it" is the resulting bug report. With
/// the inscribed square every lethal pixel is a pixel of the ball; the cost is
/// that the four edge-midpoints of the ball are harmless, which nobody has
/// ever noticed in any game.
pub fn ball_size(radius: f32) -> Vec2 {
    Vec2::splat(radius * std::f32::consts::SQRT_2)
}

/// The collider a ball of radius `radius` carries: a hazard box centred on the
/// entity's `Position`, which for a ball is the centre of the ball rather than
/// a corner of anything.
pub fn ball_collider(radius: f32) -> Collider {
    let size = ball_size(radius);
    Collider {
        rect_offset: -size / 2.0,
        size,
        kind: ColliderKind::Hazard,
    }
}

/// Spawn an entity for every swinging hazard the map places.
///
/// Called from `Sim::new` after the map's entity list, the fires and the
/// platforms, so that adding a ball to a map cannot renumber the NPCs a tape
/// addresses by spawn index.
pub fn spawn_pendulums(world: &mut World, level: &LevelData) {
    for spawn in &level.pendulums {
        let pendulum = Pendulum::new(
            spawn.anchor,
            spawn.length,
            spawn.amplitude,
            spawn.period,
            spawn.phase,
        );
        // Placed where tick 0 says it is, not at rest: `Sim::new` builds the
        // geometry before the first `step`, and a ball authored with a phase
        // has to start where its phase puts it.
        world.spawn((
            Position(pendulum.at(0)),
            ball_collider(spawn.radius),
            pendulum,
        ));
    }
}

/// Swing every ball to where this tick puts it.
///
/// One line of work, because all of the difficulty is in [`Pendulum::at`] being
/// a function rather than a state machine. There is no equivalent of the
/// mover's rider carry: a hazard obstructs nothing, so nothing is ever standing
/// on one to be carried.
pub fn advance(world: &mut World, tick: u64) {
    for (_, (pos, pendulum)) in world.query_mut::<(&mut Position, &Pendulum)>() {
        pos.0 = pendulum.at(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{Body, Size, Velocity};
    use crate::physics::{Aabb, Geometry};
    use crate::sim::event::{DeathCause, GameEvent};
    use crate::sim::{Sim, TICK};
    use crate::systems::body;
    use crate::systems::input::PlayerInput;

    const ANCHOR: Vec2 = Vec2::new(100.0, 40.0);

    fn quarter_turn() -> Pendulum {
        // Amplitude 90 degrees over four ticks: the ball is at the right
        // extreme, straight down, the left extreme, straight down, and back.
        Pendulum::new(ANCHOR, 100.0, std::f32::consts::FRAC_PI_2, 4, 0)
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    /// The closed form, against positions worked out by hand.
    #[test]
    fn a_swing_is_a_closed_form_function_of_the_tick() {
        let swing = quarter_turn();

        // Starts at the extreme, momentarily still: cos(0) = 1, so the angle
        // is the whole amplitude. A quarter turn of chain is horizontal.
        assert!(close(swing.at(0), ANCHOR + Vec2::new(100.0, 0.0)), "right");
        // A quarter of the cycle later the angle is zero: straight down, and
        // `+y` is down.
        assert!(close(swing.at(1), ANCHOR + Vec2::new(0.0, 100.0)), "bottom");
        assert!(close(swing.at(2), ANCHOR + Vec2::new(-100.0, 0.0)), "left");
        assert!(close(swing.at(3), ANCHOR + Vec2::new(0.0, 100.0)), "bottom");
        assert_eq!(swing.at(4), swing.at(0), "and round again, exactly");

        // A hand-computable interior point, so the shape of the motion is
        // pinned and not just its four corners: a 60-degree swing over six
        // ticks is at cos(60 degrees) = 1/2 of its amplitude on tick 1, i.e.
        // 30 degrees off the bottom.
        let sixty = Pendulum::new(Vec2::ZERO, 100.0, std::f32::consts::FRAC_PI_3, 6, 0);
        assert!(
            close(sixty.at(1), Vec2::new(50.0, 3.0f32.sqrt() * 50.0)),
            "30 degrees off the bottom, at {:?}",
            sixty.at(1)
        );
    }

    /// The acceptance criterion: evaluate at *t* and at *t + period* and get
    /// the same answer, with no accumulated error, because nothing
    /// accumulates. An integrated pendulum fails this within one screen of
    /// ticks and fails it worse the longer it runs.
    #[test]
    fn the_swing_is_exactly_periodic_forever() {
        let swing = Pendulum::new(ANCHOR, 137.0, 1.1, 97, 13);
        for tick in 0..1_000u64 {
            assert_eq!(
                swing.at(tick),
                swing.at(tick + swing.period as u64),
                "tick {tick}"
            );
        }
        // ...including a very long way out, where an integrated ball would
        // have wandered into a different pixel entirely — or, worse, into a
        // different amplitude.
        assert_eq!(swing.at(1_000_000 * 97 + 40), swing.at(40));
    }

    /// The ball never leaves the circle the chain describes, and never swings
    /// past the amplitude it was given. This is the property an explicit-Euler
    /// integrator breaks first: it pumps energy in and the arc creeps wider.
    #[test]
    fn the_arc_stays_on_its_chain_and_inside_its_amplitude() {
        let swing = Pendulum::new(ANCHOR, 80.0, 0.9, 71, 0);
        let mut widest: f32 = 0.0;
        for tick in 0..5_000u64 {
            let offset = swing.at(tick) - ANCHOR;
            assert!(
                (offset.length() - 80.0).abs() < 1e-3,
                "tick {tick}: off the chain at {offset:?}"
            );
            widest = widest.max(swing.angle_at(tick).abs());
        }
        assert!(widest <= 0.9, "swung past its amplitude, to {widest}");
        assert!(widest > 0.89, "never reached its amplitude, only {widest}");
    }

    #[test]
    fn phase_puts_two_balls_out_of_step() {
        let a = Pendulum::new(ANCHOR, 100.0, 1.0, 120, 0);
        let b = Pendulum::new(ANCHOR, 100.0, 1.0, 120, 30);
        for tick in 0..360u64 {
            assert_eq!(a.at(tick + 30), b.at(tick));
        }
        assert_ne!(a.at(0), b.at(0), "and they are not simply the same ball");
    }

    /// A degenerate component is a parked ball, not a panic or a NaN. Maps
    /// cannot author this — the loader rejects a zero period — but a hand-built
    /// `Pendulum` can, and division by zero would poison every position.
    #[test]
    fn a_zero_period_is_a_ball_hanging_still() {
        let parked = Pendulum::new(ANCHOR, 50.0, 0.5, 0, 7);
        assert_eq!(parked.at(0), parked.at(9_999));
        assert!(parked.at(9_999).is_finite());
    }

    /// The lethal box is inside the drawn ball, never outside it.
    #[test]
    fn the_lethal_box_is_inscribed_in_the_ball() {
        let size = ball_size(BALL_RADIUS);
        // Corner-to-centre distance is the radius: the corners touch the
        // circle and no part of the box is outside it.
        assert!((size.length() / 2.0 - BALL_RADIUS).abs() < 1e-4);
        assert!(size.x < BALL_RADIUS * 2.0, "smaller than the ball's width");

        // ...and it is centred on the entity's position, not hung off a corner.
        let collider = ball_collider(BALL_RADIUS);
        let box_ = collider.aabb(Vec2::new(200.0, 300.0));
        assert!(close(
            Vec2::new(box_.x + box_.w / 2.0, box_.y + box_.h / 2.0),
            Vec2::new(200.0, 300.0)
        ));
    }

    // --- inside a running Sim -------------------------------------------

    fn swinging_sim() -> (Sim, Pendulum) {
        let mut sim = Sim::fixture(&[
            "....................",
            "....................",
            "..P.................",
            "....................",
            "####################",
        ]);
        // Anchored above the floor with a chain long enough to sweep the
        // player's cell, swinging right through where they stand.
        let swing = Pendulum::new(Vec2::new(200.0, 16.0), 100.0, 1.2, 90, 0);
        sim.world
            .spawn((Position(swing.at(0)), ball_collider(BALL_RADIUS), swing));
        (sim, swing)
    }

    /// The ball is hazard geometry and only hazard geometry, on every tick of
    /// its cycle. One solid rect appearing here would be a wrecking ball you
    /// could stand on.
    #[test]
    fn a_ball_hands_the_geometry_a_hazard_and_never_a_solid() {
        let (mut sim, swing) = swinging_sim();
        let solids = sim.geometry.rects().len();

        for _ in 0..300u64 {
            sim.step(PlayerInput::default());
            assert_eq!(sim.geometry.entity_hazard_count(), 1);
            assert_eq!(
                sim.geometry.rects().len(),
                solids,
                "a swinging hazard must never become solid"
            );
        }

        // ...and the box the geometry holds is the one this tick's angle
        // names, not the previous tick's. Move `pendulum::advance` after
        // `rebuild_geometry` in `Sim::step` and this fails.
        let expected = ball_collider(BALL_RADIUS).aabb(swing.at(sim.tick - 1));
        assert_eq!(sim.geometry.hazards().last(), Some(&expected));
    }

    /// The acceptance criterion, through the real `Sim::step`: touching the
    /// ball kills, and it kills as a hazard — by the same path a spike does,
    /// with no new death cause and no new death check.
    #[test]
    fn touching_the_ball_is_a_hazard_death() {
        let mut sim = Sim::fixture(&["........", "..P.....", "########"]);
        for _ in 0..10 {
            sim.step(PlayerInput::default());
        }
        assert!(!sim.probe().dead);

        // Hang a parked ball on the player rather than timing a swing into
        // them: this is about the cause, not the approach.
        let probe = sim.probe();
        let anchor = Vec2::new(probe.x + 10.0, probe.y - 40.0);
        let ball = Pendulum::new(anchor, 40.0, 0.1, 0, 0);
        sim.world
            .spawn((Position(ball.at(0)), ball_collider(BALL_RADIUS), ball));

        sim.step(PlayerInput::default());
        assert_eq!(
            sim.events(),
            [GameEvent::Died {
                who: "player".to_string(),
                cause: DeathCause::Hazard,
            }]
        );
    }

    /// A hazard is not a solid: a body walks straight through the ball at
    /// exactly the speed it was going, and its contact flags never notice.
    ///
    /// Run over a bare `Body` rather than through `Sim::step` on purpose —
    /// the player would die on contact and stop being evidence about movement.
    /// Nothing here is player-specific, so this is what a hazard does to
    /// anything that moves.
    #[test]
    fn a_ball_does_not_block_movement() {
        const SPEED: f32 = 240.0;
        let stats = crate::assets::StatTable::shipped().get("player").unwrap();
        let size = stats.size();
        let start = Vec2::new(40.0, 200.0 - size.y);

        let mut world = World::new();
        let body = world.spawn((
            Position(start),
            Velocity(Vec2::new(SPEED, 0.0)),
            Size(size),
            Body::new(start, stats.gravity, stats.max_fall),
        ));
        // A big ball parked in the corridor, straight across the path: the
        // chain hangs from above and the body walks through the bottom of it.
        let ball = Pendulum::new(Vec2::new(160.0, 100.0), 84.0, 0.0, 0, 0);
        world.spawn((Position(ball.at(0)), ball_collider(24.0), ball));

        let mut geometry = Geometry::from_level(&[Aabb::new(0.0, 200.0, 400.0, 32.0)], &[], &[]);

        let mut passed_through = false;
        for tick in 0..60u64 {
            let expected_x = start.x + SPEED * TICK * tick as f32;
            advance(&mut world, tick);
            body::rebuild_geometry(&mut geometry, &world);
            body::move_bodies(&mut world, &geometry, TICK);

            let pos = world.get::<&Position>(body).unwrap().0;
            let vel = world.get::<&Velocity>(body).unwrap().0;
            let box_ = Aabb::new(pos.x, pos.y, size.x, size.y);
            let ball_box = ball_collider(24.0).aabb(ball.at(tick));

            assert_eq!(
                vel.x, SPEED,
                "tick {tick}: the ball took the body's speed away"
            );
            assert_eq!(
                pos.x,
                start.x + SPEED * TICK * (tick + 1) as f32,
                "tick {tick}: expected to be past {expected_x}, but something stopped it"
            );
            passed_through |= box_.overlaps(&ball_box);
        }
        assert!(
            passed_through,
            "the body never reached the ball, so this proved nothing"
        );
        assert_eq!(
            geometry.entity_rect_count(),
            0,
            "the ball contributed no solid geometry at all"
        );
    }
}
