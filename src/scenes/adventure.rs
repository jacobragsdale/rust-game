//! Adventure mode: a Tiled/ASCII level rendered as pixel art on a 640x360
//! internal canvas, scaled up to the window. First playable slice of the RPG.
//!
//! This scene owns no gameplay logic. It holds a [`Sim`] — which runs equally
//! well headless — plus the GPU resources needed to look at it. Everything
//! `update` does is feed the sim one tick of input, which is why the game and
//! the `sim` binary can never disagree about what happens.

use std::rc::Rc;

use ggez::glam::Vec2;
use ggez::graphics::{
    self, Canvas, Color, DrawParam, Image, ImageFormat, InstanceArray, Mesh, MeshBuilder, Sampler,
};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::assets::TilesetDef;
use crate::debug::DebugOverlay;
use crate::ecs::components::{AnimationState, Avatar, Position, Sprite};
use crate::scenes::{pause::PauseScene, Resources, Scene, Transition};
use crate::sim::Sim;
use crate::systems::{camera::Camera, input};

pub const INTERNAL_WIDTH: f32 = 640.0;
pub const INTERNAL_HEIGHT: f32 = 360.0;

const CLEAR_COLOR: Color = Color {
    r: 0.04,
    g: 0.03,
    b: 0.08,
    a: 1.0,
};

pub struct AdventureScene {
    /// The whole game state and its rules; contains no graphics resources.
    sim: Sim,
    camera: Camera,
    /// 640x360 offscreen target; the window draw scales this up.
    internal: Image,
    tileset: Rc<TilesetDef>,
    tile_batch: InstanceArray,
    tile_sheet_size: Vec2,
    player_image: Image,
    debug: DebugOverlay,
}

impl AdventureScene {
    pub fn new(ctx: &mut Context, res: &mut Resources, map: &str) -> anyhow::Result<Self> {
        let sim = Sim::load(&mut res.assets, map)?;

        let tileset = res.assets.tileset(&sim.level.tileset)?;
        let tile_image = res
            .assets
            .image(ctx, &tileset.image, tileset.transparent_color)?;
        let tile_sheet_size = Vec2::new(tile_image.width() as f32, tile_image.height() as f32);
        let tile_batch = InstanceArray::new(ctx, tile_image);

        let player_image = res.assets.image(ctx, &sim.clips.sheet, None)?;

        let internal = Image::new_canvas_image(
            ctx,
            ImageFormat::Rgba8UnormSrgb,
            INTERNAL_WIDTH as u32,
            INTERNAL_HEIGHT as u32,
            1,
        );

        let camera = Camera::new(
            Vec2::new(INTERNAL_WIDTH, INTERNAL_HEIGHT),
            Vec2::new(sim.level.pixel_width(), sim.level.pixel_height()),
        );

        Ok(AdventureScene {
            sim,
            camera,
            internal,
            tileset,
            tile_batch,
            tile_sheet_size,
            player_image,
            debug: DebugOverlay::from_env(),
        })
    }

