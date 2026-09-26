//! The headless backend: a [`Frame`] into an image, with no window and no GPU.
//!
//! This is what makes "does it look right?" a question an agent can answer on
//! its own. `cargo run --bin render` steps a tape to any tick, builds that
//! tick's frame with the same code the game draws with, and hands it here.
//!
//! It is written to agree with [`super::gpu`] rather than to be a good
//! rasterizer in general, which decides every choice below:
//!
//! - **Colour is blended in linear light.** The game draws into an sRGB canvas:
//!   the GPU decodes every texel to linear, converts every `Color` (which ggez
//!   treats as sRGB) to linear, multiplies and blends there, and encodes on the
//!   way out. So this does exactly that. An opaque `Color` therefore comes out
//!   as itself, while half-transparent white over black comes out at 188, not
//!   128 — checked against a frame read back off the GPU with
//!   `SUPERGAME_CAPTURE`, which is what caught the first version of this file
//!   treating colours as linear already.
//! - **Coverage is by pixel centre**, which is how a GPU rasterizes a triangle:
//!   a pixel is inside a shape if its centre is.
//! - **Sampling is nearest**, as the game's sampler is, so pixel art stays
//!   pixel art at any integer scale.

use std::collections::HashMap;

use ggez::glam::Vec2;

use super::{font, Cmd, Color, Frame, Rect, FONT_SHEET};
use crate::assets::Assets;

/// A decoded sheet, in linear light with straight alpha.
struct Sheet {
    width: u32,
    height: u32,
    texels: Vec<[f32; 4]>,
}

impl Sheet {
    fn from_rgba(image: &image::RgbaImage) -> Sheet {
        Sheet {
            width: image.width(),
            height: image.height(),
            texels: image
                .pixels()
                .map(|p| {
                    [
                        to_linear(p[0]),
                        to_linear(p[1]),
                        to_linear(p[2]),
                        p[3] as f32 / 255.0,
                    ]
                })
                .collect(),
        }
    }

    fn texel(&self, x: i64, y: i64) -> Option<[f32; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        Some(self.texels[(y as u32 * self.width + x as u32) as usize])
    }
}

/// Draws frames into images, loading each sheet the first time a frame asks
/// for it.
pub struct CpuRenderer {
    assets: Assets,
    sheets: HashMap<String, Option<Sheet>>,
    /// Colour keys for sheets that need one, which today is only ever a
    /// tileset — see [`crate::assets::TilesetDef::transparent_color`].
    keys: HashMap<String, (u8, u8, u8)>,
}

impl CpuRenderer {
    pub fn new(assets: Assets) -> CpuRenderer {
        CpuRenderer {
            assets,
            sheets: HashMap::new(),
            keys: HashMap::new(),
        }
    }

    /// Load `sheet` with a colour key, the way the game loads a tileset.
    pub fn set_color_key(&mut self, sheet: &str, key: Option<(u8, u8, u8)>) {
        if let Some(key) = key {
            self.keys.insert(sheet.to_string(), key);
            self.sheets.remove(sheet);
        }
    }

    fn sheet(&mut self, name: &str) -> Option<&Sheet> {
        if !self.sheets.contains_key(name) {
            let loaded = if name == FONT_SHEET {
                Some(Sheet::from_rgba(font::atlas()))
            } else {
                // A sheet that will not load draws nothing rather than failing
                // the whole picture: a missing image is exactly the thing a
                // screenshot should show, and `tests/data.rs` is where it fails.
                self.assets
                    .decode_image(name, self.keys.get(name).copied())
                    .ok()
                    .map(|image| Sheet::from_rgba(&image))
            };
            self.sheets.insert(name.to_string(), loaded);
        }
        self.sheets.get(name).and_then(Option::as_ref)
    }

