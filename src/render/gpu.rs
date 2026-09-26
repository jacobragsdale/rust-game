//! The window backend: a [`Frame`] onto the GPU.
//!
//! Draws into a 640x360 canvas image and scales that into the window with
//! nearest sampling, so every pixel of art is an integer block of screen
//! pixels at any window size.
//!
//! The only cleverness is batching. A frame is a few hundred commands — every
//! tile and glyph is one — and a draw call each would be a few hundred draw
//! calls. So consecutive images from one sheet become one instanced draw, and
//! consecutive shapes become one mesh; the order of the list, which is the
//! draw order, is kept exactly.

use std::collections::HashMap;

use ggez::glam::Vec2;
use ggez::graphics::{
    Canvas, DrawMode, DrawParam, Image, ImageFormat, InstanceArray, Mesh, MeshBuilder, Sampler,
};
use ggez::{Context, GameResult};

use super::{font, Cmd, Frame, Rect, FONT_SHEET, VIEW};
use crate::assets::Assets;

pub struct GpuRenderer {
    target: Image,
    sheets: HashMap<String, Option<Image>>,
    /// Instance buffers per sheet, reused frame to frame. More than one per
    /// sheet because a frame can come back to a sheet after drawing something
    /// else, and a buffer already handed to the canvas cannot be refilled
    /// until the canvas is finished.
    pools: HashMap<String, Vec<InstanceArray>>,
}

enum Batch {
    Empty,
    Shapes(MeshBuilder),
    Images {
        sheet: String,
        params: Vec<DrawParam>,
    },
}

impl GpuRenderer {
    pub fn new(ctx: &mut Context) -> GpuRenderer {
        GpuRenderer {
            target: Image::new_canvas_image(
                ctx,
                ImageFormat::Rgba8UnormSrgb,
                VIEW.x as u32,
                VIEW.y as u32,
                1,
            ),
            sheets: HashMap::new(),
            pools: HashMap::new(),
        }
    }

    fn sheet(&mut self, ctx: &mut Context, assets: &mut Assets, name: &str) -> Option<Image> {
        if !self.sheets.contains_key(name) {
            let loaded = if name == FONT_SHEET {
                let atlas = font::atlas();
                Some(Image::from_pixels(
                    ctx,
                    atlas.as_raw(),
                    ImageFormat::Rgba8UnormSrgb,
                    atlas.width(),
                    atlas.height(),
                ))
            } else {
                match assets.image(ctx, name, None) {
                    Ok(image) => Some(image),
                    Err(err) => {
                        // Once, not per frame: the headless renderer draws the
                        // same gap, and `tests/data.rs` is where it fails.
                        eprintln!("missing sheet `{name}`: {err:#}");
                        None
                    }
                }
            };
            self.sheets.insert(name.to_string(), loaded);
        }
        self.sheets.get(name).cloned().flatten()
    }

    /// Draw `frame` into the internal canvas.
    pub fn draw(&mut self, ctx: &mut Context, assets: &mut Assets, frame: &Frame) -> GameResult {
        let mut canvas = Canvas::from_image(ctx, self.target.clone(), frame.clear);
        canvas.set_sampler(Sampler::nearest_clamp());
        let mut used: HashMap<String, usize> = HashMap::new();
        let mut batch = Batch::Empty;

        for cmd in &frame.cmds {
            match cmd {
                Cmd::Image {
                    sheet,
                    src,
                    dest,
                    flip_x,
                    scale,
                    tint,
                } => {
                    let same = matches!(&batch, Batch::Images { sheet: s, .. } if s == sheet);
                    if !same {
                        self.flush(ctx, assets, &mut canvas, &mut used, &mut batch)?;
                        batch = Batch::Images {
                            sheet: sheet.clone(),
                            params: Vec::new(),
                        };
                    }
                    let Some(image) = self.sheet(ctx, assets, sheet) else {
                        continue;
                    };
                    let (w, h) = (image.width() as f32, image.height() as f32);
                    let uv = Rect::new(src.x / w, src.y / h, src.w / w, src.h / h);
                    let mut param = DrawParam::new().src(uv).color(*tint);
                    param = if *flip_x {
                        param
                            .dest(*dest + Vec2::new(src.w * scale, 0.0))
                            .scale(Vec2::new(-scale, *scale))
                    } else {
                        param.dest(*dest).scale(Vec2::splat(*scale))
                    };
                    if let Batch::Images { params, .. } = &mut batch {
                        params.push(param);
                    }
                }
                shape => {
                    if !matches!(batch, Batch::Shapes(_)) {
                        self.flush(ctx, assets, &mut canvas, &mut used, &mut batch)?;
                        batch = Batch::Shapes(MeshBuilder::new());
                    }
                    if let Batch::Shapes(mb) = &mut batch {
                        add_shape(mb, shape)?;
                    }
                }
            }
        }
        self.flush(ctx, assets, &mut canvas, &mut used, &mut batch)?;
        canvas.finish(ctx)
    }

