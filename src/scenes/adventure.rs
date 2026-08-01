//! Adventure mode: a Tiled/ASCII level rendered as pixel art on a 640x360
//! internal canvas, scaled up to the window. First playable slice of the RPG.
//!
//! This scene owns no gameplay logic. It holds a [`Sim`] — which runs equally
//! well headless — plus the GPU resources needed to look at it. Everything
//! `update` does is feed the sim one tick of input, which is why the game and
//! the `sim` binary can never disagree about what happens.

use std::collections::HashMap;
use std::rc::Rc;

use ggez::glam::Vec2;
use ggez::graphics::{
    self, Canvas, Color, DrawParam, Image, ImageFormat, InstanceArray, Mesh, MeshBuilder, Sampler,
};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::assets::TilesetDef;
use crate::debug::DebugOverlay;
use crate::ecs::components::{
    AnimationState, Avatar, Collider, Fire, Health, Mover, Patrol, Pendulum, Pickup, Position,
    Size, Sprite,
};
use crate::scenes::{dialogue, inventory, pause::PauseScene, Resources, Scene, Transition};
use crate::sim::{Mode, Sim};
use crate::systems::{camera::Camera, input};

pub const INTERNAL_WIDTH: f32 = 640.0;
pub const INTERNAL_HEIGHT: f32 = 360.0;

/// How long the white hit flash lasts.
const FLASH_TICKS: u32 = 6;

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
    /// Every sprite sheet the world's entities reference, by name. Uploaded
    /// once at scene construction rather than per frame.
    sheets: HashMap<String, Image>,
    debug: DebugOverlay,
}

impl AdventureScene {
    /// Start a map from the beginning.
    pub fn new(ctx: &mut Context, res: &mut Resources, map: &str) -> anyhow::Result<Self> {
        let sim = Sim::load(&mut res.assets, map)?;
        AdventureScene::from_sim(ctx, res, sim)
    }

    /// Pick a saved run back up.
    ///
    /// A second constructor rather than a `map` parameter that sometimes means
    /// "and then move everything": a save names its own map, and
    /// [`Sim::load_save`] is what turns one back into a world. Everything below
    /// this line is graphics, which is why the two constructors meet at
    /// [`AdventureScene::from_sim`] — the scene cannot tell a loaded run from a
    /// fresh one, and nothing about a save is decided here.
    pub fn from_save(
        ctx: &mut Context,
        res: &mut Resources,
        save: &crate::save::SaveState,
    ) -> anyhow::Result<Self> {
        let sim = Sim::load_save(&mut res.assets, save)?;
        AdventureScene::from_sim(ctx, res, sim)
    }

