//! Asset cache: images (with optional color-key transparency), animation clip
//! sets, and tileset definitions. Data files are RON under `assets/data/`;
//! images live under `assets/graphics/`. Everything is cached on first use.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context as _;
use ggez::glam::Vec2;
use ggez::graphics::{Image, ImageFormat, Rect};
use ggez::Context;
use serde::Deserialize;

/// A named animation: frames are (col, row) cells on a uniform grid.
///
/// `sheet` and `frame_size` default to the [`ClipSet`]'s, and override it when
/// present. The player is one atlas with one cell size, but art packs commonly
/// ship a file per animation with a different frame width each — the knight's
/// idle is 64px wide and its attack is 144 — and a set that could only name one
/// grid could not describe that at all.
#[derive(Clone, Debug, Deserialize)]
pub struct Clip {
    pub frames: Vec<(u32, u32)>,
    pub fps: f32,
    pub looping: bool,
    /// Image name (under `assets/graphics/`, without extension).
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub frame_size: Option<(f32, f32)>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClipSet {
    /// Default image for clips that do not name their own.
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub frame_size: Option<(f32, f32)>,
    /// Drawing nudge for art that is not bottom-aligned inside its own frame.
    ///
    /// Sprites are drawn standing on their collider, which assumes the artist
    /// put the character's feet at the bottom of the cell. The knight pack does
    /// not: every one of its animations leaves 20px of empty space below the
    /// feet, so without this the knight floats two thirds of a tile above the
    /// ground it is standing on.
    #[serde(default)]
    pub offset: Option<(f32, f32)>,
    pub clips: HashMap<String, Clip>,
}

impl ClipSet {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    /// Drawing nudge for this art, or none.
    pub fn offset(&self) -> (f32, f32) {
        self.offset.unwrap_or((0.0, 0.0))
    }

    /// The sheet a clip's frames live on. The result borrows from whichever of
    /// the two provided it, hence the shared lifetime.
    pub fn sheet_of<'a>(&'a self, clip: &'a Clip) -> &'a str {
        clip.sheet
            .as_deref()
            .or(self.sheet.as_deref())
            .expect("clip set was validated on load")
    }

    /// The cell size a clip's frames are laid out on.
    pub fn frame_size_of(&self, clip: &Clip) -> (f32, f32) {
        clip.frame_size
            .or(self.frame_size)
            .expect("clip set was validated on load")
    }

    /// Normalized source rect for a frame of `clip`, given its sheet's size.
    pub fn src_rect(&self, clip: &Clip, frame: (u32, u32), sheet_w: f32, sheet_h: f32) -> Rect {
        let (fw, fh) = self.frame_size_of(clip);
        Rect::new(
            frame.0 as f32 * fw / sheet_w,
            frame.1 as f32 * fh / sheet_h,
            fw / sheet_w,
            fh / sheet_h,
        )
    }

    /// Every clip must resolve to a sheet and a frame size, one way or another.
    ///
    /// Checked once at load so that `sheet_of` and `frame_size_of` can be
    /// infallible everywhere else — a missing sheet is an authoring mistake in
    /// a RON file, not a condition the renderer should have to handle per frame.
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        let mut missing: Vec<String> = self
            .clips
            .iter()
            .filter(|(_, clip)| {
                clip.sheet.is_none() && self.sheet.is_none()
                    || clip.frame_size.is_none() && self.frame_size.is_none()
            })
            .map(|(clip_name, _)| clip_name.clone())
            .collect();
        missing.sort();

        anyhow::ensure!(
            missing.is_empty(),
            "clip set `{name}`: clips {missing:?} have no sheet or frame_size, \
             and the set does not provide a default"
        );
        Ok(())
    }
}

