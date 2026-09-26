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
//!   `%` false wall — drawn exactly as `#` is, and not there at all: the way
//!       into a secret. Nothing tells it from stone but walking into it.
//!
//! Geometry that moves has no grid character, because a character can say where
//! something is but not where it goes. A moving platform is a
//! `Platform(from: (x, y), to: (x, y), …)` entry in the entity list, and a
//! swinging hazard is a `Swing(anchor: (x, y), …)` entry — a character could
//! name the cell the chain hangs from, but not how long the chain is or how
//! far it swings, which is all of what makes the arc.
//!
//! Everything else a level is furnished with is an entry too. Cells are
//! `(column, row)`; a thing "in" a cell stands on that cell's floor.
//!
//! ```ron
//! Npc(kind: "villager", cell: (11, 10), dialogue: "smith", flag: "quest.x.dead")
//! Door(cell: (38, 9), to: "maps/dungeon.ron", at: "gate", id: "east",
//!      locked: "iron_key", keep_key: false, art: "door")
//! Exit(cell: (39, 7), size: (1, 3), to: "maps/dungeon.ron", at: "west")
//! Spawn(id: "west", cell: (2, 9))
//! Chest(cell: (20, 9), items: [("coin", 10), ("minor_potion", 1)], id: "c1")
//! Item(cell: (5, 9), item: "coin", count: 3)
//! Checkpoint(cell: (30, 9))
//! Sign(cell: (4, 9), dialogue: "sign_welcome")
//! Lever(cell: (12, 9), flag: "quest.crypt.gate")
//! Gate(cell: (16, 8), height: 2, flag: "quest.crypt.gate")
//! Decor(cell: (7, 6), prop: "torch")
//! Trigger(cell: (3, 7), size: (1, 3), dialogue: "intro", when: "quest.x")
//! ```
//!
//! `to:` is a map path relative to `assets/`, and `at:` names a `Spawn` or a
//! `Door` id on that map; `tests/data.rs` checks every one resolves. What a
//! chest, an item, a lever or a door has done is remembered in a `world.` flag
//! — `world.<map>.<id>`, where an unnamed prop's id is its kind and cell,
//! `chest_20_9` — so it stays done across a load and across leaving and coming
//! back, and a tape can assert it. A map's `name:` is what the screen calls it
//! on arrival.

use std::path::Path;

use anyhow::Context as _;
use ggez::glam::Vec2;
use serde::Deserialize;

use crate::assets::{Assets, AutotileRules, StatTable};
use crate::level::{
    merge_runs, DecorSpawn, EntitySpawn, FireSpawn, LevelData, MoverSpawn, PendulumSpawn, PropKind,
    PropSpawn,
};

/// Spelled `Level(...)` in the map files.
#[derive(Debug, Deserialize)]
#[serde(rename = "Level")]
struct LevelDef {
    tileset: String,
    /// What the screen calls this place on arrival.
    #[serde(default)]
    name: Option<String>,
    grid: Vec<String>,
    #[serde(default)]
    entities: Vec<EntityDef>,
}

