//! Player movement against static level geometry: run, jump (with coyote
//! time, buffering, and variable height), double jump, wall slide/jump,
//! drop-through platforms, and hazard death.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Avatar, Position, Size, Velocity};
use crate::level::LevelData;
use crate::physics::{self, Aabb, SolidRect};
use crate::systems::input::PlayerInput;

pub fn update(
    world: &mut World,
    level: &LevelData,
    geometry: &[SolidRect],
    input: PlayerInput,
    dt: f32,
) {
    for (_, (avatar, pos, vel, size)) in
        world.query_mut::<(&mut Avatar, &mut Position, &mut Velocity, &Size)>()
    {
        if avatar.dead() {
            avatar.dead_ticks -= 1;
            if avatar.dead_ticks == 0 {
                respawn(avatar, pos, vel, level.player_spawn);
            }
            continue;
        }

        // --- timers ---
        if avatar.grounded {
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

        // --- horizontal: accelerate toward the target speed, brake to zero ---
        let dir = f32::from(input.right) - f32::from(input.left);
        let target = dir * Avatar::RUN_SPEED;
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

        // --- wall slide: airborne, falling, pressing into a solid wall ---
        avatar.wall_sliding = false;
        if !avatar.grounded
            && vel.0.y > 0.0
            && dir != 0.0
            && physics::touching_wall(pos.0, size.0, geometry, dir)
        {
            avatar.wall_sliding = true;
            avatar.wall_dir = dir;
            vel.0.y = vel.0.y.min(Avatar::WALL_SLIDE_SPEED);
            avatar.double_jumping = false;
        }

        // --- jumps, in priority order ---
        if avatar.jump_buffer > 0 {
            let can_ground_jump = avatar.coyote_ticks <= Avatar::COYOTE_TICKS;

            if input.down && avatar.grounded && avatar.on_one_way_only {
                // drop through the platform instead of jumping
                avatar.drop_ticks = Avatar::DROP_TICKS;
                avatar.grounded = false;
                avatar.jump_buffer = 0;
            } else if can_ground_jump {
                vel.0.y = -Avatar::JUMP_SPEED;
                launch(avatar);
            } else if avatar.wall_sliding {
                vel.0.x = -avatar.wall_dir * Avatar::WALL_JUMP_PUSH;
                vel.0.y = -Avatar::WALL_JUMP_SPEED;
                avatar.facing_right = avatar.wall_dir < 0.0;
                avatar.air_jumps = Avatar::MAX_AIR_JUMPS;
                launch(avatar);
            } else if avatar.air_jumps > 0 {
                vel.0.y = -Avatar::DOUBLE_JUMP_SPEED;
                avatar.air_jumps -= 1;
                avatar.double_jumping = true;
                launch(avatar);
            }
        }

        // --- gravity, with a heavier pull when the jump key is released
        //     mid-rise (variable jump height) ---
        let gravity = if vel.0.y < 0.0 && !input.jump_held {
            Avatar::GRAVITY + Avatar::LOW_JUMP_GRAVITY
        } else {
            Avatar::GRAVITY
        };
        vel.0.y = (vel.0.y + gravity * dt).min(Avatar::MAX_FALL);
        if avatar.wall_sliding {
            vel.0.y = vel.0.y.min(Avatar::WALL_SLIDE_SPEED);
        }

        // --- integrate & collide ---
        avatar.prev_pos = pos.0;
        pos.0 += vel.0 * dt;

        let contact = if avatar.drop_ticks > 0 {
            let solids_only: Vec<SolidRect> =
                geometry.iter().copied().filter(|s| !s.one_way).collect();
            physics::resolve_move(
                &mut pos.0,
                &mut vel.0,
                avatar.prev_pos,
                size.0,
                &solids_only,
            )
        } else {
            physics::resolve_move(&mut pos.0, &mut vel.0, avatar.prev_pos, size.0, geometry)
        };
        avatar.grounded = contact.grounded;
        avatar.on_one_way_only = contact.on_one_way && !contact.on_solid;
        if contact.grounded {
            avatar.air_jumps = Avatar::MAX_AIR_JUMPS;
            avatar.double_jumping = false;
        }
        if vel.0.y >= 0.0 {
            avatar.double_jumping = false;
        }

        avatar.crouching = avatar.grounded && input.down && dir == 0.0;

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
        let body = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        let spiked = level.hazards.iter().any(|h| body.overlaps(h));
        let fell_out = pos.0.y > level.pixel_height() + 100.0;
        if spiked || fell_out {
            avatar.dead_ticks = Avatar::DEATH_TICKS;
            vel.0 = Vec2::ZERO;
        }
    }
}

/// Shared bookkeeping for every kind of jump.
fn launch(avatar: &mut Avatar) {
    avatar.jump_buffer = 0;
    avatar.grounded = false;
    avatar.wall_sliding = false;
    // lock out coyote jumps until the next real landing
    avatar.coyote_ticks = u32::MAX - 1;
}

fn respawn(avatar: &mut Avatar, pos: &mut Position, vel: &mut Velocity, spawn: Vec2) {
    *avatar = Avatar::new(spawn);
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

    fn test_level() -> LevelData {
        LevelData {
            tileset: "test".to_string(),
            width: 20,
            height: 10,
            tile_size: 32.0,
            background: vec![],
            tiles: vec![],
            solids: vec![Aabb::new(0.0, FLOOR_Y, 640.0, 64.0)],
            one_way: vec![],
            hazards: vec![],
            player_spawn: Vec2::new(100.0, FLOOR_Y - Avatar::HEIGHT),
            entities: vec![],
        }
    }

    fn geometry(level: &LevelData) -> Vec<SolidRect> {
        let mut geo: Vec<SolidRect> = level.solids.iter().map(|&r| SolidRect::solid(r)).collect();
        geo.extend(level.one_way.iter().map(|&r| SolidRect::one_way(r)));
        geo
    }

    fn spawn_avatar(world: &mut World, pos: Vec2) {
        world.spawn((
            Avatar::new(pos),
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

    fn settle(world: &mut World, level: &LevelData, geo: &[SolidRect]) {
        for _ in 0..10 {
            update(world, level, geo, PlayerInput::default(), DT);
        }
        assert!(state(world).0.grounded, "avatar failed to settle on ground");
    }

    const JUMP: PlayerInput = PlayerInput {
        left: false,
        right: false,
        down: false,
        jump_pressed: true,
        jump_held: true,
    };
    const HOLD_JUMP: PlayerInput = PlayerInput {
        left: false,
        right: false,
        down: false,
        jump_pressed: false,
        jump_held: true,
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
        let (avatar, _, vel) = state(&mut world);
        assert!(!avatar.grounded);
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
        assert!(avatar.grounded);
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
            if !state(&mut world).0.grounded {
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

    #[test]
    fn down_jump_drops_through_platform_but_not_solid_ground() {
        let mut level = test_level();
        level.solids = vec![Aabb::new(0.0, 400.0, 640.0, 32.0)]; // real floor
        level.one_way = vec![Aabb::new(0.0, 256.0, 640.0, 8.0)]; // platform above
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(100.0, 256.0 - Avatar::HEIGHT));
        settle(&mut world, &level, &geo);
        let (avatar, _, _) = state(&mut world);
        assert!(avatar.on_one_way_only);

        // down + jump: fall through the platform...
        let down_jump = PlayerInput {
            down: true,
            jump_pressed: true,
            jump_held: true,
            ..Default::default()
        };
        update(&mut world, &level, &geo, down_jump, DT);
        let (avatar, _, _) = state(&mut world);
        assert!(!avatar.grounded, "dropped off the platform");

        // ...and land on the solid floor below
        for _ in 0..120 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (avatar, pos, _) = state(&mut world);
        assert!(avatar.grounded);
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