    fn flush(
        &mut self,
        ctx: &mut Context,
        assets: &mut Assets,
        canvas: &mut Canvas,
        used: &mut HashMap<String, usize>,
        batch: &mut Batch,
    ) -> GameResult {
        match std::mem::replace(batch, Batch::Empty) {
            Batch::Empty => {}
            Batch::Shapes(mb) => {
                let data = mb.build();
                if !data.vertices.is_empty() {
                    canvas.draw(&Mesh::from_data(ctx, data), DrawParam::default());
                }
            }
            Batch::Images { sheet, params } => {
                if params.is_empty() {
                    return Ok(());
                }
                let Some(image) = self.sheet(ctx, assets, &sheet) else {
                    return Ok(());
                };
                let index = used.entry(sheet.clone()).or_insert(0);
                let pool = self.pools.entry(sheet).or_default();
                if pool.len() <= *index {
                    pool.push(InstanceArray::new(ctx, image));
                }
                let array = &mut pool[*index];
                *index += 1;
                array.set(params);
                canvas.draw(array, DrawParam::default());
            }
        }
        Ok(())
    }

    /// Read the internal canvas back off the GPU — exactly the pixels the
    /// window is showing, before any scaling or colour management. What
    /// `SUPERGAME_CAPTURE` writes, and how the CPU backend is checked against
    /// this one.
    pub fn capture(&self, ctx: &Context) -> GameResult<image::RgbaImage> {
        let pixels = self.target.to_pixels(ctx)?;
        image::RgbaImage::from_raw(self.target.width(), self.target.height(), pixels).ok_or_else(
            || ggez::GameError::RenderError("the canvas read back at the wrong size".to_string()),
        )
    }

    /// Scale the internal canvas into the window, letterboxed, with nearest
    /// sampling so the pixels stay square.
    pub fn present(&self, ctx: &Context, canvas: &mut Canvas) {
        let (win_w, win_h) = ctx.gfx.drawable_size();
        let scale = (win_w / VIEW.x).min(win_h / VIEW.y);
        let dest = Vec2::new(
            (win_w - VIEW.x * scale) / 2.0,
            (win_h - VIEW.y * scale) / 2.0,
        );
        canvas.set_sampler(Sampler::nearest_clamp());
        canvas.draw(
            &self.target,
            DrawParam::new().dest(dest).scale(Vec2::splat(scale)),
        );
        canvas.set_sampler(Sampler::default());
    }
}

fn add_shape(mb: &mut MeshBuilder, shape: &Cmd) -> GameResult {
    match shape {
        Cmd::Rect { rect, color } => {
            mb.rectangle(DrawMode::fill(), *rect, *color)?;
        }
        Cmd::Poly { points, color } => {
            // A degenerate polygon is an error to the tessellator and nothing
            // at all on screen; the CPU backend draws nothing for it either.
            if points.len() >= 3 {
                let _ = mb.polygon(DrawMode::fill(), points, *color);
            }
        }
        Cmd::Circle {
            centre,
            radius,
            color,
        } => {
            if *radius > 0.0 {
                mb.circle(DrawMode::fill(), *centre, *radius, 0.2, *color)?;
            }
        }
        Cmd::Line {
            from,
            to,
            width,
            color,
        } => {
            if from != to {
                mb.line(&[*from, *to], *width, *color)?;
            }
        }
        Cmd::Image { .. } => unreachable!("images are batched separately"),
    }
    Ok(())
}
