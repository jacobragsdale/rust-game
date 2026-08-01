//! Turning map placements into entities.
//!
//! Maps name what they want by string — a `K` in the grid, or
//! `Npc(kind: "knight", ...)` in the entity list — and this is the one place
//! that decides what such a string becomes. Before it existed, [`EntitySpawn`]
//! was parsed out of every map and thrown away.
//!
//! A string registry rather than an enum on purpose: adding an NPC should be a
//! data file plus one arm here, and a map that names something unknown should
//! fail loudly at load with the name it did not recognize, rather than being a
//! compile error in a file the level designer never opens.

use std::sync::Arc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{ClipSet, StatBlock};
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Health, Hostile, Kind, Patrol, Position, Size, Sprite,
    Stats, Team, Velocity,
};
use crate::level::EntitySpawn;

/// Every entity kind a map may place. `Sim` loads a clip set and a stat block
/// per name, so a new kind needs `assets/data/animations/{kind}.ron` to exist
/// and an entry in `assets/data/stats.ron`.
pub const KINDS: &[&str] = &["knight"];

/// Spawn the player at the level's spawn point.
///
/// `stats` is the `"player"` block of `assets/data/stats.ron`. Every number
/// this entity is built from comes out of it — there are none left here.
pub fn player(
    world: &mut World,
    spawn: Vec2,
    clips: Arc<ClipSet>,
    stats: Arc<StatBlock>,
) -> hecs::Entity {
    let offset = art_offset(&clips);
    world.spawn((
        Avatar::new(stats.avatar()),
        Body::new(spawn, stats.gravity, stats.max_fall),
        Team::Player,
        Health::new(stats.max_health, stats.iframe_ticks),
        Attacking::default(),
        Position(spawn),
        Velocity(Vec2::ZERO),
        Size(stats.size()),
        Sprite { clips, offset },
        AnimationState::new("idle"),
        Stats(stats),
    ))
}

/// Spawn one map placement. `pos` is the top-left of the cell it was placed in;
/// the entity is stood on that cell's floor, horizontally centred, the same way
/// the player's spawn point is resolved.
///
/// `stats` is the block `assets/data/stats.ron` holds for `placement.kind`.
pub fn entity(
    world: &mut World,
    placement: &EntitySpawn,
    tile_size: f32,
    clips: Arc<ClipSet>,
    stats: Arc<StatBlock>,
) -> anyhow::Result<hecs::Entity> {
    match placement.kind.as_str() {
        "knight" => {
            let pos = stand_in_cell(placement.pos, tile_size, stats.size());
            let offset = art_offset(&clips);
            Ok(world.spawn((
                Kind(placement.kind.clone()),
                Patrol::new(1.0, stats.run_speed),
                Hostile::new(pos.x, &stats.attack),
                Team::Enemy,
                Health::new(stats.max_health, stats.iframe_ticks),
                Attacking::default(),
                Position(pos),
                Velocity(Vec2::ZERO),
                Size(stats.size()),
                Body::new(pos, stats.gravity, stats.max_fall),
                Sprite { clips, offset },
                AnimationState::new("idle"),
                Stats(stats),
            )))
        }
        other => anyhow::bail!(
            "map places unknown entity kind `{other}` (known kinds: {})",
            KINDS.join(", ")
        ),
    }
}

/// The drawing nudge this art asks for, as a vector.
fn art_offset(clips: &ClipSet) -> Vec2 {
    let (x, y) = clips.offset();
    Vec2::new(x, y)
}

/// Stand a collider of `size` on the floor of the cell whose top-left is
/// `cell`, centred horizontally.
fn stand_in_cell(cell: Vec2, tile_size: f32, size: Vec2) -> Vec2 {
    Vec2::new(
        cell.x + (tile_size - size.x) / 2.0,
        cell.y + tile_size - size.y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(kind: &str) -> EntitySpawn {
        EntitySpawn {
            kind: kind.to_string(),
            pos: Vec2::new(64.0, 96.0),
        }
    }

    fn clips() -> Arc<ClipSet> {
        Arc::new(crate::sim::fixture_clips())
    }

    /// The real numbers, from the real file. Movement and combat values are
    /// content; a test that invented its own would not be testing the game.
    fn stats(kind: &str) -> Arc<StatBlock> {
        crate::assets::StatTable::shipped()
            .get(kind)
            .unwrap_or_else(|e| panic!("{e:#}"))
    }

    #[test]
    fn a_knight_stands_on_the_floor_of_its_cell() {
        let mut world = World::new();
        let size = stats("knight").size();
        let e = entity(
            &mut world,
            &placement("knight"),
            32.0,
            clips(),
            stats("knight"),
        )
        .unwrap();

        let pos = world.get::<&Position>(e).unwrap().0;
        assert_eq!(pos.y + size.y, 96.0 + 32.0, "feet on the cell floor");
        assert_eq!(pos.x + size.x / 2.0, 64.0 + 16.0, "centred in the cell");
    }

    /// A typo in a map should name itself, not spawn nothing and leave the
    /// designer wondering where their NPC went.
    #[test]
    fn an_unknown_kind_is_an_error_naming_the_kind() {
        let mut world = World::new();
        let err = entity(
            &mut world,
            &placement("dragon"),
            32.0,
            clips(),
            stats("knight"),
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("dragon"), "{text}");
        assert!(text.contains("knight"), "should list what is known: {text}");
    }

    /// Every advertised kind must actually spawn, or `Sim` would try to load a
    /// clip set for something that cannot exist.
    #[test]
    fn every_advertised_kind_spawns() {
        for kind in KINDS {
            let mut world = World::new();
            entity(&mut world, &placement(kind), 32.0, clips(), stats(kind))
                .unwrap_or_else(|e| panic!("`{kind}` is advertised but fails to spawn: {e:#}"));
        }
    }

    /// The table is the other half of `KINDS`: a kind that spawns but has no
    /// block cannot be built at all. `tests/data.rs` checks the same edge from
    /// the content side; this one fails without leaving the crate.
    #[test]
    fn every_advertised_kind_has_a_stat_block() {
        let table = crate::assets::StatTable::shipped();
        for kind in std::iter::once("player").chain(KINDS.iter().copied()) {
            table
                .get(kind)
                .unwrap_or_else(|e| panic!("`{kind}` is spawnable but has no stats: {e:#}"));
        }
    }

    /// An unknown kind must name itself here too, not fail with a lookup miss
    /// somewhere downstream.
    #[test]
    fn an_unknown_kind_has_no_stat_block_and_says_so() {
        let err = crate::assets::StatTable::shipped()
            .get("dragon")
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("dragon"), "{text}");
        assert!(text.contains("knight"), "should list what is known: {text}");
    }
}
