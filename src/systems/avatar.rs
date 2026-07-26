//! What the player *decides*: run, jump (with coyote time, buffering, and
//! variable height), double jump, wall slide/jump, drop-through, and dying.
//!
//! Actually moving through the world is [`crate::systems::body`], shared with
//! everything else that has a [`Body`]. That split is why this file is all
//! intent and no integration, and it puts the tick in three phases:
//!
//! 1. [`control`] reads input and decides a velocity, plus the per-tick knobs
//!    on the body (how heavy gravity is, whether to fall through platforms).
//! 2. `move_bodies` moves every body and records what it touched.
//! 3. [`after_move`] reacts to where the body actually ended up — landing,
//!    crouching, map bounds, and whether any of it was lethal.
//!
//! Phases 1 and 3 must agree about skipping: a frozen body does not move, so
//! it must not draw conclusions from a stale contact either. `Body::frozen` is
//! the single flag both consult.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Attacking, Avatar, Body, Health, Position, Size, Velocity};
use crate::level::LevelData;
use crate::physics::{self, Aabb, SolidRect};
use crate::sim::event::{DeathCause, GameEvent};
use crate::systems::input::PlayerInput;

/// Phase 1: turn this tick's input into a velocity and body settings.
pub fn control(
    world: &mut World,
    level: &LevelData,
    geometry: &[SolidRect],
    input: PlayerInput,
    dt: f32,
    events: &mut Vec<GameEvent>,
) {
    for (_, (avatar, pos, vel, size, body, health, attacking)) in world.query_mut::<(
        &mut Avatar,
        &mut Position,
        &mut Velocity,
        &Size,
        &mut Body,
        &mut Health,
        &mut Attacking,
    )>() {
        let stunned = health.hitstun > 0;
        if avatar.dead() {
            avatar.dead_ticks -= 1;
            if avatar.dead_ticks == 0 {
                respawn(avatar, pos, vel, body, health, attacking, level.player_spawn);
                events.push(GameEvent::Respawned);
            }
            // Still frozen on the tick of the respawn itself, so the player
            // reappears standing still rather than mid-fall.
            body.frozen = true;
            continue;
        }
        body.frozen = false;

        // --- knocked about: the body keeps flying, the player has no say ---
        //
        // Deliberately not `Body::frozen`, which would stop the knockback dead
        // in mid-air. Steering is what is taken away, not momentum.
        if stunned {
            avatar.jump_buffer = 0;
            continue;
        }

        // --- timers ---
        if body.grounded {
            avatar.coyote_ticks = 0;
        } else {
            avatar.coyote_ticks = avatar.coyote_ticks.saturating_add(1);
        }
        if input.jump_pressed {
            avatar.jump_buffer = Avatar::JUMP_BUFFER_TICKS;
        } else {
            avatar.jump_buffer = avatar.jump_buffer.saturating_sub(1);
        }
        avatar.drop_ticks = avatar.drop_ticks.saturating_sub(1);

        // --- attack: one press, one swing, and no swinging mid-swing ---
        if input.attack_pressed && !attacking.busy() {
            attacking.start(Avatar::ATTACK);
            events.push(GameEvent::Attacked {
                attack: Avatar::ATTACK.to_string(),
            });
        }

        // --- horizontal: accelerate toward the target speed, brake to zero ---
        let dir = f32::from(input.right) - f32::from(input.left);
        // Rooted on the ground while committed to a swing, so an attack costs
        // something. Air momentum is left alone -- stopping dead mid-jump
        // reads as a bug rather than as commitment.
        let target = if attacking.busy() && body.grounded {
            0.0
        } else {
            dir * Avatar::RUN_SPEED
        };
        let rate = if dir != 0.0 {
            Avatar::ACCEL
        } else {
            Avatar::DECEL
        };
        vel.0.x = move_toward(vel.0.x, target, rate * dt);
        if dir > 0.0 {
            avatar.facing_right = true;
        } else if dir < 0.0 {
            avatar.facing_right = false;
        }

        // --- wall contact, tracked apart from the slide ---
        // A wall jump used to require an active slide, which meant airborne
        // *and* falling *and* still holding into the wall. So hitting a wall
        // on the way up did nothing, and easing off the key for one tick to
        // line up the kick threw the jump away. Contact alone arms the wall
        // jump, and the grace window keeps it armed just after letting go.
        let wall_side = if body.grounded {
            0.0
        } else {
            physics::wall_contact(pos.0, size.0, geometry, dir)
        };
        if wall_side != 0.0 {
            avatar.wall_dir = wall_side;
            avatar.wall_coyote_ticks = 0;
        } else {
            avatar.wall_coyote_ticks = avatar.wall_coyote_ticks.saturating_add(1);
        }

        // --- wall slide: falling while pressing into the wall being touched ---
        avatar.wall_sliding = wall_side != 0.0 && dir == wall_side && vel.0.y > 0.0;
        if avatar.wall_sliding {
            vel.0.y = vel.0.y.min(Avatar::WALL_SLIDE_SPEED);
            avatar.double_jumping = false;
        }

        // --- drop through, then jumps in priority order ---

        // Down alone drops through a one-way platform: there is nothing worth
        // crouching on up there, and standing on a platform makes "down" mean
        // "go down". Crouching still works on solid ground, which is what
        // `on_one_way_only` distinguishes. A buffered jump press is discarded
        // so the drop is not immediately cancelled by a jump on the way out.
        if input.down && body.grounded && body.on_one_way_only() {
            avatar.drop_ticks = Avatar::DROP_TICKS;
            avatar.jump_buffer = 0;
            events.push(GameEvent::DroppedThrough);
        } else if avatar.jump_buffer > 0 {
            let can_ground_jump = avatar.coyote_ticks <= Avatar::COYOTE_TICKS;
            let can_wall_jump = avatar.wall_coyote_ticks <= Avatar::WALL_COYOTE_TICKS;

            if can_ground_jump {
                vel.0.y = -Avatar::JUMP_SPEED;
                launch(avatar);
                events.push(GameEvent::Jumped);
            } else if can_wall_jump {
                vel.0.x = -avatar.wall_dir * Avatar::WALL_JUMP_PUSH;
                vel.0.y = -Avatar::WALL_JUMP_SPEED;
                avatar.facing_right = avatar.wall_dir < 0.0;
                avatar.air_jumps = Avatar::MAX_AIR_JUMPS;
                let wall_dir = avatar.wall_dir;
                launch(avatar);
                events.push(GameEvent::WallJumped { wall_dir });
            } else if avatar.air_jumps > 0 {
                vel.0.y = -Avatar::DOUBLE_JUMP_SPEED;
                avatar.air_jumps -= 1;
                avatar.double_jumping = true;
                launch(avatar);
                events.push(GameEvent::DoubleJumped);
            }
        }

        // --- hand the body its settings for this tick ---

        // Heavier gravity while rising with the jump key released is what
        // makes a tap a short hop and a hold a full jump. Read after the jump
        // block, so a launch on this very tick is already accounted for.
        body.gravity = if vel.0.y < 0.0 && !input.jump_held {
            Avatar::GRAVITY + Avatar::LOW_JUMP_GRAVITY
        } else {
            Avatar::GRAVITY
        };
        // Also read after the jump block: `launch` clears `wall_sliding`, and
        // a wall jump must not have its upward kick capped by the slide.
        body.fall_cap = avatar.wall_sliding.then_some(Avatar::WALL_SLIDE_SPEED);
        body.ignore_one_way = avatar.drop_ticks > 0;
    }
}