/// One attack's timing, reach, and effect. See `assets/data/attacks.ron`.
/// Spelled `AttackDef(...)` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "AttackDef")]
pub struct AttackDef {
    /// Animation clip to play on the attacker, from its own clip set.
    pub clip: String,
    /// Total ticks the attacker is committed for.
    pub duration: u32,
    /// `[start, end)` in ticks, during which the hitbox exists.
    pub active: (u32, u32),
    /// Extra ticks of commitment after the animation ends. A miss should cost
    /// something, or mashing is always the best play.
    #[serde(default)]
    pub recovery: u32,
    /// What pressing attack again turns this into, if anything.
    #[serde(default)]
    pub chain: Option<String>,
    /// Hitbox position relative to the attacker's collider, facing right.
    pub offset: (f32, f32),
    pub size: (f32, f32),
    pub damage: i32,
    /// Impulse applied to whatever is hit, rightward.
    pub knockback: (f32, f32),
    /// Ticks the victim loses control for.
    pub hitstun: u32,
}

impl AttackDef {
    pub fn is_active(&self, elapsed: u32) -> bool {
        elapsed >= self.active.0 && elapsed < self.active.1
    }

    /// The animation is over; the attacker is still committed until
    /// [`AttackDef::released`].
    pub fn finished(&self, elapsed: u32) -> bool {
        elapsed >= self.duration
    }

    /// Free to act again.
    pub fn released(&self, elapsed: u32) -> bool {
        elapsed >= self.duration + self.recovery
    }

    /// Does this attack continue into another?
    pub fn chains(&self) -> bool {
        self.chain.is_some()
    }

    /// The hitbox in world space for an attacker whose collider is at `pos`
    /// with size `size`, facing `facing_right`.
    pub fn hitbox(&self, pos: Vec2, size: Vec2, facing_right: bool) -> Rect {
        let (w, h) = self.size;
        let x = if facing_right {
            pos.x + self.offset.0
        } else {
            // mirror the whole box about the collider's centre
            pos.x + size.x - self.offset.0 - w
        };
        Rect::new(x, pos.y + self.offset.1, w, h)
    }

    /// Knockback impulse, pointing away from an attacker facing `facing_right`.
    pub fn impulse(&self, facing_right: bool) -> Vec2 {
        let (x, y) = self.knockback;
        Vec2::new(if facing_right { x } else { -x }, y)
    }
}

/// Every attack in the game, by id. Spelled `Attacks({...})` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Attacks")]
pub struct AttackTable(pub HashMap<String, AttackDef>);

impl AttackTable {
    pub fn get(&self, id: &str) -> Option<&AttackDef> {
        self.0.get(id)
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
    clip_sets: HashMap<String, Arc<ClipSet>>,
    tilesets: HashMap<String, Rc<TilesetDef>>,
    attacks: Option<Arc<AttackTable>>,
}

impl Default for Assets {
    fn default() -> Self {
        Assets::new()
    }
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
            attacks: None,
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
    pub fn clip_set(&mut self, name: &str) -> anyhow::Result<Arc<ClipSet>> {
        if let Some(set) = self.clip_sets.get(name) {
            return Ok(set.clone());
        }
        let path = self
            .base
            .join("data/animations")
            .join(format!("{name}.ron"));
        let set: ClipSet = load_ron(&path)?;
        set.validate(name)?;
        let set = Arc::new(set);
        self.clip_sets.insert(name.to_string(), set.clone());
        Ok(set)
    }

    /// Load `assets/data/attacks.ron`. One table for the whole game.
    pub fn attacks(&mut self) -> anyhow::Result<Arc<AttackTable>> {
        if let Some(table) = &self.attacks {
            return Ok(table.clone());
        }
        let table: AttackTable = load_ron(&self.base.join("data/attacks.ron"))?;
        let table = Arc::new(table);
        self.attacks = Some(table.clone());
        Ok(table)
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

/// Parse a RON data file.
///
/// `implicit_some` is on so that an optional field can be written as
/// `sheet: "knight/knightIdle"` rather than `sheet: Some("knight/knightIdle")`.
/// These files are hand-authored content; making every optional field announce
/// its optionality is noise for whoever is writing the twentieth NPC.
fn load_ron<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> anyhow::Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))
}