    fn queue_tiles(&mut self) {
        let offset = self.camera.offset();
        let level = &self.sim.level;
        let ts = level.tile_size;
        let x0 = (offset.x / ts).floor().max(0.0) as u32;
        let y0 = (offset.y / ts).floor().max(0.0) as u32;
        let x1 = (((offset.x + INTERNAL_WIDTH) / ts).ceil() as u32 + 1).min(level.width);
        let y1 = (((offset.y + INTERNAL_HEIGHT) / ts).ceil() as u32 + 1).min(level.height);

        let mut params = Vec::new();
        for layer in [&level.background, &level.tiles] {
            if layer.is_empty() {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let Some(tile) = layer[(y * level.width + x) as usize] else {
                        continue;
                    };
                    let src =
                        self.tileset
                            .src_rect(tile, self.tile_sheet_size.x, self.tile_sheet_size.y);
                    let dest = Vec2::new(x as f32 * ts, y as f32 * ts) - offset;
                    params.push(DrawParam::new().src(src).dest(dest.floor()));
                }
            }
        }
        self.tile_batch.set(params);
    }

    fn draw_hazards(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        // Placeholder spikes (the original spike art was never committed):
        // white triangles across each hazard strip. Replaced in Phase 3.
        if self.sim.level.hazards.is_empty() {
            return Ok(());
        }
        let offset = self.camera.offset();
        let mut mb = MeshBuilder::new();
        let spike_w = 8.0;
        for hazard in &self.sim.level.hazards {
            let mut x = hazard.x;
            while x + spike_w <= hazard.right() + 0.5 {
                mb.polygon(
                    graphics::DrawMode::fill(),
                    &[
                        Vec2::new(x, hazard.bottom()) - offset,
                        Vec2::new(x + spike_w, hazard.bottom()) - offset,
                        Vec2::new(x + spike_w / 2.0, hazard.y) - offset,
                    ],
                    Color::from_rgb(220, 220, 230),
                )?;
                x += spike_w;
            }
        }
        let mesh = Mesh::from_data(ctx, mb.build());
        canvas.draw(&mesh, DrawParam::default());
        Ok(())
    }

    fn draw_player(&self, canvas: &mut Canvas) {
        let offset = self.camera.offset();
        let clips = &self.sim.clips;
        let (fw, _) = clips.frame_size;
        let sheet_w = self.player_image.width() as f32;
        let sheet_h = self.player_image.height() as f32;

        for (_, (pos, sprite, anim, avatar)) in self
            .sim
            .world
            .query::<(&Position, &Sprite, &AnimationState, &Avatar)>()
            .iter()
        {
            let Some(clip) = clips.clip(&anim.clip) else {
                continue;
            };
            let frame = clip.frames[anim.frame.min(clip.frames.len() - 1)];
            let src = clips.src_rect(frame, sheet_w, sheet_h);
            let draw_pos = (pos.0 + sprite.offset - offset).floor();

            let param = if avatar.facing_right {
                DrawParam::new().src(src).dest(draw_pos)
            } else {
                // mirror around the sprite's own width
                DrawParam::new()
                    .src(src)
                    .dest(draw_pos + Vec2::new(fw, 0.0))
                    .scale(Vec2::new(-1.0, 1.0))
            };
            canvas.draw(&self.player_image, param);
        }
    }
}

impl Scene for AdventureScene {
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition> {
        self.sim.step(input::read(ctx, &mut res.jump));
        crate::systems::camera::follow_avatar(&mut self.sim.world, &mut self.camera);
        Ok(Transition::None)
    }

    /// Render the world to the internal canvas before the frame canvas opens.
    fn pre_draw(&mut self, ctx: &mut Context, _res: &mut Resources) -> GameResult {
        self.queue_tiles();

        let mut canvas = Canvas::from_image(ctx, self.internal.clone(), CLEAR_COLOR);
        canvas.set_sampler(Sampler::nearest_clamp());
        canvas.draw(&self.tile_batch, DrawParam::default());
        self.draw_hazards(ctx, &mut canvas)?;
        self.draw_player(&mut canvas);
        self.debug.draw(
            ctx,
            &mut canvas,
            &self.sim,
            self.camera.offset(),
            Vec2::new(INTERNAL_WIDTH, INTERNAL_HEIGHT),
        )?;
        canvas.finish(ctx)
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, _res: &mut Resources) -> GameResult {
        // Uniform scale-up with nearest sampling; letterbox any aspect gap.
        let (win_w, win_h) = ctx.gfx.drawable_size();
        let scale = (win_w / INTERNAL_WIDTH).min(win_h / INTERNAL_HEIGHT);
        let dest = Vec2::new(
            (win_w - INTERNAL_WIDTH * scale) / 2.0,
            (win_h - INTERNAL_HEIGHT * scale) / 2.0,
        );

        canvas.set_sampler(Sampler::nearest_clamp());
        canvas.draw(
            &self.internal,
            DrawParam::new().dest(dest).scale(Vec2::splat(scale)),
        );
        canvas.set_sampler(Sampler::default());
        Ok(())
    }

    fn key_down(
        &mut self,
        _ctx: &mut Context,
        _res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            VirtualKeyCode::P => Transition::Push(Box::new(PauseScene)),
            VirtualKeyCode::M => Transition::Reset, // back to the main menu
            VirtualKeyCode::F1 => {
                self.debug.toggle();
                Transition::None
            }
            _ => Transition::None,
        }
    }
}
