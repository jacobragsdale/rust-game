//! The title screen: start a new game, continue a saved one, or quit.

use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::render::Frame;
use crate::save::SaveStore;
use crate::scenes::menu::Menu;
use crate::scenes::pause::SLOT;
use crate::scenes::{adventure::AdventureScene, Resources, Scene, Transition};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    NewGame,
    Continue,
    Quit,
}

impl Entry {
    fn label(self) -> &'static str {
        match self {
            Entry::NewGame => "New Game",
            Entry::Continue => "Continue",
            Entry::Quit => "Quit",
        }
    }
}

#[derive(Default)]
pub struct MainMenuScene {
    selection: usize,
    /// Why the last attempt to start did not, if it did not.
    status: Option<String>,
}

impl MainMenuScene {
    /// What is on offer: `Continue` only when there is a save to continue.
    fn entries(res: &Resources) -> Vec<Entry> {
        let mut entries = vec![Entry::NewGame];
        if res.saves.exists(SLOT).unwrap_or(false) {
            entries.push(Entry::Continue);
        }
        entries.push(Entry::Quit);
        entries
    }

    fn start(&mut self, res: &mut Resources, entry: Entry, ctx: &mut Context) -> Transition {
        let started = match entry {
            Entry::NewGame => {
                let map = res.config.game.start_map.clone();
                AdventureScene::new(res, &map)
            }
            Entry::Continue => res
                .saves
                .read(SLOT)
                .map_err(anyhow::Error::from)
                .and_then(|save| AdventureScene::from_save(res, &save)),
            Entry::Quit => {
                ctx.request_quit();
                return Transition::None;
            }
        };
        match started {
            Ok(scene) => Transition::Push(Box::new(scene)),
            Err(err) => {
                self.status = Some(format!("{err:#}"));
                Transition::None
            }
        }
    }
}

impl Scene for MainMenuScene {
    fn update(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        Ok(Transition::None)
    }

    fn draw(&self, frame: &mut Frame, res: &Resources) {
        let entries = MainMenuScene::entries(res);
        let labels: Vec<&str> = entries.iter().map(|e| e.label()).collect();
        Menu {
            title: "SuperGame",
            entries: &labels,
            selection: self.selection.min(labels.len() - 1),
            footer: self.status.as_deref().unwrap_or(
                "arrows/WASD move, space jump, J attack, C cast\nE talk, I bag, P pause, Esc back",
            ),
            scrim: false,
        }
        .draw(frame);
    }

    fn key_down(
        &mut self,
        ctx: &mut Context,
        res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        let entries = MainMenuScene::entries(res);
        match key {
            VirtualKeyCode::Up | VirtualKeyCode::W => {
                self.selection = self.selection.saturating_sub(1);
                Transition::None
            }
            VirtualKeyCode::Down | VirtualKeyCode::S => {
                self.selection = (self.selection + 1).min(entries.len() - 1);
                Transition::None
            }
            VirtualKeyCode::Return => {
                let entry = entries[self.selection.min(entries.len() - 1)];
                self.start(res, entry, ctx)
            }
            _ => Transition::None,
        }
    }
}