#[derive(Debug, Deserialize)]
enum EntityDef {
    Npc {
        kind: String,
        cell: (u32, u32),
        #[serde(default)]
        dialogue: Option<String>,
        #[serde(default)]
        flag: Option<String>,
    },
    Door {
        cell: (u32, u32),
        to: String,
        at: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        locked: Option<String>,
        #[serde(default)]
        keep_key: bool,
        #[serde(default)]
        art: Option<String>,
    },
    Exit {
        cell: (u32, u32),
        #[serde(default = "default_exit_size")]
        size: (u32, u32),
        to: String,
        at: String,
    },
    Spawn {
        id: String,
        cell: (u32, u32),
    },
    Chest {
        cell: (u32, u32),
        items: Vec<(String, u32)>,
        #[serde(default)]
        id: Option<String>,
    },
    Item {
        cell: (u32, u32),
        item: String,
        #[serde(default = "default_count")]
        count: u32,
        #[serde(default)]
        id: Option<String>,
    },
    Checkpoint {
        cell: (u32, u32),
    },
    Sign {
        cell: (u32, u32),
        dialogue: String,
    },
    Lever {
        cell: (u32, u32),
        flag: String,
        #[serde(default)]
        id: Option<String>,
    },
    Gate {
        cell: (u32, u32),
        flag: String,
        #[serde(default = "default_gate_height")]
        height: u32,
    },
    Decor {
        cell: (u32, u32),
        prop: String,
    },
    /// A region that opens a conversation the first time the player walks
    /// into it — once `when` is set, if it names a flag. The same default
    /// size as an exit: a doorway you cannot jump over.
    Trigger {
        cell: (u32, u32),
        #[serde(default = "default_exit_size")]
        size: (u32, u32),
        dialogue: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        when: Option<String>,
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
    /// A spiked ball on a chain, swinging under a fixed anchor. `anchor` is
    /// the cell the chain hangs from and the pivot is that cell's *centre* —
    /// a pivot is a point, not a box, so the corner every other cell reference
    /// means here would put the swing half a tile off what the map looks like.
    ///
    /// `amplitude` is the half-swing in **degrees** either side of straight
    /// down, and `period` is one full there-and-back in ticks. Everything but
    /// the anchor has a default, so `Swing(anchor: (12, 5))` is a complete
    /// hazard.
    Swing {
        anchor: (u32, u32),
        #[serde(default = "default_swing_length")]
        length: f32,
        #[serde(default = "default_swing_amplitude")]
        amplitude: f32,
        #[serde(default = "default_swing_period")]
        period: u32,
        #[serde(default)]
        phase: u32,
        #[serde(default = "default_swing_radius")]
        radius: f32,
    },
}

/// One tile wide and three tall: a doorway at the edge of the map that a
/// body cannot jump over.
fn default_exit_size() -> (u32, u32) {
    (1, 3)
}

fn default_count() -> u32 {
    1
}

/// Two tiles: taller than the player, so a closed gate cannot be walked
/// under or stood in.
fn default_gate_height() -> u32 {
    2
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

/// Three tiles of chain: long enough for the arc to read as a swing rather
/// than a wobble, short enough that the ball stays in one screen.
fn default_swing_length() -> f32 {
    96.0
}

/// Sixty degrees either side of straight down. Wide enough that the ball is
/// clearly high at the extremes and low in the middle — which is the whole
/// mechanic, since the only place it is at head height is the bottom.
fn default_swing_amplitude() -> f32 {
    60.0
}

/// Four seconds for one there-and-back. A wrecking ball is heavy; a fast one
/// reads as a coin flip rather than as a pattern, exactly like a fire that
/// blinks too quickly.
fn default_swing_period() -> u32 {
    240
}

/// A ball a little under a tile across.
fn default_swing_radius() -> f32 {
    crate::systems::pendulum::BALL_RADIUS
}

/// Load an ASCII map. `player` is the player's collider box, which the spawn
/// resolver needs and which no entity exists yet to supply — see
/// [`crate::level::LevelData::load`].
pub fn load(path: &Path, assets: &mut Assets, player: Vec2) -> anyhow::Result<LevelData> {
    // The same parser every other content file goes through, `implicit_some`
    // included, so a map writes `name: "The Village"` like everything else.
    let def: LevelDef = crate::assets::load_ron(path)?;
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
        name: None,
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
    /// Looks solid, is empty: `%`.
    False,
    Platform,
    Hazard,
}

impl Cell {
    /// Whether it is drawn as stone. A false wall is — that is the point of
    /// it — so its neighbours tile as if it were there, and nothing about the
    /// wall around it gives it away.
    fn looks_solid(self) -> bool {
        matches!(self, Cell::Solid | Cell::False)
    }
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
    // Every row the same width. A short row used to be padded with empty
    // cells, which quietly deleted whatever wall its missing characters were
    // meant to be — a map is a rectangle, and a ragged one is a typo.
    for (y, row) in def.grid.iter().enumerate() {
        let len = row.chars().count() as u32;
        anyhow::ensure!(
            len == width,
            "row {y} is {len} cells wide but the map is {width}: {row:?}"
        );
    }
    // A cell an entity names has to be on the map. Off it, a platform runs
    // through nothing, an NPC falls forever, and nothing says why.
    let on_map = |what: &str, cell: (u32, u32)| -> anyhow::Result<()> {
        anyhow::ensure!(
            cell.0 < width && cell.1 < height,
            "{what} at cell {cell:?} is off the {width}x{height} map"
        );
        Ok(())
    };

    let mut cells = vec![Cell::Empty; (width * height) as usize];
    let mut player_spawn = None;
    let mut entities = Vec::new();
    let mut fires = Vec::new();
    let mut movers = Vec::new();
    let mut pendulums = Vec::new();
    let mut props = Vec::new();
    let mut spawns: Vec<(String, Vec2)> = Vec::new();
    let mut decor = Vec::new();

    for (y, row) in def.grid.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let (x32, y32) = (x as u32, y as u32);
            let index = (y32 * width + x32) as usize;
            match ch {
                '#' => cells[index] = Cell::Solid,
                '%' => cells[index] = Cell::False,
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
                    ..Default::default()
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

    let corner = |c: (u32, u32)| Vec2::new(c.0 as f32 * tile_size, c.1 as f32 * tile_size);

    for entity in &def.entities {
        match entity {
            EntityDef::Npc { cell, .. }
            | EntityDef::Door { cell, .. }
            | EntityDef::Fire { cell, .. }
            | EntityDef::Exit { cell, .. }
            | EntityDef::Spawn { cell, .. }
            | EntityDef::Chest { cell, .. }
            | EntityDef::Item { cell, .. }
            | EntityDef::Checkpoint { cell }
            | EntityDef::Sign { cell, .. }
            | EntityDef::Lever { cell, .. }
            | EntityDef::Gate { cell, .. }
            | EntityDef::Decor { cell, .. }
            | EntityDef::Trigger { cell, .. } => {
                on_map("an entity", *cell)?;
            }
            EntityDef::Platform { from, to, .. } => {
                on_map("a platform's `from`", *from)?;
                on_map("a platform's `to`", *to)?;
            }
            EntityDef::Swing { anchor, .. } => on_map("a swing's anchor", *anchor)?,
        }
        match entity {
            EntityDef::Npc {
                kind,
                cell,
                dialogue,
                flag,
            } => entities.push(EntitySpawn {
                kind: kind.clone(),
                pos: corner(*cell),
                dialogue: dialogue.clone(),
                flag: flag.clone(),
            }),
            EntityDef::Door {
                cell,
                to,
                at,
                id,
                locked,
                keep_key,
                art,
            } => props.push(PropSpawn {
                cell: corner(*cell),
                at: *cell,
                kind: PropKind::Door {
                    id: id.clone(),
                    to: to.clone(),
                    at: at.clone(),
                    locked: locked.clone(),
                    keep_key: *keep_key,
                    art: art.clone(),
                },
            }),
            EntityDef::Exit { cell, size, to, at } => {
                anyhow::ensure!(size.0 > 0 && size.1 > 0, "an exit must have a size");
                props.push(PropSpawn {
                    cell: corner(*cell),
                    at: *cell,
                    kind: PropKind::Exit {
                        size: *size,
                        to: to.clone(),
                        at: at.clone(),
                    },
                });
            }
            EntityDef::Spawn { id, cell } => {
                anyhow::ensure!(
                    !spawns.iter().any(|(other, _)| other == id),
                    "two spawns are called `{id}`"
                );
                let foot = cell_floor_pos(cell.0, cell.1, tile_size, player.x, player.y);
                spawns.push((id.clone(), foot));
            }
            EntityDef::Chest { cell, items, id } => props.push(PropSpawn {
                cell: corner(*cell),
                at: *cell,
                kind: PropKind::Chest {
                    id: id.clone(),
                    items: items.clone(),
                },
            }),
            EntityDef::Item {
                cell,
                item,
                count,
                id,
            } => {
                anyhow::ensure!(*count > 0, "an item must be at least one of something");
                props.push(PropSpawn {
                    cell: corner(*cell),
                    at: *cell,
                    kind: PropKind::Item {
                        id: id.clone(),
                        item: item.clone(),
                        count: *count,
                    },
                });
            }
            EntityDef::Checkpoint { cell } => props.push(PropSpawn {
                cell: corner(*cell),
                at: *cell,
                kind: PropKind::Checkpoint,
            }),
            EntityDef::Sign { cell, dialogue } => props.push(PropSpawn {
                cell: corner(*cell),
                at: *cell,
                kind: PropKind::Sign {
                    dialogue: dialogue.clone(),
                },
            }),
            EntityDef::Lever { cell, flag, id } => props.push(PropSpawn {
                cell: corner(*cell),
                at: *cell,
                kind: PropKind::Lever {
                    id: id.clone(),
                    flag: flag.clone(),
                },
            }),
            EntityDef::Gate { cell, flag, height } => {
                anyhow::ensure!(*height > 0, "a gate must be at least one tile tall");
                props.push(PropSpawn {
                    cell: corner(*cell),
                    at: *cell,
                    kind: PropKind::Gate {
                        flag: flag.clone(),
                        height: *height,
                    },
                });
            }
            EntityDef::Decor { cell, prop } => decor.push(DecorSpawn {
                cell: corner(*cell),
                prop: prop.clone(),
            }),
            EntityDef::Trigger {
                cell,
                size,
                dialogue,
                id,
                when,
            } => {
                anyhow::ensure!(size.0 > 0 && size.1 > 0, "a trigger must have a size");
                props.push(PropSpawn {
                    cell: corner(*cell),
                    at: *cell,
                    kind: PropKind::Trigger {
                        id: id.clone(),
                        size: *size,
                        dialogue: dialogue.clone(),
                        when: when.clone(),
                    },
                });
            }
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
            EntityDef::Swing {
                anchor,
                length,
                amplitude,
                period,
                phase,
                radius,
            } => {
                anyhow::ensure!(*length > 0.0, "a swing's chain must have a length");
                anyhow::ensure!(
                    *amplitude > 0.0 && *amplitude <= 90.0,
                    "a swing's amplitude must be between 0 and 90 degrees, not {amplitude}"
                );
                // A zero period is a ball hanging still. `Pendulum::at` copes
                // with it, but nothing in a map means it, and an invisible
                // parked hazard is worse than a load error.
                anyhow::ensure!(*period > 0, "a swing's period must be at least one tick");
                anyhow::ensure!(*radius > 0.0, "a swing's ball must have a radius");
                pendulums.push(PendulumSpawn {
                    // The centre of the cell, not its corner: the pivot is a
                    // point, and a point in a cell means the middle of it.
                    anchor: Vec2::new(
                        (anchor.0 as f32 + 0.5) * tile_size,
                        (anchor.1 as f32 + 0.5) * tile_size,
                    ),
                    length: *length,
                    amplitude: amplitude.to_radians(),
                    period: *period,
                    phase: *phase,
                    radius: *radius,
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
                Cell::Solid | Cell::False => {
                    let open_up = !at(x, y - 1).looks_solid();
                    let open_left = !at(x - 1, y).looks_solid();
                    let open_right = !at(x + 1, y).looks_solid();
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
            if !at(x, y).looks_solid() && !rules.background.is_empty() {
                // deterministic variety, stable across loads
                let variant = (x as usize * 7 + y as usize * 13) % rules.background.len();
                background[index] = Some(rules.background[variant]);
            }
        }
    }

    let solid_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Solid).collect();
    let platform_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Platform).collect();
    let hazard_flags: Vec<bool> = cells.iter().map(|c| *c == Cell::Hazard).collect();

    // A map reached only through its doors has no `P`, and needs none: where
    // a new game would start is its first named arrival point.
    let player_spawn = player_spawn
        .or_else(|| spawns.first().map(|(_, at)| *at))
        .context("map has no player spawn (a P, or at least one Spawn)")?;

    // Two things answering to one name would make `at:` ambiguous and give two
    // props one world flag, which is two chests that open as one.
    let mut names: Vec<String> = props.iter().map(PropSpawn::name).collect();
    names.sort();
    if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
        anyhow::bail!("two props are called `{}`: give one an `id`", pair[0]);
    }

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
        pendulums,
        player_spawn,
        entities,
        props,
        spawns,
        decor,
        title: def.name,
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
            name: None,
            grid: grid.iter().map(|s| s.to_string()).collect(),
            entities: vec![],
        };
        build(def, 32.0, &rules(), player()).unwrap()
    }

    /// A false wall is drawn as the stone around it — with the tile the stone
    /// would have had — and collides with nothing.
    #[test]
    fn a_false_wall_looks_like_stone_and_is_not_there() {
        let real = parse(&["P....", "#####", "#####"]);
        let fake = parse(&["P....", "##%##", "##%##"]);
        assert_eq!(real.tiles, fake.tiles, "not one tile tells them apart");
        assert_eq!(real.background, fake.background);
        let hole = Aabb::new(64.0, 32.0, 32.0, 64.0);
        assert!(real.solids.iter().any(|s| s.overlaps(&hole)));
        assert!(
            !fake.solids.iter().any(|s| s.overlaps(&hole)),
            "walk into it"
        );
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
            name: None,
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

    /// Cells and degrees in, pixels and radians out — and the anchor lands in
    /// the *middle* of the cell it names, because a pivot is a point.
    #[test]
    fn a_swing_is_authored_in_cells_and_degrees() {
        let level: LevelDef = ron::from_str(
            r#"Level(
                tileset: "test",
                grid: ["P.........", ".........."],
                entities: [
                    Swing(anchor: (4, 1), length: 160.0, amplitude: 30.0, period: 90, phase: 15),
                    Swing(anchor: (8, 0)),
                ],
            )"#,
        )
        .expect("the swing form parses");
        let level = build(level, 32.0, &rules(), player()).unwrap();

        assert_eq!(level.pendulums.len(), 2);
        let authored = level.pendulums[0];
        assert_eq!(authored.anchor, Vec2::new(4.5 * 32.0, 1.5 * 32.0));
        assert_eq!(authored.length, 160.0);
        assert!((authored.amplitude - std::f32::consts::FRAC_PI_6).abs() < 1e-6);
        assert_eq!(authored.period, 90);
        assert_eq!(authored.phase, 15);

        // Everything but the anchor is optional, so the short form is a
        // complete hazard on the house numbers.
        let plain = level.pendulums[1];
        assert_eq!(plain.anchor, Vec2::new(8.5 * 32.0, 0.5 * 32.0));
        assert_eq!(plain.length, default_swing_length());
        assert!((plain.amplitude - default_swing_amplitude().to_radians()).abs() < 1e-6);
        assert_eq!(plain.period, default_swing_period());
        assert_eq!(plain.radius, crate::systems::pendulum::BALL_RADIUS);

        // A swing is geometry, not an NPC: it must stay out of the list that
        // `knight.0` and friends are counted from.
        assert!(level.entities.is_empty());
    }

    #[test]
    fn a_degenerate_swing_is_rejected_rather_than_shipped() {
        for entity in [
            "Swing(anchor: (4, 1), length: 0.0)",
            "Swing(anchor: (4, 1), amplitude: 0.0)",
            "Swing(anchor: (4, 1), amplitude: 120.0)",
            "Swing(anchor: (4, 1), period: 0)",
            "Swing(anchor: (4, 1), radius: 0.0)",
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

    /// A short row used to be padded with empty cells, which silently deleted
    /// whatever wall its missing characters were meant to be.
    #[test]
    fn a_ragged_grid_is_rejected_naming_the_row() {
        let def = LevelDef {
            tileset: "test".to_string(),
            name: None,
            grid: vec!["#####".to_string(), "#P..".to_string(), "#####".to_string()],
            entities: vec![],
        };
        let err = format!("{:#}", build(def, 32.0, &rules(), player()).unwrap_err());
        assert!(err.contains("row 1"), "{err}");
    }

    #[test]
    fn an_entity_off_the_map_is_rejected() {
        for entity in [
            "Npc(kind: \"knight\", cell: (9, 0))",
            "Fire(cell: (0, 7))",
            "Platform(from: (1, 0), to: (30, 0))",
            "Swing(anchor: (5, 2))",
        ] {
            let def: LevelDef = ron::from_str(&format!(
                r#"Level(tileset: "test", grid: ["P....", "....."], entities: [{entity}])"#
            ))
            .expect("parses");
            let err = build(def, 32.0, &rules(), player())
                .expect_err(&format!("{entity} is off a 5x2 map"));
            assert!(format!("{err:#}").contains("off the 5x2 map"), "{err:#}");
        }
    }

    #[test]
    fn unknown_characters_are_rejected() {
        let def = LevelDef {
            tileset: "test".to_string(),
            name: None,
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
            name: None,
            grid: vec!["###".to_string()],
            entities: vec![],
        };
        assert!(build(def, 32.0, &rules(), player()).is_err());
    }
}
