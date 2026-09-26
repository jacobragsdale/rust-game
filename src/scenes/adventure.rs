//! Adventure mode: the game being played.
//!
//! This scene owns no gameplay logic and no graphics. It holds a [`Sim`] —
//! which runs equally well headless — and a [`View`], which is what the player
//! sees of it. `update` feeds the sim one tick of input and lets the view keep
//! up; `draw` asks the view for a frame. That is the whole of it, which is why
//! the game, the `sim` binary and the `render` binary can never disagree about
//! what happens or what it looks like.

use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameError, GameResult};

use crate::render::Frame;
use crate::scenes::{pause::PauseScene, Resources, Scene, Transition};
use crate::sim::Sim;
use crate::systems::input;
use crate::view::View;

pub struct AdventureScene {
    /// The whole game state and its rules; contains no graphics resources.
    sim: Sim,
    view: View,
}

impl AdventureScene {
    /// Start a map from the beginning.
    pub fn new(res: &mut Resources, map: &str) -> anyhow::Result<Self> {
        let sim = Sim::load(&mut res.assets, map)?;
        AdventureScene::from_sim(res, sim)
    }

    /// Pick a saved run back up.
    ///
    /// A second constructor rather than a `map` parameter that sometimes means
    /// "and then move everything": a save names its own map, and
    /// [`Sim::load_save`] is what turns one back into a world. The two meet at
    /// [`AdventureScene::from_sim`], so the scene cannot tell a loaded run from
    /// a fresh one and nothing about a save is decided here.
    pub fn from_save(res: &mut Resources, save: &crate::save::SaveState) -> anyhow::Result<Self> {
        let sim = Sim::load_save(&mut res.assets, save)?;
        AdventureScene::from_sim(res, sim)
    }

    fn from_sim(res: &mut Resources, sim: Sim) -> anyhow::Result<Self> {
        let view = View::new(&sim, &mut res.assets)?;
        Ok(AdventureScene { sim, view })
    }

    /// The pause overlay for this run, holding a snapshot of it.
    ///
    /// The snapshot is taken *here*, when the world stops, rather than when
    /// `Save` is chosen: a paused scene does not update the one below it, so
    /// the two are the same state, and the menu never holds a `&mut Sim` it
    /// could decide something with.
    pub fn pause(&self) -> PauseScene {
        PauseScene::new(self.sim.save())
    }
}

impl Scene for AdventureScene {
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition> {
        self.sim.step(input::read(ctx, &mut res.input));
        self.view
            .after_step(&self.sim, &mut res.assets)
            .map_err(|err| GameError::CustomError(format!("{err:#}")))?;
        Ok(Transition::None)
    }

    fn draw(&self, frame: &mut Frame, _res: &Resources) {
        self.view.draw(&self.sim, frame);
    }

    fn key_down(
        &mut self,
        _ctx: &mut Context,
        _res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            // `Pause` is also what the app sends when the window loses focus.
            VirtualKeyCode::P | VirtualKeyCode::Pause => Transition::Push(Box::new(self.pause())),
            VirtualKeyCode::F1 => {
                self.view.debug.toggle();
                Transition::None
            }
            _ => Transition::None,
        }
    }
}
