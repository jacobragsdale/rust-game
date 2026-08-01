//! The primary, human/model-readable map format: an ASCII grid where each
//! character is one tile, plus a declarative entity list. Tile art is chosen
//! by auto-tiling rules from the tileset definition, so map files never
//! contain tile indices.
//!
//! Legend:
//!   `#` solid          `=` one-way platform    `^` hazard (spikes)
//!   `.` / ` ` empty    `P` player spawn        `K` knight (entity)
//!   `F` fire (a hazard on the default cycle; author the timing with a
//!       `Fire(cell: (x, y), period: …, duty: …, phase: …)` entry instead)
//!
//! Geometry that moves has no grid character, because a character can say where
//! something is but not where it goes. A moving platform is a
//! `Platform(from: (x, y), to: (x, y), …)` entry in the entity list.

use std::fs;
use std::path::Path;

use anyhow::Context as _;
use ggez::glam::Vec2;
use serde::Deserialize;

use crate::assets::{Assets, AutotileRules, StatTable};
use crate::level::{merge_runs, EntitySpawn, FireSpawn, LevelData, MoverSpawn};

/// Spelled `Level(...)` in the map files.
#[derive(Debug, Deserialize)]
#[serde(rename = "Level")]
struct LevelDef {
    tileset: String,
    grid: Vec<String>,
    #[serde(default)]
    entities: Vec<EntityDef>,
}

#[derive(Debug, Deserialize)]
enum EntityDef {
    Npc {
        kind: String,
        cell: (u32, u32),
    },
    Door {
        cell: (u32, u32),
        to: String,
    },
    /// A fire with authored timing. Every field but the cell defaults to what
    /// the grid's `F` uses, so `Fire(cell: (3, 5), phase: 60)` is enough to
    /// make one take turns with a plain `F`.
    Fire {
        cell: (u32, u32),
        #[serde(default = "default_fire_period")]
        period: u32,
        #[serde(default = "default_fire_duty")]
        duty: u32,
        #[serde(default)]
        phase: u32,
    },
    /// A platform shuttling between two cells. `from` and `to` are the cell
    /// its box's top-left sits in at either end of the path; the box is
    /// `tiles` tiles wide, and as thick as a one-way strip or a whole tile
    /// depending on `one_way`. Everything but the two ends has a default, so
    /// `Platform(from: (4, 8), to: (12, 8))` is a complete platform.
    Platform {
        from: (u32, u32),
        to: (u32, u32),
        #[serde(default = "default_platform_tiles")]
        tiles: u32,
        #[serde(default = "default_platform_speed")]
        speed: f32,
        #[serde(default)]
        one_way: bool,
        #[serde(default)]
        phase: u32,
    },
}

fn default_fire_period() -> u32 {
    crate::systems::hazard::FIRE_PERIOD
}

fn default_fire_duty() -> u32 {
    crate::systems::hazard::FIRE_DUTY
}

/// Three tiles: wide enough to stand on comfortably and to land on from a
/// jump, narrow enough to read as a platform rather than as moving floor.
fn default_platform_tiles() -> u32 {
    3
}

/// 60 px/s — under half a walk. A platform is a ride, not a race, and a slow
/// one is legible: you can watch it, decide, and step on.
fn default_platform_speed() -> f32 {
    60.0
}

/// Load an ASCII map. `player` is the player's collider box, which the spawn
/// resolver needs and which no entity exists yet to supply — see
/// [`crate::level::LevelData::load`].
pub fn load(path: &Path, assets: &mut Assets, player: Vec2) -> anyhow::Result<LevelData> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let def: LevelDef =
        ron::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))?;
    let tileset = assets.tileset(&def.tileset)?;
    build(def, tileset.tile_size as f32, &tileset.rules, player)
        .with_context(|| format!("invalid map {}", path.display()))
}

/// The tile size fixture grids are built at, matching the shipped tilesets.
pub const FIXTURE_TILE_SIZE: f32 = 32.0;

/// Placeholder art for [`from_grid`]. Collision comes from the grid
/// characters, never from these indices, so any distinct values will do —
/// distinct so that a test *could* assert on which tile was chosen.
const FIXTURE_RULES: AutotileRules = AutotileRules {
    solid_top_left: 0,
    solid_top: 1,
    solid_top_right: 2,
    solid_left: 3,
    solid_fill: 4,
    solid_right: 5,
    platform: 6,
    background: Vec::new(),
};

