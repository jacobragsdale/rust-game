//! Asset cache: images (with optional color-key transparency), animation clip
//! sets, and tileset definitions. Data files are RON under `assets/data/`;
//! images live under `assets/graphics/`. Everything is cached on first use.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Context as _;
use ggez::graphics::{Image, ImageFormat, Rect};
use ggez::Context;
use serde::Deserialize;

/// A named animation: frames are (col, row) cells on a uniform sprite-sheet
/// grid of `ClipSet::frame_size` pixels.
#[derive(Clone, Debug, Deserialize)]
pub struct Clip {
    pub frames: Vec<(u32, u32)>,
    pub fps: f32,
    pub looping: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClipSet {
    /// Image name (under `assets/graphics/`, without extension).
    pub sheet: String,
    pub frame_size: (f32, f32),
    pub clips: HashMap<String, Clip>,
}

impl ClipSet {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    /// Normalized source rect for a frame, given the sheet's pixel size.
    pub fn src_rect(&self, frame: (u32, u32), sheet_w: f32, sheet_h: f32) -> Rect {
        let (fw, fh) = self.frame_size;
        Rect::new(
            frame.0 as f32 * fw / sheet_w,
            frame.1 as f32 * fh / sheet_h,
            fw / sheet_w,
            fh / sheet_h,
        )
    }
}

/// How a logical map cell maps onto tiles of the atlas, chosen by looking at
/// a solid cell's neighbors. All values are 0-based tile indices.
/// Spelled `Rules(...)` in the RON files.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Rules")]
pub struct AutotileRules {
    pub solid_top_left: u32,
    pub solid_top: u32,
    pub solid_top_right: u32,
    pub solid_left: u32,
    pub solid_fill: u32,
    pub solid_right: u32,
    pub platform: u32,
    /// Variants scattered over empty cells for texture.
    pub background: Vec<u32>,
}

/// Spelled `Tileset(...)` in the RON files.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Tileset")]
pub struct TilesetDef {
    /// Image name (under `assets/graphics/`, without extension).
    pub image: String,
    pub tile_size: u32,
    pub columns: u32,
    /// Pixels of exactly this color become transparent (e.g. magenta keys).
    pub transparent_color: Option<(u8, u8, u8)>,
    pub rules: AutotileRules,
}

impl TilesetDef {
    /// Normalized source rect for a tile index.
    pub fn src_rect(&self, tile: u32, sheet_w: f32, sheet_h: f32) -> Rect {
        let ts = self.tile_size as f32;
        let col = tile % self.columns;
        let row = tile / self.columns;
        Rect::new(
            col as f32 * ts / sheet_w,
            row as f32 * ts / sheet_h,
            ts / sheet_w,
            ts / sheet_h,
        )
    }
}

pub struct Assets {
    base: PathBuf,
    images: HashMap<String, Image>,
    clip_sets: HashMap<String, Rc<ClipSet>>,
    tilesets: HashMap<String, Rc<TilesetDef>>,
}

impl Assets {
    pub fn new() -> Self {
        // Prefer ./assets (running from the repo root), fall back to the
        // crate directory (running the binary from elsewhere during dev).
        let cwd = PathBuf::from("assets");
        let base = if cwd.is_dir() {
            cwd
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
        };
        Assets {
            base,
            images: HashMap::new(),
            clip_sets: HashMap::new(),
            tilesets: HashMap::new(),
        }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base
    }

    /// Decode `assets/graphics/{name}.png` to RGBA, applying an optional
    /// color key.
    ///
    /// Split out of [`Assets::image`] because it needs no graphics context,
    /// which lets asset checks inspect exactly the pixels the game uploads.
    pub fn decode_image(
        &self,
        name: &str,
        color_key: Option<(u8, u8, u8)>,
    ) -> anyhow::Result<image::RgbaImage> {
        let path = self.base.join("graphics").join(format!("{name}.png"));
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read image {}", path.display()))?;
        let decoded = image::load_from_memory(&bytes)
            .with_context(|| format!("failed to decode image {}", path.display()))?;
        let mut rgba = decoded.to_rgba8();

        if let Some((r, g, b)) = color_key {
            for pixel in rgba.pixels_mut() {
                if pixel[0] == r && pixel[1] == g && pixel[2] == b {
                    *pixel = image::Rgba([0, 0, 0, 0]);
                }
            }
        }

        Ok(rgba)
    }

    /// Load `assets/graphics/{name}.png`, applying an optional color key.
    /// Images are cheap to clone (shared GPU handle).
    pub fn image(
        &mut self,
        ctx: &mut Context,
        name: &str,
        color_key: Option<(u8, u8, u8)>,
    ) -> anyhow::Result<Image> {
        if let Some(image) = self.images.get(name) {
            return Ok(image.clone());
        }

        let rgba = self.decode_image(name, color_key)?;
        let (w, h) = rgba.dimensions();
        let image = Image::from_pixels(ctx, rgba.as_raw(), ImageFormat::Rgba8UnormSrgb, w, h);
        self.images.insert(name.to_string(), image.clone());
        Ok(image)
    }

    /// Load `assets/data/animations/{name}.ron`.
    pub fn clip_set(&mut self, name: &str) -> anyhow::Result<Rc<ClipSet>> {
        if let Some(set) = self.clip_sets.get(name) {
            return Ok(set.clone());
        }
        let path = self
            .base
            .join("data/animations")
            .join(format!("{name}.ron"));
        let set: ClipSet = load_ron(&path)?;
        let set = Rc::new(set);
        self.clip_sets.insert(name.to_string(), set.clone());
        Ok(set)
    }

    /// Load `assets/data/tilesets/{name}.ron`.
    pub fn tileset(&mut self, name: &str) -> anyhow::Result<Rc<TilesetDef>> {
        if let Some(def) = self.tilesets.get(name) {
            return Ok(def.clone());
        }
        let path = self.base.join("data/tilesets").join(format!("{name}.ron"));
        let def: TilesetDef = load_ron(&path)?;
        let def = Rc::new(def);
        self.tilesets.insert(name.to_string(), def.clone());
        Ok(def)
    }
}

fn load_ron<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> anyhow::Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}