    /// Build the scene around a sim that already exists: upload the tiles and
    /// the sheets its world references, and point a camera at it.
    fn from_sim(ctx: &mut Context, res: &mut Resources, sim: Sim) -> anyhow::Result<Self> {
        let tileset = res.assets.tileset(&sim.level.tileset)?;
        let tile_image = res
            .assets
            .image(ctx, &tileset.image, tileset.transparent_color)?;
        let tile_sheet_size = Vec2::new(tile_image.width() as f32, tile_image.height() as f32);
        let tile_batch = InstanceArray::new(ctx, tile_image);

        // Collect the distinct sheets across every entity's clip set. Doing it
        // from the world means adding an NPC needs no change here.
        let mut wanted: Vec<String> = Vec::new();
        for (_, sprite) in sim.world.query::<&Sprite>().iter() {
            for clip in sprite.clips.clips.values() {
                let sheet = sprite.clips.sheet_of(clip);
                if !wanted.iter().any(|s| s == sheet) {
                    wanted.push(sheet.to_string());
                }
            }
        }
        let mut sheets = HashMap::new();
        for name in wanted {
            let image = res.assets.image(ctx, &name, None)?;
            sheets.insert(name, image);
        }

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
            sheets,
            debug: DebugOverlay::from_env(),
        })
    }

    /// The pause overlay for this run, holding a snapshot of it.
    ///
    /// Public because the boot shortcut in [`crate::app`] opens it too — a
    /// menu is the one thing in the game a tape cannot press a key in, so
    /// being able to boot straight into it is how it gets looked at.
    ///
    /// The snapshot is taken *here*, when the world stops, rather than when
    /// `Save` is chosen: a paused scene does not update the one below it, so
    /// the two are the same state, and the menu never holds a `&mut Sim` it
    /// could decide something with.
    pub fn pause(&self) -> PauseScene {
        PauseScene::new(self.sim.save())
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
        let fires = self.sim.world.query::<(&Position, &Fire)>().iter().count();
        if self.sim.level.hazards.is_empty() && fires == 0 {
            return Ok(());
        }
        let offset = self.camera.offset();
        let mut mb = MeshBuilder::new();

        // Placeholder spikes (the original spike art was never committed):
        // white triangles across each hazard strip. Replaced in Phase 3.
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

        // Fires. Placeholder art like the spikes, but the on/off states have
        // to be unmistakable at a glance: an invisible hazard is the bug this
        // whole feature is most likely to ship.
        for (entity, (pos, fire)) in self.sim.world.query::<(&Position, &Fire)>().iter() {
            let box_ = fire.collider.aabb(pos.0);
            let (left, right) = (box_.x - offset.x, box_.right() - offset.x);
            let (top, bottom) = (box_.y - offset.y, box_.bottom() - offset.y);
            let mid = (left + right) / 2.0;

            if crate::systems::hazard::is_lit(&self.sim.world, entity) {
                // A full-height flame in two layers, so it reads as fire and
                // not as an orange triangle.
                mb.polygon(
                    graphics::DrawMode::fill(),
                    &[
                        Vec2::new(left, bottom),
                        Vec2::new(right, bottom),
                        Vec2::new(mid, top),
                    ],
                    Color::from_rgb(240, 120, 30),
                )?;
                mb.polygon(
                    graphics::DrawMode::fill(),
                    &[
                        Vec2::new(left + box_.w * 0.25, bottom),
                        Vec2::new(right - box_.w * 0.25, bottom),
                        Vec2::new(mid, top + box_.h * 0.35),
                    ],
                    Color::from_rgb(255, 226, 120),
                )?;
            } else {
                // Out: a low bed of coals, dark and only a few pixels tall.
                // Same footprint, nothing like the same silhouette.
                mb.rectangle(
                    graphics::DrawMode::fill(),
                    graphics::Rect::new(left, bottom - 4.0, box_.w, 4.0),
                    Color::from_rgb(90, 40, 35),
                )?;
                mb.rectangle(
                    graphics::DrawMode::fill(),
                    graphics::Rect::new(left + 2.0, bottom - 2.0, box_.w - 4.0, 2.0),
                    Color::from_rgb(150, 60, 35),
                )?;
            }
        }

        let mesh = Mesh::from_data(ctx, mb.build());
        canvas.draw(&mesh, DrawParam::default());
        Ok(())
    }

    /// Draw the moving platforms.
    ///
    /// Placeholder art, like the spikes and the fires, but drawn from the
    /// collider the physics actually uses rather than from the `Mover`'s path —
    /// so if the two ever disagree, the platform visibly leaves its own
    /// collision box behind instead of hiding the bug.
    fn draw_movers(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let offset = self.camera.offset();
        let mut any = false;
        let mut mb = MeshBuilder::new();

        for (_, (pos, collider)) in self
            .sim
            .world
            .query::<(&Position, &Collider)>()
            .with::<&Mover>()
            .iter()
        {
            let box_ = collider.aabb(pos.0);
            let rect = graphics::Rect::new(box_.x - offset.x, box_.y - offset.y, box_.w, box_.h);
            mb.rectangle(
                graphics::DrawMode::fill(),
                rect,
                Color::from_rgb(96, 88, 104),
            )?;
            // A bright lip along the walkable surface: the top edge is the
            // only part of a platform the player is really aiming at.
            mb.rectangle(
                graphics::DrawMode::fill(),
                graphics::Rect::new(rect.x, rect.y, rect.w, 3.0_f32.min(box_.h)),
                Color::from_rgb(196, 186, 210),
            )?;
            any = true;
        }

        if any {
            let mesh = Mesh::from_data(ctx, mb.build());
            canvas.draw(&mesh, DrawParam::default());
        }
        Ok(())
    }

    /// Draw the items lying on the floor.
    ///
    /// Coloured quads rather than sprites, because item art does not exist
    /// yet: a pickup carries no `Sprite`, so it is invisible to `draw_sprites`
    /// entirely, and this fills the gap from the item's kind. The palette is
    /// [`inventory::tint`]'s, shared with the bag — a red square on the ground
    /// and a red square in the inventory have to be the same potion.
    ///
    /// Drawn from the collider the pickup is actually collected on, so art
    /// that disagreed with the reach would be visible rather than hidden.
    fn draw_pickups(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let offset = self.camera.offset();
        let mut any = false;
        let mut mb = MeshBuilder::new();

        for (_, (pos, size, pickup)) in self.sim.world.query::<(&Position, &Size, &Pickup)>().iter()
        {
            let at = (pos.0 - offset).floor();
            mb.rectangle(
                graphics::DrawMode::fill(),
                graphics::Rect::new(at.x - 1.0, at.y - 1.0, size.0.x + 2.0, size.0.y + 2.0),
                Color::from_rgb(18, 16, 26),
            )?;
            mb.rectangle(
                graphics::DrawMode::fill(),
                graphics::Rect::new(at.x, at.y, size.0.x, size.0.y),
                inventory::tint(&self.sim.items, &pickup.item),
            )?;
            any = true;
        }

        if any {
            let mesh = Mesh::from_data(ctx, mb.build());
            canvas.draw(&mesh, DrawParam::default());
        }
        Ok(())
    }

    /// Draw the swinging hazards: a chain to the anchor and a spiked ball on
    /// the end of it.
    ///
    /// Placeholder art like the spikes, the fires and the platforms, and drawn
    /// off the collider the physics actually uses for the same reason
    /// [`AdventureScene::draw_movers`] is. The ball's radius is *derived* from
    /// its collider — the lethal box is the square inscribed in the circle, so
    /// the circle is the box's diagonal — rather than stored beside it. There
    /// is therefore no second number that could be edited out of step with the
    /// first, which is the failure mode that makes a hazard look like it kills
    /// from a pixel away.
    fn draw_swings(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let offset = self.camera.offset();
        let mut any = false;
        let mut mb = MeshBuilder::new();

        for (_, (pos, collider, pendulum)) in self
            .sim
            .world
            .query::<(&Position, &Collider, &Pendulum)>()
            .iter()
        {
            let anchor = pendulum.anchor - offset;
            let centre = pos.0 - offset;
            let radius = collider.size.x * std::f32::consts::SQRT_2 / 2.0;

            // The chain, and a bracket where it is bolted to the world.
            mb.line(&[anchor, centre], 2.0, Color::from_rgb(120, 116, 128))?;
            mb.rectangle(
                graphics::DrawMode::fill(),
                graphics::Rect::new(anchor.x - 4.0, anchor.y - 4.0, 8.0, 8.0),
                Color::from_rgb(80, 76, 88),
            )?;

            // Spikes first, so the ball is drawn over their roots and they
            // read as sticking out of it rather than lying across it.
            const SPIKES: usize = 8;
            for i in 0..SPIKES {
                let angle = std::f32::consts::TAU * i as f32 / SPIKES as f32;
                let out = Vec2::new(angle.cos(), angle.sin());
                let across = Vec2::new(-out.y, out.x) * radius * 0.4;
                mb.polygon(
                    graphics::DrawMode::fill(),
                    &[
                        centre + out * radius * 1.55,
                        centre + across,
                        centre - across,
                    ],
                    Color::from_rgb(190, 186, 200),
                )?;
            }
            mb.circle(
                graphics::DrawMode::fill(),
                centre,
                radius,
                0.2,
                Color::from_rgb(64, 60, 72),
            )?;
            // A highlight up and to the left, so the ball reads as a sphere
            // and not as a hole in the level.
            mb.circle(
                graphics::DrawMode::fill(),
                centre - Vec2::splat(radius * 0.3),
                radius * 0.35,
                0.2,
                Color::from_rgb(120, 114, 132),
            )?;
            any = true;
        }

        if any {
            let mesh = Mesh::from_data(ctx, mb.build());
            canvas.draw(&mesh, DrawParam::default());
        }
        Ok(())
    }

    /// Draw every sprite in the world, whatever sheet it comes from.
    ///
    fn draw_sprites(&self, canvas: &mut Canvas) {
        let offset = self.camera.offset();

        for (entity, (pos, size, sprite, anim)) in self
            .sim
            .world
            .query::<(&Position, &Size, &Sprite, &AnimationState)>()
            .iter()
        {
            let Some(clip) = sprite.clips.clip(&anim.clip) else {
                continue;
            };
            let Some(image) = self.sheets.get(sprite.clips.sheet_of(clip)) else {
                continue;
            };

            let (sheet_w, sheet_h) = (image.width() as f32, image.height() as f32);
            let (fw, fh) = sprite.clips.frame_size_of(clip);
            let frame = clip.frames[anim.frame.min(clip.frames.len() - 1)];
            let src = sprite.clips.src_rect(clip, frame, sheet_w, sheet_h);
            let draw_pos = (sprite.draw_origin(pos.0, size.0, (fw, fh)) - offset).floor();

            let param = if self.faces_right(entity) {
                DrawParam::new().src(src).dest(draw_pos)
            } else {
                // mirror around the sprite's own width
                DrawParam::new()
                    .src(src)
                    .dest(draw_pos + Vec2::new(fw, 0.0))
                    .scale(Vec2::new(-1.0, 1.0))
            };
            canvas.draw(image, param.color(self.tint(entity)));
        }
    }

    /// What colour to draw an entity's sprite.
    ///
    /// Two things multiply here. The hit flash is a state, and the kind tint is
    /// a stand-in for art that does not exist: `assets/data/animations/
    /// villager.ron` is the knight's sprite pack unmodified, because there is
    /// no villager art in the repo, so a villager and the thing that stabs you
    /// would otherwise be the same pixels. Recolouring at draw time means
    /// exactly one thing to delete the day real art lands.
    fn tint(&self, entity: hecs::Entity) -> Color {
        let base = self.kind_tint(entity);
        let flash = self.hit_tint(entity);
        Color::new(
            base.r * flash.r,
            base.g * flash.g,
            base.b * flash.b,
            base.a * flash.a,
        )
    }

    /// The stand-in recolour for a kind with no art of its own.
    fn kind_tint(&self, entity: hecs::Entity) -> Color {
        match self
            .sim
            .world
            .get::<&crate::ecs::components::Kind>(entity)
            .ok()
            .map(|kind| kind.0.clone())
            .as_deref()
        {
            // Green and washed out: unmistakably not a knight at a glance,
            // which is the whole job.
            Some("villager") => Color::new(0.55, 0.95, 0.62, 1.0),
            _ => Color::WHITE,
        }
    }

    /// A blown-out tint for the first few ticks after being hit.
    ///
    /// The knight's art has no hurt animation, so without this a hit changes
    /// nothing about it except a number — the blow lands on something that
    /// does not react. Driven off the tail of the i-frame window so it flashes
    /// on the hit rather than for the whole invulnerable period.
    fn hit_tint(&self, entity: hecs::Entity) -> Color {
        let flashing = self
            .sim
            .world
            .get::<&Health>(entity)
            .is_ok_and(|h| h.iframes + FLASH_TICKS > h.iframe_ticks && !h.dead());
        if flashing {
            Color::new(3.0, 3.0, 3.0, 1.0)
        } else {
            Color::WHITE
        }
    }

    /// Which way an entity is facing. The player tracks it on `Avatar`, a
    /// patroller in its walk direction; anything else faces right.
    fn faces_right(&self, entity: hecs::Entity) -> bool {
        if let Ok(avatar) = self.sim.world.get::<&Avatar>(entity) {
            return avatar.facing_right;
        }
        if let Ok(patrol) = self.sim.world.get::<&Patrol>(entity) {
            return patrol.dir >= 0.0;
        }
        true
    }
}

