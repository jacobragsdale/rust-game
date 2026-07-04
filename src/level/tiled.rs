//! Loads Tiled `.tmx` maps (the legacy SuperGame castle level) into
//! `LevelData`. Collision comes from the tileset's per-tile `collidable`
//! property. Only tiles from the map's first tileset are rendered; the
//! original repo never shipped the dungeon tileset's image.

use std::path::Path;

use anyhow::Context as _;
use ggez::glam::Vec2;

use crate::ecs::components::Avatar;
use crate::level::{merge_runs, LevelData};

pub fn load(path: &Path) -> anyhow::Result<LevelData> {
    let map = tiled::Loader::new()
        .load_tmx_map(path)
        .with_context(|| format!("failed to load {}", path.display()))?;

    let (width, height) = (map.width, map.height);
    let tile_size = map.tile_width as f32;
    let cell_count = (width * height) as usize;

    let mut background = vec![None; cell_count];
    let mut tiles = vec![None; cell_count];
    let mut solid_cells = vec![false; cell_count];
    let mut skipped = 0usize;

    for (layer_index, layer) in map.layers().enumerate() {
        let Some(tile_layer) = layer.as_tile_layer() else {
            continue;
        };
        for y in 0..height {
            for x in 0..width {
                let Some(tile) = tile_layer.get_tile(x as i32, y as i32) else {
                    continue;
                };
                let index = (y * width + x) as usize;

                if tile.tileset_index() == 0 {
                    let target = if layer_index == 0 {
                        &mut background
                    } else {
                        &mut tiles
                    };
                    target[index] = Some(tile.id());
                } else {
                    // Tileset without a shipped image; can't render it.
                    skipped += 1;
                }

                let collidable = tile.get_tile().is_some_and(|t| {
                    matches!(
                        t.properties.get("collidable"),
                        Some(tiled::PropertyValue::BoolValue(true))
                    )
                });
                if collidable {
                    solid_cells[index] = true;
                }
            }
        }
    }

    if skipped > 0 {
        eprintln!(
            "{}: skipped {skipped} tiles from tilesets without images",
            path.display()
        );
    }

    let solids = merge_runs(&solid_cells, width, height, tile_size, tile_size, 0.0);
    let player_spawn = find_spawn(&solid_cells, width, height, tile_size)
        .context("no standable spawn cell found")?;

    Ok(LevelData {
        tileset: "castle".to_string(),
        width,
        height,
        tile_size,
        background,
        tiles,
        solids,
        one_way: Vec::new(),
        hazards: Vec::new(),
        player_spawn,
        entities: Vec::new(),
    })
}

/// The tmx format has no spawn marker; stand the player on the lowest ledge
/// in the leftmost column that has one.
fn find_spawn(solid: &[bool], width: u32, height: u32, tile_size: f32) -> Option<Vec2> {
    for x in 1..width {
        for y in (0..height - 1).rev() {
            let open = !solid[(y * width + x) as usize];
            let floor_below = solid[((y + 1) * width + x) as usize];
            if open && floor_below {
                return Some(Vec2::new(
                    x as f32 * tile_size + (tile_size - Avatar::WIDTH) / 2.0,
                    (y + 1) as f32 * tile_size - Avatar::HEIGHT,
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn castle_map_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/graphics/levels/castle_level.tmx")
    }

    #[test]
    fn legacy_castle_level_loads() {
        let level = load(&castle_map_path()).unwrap();

        assert_eq!(level.width, 50);
        assert_eq!(level.height, 20);
        assert_eq!(level.tile_size, 32.0);
        assert!(!level.solids.is_empty(), "castle map should have collision");
        // spawn is inside the map and stands on something
        assert!(level.player_spawn.x > 0.0);
        assert!(level.player_spawn.y < level.pixel_height());
    }
}
