//! Every shipped map must be geometrically usable.
//!
//! These checks exist because the failures they catch are invisible: a
//! platform with too little headroom looks perfectly normal in the ASCII grid
//! and in the running game, and reads as a physics bug when you try to land
//! on it.

use std::path::{Path, PathBuf};

use ggez::glam::Vec2;

use supergame::assets::Assets;
use supergame::ecs::components::Avatar;
use supergame::level::LevelData;

fn map_paths() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    paths.sort();
    paths
}

fn body() -> Vec2 {
    Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)
}

#[test]
fn every_map_parses() {
    let mut assets = Assets::new();
    for path in map_paths() {
        LevelData::load(&path, &mut assets).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
    }
}

/// The player is 34px tall and a tile is 32px, so a single empty row above a
/// platform is not standing room. Two pixels is the whole difference between
/// a usable platform and one the player falls straight through.
#[test]
fn every_one_way_platform_has_room_to_stand_on() {
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level = LevelData::load(&path, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        for issue in level.blocked_platforms(body()) {
            problems.push(format!("{name}: {}", issue.describe()));
        }
    }

    assert!(
        problems.is_empty(),
        "{} unusable platform(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// A spawn point inside geometry leaves the player being shoved out by the
/// depenetration fallback on the first tick, which looks like a teleport.
#[test]
fn every_spawn_point_is_clear_of_solid_geometry() {
    use supergame::physics::Aabb;

    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level = LevelData::load(&path, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let spawn = level.player_spawn;
        let occupied = Aabb::new(spawn.x, spawn.y, Avatar::WIDTH, Avatar::HEIGHT);
        if let Some(hit) = level.solids.iter().find(|s| occupied.overlaps(s)) {
            problems.push(format!(
                "{name}: spawn ({:.0}, {:.0}) is inside solid {hit:?}",
                spawn.x, spawn.y
            ));
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n  "));
}
