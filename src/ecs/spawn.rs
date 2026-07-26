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

use crate::assets::ClipSet;
use crate::ecs::components::{
    AnimationState, Attacking, Avatar, Body, Health, Kind, Patrol, Position, Size, Sprite, Team,
    Velocity,
};
use crate::level::EntitySpawn;

/// Every entity kind a map may place. `Sim` loads a clip set per name, so a
/// new kind needs `assets/data/animations/{kind}.ron` to exist.
pub const KINDS: &[&str] = &["knight"];

/// The knight's collider. Narrower and shorter than the player's, matching a
/// drawn body of about 26x30 px.
pub const KNIGHT_SIZE: Vec2 = Vec2::new(22.0, 30.0);
pub const KNIGHT_SPEED: f32 = 55.0;
pub const KNIGHT_HEALTH: i32 = 3;
pub const KNIGHT_GRAVITY: f32 = 1400.0;
pub const KNIGHT_MAX_FALL: f32 = 900.0;

/// Spawn the player at the level's spawn point.
pub fn player(world: &mut World, spawn: Vec2, clips: Arc<ClipSet>) -> hecs::Entity {
    let offset = art_offset(&clips);
    world.spawn((
        Avatar::new(),
        Avatar::body(spawn),
        Team::Player,
        Health::new(Avatar::MAX_HEALTH),
        Attacking::default(),
        Position(spawn),
        Velocity(Vec2::ZERO),
        Size(Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)),
        Sprite { clips, offset },
        AnimationState::new("idle"),
    ))
}

/// Spawn one map placement. `pos` is the top-left of the cell it was placed in;
/// the entity is stood on that cell's floor, horizontally centred, the same way
/// the player's spawn point is resolved.
pub fn entity(
    world: &mut World,
    placement: &EntitySpawn,
    tile_size: f32,
    clips: Arc<ClipSet>,
) -> anyhow::Result<hecs::Entity> {
    match placement.kind.as_str() {
        "knight" => {
            let pos = stand_in_cell(placement.pos, tile_size, KNIGHT_SIZE);
            let offset = art_offset(&clips);
            Ok(world.spawn((
                Kind(placement.kind.clone()),
                Patrol::new(1.0, KNIGHT_SPEED),
                Team::Enemy,
                Health::new(KNIGHT_HEALTH),
                Attacking::default(),
                Position(pos),
                Velocity(Vec2::ZERO),
                Size(KNIGHT_SIZE),
                Body::new(pos, KNIGHT_GRAVITY, KNIGHT_MAX_FALL),
                Sprite { clips, offset },
                AnimationState::new("idle"),
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

    #[test]
    fn a_knight_stands_on_the_floor_of_its_cell() {
        let mut world = World::new();
        let e = entity(&mut world, &placement("knight"), 32.0, clips()).unwrap();

        let pos = world.get::<&Position>(e).unwrap().0;
        assert_eq!(pos.y + KNIGHT_SIZE.y, 96.0 + 32.0, "feet on the cell floor");
        assert_eq!(
            pos.x + KNIGHT_SIZE.x / 2.0,
            64.0 + 16.0,
            "centred in the cell"
        );
    }

    /// A typo in a map should name itself, not spawn nothing and leave the
    /// designer wondering where their NPC went.
    #[test]
    fn an_unknown_kind_is_an_error_naming_the_kind() {
        let mut world = World::new();
        let err = entity(&mut world, &placement("dragon"), 32.0, clips()).unwrap_err();
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
            entity(&mut world, &placement(kind), 32.0, clips())
                .unwrap_or_else(|e| panic!("`{kind}` is advertised but fails to spawn: {e:#}"));
        }
    }
}
