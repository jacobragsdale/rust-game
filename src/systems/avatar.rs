//! Adventure-mode player movement against static level geometry.

use ggez::glam::Vec2;
use hecs::World;

use crate::ecs::components::{Avatar, Position, Size, Velocity};
use crate::level::LevelData;
use crate::physics::{self, SolidRect};
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
        // Horizontal: accelerate toward the target speed, brake to zero.
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

        if input.jump_pressed && avatar.grounded {
            vel.0.y = -Avatar::JUMP_SPEED;
            avatar.grounded = false;
        }

        vel.0.y = (vel.0.y + Avatar::GRAVITY * dt).min(Avatar::MAX_FALL);

        avatar.prev_pos = pos.0;
        pos.0 += vel.0 * dt;

        let contact =
            physics::resolve_move(&mut pos.0, &mut vel.0, avatar.prev_pos, size.0, geometry);
        avatar.grounded = contact.grounded;

        // Stay inside the map horizontally.
        let max_x = level.pixel_width() - size.0.x;
        if pos.0.x < 0.0 {
            pos.0.x = 0.0;
            vel.0.x = 0.0;
        } else if pos.0.x > max_x {
            pos.0.x = max_x;
            vel.0.x = 0.0;
        }

        // Fell out of the world: respawn (real death handling is Phase 3).
        if pos.0.y > level.pixel_height() + 100.0 {
            pos.0 = level.player_spawn;
            avatar.prev_pos = pos.0;
            vel.0 = Vec2::ZERO;
        }
    }
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
    use crate::physics::Aabb;

    const DT: f32 = 1.0 / 60.0;

    fn test_level() -> LevelData {
        LevelData {
            tileset: "test".to_string(),
            width: 20,
            height: 10,
            tile_size: 32.0,
            background: vec![],
            tiles: vec![],
            solids: vec![Aabb::new(0.0, 256.0, 640.0, 64.0)], // floor
            one_way: vec![],
            hazards: vec![],
            player_spawn: Vec2::new(100.0, 256.0 - Avatar::HEIGHT),
            entities: vec![],
        }
    }

    fn geometry(level: &LevelData) -> Vec<SolidRect> {
        level
            .solids
            .iter()
            .map(|&r| SolidRect::solid(r, 0.0))
            .collect()
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

    #[test]
    fn walks_lands_and_jumps() {
        let level = test_level();
        let geo = geometry(&level);
        let mut world = World::new();
        spawn_avatar(&mut world, level.player_spawn);

        // settle onto the floor
        for _ in 0..10 {
            update(&mut world, &level, &geo, PlayerInput::default(), DT);
        }
        let (avatar, pos, _) = state(&mut world);
        assert!(avatar.grounded);
        assert_eq!(pos.y, 256.0 - Avatar::HEIGHT);

        // run right, reach top speed, face right
        let run = PlayerInput {
            right: true,
            ..Default::default()
        };
        for _ in 0..30 {
            update(&mut world, &level, &geo, run, DT);
        }
        let (avatar, pos2, vel) = state(&mut world);
        assert!(avatar.facing_right);
        assert!(pos2.x > pos.x);
        assert_eq!(vel.x, Avatar::RUN_SPEED);

        // jump launches once
        let jump = PlayerInput {
            jump_pressed: true,
            ..Default::default()
        };
        update(&mut world, &level, &geo, jump, DT);
        let (avatar, _, vel) = state(&mut world);
        assert!(!avatar.grounded);
        assert!(vel.y < 0.0);

        // pressing jump again mid-air does nothing
        let vy_before = vel.y;
        update(&mut world, &level, &geo, jump, DT);
        let (_, _, vel) = state(&mut world);
        assert!(vel.y > vy_before, "gravity applies, no double jump");
    }

    #[test]
    fn respawns_after_falling_out() {
        let level = test_level();
        let geo: Vec<SolidRect> = vec![]; // no floor at all
        let mut world = World::new();
        spawn_avatar(&mut world, Vec2::new(100.0, level.pixel_height() + 200.0));

        update(&mut world, &level, &geo, PlayerInput::default(), DT);
        let (_, pos, vel) = state(&mut world);
        assert_eq!(pos, level.player_spawn);
        assert_eq!(vel, Vec2::ZERO);
    }
}