impl Scene for AdventureScene {
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition> {
        self.sim.step(input::read(ctx, &mut res.input));
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
        self.draw_movers(ctx, &mut canvas)?;
        self.draw_swings(ctx, &mut canvas)?;
        self.draw_pickups(ctx, &mut canvas)?;
        self.draw_sprites(&mut canvas);
        crate::hud::draw(&mut canvas, &self.sim, self.camera.offset());
        // The overlays follow the sim's mode rather than the scene stack: the
        // mode is the one place "the inventory is open" or "a conversation is
        // running" is recorded, and a stack entry beside it would be a second
        // one that could disagree. Matched exhaustively rather than tested with
        // `if`, so a third modal state cannot be added without deciding here
        // what it looks like.
        match self.sim.mode() {
            Mode::Playing => {}
            Mode::Inventory => inventory::draw(
                &mut canvas,
                &self.sim,
                Vec2::new(INTERNAL_WIDTH, INTERNAL_HEIGHT),
            ),
            Mode::Dialogue => dialogue::draw(
                &mut canvas,
                &self.sim,
                Vec2::new(INTERNAL_WIDTH, INTERNAL_HEIGHT),
            ),
        }
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
            VirtualKeyCode::P => Transition::Push(Box::new(self.pause())),
            VirtualKeyCode::M => Transition::Reset, // back to the main menu
            VirtualKeyCode::F1 => {
                self.debug.toggle();
                Transition::None
            }
            _ => Transition::None,
        }
    }
}