    /// Draw `frame` and return the picture.
    pub fn render(&mut self, frame: &Frame) -> image::RgbaImage {
        let (w, h) = (frame.size.x as u32, frame.size.y as u32);
        let clear = linear(frame.clear);
        let mut target = Target {
            width: w,
            height: h,
            pixels: vec![clear; (w * h) as usize],
        };

        for cmd in &frame.cmds {
            match cmd {
                Cmd::Rect { rect, color } => target.fill_rect(*rect, *color),
                Cmd::Poly { points, color } => target.fill_poly(points, *color),
                Cmd::Circle {
                    centre,
                    radius,
                    color,
                } => target.fill_circle(*centre, *radius, *color),
                Cmd::Line {
                    from,
                    to,
                    width,
                    color,
                } => {
                    let along = *to - *from;
                    if along.length_squared() > 0.0 {
                        let normal = Vec2::new(-along.y, along.x).normalize() * (*width / 2.0);
                        target.fill_poly(
                            &[*from + normal, *to + normal, *to - normal, *from - normal],
                            *color,
                        );
                    }
                }
                Cmd::Image {
                    sheet,
                    src,
                    dest,
                    flip_x,
                    scale,
                    tint,
                } => {
                    if let Some(sheet) = self.sheet(sheet) {
                        target.blit(sheet, *src, *dest, *flip_x, *scale, *tint);
                    }
                }
            }
        }

        let mut out = image::RgbaImage::new(w, h);
        for (pixel, value) in out.pixels_mut().zip(&target.pixels) {
            *pixel = image::Rgba([
                to_srgb(value[0]),
                to_srgb(value[1]),
                to_srgb(value[2]),
                (value[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            ]);
        }
        out
    }
}

struct Target {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

impl Target {
    /// Source-over, in linear light — the GPU's `ALPHA` blend mode.
    fn blend(&mut self, x: i64, y: i64, src: [f32; 4]) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let a = src[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let dst = &mut self.pixels[(y as u32 * self.width + x as u32) as usize];
        for i in 0..3 {
            // A tint above 1 is a blow-out, clamped as a UNORM target clamps.
            dst[i] = src[i].clamp(0.0, 1.0) * a + dst[i] * (1.0 - a);
        }
        dst[3] = a + dst[3] * (1.0 - a);
    }

    /// The pixels whose centres fall inside `[x0, x1) x [y0, y1)`.
    fn span(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> (i64, i64, i64, i64) {
        let clamp_x = |v: f32| (v - 0.5).ceil().clamp(0.0, self.width as f32) as i64;
        let clamp_y = |v: f32| (v - 0.5).ceil().clamp(0.0, self.height as f32) as i64;
        (clamp_x(x0), clamp_y(y0), clamp_x(x1), clamp_y(y1))
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let (x0, y0, x1, y1) = self.span(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
        let src = linear(color);
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend(x, y, src);
            }
        }
    }

    fn fill_poly(&mut self, points: &[Vec2], color: Color) {
        if points.len() < 3 {
            return;
        }
        let (mut lo, mut hi) = (points[0], points[0]);
        for p in points {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        // Winding-agnostic: a centre is inside a convex polygon when it is on
        // the same side of every edge.
        let edge =
            |a: Vec2, b: Vec2, p: Vec2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let src = linear(color);
        let (x0, y0, x1, y1) = self.span(lo.x, lo.y, hi.x, hi.y);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let (mut pos, mut neg) = (false, false);
                for i in 0..points.len() {
                    let e = edge(points[i], points[(i + 1) % points.len()], p);
                    pos |= e > 0.0;
                    neg |= e < 0.0;
                }
                if !(pos && neg) {
                    self.blend(x, y, src);
                }
            }
        }
    }

    fn fill_circle(&mut self, centre: Vec2, radius: f32, color: Color) {
        let src = linear(color);
        let (x0, y0, x1, y1) = self.span(
            centre.x - radius,
            centre.y - radius,
            centre.x + radius,
            centre.y + radius,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if p.distance_squared(centre) <= radius * radius {
                    self.blend(x, y, src);
                }
            }
        }
    }

    fn blit(
        &mut self,
        sheet: &Sheet,
        src: Rect,
        dest: Vec2,
        flip_x: bool,
        scale: f32,
        tint: Color,
    ) {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let tint = linear(tint);
        let (x0, y0, x1, y1) = self.span(
            dest.x,
            dest.y,
            dest.x + src.w * scale,
            dest.y + src.h * scale,
        );
        for y in y0..y1 {
            let v = ((y as f32 + 0.5 - dest.y) / scale).floor();
            for x in x0..x1 {
                let mut u = ((x as f32 + 0.5 - dest.x) / scale).floor();
                if flip_x {
                    u = src.w - 1.0 - u;
                }
                let Some(texel) = sheet.texel((src.x + u) as i64, (src.y + v) as i64) else {
                    continue;
                };
                self.blend(
                    x,
                    y,
                    [
                        texel[0] * tint[0],
                        texel[1] * tint[1],
                        texel[2] * tint[2],
                        texel[3] * tint[3],
                    ],
                );
            }
        }
    }
}

fn to_linear(c: u8) -> f32 {
    decode(c as f32 / 255.0)
}

/// The sRGB transfer function, inverted. Applied to components above 1 too,
/// the way the GPU applies it to a blow-out tint, so they stay above 1.
fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A `Color` — sRGB, as ggez reads one — in linear light, alpha untouched.
fn linear(color: Color) -> [f32; 4] {
    [decode(color.r), decode(color.g), decode(color.b), color.a]
}

fn to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(frame: &Frame) -> image::RgbaImage {
        CpuRenderer::new(Assets::new()).render(frame)
    }

