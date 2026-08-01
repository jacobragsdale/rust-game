//! Level data, format-agnostic. Both the ASCII RON format (primary authoring
//! path) and Tiled `.tmx` maps normalize into `LevelData`; the game never
//! knows which format a level came from.

pub mod ascii;
pub mod tiled;
pub mod validate;

use std::path::Path;

use ggez::glam::Vec2;

use crate::assets::Assets;
use crate::physics::Aabb;

/// Height of a one-way platform's collision strip, in pixels, measured down
/// from the top of its cell.
///
/// This is a contract with the art: the tileset's `platform` tile has to draw
/// its walkable surface in exactly this band. When it does not, the collision
/// is somewhere the player cannot see, and the platform feels broken in a way
/// that looks like a physics bug. `tests/assets.rs` checks the two agree.
pub const ONE_WAY_THICKNESS: f32 = 8.0;

/// NPC/door/etc. placements parsed from maps. Consumed in Phase 3 when NPCs
/// spawn from data; until then only tests read it.
#[derive(Clone, Debug)]
pub struct EntitySpawn {
    #[allow(dead_code)]
    pub kind: String,
    #[allow(dead_code)]
    pub pos: Vec2,
}

/// A fire the map places: where it burns, and when.
///
/// Apart from [`EntitySpawn`] on purpose. That list is addressed by spawn
/// index — `knight.0` in a tape means the first entry — so a fire dropped into
/// it would renumber every NPC after it in every tape that runs on the map.
/// Fires are also not a `kind` anything looks art up for: they are geometry
/// with a timer, and [`crate::systems::hazard::spawn_fires`] is all they need.
#[derive(Clone, Copy, Debug)]
pub struct FireSpawn {
    /// Top-left of the cell it was placed in, in world pixels.
    pub cell: Vec2,
    /// Ticks in one full on/off cycle.
    pub period: u32,
    /// How many of those it burns for.
    pub duty: u32,
    /// Offset into the cycle, so two fires can alternate.
    pub phase: u32,
}

#[derive(Clone, Debug)]
pub struct LevelData {
    /// Name of the tileset definition (`assets/data/tilesets/{name}.ron`).
    pub tileset: String,
    pub width: u32,
    pub height: u32,
    pub tile_size: f32,
    /// Background tile per cell (drawn first), row-major, `width * height`.
    pub background: Vec<Option<u32>>,
    /// Foreground tile per cell (drawn over the background).
    pub tiles: Vec<Option<u32>>,
    /// Merged collision rectangles (world px).
    pub solids: Vec<Aabb>,
    /// One-way platforms: land from above, jump through from below.
    pub one_way: Vec<Aabb>,
    /// Static hazard zones: spikes carved into the grid.
    pub hazards: Vec<Aabb>,
    /// Fires, which are hazards that come and go. Not in `hazards` because
    /// they are not static: each becomes an entity that owns its collider and
    /// hands it to the geometry only while lit.
    pub fires: Vec<FireSpawn>,
    pub player_spawn: Vec2,
    #[allow(dead_code)]
    pub entities: Vec<EntitySpawn>,
}

impl LevelData {
    pub fn load(path: &Path, assets: &mut Assets) -> anyhow::Result<LevelData> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("ron") => ascii::load(path, assets),
            Some("tmx") => tiled::load(path),
            other => anyhow::bail!("unsupported map format: {other:?} ({})", path.display()),
        }
    }

    /// An empty level of the given size: no geometry, no art, spawn at the
    /// origin. Tests that need one specific piece of geometry build it from
    /// here and set that one field, rather than writing out every field of
    /// `LevelData` — which otherwise means every new field breaks every test.
    pub fn empty(width: u32, height: u32, tile_size: f32) -> LevelData {
        LevelData {
            tileset: "test".to_string(),
            width,
            height,
            tile_size,
            background: vec![],
            tiles: vec![],
            solids: vec![],
            one_way: vec![],
            hazards: vec![],
            fires: vec![],
            player_spawn: Vec2::ZERO,
            entities: vec![],
        }
    }

    /// Build a level straight from an ASCII grid, with no tileset and no
    /// filesystem — the same legend the `.ron` maps use (see
    /// [`crate::level::ascii`]). Tile art is placeholder, since nothing about
    /// collision depends on which atlas index gets drawn.
    ///
    /// This is what makes a test for a new system cheap to write: the map is
    /// three lines at the top of the test, and the geometry is visible in the
    /// test rather than in a fixture file somewhere else.
    pub fn from_grid(grid: &[&str]) -> anyhow::Result<LevelData> {
        ascii::from_grid(grid)
    }

    pub fn pixel_width(&self) -> f32 {
        self.width as f32 * self.tile_size
    }

    pub fn pixel_height(&self) -> f32 {
        self.height as f32 * self.tile_size
    }
}

/// Merge horizontal runs of flagged cells into rectangles, one per (row, run).
/// `height` and `y_offset` shape the rect within the cell row (used for thin
/// one-way platform tops).
pub(crate) fn merge_runs(
    cells: &[bool],
    width: u32,
    height: u32,
    tile_size: f32,
    rect_height: f32,
    y_offset: f32,
) -> Vec<Aabb> {
    let mut rects = Vec::new();
    for y in 0..height {
        let mut run_start: Option<u32> = None;
        for x in 0..=width {
            let filled = x < width && cells[(y * width + x) as usize];
            match (filled, run_start) {
                (true, None) => run_start = Some(x),
                (false, Some(start)) => {
                    rects.push(Aabb::new(
                        start as f32 * tile_size,
                        y as f32 * tile_size + y_offset,
                        (x - start) as f32 * tile_size,
                        rect_height,
                    ));
                    run_start = None;
                }
                _ => {}
            }
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_runs_combines_adjacent_cells() {
        // row 0: cells 1,2,3 solid; row 1: cell 0 solid
        let width = 5;
        let mut cells = vec![false; 10];
        cells[1] = true;
        cells[2] = true;
        cells[3] = true;
        cells[5] = true;

        let rects = merge_runs(&cells, width, 2, 32.0, 32.0, 0.0);

        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], Aabb::new(32.0, 0.0, 96.0, 32.0));
        assert_eq!(rects[1], Aabb::new(0.0, 32.0, 32.0, 32.0));
    }
}
