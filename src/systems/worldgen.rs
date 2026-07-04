//! Endless-runner terrain generation: despawns entities that scrolled off the
//! left edge and emits spawn requests to keep the platform buffer filled past
//! the right edge.

use hecs::World;
use rand::Rng;

use crate::ecs::components::{Position, Scroller, Size, Solid, SpikeBall};
use crate::ecs::events::{Events, SpawnRequest};

#[derive(Clone, Debug)]
pub struct WorldGen {
    pub spawn_margin: f32,
    pub platform_height: f32,
    pub platform_width_range: (f32, f32),
    pub platform_gap_range: (f32, f32),
    pub platform_y_levels: Vec<f32>,
    pub spike_chance: f64,
}

impl WorldGen {
    pub fn new(world_height: f32) -> Self {
        WorldGen {
            spawn_margin: 400.0,
            platform_height: 30.0,
            platform_width_range: (180.0, 360.0),
            platform_gap_range: (220.0, 360.0),
            platform_y_levels: vec![
                world_height - 150.0,
                world_height - 300.0,
                world_height - 450.0,
            ],
            spike_chance: 0.1,
        }
    }
}

/// Despawn scrolling entities that are fully off the left edge.
pub fn cleanup(world: &mut World, gen: &WorldGen) {
    let margin = gen.spawn_margin;
    let mut doomed = Vec::new();

    for (e, (pos, size, _, _)) in world
        .query::<(&Position, &Size, &Solid, &Scroller)>()
        .iter()
    {
        if pos.0.x + size.0.x < -margin {
            doomed.push(e);
        }
    }
    for (e, (pos, _, _)) in world.query::<(&Position, &SpikeBall, &Scroller)>().iter() {
        if pos.0.x + SpikeBall::TOTAL_RADIUS < -margin {
            doomed.push(e);
        }
    }

    for e in doomed {
        let _ = world.despawn(e);
    }
}

/// Emit spawn requests until platforms cover the area past the right edge.
pub fn ensure_buffer(
    world: &mut World,
    gen: &WorldGen,
    rng: &mut impl Rng,
    view_width: f32,
    events: &mut Events,
) {
    let mut right_edge = world
        .query_mut::<(&Position, &Size, &Solid)>()
        .into_iter()
        .map(|(_, (pos, size, _))| pos.0.x + size.0.x)
        .fold(0.0, f32::max);

    if right_edge == 0.0 {
        right_edge = view_width * 0.25;
    }

    fill_from(right_edge, gen, rng, view_width, events);
}

/// Build the starting world: the wide start platform plus terrain out past
/// the right edge. Returns the player spawn position.
pub fn initial_world(
    gen: &WorldGen,
    rng: &mut impl Rng,
    view_width: f32,
    events: &mut Events,
) -> ggez::glam::Vec2 {
    use crate::ecs::components::Player;

    let start_width = 320.0;
    let start_x = 260.0;
    let start_y = gen.platform_y_levels[0];

    events.spawns.push(SpawnRequest::Platform {
        x: start_x,
        y: start_y,
        w: start_width,
        h: gen.platform_height,
    });
    fill_from(start_x + start_width, gen, rng, view_width, events);

    ggez::glam::Vec2::new(
        start_x + (start_width - Player::SIZE) / 2.0,
        start_y - Player::SIZE,
    )
}

fn fill_from(
    mut right_edge: f32,
    gen: &WorldGen,
    rng: &mut impl Rng,
    view_width: f32,
    events: &mut Events,
) {
    let target_edge = view_width + gen.spawn_margin;
    while right_edge < target_edge {
        let gap = rng.gen_range(gen.platform_gap_range.0..=gen.platform_gap_range.1);
        let width = rng.gen_range(gen.platform_width_range.0..=gen.platform_width_range.1);
        let y_index = rng.gen_range(0..gen.platform_y_levels.len());
        let y = gen.platform_y_levels[y_index];
        let x = right_edge + gap;

        events.spawns.push(SpawnRequest::Platform {
            x,
            y,
            w: width,
            h: gen.platform_height,
        });

        if rng.gen_bool(gen.spike_chance) {
            let ball_x = rng.gen_range(x..=(x + width));
            let ball_y = rng.gen_range((y - 200.0)..=(y - 50.0));
            events.spawns.push(SpawnRequest::SpikeBall {
                x: ball_x,
                y: ball_y,
            });
        }

        right_edge = x + width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::spawn;
    use ggez::glam::Vec2;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    const VIEW_W: f32 = 1900.0;

    #[test]
    fn buffer_fills_past_the_right_edge() {
        let mut world = World::new();
        let gen = WorldGen::new(1060.0);
        let mut rng = StdRng::seed_from_u64(7);
        let mut events = Events::default();

        ensure_buffer(&mut world, &gen, &mut rng, VIEW_W, &mut events);
        spawn::drain(&mut world, &mut events);

        let right_edge = world
            .query_mut::<(&Position, &Size, &Solid)>()
            .into_iter()
            .map(|(_, (pos, size, _))| pos.0.x + size.0.x)
            .fold(0.0, f32::max);
        assert!(right_edge >= VIEW_W + gen.spawn_margin);

        // every platform sits on a configured y level
        for (_, (pos, _, _)) in world.query_mut::<(&Position, &Size, &Solid)>() {
            assert!(gen.platform_y_levels.contains(&pos.0.y));
        }
    }

    #[test]
    fn cleanup_removes_offscreen_scrollers_only() {
        let mut world = World::new();
        let gen = WorldGen::new(1060.0);

        let offscreen = world.spawn((
            Position(Vec2::new(-1000.0, 910.0)),
            Size(Vec2::new(200.0, 30.0)),
            Solid,
            Scroller,
        ));
        let onscreen = world.spawn((
            Position(Vec2::new(500.0, 910.0)),
            Size(Vec2::new(200.0, 30.0)),
            Solid,
            Scroller,
        ));
        let offscreen_ball =
            world.spawn((Position(Vec2::new(-1000.0, 500.0)), SpikeBall, Scroller));

        cleanup(&mut world, &gen);

        assert!(!world.contains(offscreen));
        assert!(!world.contains(offscreen_ball));
        assert!(world.contains(onscreen));
    }

    #[test]
    fn initial_world_centers_player_on_start_platform() {
        let gen = WorldGen::new(1060.0);
        let mut rng = StdRng::seed_from_u64(7);
        let mut events = Events::default();

        let spawn_pos = initial_world(&gen, &mut rng, VIEW_W, &mut events);

        assert_eq!(
            spawn_pos,
            Vec2::new(260.0 + (320.0 - 100.0) / 2.0, 910.0 - 100.0)
        );
        assert!(matches!(
            events.spawns[0],
            SpawnRequest::Platform { x, w, .. } if x == 260.0 && w == 320.0
        ));
    }
}