    #[test]
    fn a_rect_covers_exactly_the_pixels_whose_centres_it_contains() {
        let mut frame = Frame::new(Vec2::new(8.0, 8.0), Color::BLACK);
        frame.rect(Vec2::new(2.0, 2.0), Vec2::new(3.0, 2.0), Color::WHITE);
        let image = render(&frame);
        let lit: Vec<(u32, u32)> = image
            .enumerate_pixels()
            .filter(|(_, _, p)| p[0] == 255)
            .map(|(x, y, _)| (x, y))
            .collect();
        assert_eq!(lit, vec![(2, 2), (3, 2), (4, 2), (2, 3), (3, 3), (4, 3)]);
    }

    /// A `Color` is sRGB, so an opaque one comes out as itself.
    #[test]
    fn an_opaque_colour_comes_out_as_itself() {
        let mut frame = Frame::new(Vec2::new(1.0, 1.0), Color::BLACK);
        frame.rect(Vec2::ZERO, Vec2::ONE, Color::new(0.5, 0.25, 1.0, 1.0));
        assert_eq!(render(&frame).get_pixel(0, 0).0, [128, 64, 255, 255]);
    }

    /// The numbers a frame read back off the GPU gave: the health bar's red
    /// under the pause menu's scrim.
    #[test]
    fn blending_matches_the_gpu() {
        let mut frame = Frame::new(Vec2::new(1.0, 1.0), Color::BLACK);
        frame.rect(Vec2::ZERO, Vec2::ONE, Color::new(0.85, 0.22, 0.28, 1.0));
        frame.rect(Vec2::ZERO, Vec2::ONE, Color::new(0.02, 0.02, 0.05, 0.6));
        let got = render(&frame).get_pixel(0, 0).0;
        for (got, want) in got.iter().zip([144u8, 35, 46]) {
            assert!(got.abs_diff(want) <= 1, "{got} vs {want}");
        }
    }

    #[test]
    fn half_alpha_blends_halfway_in_linear_light() {
        let mut frame = Frame::new(Vec2::new(1.0, 1.0), Color::BLACK);
        frame.rect(Vec2::ZERO, Vec2::ONE, Color::new(1.0, 1.0, 1.0, 0.5));
        assert_eq!(render(&frame).get_pixel(0, 0)[0], 188);
    }

    #[test]
    fn text_draws_from_the_font_atlas() {
        let mut frame = Frame::new(Vec2::new(40.0, 12.0), Color::BLACK);
        let width = frame.text(Vec2::new(1.0, 1.0), "Hi", Color::WHITE);
        let image = render(&frame);
        let lit = image.pixels().filter(|p| p[0] == 255).count();
        assert!(lit > 10, "only {lit} pixels of text");
        assert_eq!(width, font::width("Hi"));
    }

    /// A mirrored blit reads the source right to left.
    #[test]
    fn a_flipped_blit_mirrors_the_source() {
        let mut frame = Frame::new(Vec2::new(10.0, 12.0), Color::BLACK);
        // `L` has a vertical stroke on its left; mirrored, it is on the right.
        let l = font::glyph('L').unwrap();
        let src = Rect::new(l.atlas.0 as f32, l.atlas.1 as f32, 5.0, 9.0);
        frame.image(FONT_SHEET, src, Vec2::ZERO, true, Color::WHITE);
        let image = render(&frame);
        assert_eq!(image.get_pixel(4, 0)[0], 255, "stroke on the right");
        assert_eq!(image.get_pixel(0, 0)[0], 0);
    }

    #[test]
    fn a_missing_sheet_draws_nothing_rather_than_failing() {
        let mut frame = Frame::new(Vec2::new(4.0, 4.0), Color::BLACK);
        frame.image(
            "no/such/sheet",
            Rect::new(0.0, 0.0, 4.0, 4.0),
            Vec2::ZERO,
            false,
            Color::WHITE,
        );
        assert!(render(&frame).pixels().all(|p| p[0] == 0));
    }
}