/// Phase 3: react to where the body actually ended up.
///
/// Takes the same input as [`control`] because a couple of decisions depend on
/// both what was pressed and where the body finished — crouching needs to know
/// the player is holding down *and* is now standing on something.
pub fn after_move(
    world: &mut World,
    level: &LevelData,
    input: PlayerInput,
    events: &mut Vec<GameEvent>,
) {
    let dir = f32::from(input.right) - f32::from(input.left);

    for (_, (avatar, pos, vel, size, body)) in
        world.query_mut::<(&mut Avatar, &mut Position, &mut Velocity, &Size, &Body)>()
    {
        // Frozen bodies did not move, so there is no new contact to read and
        // nothing new can have killed them.
        if body.frozen {
            continue;
        }

        if body.grounded {
            avatar.air_jumps = Avatar::MAX_AIR_JUMPS;
            avatar.double_jumping = false;
            if body.landed {
                events.push(GameEvent::Landed {
                    on_one_way: body.on_one_way_only(),
                });
            }
        }
        if vel.0.y >= 0.0 {
            avatar.double_jumping = false;
        }

        avatar.crouching = body.grounded && input.down && dir == 0.0;

        // --- map bounds ---
        let max_x = level.pixel_width() - size.0.x;
        if pos.0.x < 0.0 {
            pos.0.x = 0.0;
            vel.0.x = 0.0;
        } else if pos.0.x > max_x {
            pos.0.x = max_x;
            vel.0.x = 0.0;
        }

        // --- lethal stuff ---
        let hitbox = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        let spiked = level.hazards.iter().any(|h| hitbox.overlaps(h));
        let fell_out = pos.0.y > level.pixel_height() + 100.0;
        if spiked || fell_out {
            avatar.dead_ticks = Avatar::DEATH_TICKS;
            vel.0 = Vec2::ZERO;
            events.push(GameEvent::Died {
                who: "player".to_string(),
                cause: if spiked {
                    DeathCause::Hazard
                } else {
                    DeathCause::FellOutOfWorld
                },
            });
        }
    }
}