/// Build a level from an ASCII grid with placeholder tile art. See
/// [`crate::level::LevelData::from_grid`].
///
/// The player's box comes from the shipped stat table rather than from a
/// parameter: a fixture grid has no asset cache to thread one through, and the
/// spawn point of a fixture has to be the spawn point the real game would
/// compute or the fixture is testing a different player. This is the same call
/// `Sim::fixture` makes for the attack and stat tables, for the same reason.
pub fn from_grid(grid: &[&str]) -> anyhow::Result<LevelData> {
    let def = LevelDef {
        tileset: "fixture".to_string(),
        grid: grid.iter().map(|row| row.to_string()).collect(),
        entities: Vec::new(),
    };
    let player = StatTable::shipped().get("player")?.size();
    build(def, FIXTURE_TILE_SIZE, &FIXTURE_RULES, player)
}

#[derive(Clone, Copy, PartialEq)]
enum Cell {
    Empty,
    Solid,
    Platform,
    Hazard,
}

fn build(
    def: LevelDef,
    tile_size: f32,
    rules: &AutotileRules,
    player: Vec2,
) -> anyhow::Result<LevelData> {
    let height = def.grid.len() as u32;
    anyhow::ensure!(height > 0, "map grid is empty");
    let width = def.grid.iter().map(|r| r.chars().count()).max().unwrap() as u32;

    let mut cells = vec![Cell::Empty; (width * height) as usize];
    let mut player_spawn = None;
    let mut entities = Vec::new();
    let mut fires = Vec::new();
    let mut movers = Vec::new();

    for (y, row) in def.grid.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let (x32, y32) = (x as u32, y as u32);
            let index = (y32 * width + x32) as usize;
            match ch {
                '#' => cells[index] = Cell::Solid,
                '=' => cells[index] = Cell::Platform,
                '^' => cells[index] = Cell::Hazard,
                '.' | ' ' => {}
                'P' => {
                    anyhow::ensure!(player_spawn.is_none(), "multiple player spawns");
                    player_spawn = Some(cell_floor_pos(x32, y32, tile_size, player.x, player.y));
                }
                'K' => entities.push(EntitySpawn {
                    kind: "knight".to_string(),
                    pos: Vec2::new(x as f32 * tile_size, y as f32 * tile_size),
                }),
                'F' => fires.push(FireSpawn {
                    cell: Vec2::new(x as f32 * tile_size, y as f32 * tile_size),
                    period: crate::systems::hazard::FIRE_PERIOD,
                    duty: crate::systems::hazard::FIRE_DUTY,
                    phase: 0,
                }),
                other => anyhow::bail!("unknown map character {other:?} at ({x}, {y})"),
            }
        }
    }

    let player_spawn = player_spawn.context("map has no player spawn (P)")?;

    for entity in &def.entities {
        match entity {
            EntityDef::Npc { kind, cell } => entities.push(EntitySpawn {
                kind: kind.clone(),
                pos: Vec2::new(cell.0 as f32 * tile_size, cell.1 as f32 * tile_size),
            }),
            EntityDef::Door { cell, to } => entities.push(EntitySpawn {
                kind: format!("door:{to}"),
                pos: Vec2::new(cell.0 as f32 * tile_size, cell.1 as f32 * tile_size),
            }),
            EntityDef::Fire {
                cell,
                period,
                duty,
                phase,
            } => fires.push(FireSpawn {
                cell: Vec2::new(cell.0 as f32 * tile_size, cell.1 as f32 * tile_size),
                period: *period,
                duty: *duty,
                phase: *phase,
            }),
            EntityDef::Platform {
                from,
                to,
                tiles,
                speed,
                one_way,
                phase,
            } => {
                anyhow::ensure!(*tiles > 0, "a platform must be at least one tile wide");
                anyhow::ensure!(*speed > 0.0, "a platform's speed must be positive");
                let corner =
                    |c: &(u32, u32)| Vec2::new(c.0 as f32 * tile_size, c.1 as f32 * tile_size);
                movers.push(MoverSpawn {
                    from: corner(from),
                    to: corner(to),
                    // A one-way platform presents the same thin strip a `=`
                    // cell carves, so the collision is in the same band whether
                    // it moves or not; a solid one fills its cell.
                    size: Vec2::new(
                        *tiles as f32 * tile_size,
                        if *one_way {
                            crate::level::ONE_WAY_THICKNESS
                        } else {
                            tile_size
                        },
                    ),
                    speed: *speed,
                    one_way: *one_way,
                    phase: *phase,
                });
            }
        }
    }

    let at = |x: i64, y: i64| -> Cell {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            Cell::Solid // out of bounds counts as solid so map borders render as fill
        } else {
            cells[(y as u32 * width + x as u32) as usize]
        }
    };

    // Auto-tiling: pick atlas tiles from each solid cell's neighbors.
    let mut tiles = vec![None; (width * height) as usize];
    let mut background = vec![None; (width * height) as usize];
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let index = (y as u32 * width + x as u32) as usize;
            match at(x, y) {
                Cell::Solid => {
                    let open_up = at(x, y - 1) != Cell::Solid;
                    let open_left = at(x - 1, y) != Cell::Solid;
                    let open_right = at(x + 1, y) != Cell::Solid;
                    tiles[index] = Some(match (open_up, open_left, open_right) {
                        (true, true, _) => rules.solid_top_left,
                        (true, _, true) => rules.solid_top_right,
                        (true, false, false) => rules.solid_top,
                        (false, true, _) => rules.solid_left,
                        (false, false, true) => rules.solid_right,
                        (false, false, false) => rules.solid_fill,
                    });
                }
                Cell::Platform => {
                    tiles[index] = Some(rules.platform);
                }
                Cell::Empty | Cell::Hazard => {}
            }
            if at(x, y) != Cell::Solid && !rules.background.is_empty() {
                // deterministic variety, stable across loads
                let variant = (x as usize * 7 + y as usize * 13) % rules.background.len();
                background[index] = Some(rules.background[variant]);
            }
        }
    }

    let solid_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Solid).collect();
    let platform_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Platform).collect();
    let hazard_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Hazard).collect();

    Ok(LevelData {
        tileset: def.tileset,
        width,
        height,
        tile_size,
        background,
        tiles,
        solids: merge_runs(&solid_flags, width, height, tile_size, tile_size, 0.0),
        // Thin strip at the top of the cell: land on it, jump through it.
        // The tileset's `platform` tile must draw its surface in exactly this
        // band, or the collision sits somewhere the player cannot see.
        one_way: merge_runs(
            &platform_flags,
            width,
            height,
            tile_size,
            crate::level::ONE_WAY_THICKNESS,
            0.0,
        ),
        // Spikes occupy the lower half of their cell.
        hazards: merge_runs(
            &hazard_flags,
            width,
            height,
            tile_size,
            tile_size / 2.0,
            tile_size / 2.0,
        ),
        fires,
        movers,
        player_spawn,
        entities,
    })
}

