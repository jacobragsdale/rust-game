//! Resolves the player against solid geometry, then applies post-contact
//! state (jump cooldown, air drag) — the ECS port of the original
//! `resolve_collisions` + `finalize_update` pair.

use hecs::World;

use crate::ecs::components::{Dead, Player, Position, Size, Solid, Velocity};
use crate::physics::{self, Aabb, SolidRect};

pub fn resolve_player(world: &mut World, scroll_speed: f32) {
    let solids: Vec<SolidRect> = world
        .query_mut::<(&Position, &Size, &Solid)>()
        .into_iter()
        .map(|(_, (pos, size, _))| {
            SolidRect::solid(
                Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y),
                scroll_speed,
            )
        })
        .collect();

    for (_, (player, pos, vel, size, dead)) in world.query_mut::<(
        &mut Player,
        &mut Position,
        &mut Velocity,
        &Size,
        Option<&Dead>,
    )>() {
        if dead.is_some() {
            continue;
        }

        let contact =
            physics::resolve_move(&mut pos.0, &mut vel.0, player.prev_pos, size.0, &solids);

        if contact.grounded {
            player.grounded = true;
            player.ride_speed = contact.ride_speed;
            if !player.was_grounded {
                // Fresh landing: jump goes on cooldown.
                player.grounded_ticks = 0;
                player.can_jump = false;
            }
        }

        if player.grounded {
            player.grounded_ticks += 1;
            if player.grounded_ticks >= Player::JUMP_DELAY_TICKS {
                player.can_jump = true;
            }
        } else if player.horizontal_input == 0.0 {
            vel.0.x *= Player::AIR_DRAG;
            if vel.0.x.abs() < Player::AIR_DRAG_STOP {
                vel.0.x = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::input::PlayerInput;
    use ggez::glam::Vec2;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn player_falls_onto_platform_and_can_jump_after_delay() {
        let mut world = World::new();
        // platform at y=500, player 30px above it
        world.spawn((
            Position(Vec2::new(0.0, 500.0)),
            Size(Vec2::new(400.0, 30.0)),
            Solid,
            crate::ecs::components::Scroller,
        ));
        let spawn = Vec2::new(100.0, 500.0 - Player::SIZE - 30.0);
        let mut player = Player::new(spawn);
        player.grounded = false;
        player.can_jump = false;
        world.spawn((
            player,
            Position(spawn),
            Velocity(Vec2::ZERO),
            Size(Vec2::splat(Player::SIZE)),
        ));

        // run full ticks until landed, then the jump-delay ticks
        let mut landed_at = None;
        for tick in 0..60 {
            crate::systems::player::update(&mut world, PlayerInput::default(), 1900.0, DT);
            resolve_player(&mut world, 420.0);
            let (_, (p, pos)) = world
                .query_mut::<(&Player, &Position)>()
                .into_iter()
                .next()
                .unwrap();
            if p.grounded && landed_at.is_none() {
                landed_at = Some(tick);
                assert_eq!(pos.0.y, 500.0 - Player::SIZE); // snapped onto platform
                assert_eq!(p.ride_speed, -420.0); // riding the scroll
                assert!(!p.can_jump); // jump on cooldown right after landing
            }
        }
        let landed_at = landed_at.expect("player never landed");
        assert!(landed_at < 59, "landed too late to observe cooldown");

        let (_, (p, _)) = world
            .query_mut::<(&Player, &Position)>()
            .into_iter()
            .next()
            .unwrap();
        assert!(p.can_jump, "jump should be available after the delay");
    }
}
