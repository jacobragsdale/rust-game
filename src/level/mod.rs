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

/// NPC/door/etc. placements parsed from maps. Consumed in Phase 3 when NPCs
/// spawn from data; until then only tests read it.
#[derive(Clone, Debug)]
pub struct EntitySpawn {
    #[allow(dead_code)]
    pub kind: String,
    #[allow(dead_code)]
    pub pos: Vec2,
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
    /// Hazard zones (inert until combat lands in Phase 3).
    pub hazards: Vec<Aabb>,
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
