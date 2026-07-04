//! Player intent + integration. Collision response happens afterwards in
//! `systems::collision`, mirroring the original update order.

use hecs::World;

use crate::ecs::components::{Dead, Player, Position, Velocity};
use crate::systems::input::PlayerInput;

pub fn update(world: &mut World, input: PlayerInput, world_width: f32, dt: f32) {
    for (_, (player, pos, vel, dead)) in
        world.query_mut::<(&mut Player, &mut Position, &mut Velocity, Option<&Dead>)>()
    {
        if dead.is_some() {
            player.prev_pos = pos.0;
            vel.0.y += Player::GRAVITY * dt;
            pos.0.y += vel.0.y * dt;
            continue;
        }

        player.was_grounded = player.grounded;

        // Input -> acceleration. `grounded` still holds last tick's contact
        // result here, which is what accel/friction should be based on.
        player.horizontal_input = if input.left {
            -1.0
        } else if input.right {
            1.0
        } else {
            0.0
        };

        let accel = if player.grounded {
            Player::GROUND_ACCEL
        } else {
            Player::AIR_ACCEL
        };
        vel.0.x += player.horizontal_input * accel * dt;
        vel.0.x = vel.0.x.clamp(-Player::MAX_SPEED, Player::MAX_SPEED);

        if input.jump && player.can_jump {
            player.can_jump = false;
            player.grounded = false;
            player.grounded_ticks = 0;
            vel.0.y = -Player::JUMP_SPEED;
        }

        // Ground friction when no input is held.
        if player.grounded && player.horizontal_input == 0.0 {
            let step = Player::FRICTION * dt;
            if vel.0.x > 0.0 {
                vel.0.x -= step;
                if vel.0.x < step {
                    vel.0.x = 0.0;
                }
            } else if vel.0.x < 0.0 {
                vel.0.x += step;
                if vel.0.x > -step {
                    vel.0.x = 0.0;
                }
            }
        }

        // Integrate. Contact state is re-established by the collision system.
        player.prev_pos = pos.0;
        player.grounded = false;
        pos.0 += vel.0 * dt;
        pos.0.x += player.ride_speed * dt;
        vel.0.y += Player::GRAVITY * dt;
        player.ride_speed = 0.0;

        // Keep the player inside horizontal world bounds.
        if pos.0.x < 0.0 {
            pos.0.x = 0.0;
            vel.0.x = 0.0;
        } else if pos.0.x > world_width - Player::SIZE {
            pos.0.x = world_width - Player::SIZE;
            vel.0.x = 0.0;
        }
    }
}

/// Kill the player: freeze horizontal motion, keep gravity so they fall off
/// screen, and tag the entity `Dead`.
pub fn mark_dead(world: &mut World) {
    let mut entity = None;
    for (e, (player, vel)) in world.query_mut::<(&mut Player, &mut Velocity)>() {
        player.can_jump = false;
        player.grounded = false;
        player.horizontal_input = 0.0;
        player.ride_speed = 0.0;
        vel.0.x = 0.0;
        entity = Some(e);
    }
    if let Some(e) = entity {
        let _ = world.insert_one(e, Dead);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Size;
    use ggez::glam::Vec2;

    const DT: f32 = 1.0 / 60.0;
    const WORLD_W: f32 = 1900.0;

    fn spawn_player(world: &mut World, pos: Vec2, vel: Vec2) -> hecs::Entity {
        world.spawn((
            Player::new(pos),
            Position(pos),
            Velocity(vel),
            Size(Vec2::splat(Player::SIZE)),
        ))
    }

    fn player_state(world: &mut World) -> (Player, Vec2, Vec2) {
        let (_, (p, pos, vel)) = world
            .query_mut::<(&Player, &Position, &Velocity)>()
            .into_iter()
            .next()
            .unwrap();
        (p.clone(), pos.0, vel.0)
    }

    #[test]
    fn jump_launches_upward_and_consumes_can_jump() {
        let mut world = World::new();
        spawn_player(&mut world, Vec2::new(100.0, 100.0), Vec2::ZERO);

        let input = PlayerInput {
            jump: true,
            ..Default::default()
        };
        update(&mut world, input, WORLD_W, DT);

        let (p, pos, vel) = player_state(&mut world);
        assert!(!p.can_jump);
        // one tick of gravity has been applied after launch
        assert_eq!(vel.y, -Player::JUMP_SPEED + Player::GRAVITY * DT);
        assert!(pos.y < 100.0);
    }

    #[test]
    fn friction_stops_a_grounded_player_with_no_input() {
        let mut world = World::new();
        spawn_player(&mut world, Vec2::new(100.0, 100.0), Vec2::new(100.0, 0.0));

        // grounded=true from Player::new; friction step is 180 px/s per tick
        update(&mut world, PlayerInput::default(), WORLD_W, DT);
        let (_, _, vel) = player_state(&mut world);
        assert_eq!(vel.x, 0.0); // 100 - 180 < 180 -> snapped to zero
    }

    #[test]
    fn horizontal_speed_is_clamped() {
        let mut world = World::new();
        spawn_player(&mut world, Vec2::new(100.0, 100.0), Vec2::ZERO);

        let input = PlayerInput {
            right: true,
            ..Default::default()
        };
        for _ in 0..10 {
            update(&mut world, input, WORLD_W, DT);
        }
        let (_, _, vel) = player_state(&mut world);
        assert_eq!(vel.x, Player::MAX_SPEED);
    }

    #[test]
    fn clamped_to_world_bounds() {
        let mut world = World::new();
        spawn_player(&mut world, Vec2::new(5.0, 100.0), Vec2::new(-500.0, 0.0));

        // moving fast left from near the edge
        let input = PlayerInput {
            left: true,
            ..Default::default()
        };
        update(&mut world, input, WORLD_W, DT);

        let (_, pos, vel) = player_state(&mut world);
        assert_eq!(pos.x, 0.0);
        assert_eq!(vel.x, 0.0);
    }

    #[test]
    fn dead_player_only_falls() {
        let mut world = World::new();
        spawn_player(&mut world, Vec2::new(100.0, 100.0), Vec2::new(300.0, 0.0));
        mark_dead(&mut world);

        let input = PlayerInput {
            right: true,
            jump: true,
            ..Default::default()
        };
        update(&mut world, input, WORLD_W, DT);

        let (_, pos, vel) = player_state(&mut world);
        assert_eq!(vel.x, 0.0); // horizontal motion frozen by mark_dead
        assert_eq!(pos.x, 100.0); // input ignored
        assert!(pos.y > 100.0); // still falling
    }
}
