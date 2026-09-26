//! What the player sees of a [`Sim`]: the camera, the debug toggle, and the
//! composition of one frame — world, HUD, whichever overlay the mode calls
//! for, and the F1 overlay on top.
//!
//! **No graphics context anywhere in here.** A `View` steps alongside a sim
//! and draws into a [`Frame`], which is data; the window's backend and the
//! headless one both turn that into pixels. That is what lets
//! `cargo run --bin render` show any tick of any tape exactly as the game
//! would — the same camera that followed the same player through the same
//! ticks, not a reconstruction of it.
//!
//! State that advances with the sim but is not simulation lives here: where
//! the camera is, and — in [`fx`] — particles, shake, fades, toasts and a
//! map's title. None of it may decide anything; the sim never reads it back.

pub mod fx;
pub mod world;

use std::rc::Rc;

use ggez::glam::Vec2;

use crate::assets::{Assets, TilesetDef};
use crate::debug::DebugOverlay;
use crate::render::{Color, Frame, VIEW};
use crate::scenes::{dialogue, inventory};
use crate::sim::{Mode, Sim};
use crate::systems::camera::Camera;

/// What the world is drawn over.
const CLEAR: Color = Color::new(0.04, 0.03, 0.08, 1.0);

pub struct View {
    camera: Camera,
    pub debug: DebugOverlay,
    fx: fx::Fx,
    tileset: Rc<TilesetDef>,
    /// The map the camera and tileset were set up for. When the sim's map
    /// changes under it — a door — the view re-reads both.
    map: Option<String>,
}

impl View {
    pub fn new(sim: &Sim, assets: &mut Assets) -> anyhow::Result<View> {
        let mut view = View {
            camera: Camera::new(VIEW, Vec2::ZERO),
            debug: DebugOverlay::from_env(),
            fx: fx::Fx::new(assets.effects()?, sim),
            tileset: assets.tileset(&sim.level.tileset)?,
            map: sim.map.clone(),
        };
        view.reset(sim, assets)?;
        Ok(view)
    }

    /// Point the camera at a freshly loaded map, snapped rather than eased.
    fn reset(&mut self, sim: &Sim, assets: &mut Assets) -> anyhow::Result<()> {
        self.tileset = assets.tileset(&sim.level.tileset)?;
        self.map = sim.map.clone();
        self.camera = Camera::new(
            VIEW,
            Vec2::new(sim.level.pixel_width(), sim.level.pixel_height()),
        );
        crate::systems::camera::follow_avatar(&sim.world, &mut self.camera);
        self.fx.arrive(sim);
        Ok(())
    }

    /// Advance with the sim: called once after every [`Sim::step`].
    pub fn after_step(&mut self, sim: &Sim, assets: &mut Assets) -> anyhow::Result<()> {
        if sim.map != self.map {
            self.reset(sim, assets)?;
        }
        crate::systems::camera::follow_avatar(&sim.world, &mut self.camera);
        self.fx.after_step(sim);
        Ok(())
    }

    /// Where the camera's top-left is, in world pixels, snapped to a whole
    /// pixel so every layer is aligned to the same grid — and shaken, when
    /// something has just hit hard.
    pub fn offset(&self) -> Vec2 {
        self.camera.offset().round() + self.fx.jolt()
    }

    /// One whole frame: the world, the HUD, the overlay the mode calls for,
    /// and the debug overlay if it is on.
    pub fn draw(&self, sim: &Sim, frame: &mut Frame) {
        frame.clear = self.clear();
        let offset = self.offset();
        world::draw(frame, sim, &self.tileset, offset, VIEW);
        self.fx.draw_world(frame, offset);
        crate::hud::draw(frame, sim, offset);
        self.fx.draw_screen(frame);
        // Matched exhaustively rather than tested with `if`, so a third modal
        // state cannot be added without deciding here what it looks like.
        match sim.mode() {
            Mode::Playing => {}
            Mode::Inventory => inventory::draw(frame, sim, VIEW),
            Mode::Dialogue => dialogue::draw(frame, sim, VIEW, offset),
        }
        self.debug.draw(frame, sim, offset, VIEW);
    }

    /// The whole level at once, at 1:1, with no HUD — for looking at a map as
    /// a map. The debug overlay is drawn if it is on.
    pub fn draw_level(&self, sim: &Sim) -> Frame {
        let size = Vec2::new(sim.level.pixel_width(), sim.level.pixel_height());
        let mut frame = Frame::new(size, self.clear());
        world::draw(&mut frame, sim, &self.tileset, Vec2::ZERO, size);
        self.debug.draw(&mut frame, sim, Vec2::ZERO, size);
        frame
    }
}

impl View {
    /// What the world is drawn over on this map: its tileset's sky, or dusk.
    fn clear(&self) -> Color {
        self.tileset
            .clear
            .map_or(CLEAR, |(r, g, b)| Color::from_rgb(r, g, b))
    }
}

/// Which way an entity faces, for drawing it and for outlining it.
pub fn faces_right(world: &hecs::World, entity: hecs::Entity) -> bool {
    world::facing(world, entity)
}
