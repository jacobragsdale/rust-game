//! Every shipped map must be geometrically usable.
//!
//! These checks exist because the failures they catch are invisible: a
//! platform with too little headroom looks perfectly normal in the ASCII grid
//! and in the running game, and reads as a physics bug when you try to land
//! on it.

use std::path::{Path, PathBuf};

use ggez::glam::Vec2;

use supergame::assets::{Assets, StatBlock, StatTable};
use supergame::ecs::components::Pendulum;
use supergame::level::LevelData;
use supergame::physics::Aabb;
use supergame::systems::pendulum::ball_size;

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

/// The player's real collider, from `assets/data/stats.ron`. The whole point
/// of these checks is whether the shipped maps fit the shipped player, so the
/// box has to be the shipped one rather than a literal copied into this file.
fn player() -> std::sync::Arc<StatBlock> {
    StatTable::shipped()
        .get("player")
        .unwrap_or_else(|e| panic!("{e:#}"))
}

fn body() -> Vec2 {
    player().size()
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
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level = LevelData::load(&path, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let spawn = level.player_spawn;
        let body = body();
        let occupied = Aabb::new(spawn.x, spawn.y, body.x, body.y);
        if let Some(hit) = level.solids.iter().find(|s| occupied.overlaps(s)) {
            problems.push(format!(
                "{name}: spawn ({:.0}, {:.0}) is inside solid {hit:?}",
                spawn.x, spawn.y
            ));
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n  "));
}

/// A moving platform whose path runs into the level is the map-authoring bug
/// that reads as a physics bug.
///
/// Two things go wrong when it happens, and neither looks like a map problem
/// from inside the game. The platform disappears into a wall, which looks like
/// geometry failing to render. And it can wedge a body between itself and that
/// wall with nowhere legal to put it — the crush case in
/// `tests/physics_diagnostics.rs`, which the physics survives but which is
/// never what an author meant. A platform is a ride; a ride that ends inside a
/// wall is a mistake in the map, and this is where the repo catches those.
///
/// The path is sampled rather than bounded by the union of its ends, so a
/// diagonal path is checked where it actually goes rather than over the whole
/// rectangle it spans.
#[test]
fn every_moving_platform_has_a_path_clear_of_the_level() {
    /// Enough samples that nothing the size of a tile can hide between two of
    /// them on any path a map can author.
    const SAMPLES: u32 = 256;

    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level = LevelData::load(&path, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        for (index, mover) in level.movers.iter().enumerate() {
            for sample in 0..=SAMPLES {
                let at = mover.from.lerp(mover.to, sample as f32 / SAMPLES as f32);
                let box_ = Aabb::new(at.x, at.y, mover.size.x, mover.size.y);
                if let Some(hit) = level.solids.iter().find(|s| box_.overlaps(s)) {
                    problems.push(format!(
                        "{name}: platform {index} ({:.0}, {:.0}) -> ({:.0}, {:.0}) passes \
                         through solid {hit:?} at ({:.0}, {:.0})",
                        mover.from.x, mover.from.y, mover.to.x, mover.to.y, at.x, at.y
                    ));
                    break;
                }
            }

            // ...and the spawn point is not somewhere the platform sweeps
            // through, for the same reason a spawn inside a wall is rejected.
            let spawn = level.player_spawn;
            let body = body();
            let occupied = Aabb::new(spawn.x, spawn.y, body.x, body.y);
            for sample in 0..=SAMPLES {
                let at = mover.from.lerp(mover.to, sample as f32 / SAMPLES as f32);
                if occupied.overlaps(&Aabb::new(at.x, at.y, mover.size.x, mover.size.y)) {
                    problems.push(format!(
                        "{name}: platform {index} sweeps through the player spawn \
                         ({:.0}, {:.0})",
                        spawn.x, spawn.y
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} bad platform path(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// A swinging hazard whose arc is buried in the level is the same authoring
/// mistake as a moving platform whose path is, and it is worse to play.
///
/// A platform inside a wall at least looks wrong. A ball inside a wall is
/// simply *invisible* for part of its swing, and it stays lethal the whole
/// time it is in there — so the room contains a stretch that kills you with
/// nothing on screen to explain it. The failure reads as a physics bug, or as
/// nothing at all, and never as a map problem.
///
/// The arc is walked by angle rather than by tick, so the check says something
/// about the shape the map authored and not about how long the map said the
/// swing takes. `Pendulum::at_angle` exists for this.
#[test]
fn every_swinging_hazard_has_an_arc_clear_of_the_level() {
    /// Enough samples that nothing the size of a tile can hide between two of
    /// them on any arc a map can author.
    const SAMPLES: u32 = 256;

    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level = LevelData::load(&path, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        for (index, spawn) in level.pendulums.iter().enumerate() {
            let swing = Pendulum::new(
                spawn.anchor,
                spawn.length,
                spawn.amplitude,
                spawn.period,
                spawn.phase,
            );
            let size = ball_size(spawn.radius);
            // The ball's box at `sample` of the way from one extreme to the
            // other. Angles, not ticks, so the samples are spread evenly along
            // the arc rather than bunched at the ends where the ball dwells.
            let ball_at = |sample: u32| {
                let t = sample as f32 / SAMPLES as f32;
                let at = swing.at_angle(spawn.amplitude * (2.0 * t - 1.0));
                Aabb::new(at.x - size.x / 2.0, at.y - size.y / 2.0, size.x, size.y)
            };

            for sample in 0..=SAMPLES {
                let ball = ball_at(sample);
                if let Some(hit) = level.solids.iter().find(|s| ball.overlaps(s)) {
                    problems.push(format!(
                        "{name}: swing {index} anchored at ({:.0}, {:.0}) passes through \
                         solid {hit:?} at ({:.0}, {:.0})",
                        swing.anchor.x,
                        swing.anchor.y,
                        ball.x + size.x / 2.0,
                        ball.y + size.y / 2.0
                    ));
                    break;
                }
            }

            // ...and the ball must not sweep the spawn point. A hazard that
            // reaches it is a death *loop*: you respawn inside the thing that
            // killed you, and the run is over.
            let spawn_point = level.player_spawn;
            let body = body();
            let occupied = Aabb::new(spawn_point.x, spawn_point.y, body.x, body.y);
            for sample in 0..=SAMPLES {
                if occupied.overlaps(&ball_at(sample)) {
                    problems.push(format!(
                        "{name}: swing {index} sweeps through the player spawn ({:.0}, {:.0})",
                        spawn_point.x, spawn_point.y
                    ));
                    break;
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} bad swing arc(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}
