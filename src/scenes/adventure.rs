//! Adventure mode: a Tiled/ASCII level rendered as pixel art on a 640x360
//! internal canvas, scaled up to the window. First playable slice of the RPG.

use std::path::PathBuf;
use std::rc::Rc;

use ggez::glam::Vec2;
use ggez::graphics::{
    self, Canvas, Color, DrawParam, Image, ImageFormat, InstanceArray, Mesh, MeshBuilder, Sampler,
};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};
use hecs::World;

use crate::app::TICK;
use crate::assets::{ClipSet, TilesetDef};
use crate::ecs::components::{AnimationState, Avatar, Position, Size, Sprite, Velocity};
use crate::level::LevelData;
use crate::physics::SolidRect;
use crate::scenes::{pause::PauseScene, Resources, Scene, Transition};
use crate::systems::{animation, avatar, camera::Camera, input};

pub const INTERNAL_WIDTH: f32 = 640.0;
pub const INTERNAL_HEIGHT: f32 = 360.0;

const CLEAR_COLOR: Color = Color {
    r: 0.04,
    g: 0.03,
    b: 0.08,
    a: 1.0,
};

pub struct AdventureScene {
    world: World,
    level: LevelData,
    /// Solids + one-way platforms, pre-flattened for the physics pass.
    geometry: Vec<SolidRect>,
    camera: Camera,
    /// 640x360 offscreen target; the window draw scales this up.
    internal: Image,
    tileset: Rc<TilesetDef>,
    tile_batch: InstanceArray,
    tile_sheet_size: Vec2,
    player_image: Image,
    player_clips: Rc<ClipSet>,
}

impl AdventureScene {
    pub fn new(ctx: &mut Context, res: &mut Resources, map: &str) -> anyhow::Result<Self> {
        let map_path: PathBuf = res.assets.base_dir().join(map);
        let level = LevelData::load(&map_path, &mut res.assets)?;

        let tileset = res.assets.tileset(&level.tileset)?;
        let tile_image = res
            .assets
            .image(ctx, &tileset.image, tileset.transparent_color)?;
        let tile_sheet_size = Vec2::new(tile_image.width() as f32, tile_image.height() as f32);
        let tile_batch = InstanceArray::new(ctx, tile_image);

        let player_clips = res.assets.clip_set("player")?;
        let player_image = res.assets.image(ctx, &player_clips.sheet, None)?;

        let internal = Image::new_canvas_image(
            ctx,
            ImageFormat::Rgba8UnormSrgb,
            INTERNAL_WIDTH as u32,
            INTERNAL_HEIGHT as u32,
            1,
        );

        let mut geometry: Vec<SolidRect> =
            level.solids.iter().map(|&r| SolidRect::solid(r)).collect();
        geometry.extend(level.one_way.iter().map(|&r| SolidRect::one_way(r)));

        let mut world = World::new();
        let spawn = level.player_spawn;
        let (fw, fh) = player_clips.frame_size;
        world.spawn((
            Avatar::new(spawn),
            Position(spawn),
            Velocity(Vec2::ZERO),
            Size(Vec2::new(Avatar::WIDTH, Avatar::HEIGHT)),
            Sprite {
                // sprite centered horizontally on the collider, feet aligned
                offset: Vec2::new((Avatar::WIDTH - fw) / 2.0, Avatar::HEIGHT - fh),
            },
            AnimationState::new("idle"),
        ));

        let camera = Camera::new(
            Vec2::new(INTERNAL_WIDTH, INTERNAL_HEIGHT),
            Vec2::new(level.pixel_width(), level.pixel_height()),
        );

        Ok(AdventureScene {
            world,
            level,
            geometry,
            camera,
            internal,
            tileset,
            tile_batch,
            tile_sheet_size,
            player_image,
            player_clips,
        })
    }

    fn queue_tiles(&mut self) {
        let offset = self.camera.offset();
        let ts = self.level.tile_size;
        let x0 = (offset.x / ts).floor().max(0.0) as u32;
        let y0 = (offset.y / ts).floor().max(0.0) as u32;
        let x1 = (((offset.x + INTERNAL_WIDTH) / ts).ceil() as u32 + 1).min(self.level.width);
        let y1 = (((offset.y + INTERNAL_HEIGHT) / ts).ceil() as u32 + 1).min(self.level.height);

        let mut params = Vec::new();
        for layer in [&self.level.background, &self.level.tiles] {
            if layer.is_empty() {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let Some(tile) = layer[(y * self.level.width + x) as usize] else {
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
        if self.level.hazards.is_empty() {
            return Ok(());
        }
        let offset = self.camera.offset();
        let mut mb = MeshBuilder::new();
        let spike_w = 8.0;
        for hazard in &self.level.hazards {
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

    fn draw_player(&mut self, canvas: &mut Canvas) {
        let offset = self.camera.offset();
        let (fw, _) = self.player_clips.frame_size;
        let sheet_w = self.player_image.width() as f32;
        let sheet_h = self.player_image.height() as f32;

        for (_, (pos, sprite, anim, avatar)) in
            self.world
                .query_mut::<(&Position, &Sprite, &AnimationState, &Avatar)>()
        {
            let Some(clip) = self.player_clips.clip(&anim.clip) else {
                continue;
            };
            let frame = clip.frames[anim.frame.min(clip.frames.len() - 1)];
            let src = self.player_clips.src_rect(frame, sheet_w, sheet_h);
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
    fn update(&mut self, ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        let player_input = input::read(ctx);

        avatar::update(
            &mut self.world,
            &self.level,
            &self.geometry,
            player_input,
            TICK,
        );
        animation::select_avatar_clip(&mut self.world);
        animation::advance(&mut self.world, &self.player_clips, TICK);

        crate::systems::camera::follow_avatar(&mut self.world, &mut self.camera);
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
            _ => Transition::None,
        }
    }
}
