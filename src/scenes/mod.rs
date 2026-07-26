//! Scene stack (menus, gameplay, overlays): scenes can be transparent
//! overlays that draw (and optionally update) the scene beneath them.

pub mod adventure;
pub mod main_menu;
pub mod pause;

use ggez::graphics::Canvas;
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::assets::Assets;
use crate::config::Config;
use crate::systems::input::InputLatch;

/// Shared state that outlives any single scene.
pub struct Resources {
    pub config: Config,
    pub assets: Assets,
    /// One-shot presses waiting for a tick to act on them. Lives here rather
    /// than in the scene because presses arrive as window events, which only
    /// the app sees.
    pub input: InputLatch,
}

pub enum Transition {
    None,
    Push(Box<dyn Scene>),
    Pop,
    /// Tear the whole stack down and return to the main menu.
    Reset,
}

pub trait Scene {
    /// Runs once per fixed tick while this scene is active.
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition>;

    /// Offscreen rendering hook, called before the frame canvas is created.
    /// Scenes that render to an internal canvas (pixel-art scaling) do that
    /// work here; `draw` then just composites into the frame.
    fn pre_draw(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult {
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, res: &mut Resources) -> GameResult;

    /// One-shot key presses (menu navigation, pause). Only the top scene
    /// receives these; per-tick movement input is polled in `update`.
    fn key_down(
        &mut self,
        _ctx: &mut Context,
        _res: &mut Resources,
        _key: VirtualKeyCode,
    ) -> Transition {
        Transition::None
    }

    /// Overlay: the scene below is still drawn.
    fn draws_below(&self) -> bool {
        false
    }

    /// Overlay: the scene below keeps updating.
    fn updates_below(&self) -> bool {
        false
    }
}
