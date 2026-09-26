//! What a frame looks like, as data.
//!
//! Every screen in the game — the world, the HUD, the inventory and dialogue
//! overlays, the menus, the F1 debug view — is built as a [`Frame`]: a list of
//! [`Cmd`]s in 640x360 internal pixels, back to front. Building one needs a
//! [`crate::sim::Sim`] and nothing else — no `ggez::Context`, no window, no
//! GPU. Two backends then turn the list into pixels:
//!
//! - [`gpu`] draws it into the window, which is what a player sees;
//! - [`cpu`] draws it into an image, which is what `cargo run --bin render`
//!   writes to disk and what a test can inspect.
//!
//! **Why the split exists.** The simulation has been verifiable without a
//! window from the start: tapes, traces and events answer "did the right thing
//! happen?". Nothing answered "does it look right?" without a person at the
//! screen — screenshots needed a real window brought to the front, and a key
//! could not be pressed into one. With drawing reduced to data, an agent can
//! render the tick of any tape to a PNG and look at it, and the picture is
//! built by exactly the code the game draws with. That is the same bargain
//! [`crate::sim`] struck for gameplay, made for pixels.
//!
//! Text is not a command. It is laid out into glyph [`Cmd::Image`]s from the
//! [`font`] atlas, so neither backend has a font rasterizer and both draw the
//! same letters.

pub mod cpu;
pub mod font;
pub mod gpu;

use ggez::glam::Vec2;
pub use ggez::graphics::{Color, Rect};

/// The internal canvas: everything is drawn at this size and scaled up.
pub const VIEW: Vec2 = Vec2::new(640.0, 360.0);

/// The sheet name glyphs are drawn from. Not a file: both backends build it
/// from [`font::atlas`].
pub const FONT_SHEET: &str = "@font";

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// Copy the `src` rectangle (in pixels) of a sheet to `dest` (the top-left
    /// of where it lands), optionally mirrored and scaled up by `scale`.
    ///
    /// `tint` multiplies every texel. White draws the art as it is; a
    /// component above 1 blows the art out towards white, which is the hit
    /// flash.
    Image {
        sheet: String,
        src: Rect,
        dest: Vec2,
        flip_x: bool,
        scale: f32,
        tint: Color,
    },
    /// A filled axis-aligned rectangle.
    Rect { rect: Rect, color: Color },
    /// A filled convex polygon.
    Poly { points: Vec<Vec2>, color: Color },
    /// A filled circle.
    Circle {
        centre: Vec2,
        radius: f32,
        color: Color,
    },
    /// A straight line `width` pixels thick.
    Line {
        from: Vec2,
        to: Vec2,
        width: f32,
        color: Color,
    },
}

/// A picture, as the list of things to draw in order.
#[derive(Clone, Debug)]
pub struct Frame {
    pub size: Vec2,
    /// What is behind everything.
    pub clear: Color,
    pub cmds: Vec<Cmd>,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::new(VIEW, Color::BLACK)
    }
}

impl Frame {
    pub fn new(size: Vec2, clear: Color) -> Frame {
        Frame {
            size,
            clear,
            cmds: Vec::new(),
        }
    }

    pub fn rect(&mut self, origin: Vec2, size: Vec2, color: Color) {
        if size.x <= 0.0 || size.y <= 0.0 || color.a <= 0.0 {
            return;
        }
        self.cmds.push(Cmd::Rect {
            rect: Rect::new(origin.x, origin.y, size.x, size.y),
            color,
        });
    }

    /// A rectangle's outline, `width` pixels thick, drawn inside it.
    pub fn outline(&mut self, origin: Vec2, size: Vec2, width: f32, color: Color) {
        self.rect(origin, Vec2::new(size.x, width), color);
        self.rect(
            origin + Vec2::new(0.0, size.y - width),
            Vec2::new(size.x, width),
            color,
        );
        self.rect(origin, Vec2::new(width, size.y), color);
        self.rect(
            origin + Vec2::new(size.x - width, 0.0),
            Vec2::new(width, size.y),
            color,
        );
    }

    /// A filled box with a border around the outside of it — the panel
    /// treatment the HUD, the bag and the dialogue box all share.
    pub fn panel(&mut self, origin: Vec2, size: Vec2, fill: Color, border: Color) {
        self.rect(origin - Vec2::ONE, size + Vec2::splat(2.0), border);
        self.rect(origin, size, fill);
    }

    pub fn poly(&mut self, points: Vec<Vec2>, color: Color) {
        self.cmds.push(Cmd::Poly { points, color });
    }

    pub fn circle(&mut self, centre: Vec2, radius: f32, color: Color) {
        self.cmds.push(Cmd::Circle {
            centre,
            radius,
            color,
        });
    }

    pub fn line(&mut self, from: Vec2, to: Vec2, width: f32, color: Color) {
        self.cmds.push(Cmd::Line {
            from,
            to,
            width,
            color,
        });
    }

    /// Blit a region of a sheet at 1:1.
    pub fn image(&mut self, sheet: &str, src: Rect, dest: Vec2, flip_x: bool, tint: Color) {
        self.cmds.push(Cmd::Image {
            sheet: sheet.to_string(),
            src,
            dest,
            flip_x,
            scale: 1.0,
            tint,
        });
    }

    /// Draw a line of text with its top-left at `at`. Returns its width.
    pub fn text(&mut self, at: Vec2, text: &str, color: Color) -> f32 {
        self.text_scaled(at, text, color, 1.0)
    }

    /// Draw a line of text scaled up by a whole number, for titles.
    pub fn text_scaled(&mut self, at: Vec2, text: &str, color: Color, scale: f32) -> f32 {
        let at = at.floor();
        for (glyph, x) in font::layout(text) {
            self.cmds.push(Cmd::Image {
                sheet: FONT_SHEET.to_string(),
                src: Rect::new(
                    glyph.atlas.0 as f32,
                    glyph.atlas.1 as f32,
                    glyph.width as f32,
                    font::GLYPH_H as f32,
                ),
                dest: at + Vec2::new(x * scale, 0.0),
                flip_x: false,
                scale,
                tint: color,
            });
        }
        font::width(text) * scale
    }

    /// Text centred horizontally on `centre_x`.
    pub fn text_centred(&mut self, centre_x: f32, y: f32, text: &str, color: Color, scale: f32) {
        let w = font::width(text) * scale;
        self.text_scaled(Vec2::new(centre_x - w / 2.0, y), text, color, scale);
    }

    /// Everything drawn into this frame, then everything in `other`.
    pub fn extend(&mut self, other: Frame) {
        self.cmds.extend(other.cmds);
    }
}

/// A colour from 0-255 components with an alpha in `[0, 1]`.
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a)
}