/// Shared bookkeeping for every kind of jump.
fn launch(avatar: &mut Avatar) {
    avatar.jump_buffer = 0;
    avatar.wall_sliding = false;
    // Lock out both grace windows until the next real landing or wall touch,
    // so one press is one jump and a wall jump cannot re-fire off the wall it
    // just left.
    avatar.coyote_ticks = u32::MAX - 1;
    avatar.wall_coyote_ticks = u32::MAX - 1;
}

/// Back to a clean slate at the spawn point. The body is reset alongside the
/// avatar — leaving stale contact or a stale `prev_pos` behind would have the
/// player reappear believing they are still standing where they died.
#[allow(clippy::too_many_arguments)]
fn respawn(
    avatar: &mut Avatar,
    pos: &mut Position,
    vel: &mut Velocity,
    body: &mut Body,
    health: &mut Health,
    attacking: &mut Attacking,
    spawn: Vec2,
) {
    *avatar = Avatar::new();
    *body = Avatar::body(spawn);
    *health = Health::new(Avatar::MAX_HEALTH);
    attacking.stop();
    pos.0 = spawn;
    vel.0 = Vec2::ZERO;
}

fn move_toward(current: f32, target: f32, max_delta: f32) -> f32 {
    if (target - current).abs() <= max_delta {
        target
    } else {
        current + (target - current).signum() * max_delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const FLOOR_Y: f32 = 256.0;

    /// One whole tick of player movement — all three phases in order, with a
    /// throwaway event log.
    ///
    /// Deliberately the real sequence rather than a stub: these tests predate
    /// the control/move/react split and describe behavior that the split was
    /// not supposed to change, so running them through the composed pipeline
    /// is exactly the check that matters.
    fn update(
        world: &mut World,
        level: &LevelData,
        geometry: &[SolidRect],
        input: PlayerInput,
        dt: f32,
    ) {
        let mut events = Vec::new();
        super::control(world, level, geometry, input, dt, &mut events);
        crate::systems::body::move_bodies(world, geometry, dt);
        super::after_move(world, level, input, &mut events);
    }

    fn test_level() -> LevelData {
        let mut level = LevelData::empty(20, 10, 32.0);
        level.solids = vec![Aabb::new(0.0, FLOOR_Y, 640.0, 64.0)];
        level.player_spawn = Vec2::new(100.0, FLOOR_Y - Avatar::HEIGHT);
        level
    }

    fn geometry(level: &LevelData) -> Vec<SolidRect> {
        let mut geo: Vec<SolidRect> = level.solids.iter().map(|&r| SolidRect::solid(r)).collect();
        geo.extend(level.one_way.iter().map(|&r| SolidRect::one_way(r)));
        geo
    }

    fn spawn_avatar(world: &mut World, pos: Vec2) {
        world.spawn((
            Avatar::new(),
            Avatar::body(pos),
            Health::new(Avatar::MAX_HEALTH),
            Attacking::default(),
            Position(pos),
            Velocity(Vec2::ZERO),
            Size(Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)),
        ));
    }

    fn state(world: &mut World) -> (Avatar, Vec2, Vec2) {
        let (_, (a, p, v)) = world
            .query_mut::<(&Avatar, &Position, &Velocity)>()
            .into_iter()
            .next()
            .unwrap();
        (a.clone(), p.0, v.0)
    }

    /// Contact state, which lives on the body rather than the avatar.
    fn body(world: &mut World) -> Body {
        *world.query_mut::<&Body>().into_iter().next().unwrap().1
    }

    fn settle(world: &mut World, level: &LevelData, geo: &[SolidRect]) {
        for _ in 0..10 {
            update(world, level, geo, PlayerInput::default(), DT);
        }
        assert!(body(world).grounded, "avatar failed to settle on ground");
    }

    const JUMP: PlayerInput = PlayerInput {
        left: false,
        right: false,
        down: false,
        jump_pressed: true,
        jump_held: true,
        attack_pressed: false,
    };
    const HOLD_JUMP: PlayerInput = PlayerInput {
        left: false,
        right: false,
        down: false,
        jump_pressed: false,
        jump_held: true,
        attack_pressed: false,
    };

    #[test]
    fn walks_and_jumps() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);
        settle(&mut world, &level, &geo);

        let run = PlayerInput {
            right: true,
            ..Default::default()
        };
        for _ in 0..30 {
            update(&mut world, &level, &geo, run, DT);
        }
        let (avatar, _, vel) = state(&mut world);
        assert!(avatar.facing_right);
        assert_eq!(vel.x, Avatar::RUN_SPEED);

        update(&mut world, &level, &geo, JUMP, DT);
        let (_, _, vel) = state(&mut world);
        assert!(!body(&mut world).grounded);
        assert!(vel.y < 0.0);
    }

    #[test]
    fn double_jump_works_once_and_resets_on_landing() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);
        settle(&mut world, &level, &geo);

        // ground jump, then let the buffer drain while rising
        update(&mut world, &level, &geo, JUMP, DT);
        for _ in 0..10 {
            update(&mut world, &level, &geo, HOLD_JUMP, DT);
        }
        let (_, _, vel_before) = state(&mut world);

        // double jump relaunches upward
        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, vel) = state(&mut world);
        assert!(avatar.double_jumping);
        assert!(vel.y < vel_before.y);
        assert_eq!(avatar.air_jumps, 0);

        // a third press mid-air does nothing
        for _ in 0..10 {
            update(&mut world, &level, &geo, HOLD_JUMP, DT);
        }
        let (_, _, vel_before) = state(&mut world);
        update(&mut world, &level, &geo, JUMP, DT);
        let (_, _, vel) = state(&mut world);
        assert!(vel.y > vel_before.y, "no triple jump");

        // landing refills the air jump
        for _ in 0..200 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (avatar, _, _) = state(&mut world);
        assert!(body(&mut world).grounded);
        assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS);
    }

    #[test]
    fn coyote_time_allows_a_late_jump_but_not_after_jumping() {
        let level = test_level();
        // narrow ledge: walk off the right edge
        let mut level = level;
        level.solids = vec![Aabb::new(0.0, FLOOR_Y, 110.0, 64.0)];
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(85.0, FLOOR_Y - Avatar::HEIGHT));
        settle(&mut world, &level, &geo);

        // walk off the ledge
        let run = PlayerInput {
            right: true,
            ..Default::default()
        };
        let mut airborne_at = None;
        for tick in 0..60 {
            update(&mut world, &level, &geo, run, DT);
            if !body(&mut world).grounded {
                airborne_at = Some(tick);
                break;
            }
        }
        airborne_at.expect("never walked off the ledge");

        // 3 ticks later (within the 6-tick window) a jump still works
        for _ in 0..3 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, vel) = state(&mut world);
        assert!(vel.y < -Avatar::JUMP_SPEED * 0.8, "coyote jump fired");
        assert!(!avatar.double_jumping, "was a ground jump, not an air jump");
        assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS);
    }

    #[test]
    fn jump_buffer_fires_on_landing() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        // drop from just above the floor; press jump 2 ticks before landing
        spawn_avatar(
            &mut world,
            Vec2::new(100.0, FLOOR_Y - Avatar::HEIGHT - 12.0),
        );
        // consume the air jump so the buffered press can only be a ground jump
        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, _) = state(&mut world);
        assert_eq!(avatar.air_jumps, 0);

        // fall; press jump again while still airborne
        for _ in 0..30 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        update(&mut world, &level, &geo, JUMP, DT); // buffered press mid-fall
        let mut jumped = false;
        for _ in 0..Avatar::JUMP_BUFFER_TICKS {
            update(&mut world, &level, &geo, HOLD_JUMP, DT);
            let (_, _, vel) = state(&mut world);
            if vel.y < -Avatar::JUMP_SPEED * 0.8 {
                jumped = true;
                break;
            }
        }
        assert!(jumped, "buffered jump fired on landing");
    }

    #[test]
    fn variable_jump_height_tap_is_shorter_than_hold() {
        let level = test_level();
        let geo = geometry(&level);

        let apex = |held: bool| -> f32 {
            let mut world = World::new();
            spawn_avatar(&mut world, level.player_spawn);
            settle(&mut world, &level, &geo);
            update(&mut world, &level, &geo, JUMP, DT);
            let mut peak = f32::MAX;
            for _ in 0..120 {
                let input = if held {
                    HOLD_JUMP
                } else {
                    PlayerInput::default()
                };
                update(&mut world, &level, &geo, input, DT);
                peak = peak.min(state(&mut world).1.y);
            }
            (FLOOR_Y - Avatar::HEIGHT) - peak // height gained
        };

        let full = apex(true);
        let tap = apex(false);
        assert!(
            tap < full * 0.7,
            "tap jump ({tap:.0}px) should be well below full jump ({full:.0}px)"
        );
    }

    #[test]
    fn wall_slide_caps_fall_speed_and_wall_jump_kicks_away() {
        let mut level = test_level();
        // wall to the right of the avatar, no floor below
        level.solids = vec![Aabb::new(120.0, 0.0, 32.0, 512.0)];
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(100.0, 50.0));

        // fall while pressing right into the wall
        let press = PlayerInput {
            right: true,
            ..Default::default()
        };
        for _ in 0..60 {
            update(&mut world, &level, &geo, press, DT);
        }
        let (avatar, _, vel) = state(&mut world);
        assert!(avatar.wall_sliding);
        assert!(
            vel.y <= Avatar::WALL_SLIDE_SPEED + 0.01,
            "fall speed capped, got {}",
            vel.y
        );

        // wall jump: kicks up and away (leftward), flips facing
        let wall_jump = PlayerInput {
            right: true,
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        };
        update(&mut world, &level, &geo, wall_jump, DT);
        let (avatar, _, vel) = state(&mut world);
        assert!(vel.y < 0.0, "kicked upward");
        assert!(vel.x < 0.0, "kicked away from the wall");
        assert!(!avatar.facing_right, "faces away from the wall");
    }

    /// A wall beside the spawn, reachable by jumping and drifting right.
    fn wall_level() -> LevelData {
        let mut level = test_level();
        level.solids = vec![
            Aabb::new(0.0, FLOOR_Y, 640.0, 64.0), // floor
            Aabb::new(140.0, 0.0, 32.0, FLOOR_Y), // wall to the right
        ];
        level
    }

    /// Jump, drift right into the wall, and stop on the tick contact is made
    /// while still rising. Returns the world with the avatar in that state.
    fn rising_against_the_wall(level: &LevelData, geo: &[SolidRect]) -> World {
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);
        settle(&mut world, level, geo);

        let right_hold = PlayerInput {
            right: true,
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        };
        update(&mut world, level, geo, right_hold, DT);

        let drift = PlayerInput {
            right: true,
            jump_held: true,
            ..Default::default()
        };
        for _ in 0..20 {
            update(&mut world, level, geo, drift, DT);
            let (avatar, pos, vel) = state(&mut world);
            let touching =
                physics::touching_wall(pos, Vec2::new(Avatar::WIDTH, Avatar::HEIGHT), geo, 1.0);
            if touching && vel.y < 0.0 {
                assert!(!body(&mut world).grounded);
                assert!(!avatar.wall_sliding, "not a slide: still on the way up");
                return world;
            }
        }
        panic!("never reached the wall while rising");
    }

    /// Hitting a wall on the way up is a wall jump. Requiring an active wall
    /// slide meant the press was silently dropped unless the player was
    /// already falling.
    #[test]
    fn wall_jump_fires_while_rising_without_a_slide() {
        let level = wall_level();
        let geo = geometry(&level);
        let mut world = rising_against_the_wall(&level, &geo);

        let (_, _, before) = state(&mut world);
        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, vel) = state(&mut world);

        assert!(vel.x < 0.0, "kicked away from the wall");
        assert!(vel.y < before.y, "and upward again");
        assert!(!avatar.double_jumping, "a wall jump, not the air jump");
        assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS, "air jump intact");
    }

    /// The wall jump survives a few ticks off the wall, the same way the
    /// ground jump survives walking off a ledge. Leaning away from the wall
    /// to line up the kick is what a player actually does.
    #[test]
    fn wall_jump_survives_briefly_after_leaving_the_wall() {
        let level = wall_level();
        let geo = geometry(&level);
        let mut world = rising_against_the_wall(&level, &geo);

        // peel off the wall, then press
        let away = PlayerInput {
            left: true,
            ..Default::default()
        };
        for _ in 0..3 {
            update(&mut world, &level, &geo, away, DT);
        }
        let (avatar, pos, _) = state(&mut world);
        assert!(
            !physics::touching_wall(pos, Vec2::new(Avatar::WIDTH, Avatar::HEIGHT), &geo, 1.0),
            "no longer touching the wall"
        );
        assert!(avatar.wall_coyote_ticks <= Avatar::WALL_COYOTE_TICKS);

        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, vel) = state(&mut world);
        assert!(vel.x < 0.0, "still kicks away from the wall");
        assert!(vel.y < 0.0);
        assert!(!avatar.double_jumping);
    }

    /// ...but not forever: past the window it is an ordinary air jump.
    #[test]
    fn wall_jump_expires_after_the_grace_window() {
        let level = wall_level();
        let geo = geometry(&level);
        let mut world = rising_against_the_wall(&level, &geo);

        // peel off the wall, then coast until the window is well past — so
        // horizontal speed has bled to zero and a kick would be unmistakable
        let away = PlayerInput {
            left: true,
            ..Default::default()
        };
        for _ in 0..3 {
            update(&mut world, &level, &geo, away, DT);
        }
        for _ in 0..(Avatar::WALL_COYOTE_TICKS + 4) {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (_, _, before) = state(&mut world);
        assert_eq!(before.x, 0.0, "drifting freely, no horizontal input");
        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, vel) = state(&mut world);

        assert!(avatar.double_jumping, "fell back to the air jump");
        assert_eq!(avatar.air_jumps, 0);
        assert_eq!(vel.x, before.x, "no wall kick");
    }

    /// One press, one jump. Holding the key must not pogo the player off the
    /// ground on landing.
    #[test]
    fn holding_jump_does_not_re_jump_on_landing() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);
        settle(&mut world, &level, &geo);

        update(&mut world, &level, &geo, JUMP, DT);
        let mut landed = None;
        for tick in 0..200 {
            update(&mut world, &level, &geo, HOLD_JUMP, DT);
            if body(&mut world).grounded {
                landed = Some(tick);
                break;
            }
        }
        landed.expect("never came back down");

        // still holding, a full second later: on the ground the whole time
        for _ in 0..60 {
            update(&mut world, &level, &geo, HOLD_JUMP, DT);
            let (avatar, _, _) = state(&mut world);
            assert!(body(&mut world).grounded, "held jump must not fire again");
            assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS);
        }
    }

    /// A ground jump spends only the ground jump. (When one press reached two
    /// ticks, it spent the double jump on the same tap and left the player
    /// airborne with nothing in reserve.)
    #[test]
    fn a_ground_jump_leaves_the_air_jump_available() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);
        settle(&mut world, &level, &geo);

        update(&mut world, &level, &geo, JUMP, DT);
        let (avatar, _, _) = state(&mut world);
        assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS);
        assert!(!avatar.double_jumping);
    }

    #[test]
    fn down_jump_drops_through_platform_but_not_solid_ground() {
        let mut level = test_level();
        level.solids = vec![Aabb::new(0.0, 400.0, 640.0, 32.0)]; // real floor
        level.one_way = vec![Aabb::new(0.0, 256.0, 640.0, 8.0)]; // platform above
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(100.0, 256.0 - Avatar::HEIGHT));
        settle(&mut world, &level, &geo);
        assert!(body(&mut world).on_one_way_only());

        // down + jump: fall through the platform...
        let down_jump = PlayerInput {
            down: true,
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        };
        update(&mut world, &level, &geo, down_jump, DT);
        assert!(!body(&mut world).grounded, "dropped off the platform");

        // ...and land on the solid floor below
        for _ in 0..120 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (_, pos, _) = state(&mut world);
        assert!(body(&mut world).grounded);
        assert_eq!(pos.y, 400.0 - Avatar::HEIGHT);

        // down + jump on solid ground is just a jump
        let down_jump2 = PlayerInput {
            down: true,
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        };
        update(&mut world, &level, &geo, down_jump2, DT);
        let (_, _, vel) = state(&mut world);
        assert!(vel.y < 0.0, "jumped instead of dropping");
    }

    #[test]
    fn spikes_kill_then_respawn_at_spawn() {
        let mut level = test_level();
        level.hazards = vec![Aabb::new(90.0, FLOOR_Y - 16.0, 64.0, 16.0)];
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn); // spawn overlaps the spikes
        update(&mut world, &level, &geo, PlayerInput::default(), DT);

        let (avatar, _, _) = state(&mut world);
        assert!(avatar.dead());

        // input is ignored while dead
        let run = PlayerInput {
            right: true,
            ..Default::default()
        };
        let x_dead = state(&mut world).1.x;
        update(&mut world, &level, &geo, run, DT);
        assert_eq!(state(&mut world).1.x, x_dead);

        // after the freeze: back at spawn, alive, state reset
        for _ in 0..Avatar::DEATH_TICKS {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (avatar, pos, _) = state(&mut world);
        // (spawn overlaps spikes in this contrived level, so it re-dies —
        // but the respawn itself must have happened first)
        assert_eq!(pos, level.player_spawn);
        assert_eq!(avatar.air_jumps, Avatar::MAX_AIR_JUMPS);
    }

    #[test]
    fn falling_out_of_the_world_kills_then_respawns() {
        let level = test_level();
        let geo: Vec<SolidRect> = vec![];
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(100.0, level.pixel_height() + 200.0));

        update(&mut world, &level, &geo, PlayerInput::default(), DT);
        assert!(state(&mut world).0.dead());

        for _ in 0..Avatar::DEATH_TICKS {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (avatar, pos, vel) = state(&mut world);
        assert!(!avatar.dead());
        assert_eq!(pos, level.player_spawn);
        assert_eq!(vel, Vec2::ZERO);
    }
}
