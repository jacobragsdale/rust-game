//! Checks that art and collision agree.
//!
//! Collision rectangles are derived from the map grid, and tile art is chosen
//! by index from a tileset atlas. Nothing connects the two, so a tile index
//! that points at the wrong art produces a platform whose hitbox is somewhere
//! the player cannot see. That reads as a physics bug and is invisible in
//! every file involved — the map says `=`, the tileset says a number, and the
//! atlas is a PNG.
//!
//! These tests close that gap by looking at the actual pixels.

use std::path::{Path, PathBuf};

use supergame::assets::Assets;
use supergame::level::ONE_WAY_THICKNESS;

fn tileset_names() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/data/tilesets");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p: &PathBuf| p.extension().is_some_and(|e| e == "ron"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// Rows within a tile that contain at least one non-transparent pixel.
fn opaque_rows(rgba: &image::RgbaImage, tile: u32, columns: u32, tile_size: u32) -> Vec<u32> {
    let col = tile % columns;
    let row = tile / columns;
    let ox = col * tile_size;
    let oy = row * tile_size;

    (0..tile_size)
        .filter(|y| {
            (0..tile_size).any(|x| {
                let px = rgba.get_pixel(ox + x, oy + y);
                px[3] > 0
            })
        })
        .collect()
}

/// The whole point of the one-way strip is that you can see what you land on.
/// A `platform` tile index pointing at art outside the collision band means
/// the player lands on empty space, or falls through solid-looking pixels.
#[test]
fn platform_tiles_draw_their_surface_inside_the_collision_strip() {
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for name in tileset_names() {
        let def = assets
            .tileset(&name)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let rgba = assets
            .decode_image(&def.image, def.transparent_color)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));

        let rows = opaque_rows(&rgba, def.rules.platform, def.columns, def.tile_size);
        let strip = ONE_WAY_THICKNESS as u32;

        let Some((&first, &last)) = rows.first().zip(rows.last()) else {
            problems.push(format!(
                "{name}: platform tile {} is fully transparent",
                def.rules.platform
            ));
            continue;
        };

        if first != 0 || last >= strip {
            problems.push(format!(
                "{name}: platform tile {} draws its art at y {first}..{last}, \
                 but the collision strip is y 0..{}. The hitbox would sit \
                 {} the visible platform.",
                def.rules.platform,
                strip - 1,
                if first >= strip { "above" } else { "off" },
            ));
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n  "));
}

/// The castle atlas is palette-indexed with a tRNS chunk, so it decodes with
/// real alpha and needs no color key. This guards that reasoning: if an atlas
/// ever ships without transparency, dropping its color key would silently
/// render masked pixels as opaque blocks.
#[test]
fn tilesets_without_a_color_key_have_real_transparency() {
    let mut assets = Assets::new();

    for name in tileset_names() {
        let def = assets
            .tileset(&name)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        if def.transparent_color.is_some() {
            continue;
        }

        let rgba = assets
            .decode_image(&def.image, None)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));

        assert!(
            rgba.pixels().any(|p| p[3] == 0),
            "{name} declares no color key, but `{}` decoded with no \
             transparent pixels at all — every masked pixel would render opaque",
            def.image
        );
    }
}

/// Every frame an animation clip references must exist on its sheet. A clip
/// pointing off the edge of the atlas renders garbage or nothing.
#[test]
fn animation_frames_are_inside_their_sheet() {
    let mut assets = Assets::new();
    let clips = assets.clip_set("player").expect("player clips load");
    let sheet = assets
        .decode_image(&clips.sheet, None)
        .expect("player sheet decodes");

    let (fw, fh) = clips.frame_size;
    let (sheet_w, sheet_h) = sheet.dimensions();
    let cols = (sheet_w as f32 / fw) as u32;
    let rows = (sheet_h as f32 / fh) as u32;

    let mut problems: Vec<String> = Vec::new();
    for (name, clip) in &clips.clips {
        for &(col, row) in &clip.frames {
            if col >= cols || row >= rows {
                problems.push(format!(
                    "clip `{name}` frame ({col}, {row}) is outside the \
                     {cols}x{rows} grid of `{}`",
                    clips.sheet
                ));
            }
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n  "));
}