/// Position an entity of the given collider size standing on the floor of a
/// cell, horizontally centered.
fn cell_floor_pos(x: u32, y: u32, tile_size: f32, w: f32, h: f32) -> Vec2 {
    Vec2::new(
        x as f32 * tile_size + (tile_size - w) / 2.0,
        (y + 1) as f32 * tile_size - h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::Aabb;

    fn rules() -> AutotileRules {
        AutotileRules {
            solid_top_left: 10,
            solid_top: 11,
            solid_top_right: 12,
            solid_left: 20,
            solid_fill: 21,
            solid_right: 22,
            platform: 30,
            background: vec![40, 41],
        }
    }

    /// The player's real box, from `assets/data/stats.ron` — the same one
    /// `LevelData::load` threads in.
    fn player() -> Vec2 {
        StatTable::shipped()
            .get("player")
            .expect("`player` has a stat block")
            .size()
    }

    fn parse(grid: &[&str]) -> LevelData {
        let def = LevelDef {
            tileset: "test".to_string(),
            grid: grid.iter().map(|s| s.to_string()).collect(),
            entities: vec![],
        };
        build(def, 32.0, &rules(), player()).unwrap()
    }

    #[test]
    fn solids_merge_and_autotile_picks_edges() {
        let level = parse(&[
            "P....", //
            "#####", //
            "#####",
        ]);

        // one merged rect for each solid row
        assert_eq!(level.solids.len(), 2);
        assert_eq!(level.solids[0], Aabb::new(0.0, 32.0, 160.0, 32.0));

        // top row of the floor: exposed above; left/right map edges count as
        // solid neighbors, so col 0 is plain "top", not a corner
        let tile = |x: u32, y: u32| level.tiles[(y * level.width + x) as usize];
        assert_eq!(tile(0, 1), Some(11)); // top (map edge on the left)
        assert_eq!(tile(2, 1), Some(11)); // top mid
        assert_eq!(tile(2, 2), Some(21)); // buried fill
                                          // background never covers solid cells but fills empty ones
        let row0_start = 0usize; // first cell of the empty row
        let row1_start = level.width as usize; // first cell of the solid row
        assert!(level.background[row0_start].is_some());
        assert!(level.background[row1_start].is_none());
    }

    #[test]
    fn freestanding_block_gets_corners() {
        let level = parse(&[
            "P.....", //
            "..##..", //
            "######",
        ]);
        let tile = |x: u32, y: u32| level.tiles[(y * level.width + x) as usize];
        assert_eq!(tile(2, 1), Some(10)); // top-left corner (open up + left)
        assert_eq!(tile(3, 1), Some(12)); // top-right corner (open up + right)
    }

    #[test]
    fn platforms_are_thin_one_way_strips() {
        let level = parse(&[
            "P.....", //
            "..===.", //
            "######",
        ]);
        assert_eq!(level.one_way.len(), 1);
        assert_eq!(level.one_way[0], Aabb::new(64.0, 32.0, 96.0, 8.0));
        assert!(level.solids.len() == 1); // platforms are not solids
    }

    #[test]
    fn hazards_fill_lower_half_of_cell() {
        let level = parse(&[
            "P....", //
            ".^^..", //
            "#####",
        ]);
        assert_eq!(level.hazards.len(), 1);
        assert_eq!(level.hazards[0], Aabb::new(32.0, 48.0, 64.0, 16.0));
    }

    #[test]
    fn player_spawn_stands_on_cell_floor() {
        let level = parse(&[
            ".P...", //
            "#####",
        ]);
        // horizontally centered in cell 1, feet on the bottom of row 0
        assert_eq!(
            level.player_spawn,
            Vec2::new(32.0 + (32.0 - player().x) / 2.0, 32.0 - player().y)
        );
    }

    /// A grid `F` is a fire on the house cycle; the entity form is the same
    /// thing with the timing written down. Both have to land in the same list,
    /// or a map that mixes them places two different kinds of fire.
    #[test]
    fn fires_come_from_the_grid_and_from_the_entity_list() {
        let def = LevelDef {
            tileset: "test".to_string(),
            grid: vec!["P.F..".to_string(), "#####".to_string()],
            entities: vec![EntityDef::Fire {
                cell: (4, 0),
                period: 90,
                duty: 30,
                phase: 45,
            }],
        };
        let level = build(def, 32.0, &rules(), player()).unwrap();

        assert_eq!(level.fires.len(), 2);
        assert_eq!(level.fires[0].cell, Vec2::new(64.0, 0.0));
        assert_eq!(
            (
                level.fires[0].period,
                level.fires[0].duty,
                level.fires[0].phase
            ),
            (
                crate::systems::hazard::FIRE_PERIOD,
                crate::systems::hazard::FIRE_DUTY,
                0
            ),
            "the grid character takes the defaults"
        );
        assert_eq!(level.fires[1].cell, Vec2::new(128.0, 0.0));
        assert_eq!(
            (
                level.fires[1].period,
                level.fires[1].duty,
                level.fires[1].phase
            ),
            (90, 30, 45)
        );

        // A fire is not an NPC: it must not appear in the list that spawn
        // indices — `knight.0` and friends — are counted from.
        assert!(level.entities.is_empty());
    }

    /// Timing is optional in the authored form, so `Fire(cell: (x, y),
    /// phase: 60)` is enough to make one alternate with a plain `F`.
    #[test]
    fn an_authored_fire_defaults_its_timing() {
        let level: LevelDef = ron::from_str(
            r#"Level(
                tileset: "test",
                grid: ["P....", "....."],
                entities: [Fire(cell: (3, 0), phase: 60)],
            )"#,
        )
        .expect("the short form parses");
        let level = build(level, 32.0, &rules(), player()).unwrap();

        let fire = level.fires[0];
        assert_eq!(fire.period, crate::systems::hazard::FIRE_PERIOD);
        assert_eq!(fire.duty, crate::systems::hazard::FIRE_DUTY);
        assert_eq!(fire.phase, 60);
    }

    #[test]
    fn grid_entities_are_collected() {
        let level = parse(&[
            "P..K.", //
            "#####",
        ]);
        assert_eq!(level.entities.len(), 1);
        assert_eq!(level.entities[0].kind, "knight");
    }

    /// Cells in, pixels out — and a solid platform fills its cell while a
    /// one-way one is the same thin strip a `=` carves, so the collision band
    /// does not depend on whether a platform happens to move.
    #[test]
    fn a_platform_is_authored_in_cells_and_lands_in_pixels() {
        let level: LevelDef = ron::from_str(
            r#"Level(
                tileset: "test",
                grid: ["P.........", ".........."],
                entities: [
                    Platform(from: (2, 0), to: (7, 0), tiles: 2, speed: 80.0, phase: 15),
                    Platform(from: (4, 0), to: (4, 0), one_way: true),
                ],
            )"#,
        )
        .expect("the platform form parses");
        let level = build(level, 32.0, &rules(), player()).unwrap();

        assert_eq!(level.movers.len(), 2);
        let moving = level.movers[0];
        assert_eq!(moving.from, Vec2::new(64.0, 0.0));
        assert_eq!(moving.to, Vec2::new(224.0, 0.0));
        assert_eq!(
            moving.size,
            Vec2::new(64.0, 32.0),
            "two tiles, a tile thick"
        );
        assert_eq!(moving.speed, 80.0);
        assert_eq!(moving.phase, 15);
        assert!(!moving.one_way);

        let thin = level.movers[1];
        assert!(thin.one_way);
        assert_eq!(thin.size.y, crate::level::ONE_WAY_THICKNESS);
        assert_eq!(thin.size.x, 3.0 * 32.0, "three tiles by default");
        assert_eq!(thin.speed, 60.0, "and the house speed");

        // A platform is geometry, not an NPC: it must stay out of the list
        // that `knight.0` and friends are counted from.
        assert!(level.entities.is_empty());
    }

    #[test]
    fn a_degenerate_platform_is_rejected_rather_than_shipped() {
        for entity in [
            "Platform(from: (1, 0), to: (5, 0), tiles: 0)",
            "Platform(from: (1, 0), to: (5, 0), speed: 0.0)",
        ] {
            let def: LevelDef = ron::from_str(&format!(
                r#"Level(tileset: "test", grid: ["P....", "....."], entities: [{entity}])"#
            ))
            .expect("parses");
            assert!(
                build(def, 32.0, &rules(), player()).is_err(),
                "{entity} should not have been accepted"
            );
        }
    }

    #[test]
    fn unknown_characters_are_rejected() {
        let def = LevelDef {
            tileset: "test".to_string(),
            grid: vec!["P?#".to_string()],
            entities: vec![],
        };
        assert!(build(def, 32.0, &rules(), player()).is_err());
    }

    /// End-to-end: the shipped map + tileset + player clips must all parse.
    /// (The original RON struct-name mismatch slipped through because no
    /// test read the real files.)
    #[test]
    fn shipped_assets_parse() {
        let mut assets = crate::assets::Assets::new();
        let map_path = assets.base_dir().join("maps/castle.ron");

        let level = load(&map_path, &mut assets, player()).unwrap();
        assert!(!level.solids.is_empty());
        assert!(!level.one_way.is_empty());
        assert!(!level.hazards.is_empty());
        assert_eq!(level.entities[0].kind, "knight");

        let clips = assets.clip_set("player").unwrap();
        for clip in ["idle", "run", "jump", "fall"] {
            assert!(clips.clip(clip).is_some(), "missing player clip {clip:?}");
        }
    }

    #[test]
    fn missing_spawn_is_rejected() {
        let def = LevelDef {
            tileset: "test".to_string(),
            grid: vec!["###".to_string()],
            entities: vec![],
        };
        assert!(build(def, 32.0, &rules(), player()).is_err());
    }
}
