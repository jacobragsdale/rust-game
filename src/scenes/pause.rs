//! Pause overlay: freezes the level underneath, pops on P or Esc — and the one place
//! in the game a run can be written to disk or read back.
//!
//! **This scene decides nothing about a save.** It is handed one that
//! [`crate::sim::Sim::save`] already made, it hands one to
//! [`crate::sim::Sim::load_save`] by way of
//! [`AdventureScene::from_save`], and everything in between is a menu: which
//! row is highlighted, and what sentence to show when a store says no. What is
//! *in* a save, what survives it and what does not, is [`crate::save`]'s, and
//! `tests/save.rs` is what checks it — none of which would be true of a scene
//! that assembled the state itself.

use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::render::Frame;
use crate::save::{SaveError, SaveState, SaveStore};
use crate::scenes::menu::Menu;
use crate::scenes::{adventure::AdventureScene, Resources, Scene, Transition};
use crate::systems::input::Action;

/// The one save slot the menus read and write.
pub const SLOT: &str = "slot1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Resume,
    Save,
    Load,
    Quit,
}

impl Entry {
    const ALL: [Entry; 4] = [Entry::Resume, Entry::Save, Entry::Load, Entry::Quit];

    fn label(self) -> &'static str {
        match self {
            Entry::Resume => "Resume",
            Entry::Save => "Save",
            Entry::Load => "Load",
            Entry::Quit => "Quit to title",
        }
    }
}

pub struct PauseScene {
    snapshot: Result<SaveState, SaveError>,
    selection: usize,
    status: Option<String>,
}

impl PauseScene {
    pub fn new(snapshot: Result<SaveState, SaveError>) -> PauseScene {
        PauseScene {
            snapshot,
            selection: 0,
            status: None,
        }
    }

    fn entry(&self) -> Entry {
        Entry::ALL[self.selection.min(Entry::ALL.len() - 1)]
    }

    fn save(&mut self, res: &Resources) {
        self.status = Some(match &self.snapshot {
            Ok(state) => match res.saves.write(SLOT, state) {
                Ok(()) => format!("Saved to `{SLOT}`."),
                Err(err) => err.to_string(),
            },
            Err(err) => err.to_string(),
        });
    }

    fn load(&mut self, res: &mut Resources) -> Option<Transition> {
        let state = match res.saves.read(SLOT) {
            Ok(state) => state,
            Err(err) => {
                self.status = Some(err.to_string());
                return None;
            }
        };
        match AdventureScene::from_save(res, &state) {
            Ok(scene) => Some(Transition::Replace(Box::new(scene))),
            Err(err) => {
                self.status = Some(format!("{err:#}"));
                None
            }
        }
    }
}

impl Scene for PauseScene {
    fn update(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        Ok(Transition::None)
    }

    fn draw(&self, frame: &mut Frame, _res: &Resources) {
        let labels: Vec<&str> = Entry::ALL.iter().map(|e| e.label()).collect();
        Menu {
            title: "Paused",
            entries: &labels,
            selection: self.selection,
            footer: self
                .status
                .as_deref()
                .unwrap_or("up/down to choose, enter to pick, P or Esc to resume"),
            scrim: true,
        }
        .draw(frame);
    }

    fn key_down(
        &mut self,
        _ctx: &mut Context,
        res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            // P, or `cancel` — Esc — backs out to the game, as it backs out of
            // every other screen.
            VirtualKeyCode::P => Transition::Pop,
            key if Action::Cancel.triggered_by(key) => Transition::Pop,
            VirtualKeyCode::Up | VirtualKeyCode::W => {
                self.selection = self.selection.saturating_sub(1);
                Transition::None
            }
            VirtualKeyCode::Down | VirtualKeyCode::S => {
                self.selection = (self.selection + 1).min(Entry::ALL.len() - 1);
                Transition::None
            }
            VirtualKeyCode::Return => match self.entry() {
                Entry::Resume => Transition::Pop,
                Entry::Save => {
                    self.save(res);
                    Transition::None
                }
                Entry::Load => self.load(res).unwrap_or(Transition::None),
                Entry::Quit => Transition::Reset,
            },
            _ => Transition::None,
        }
    }

    fn draws_below(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::FileStore;

    /// A slot name is a file name, and one the store refuses turns every save
    /// from this menu into a line of red text nobody would connect to a
    /// rename. `exists` runs the same check `read` and `write` do.
    #[test]
    fn the_menus_slot_is_one_the_store_accepts() {
        let store = FileStore::new(std::env::temp_dir().join("supergame-pause-slot"));
        assert!(
            store.exists(SLOT).is_ok(),
            "`{SLOT}` is not a name the save store will take",
        );
    }

    /// And it has somewhere to put it. The directory is created on the first
    /// write, so what matters here is only that the path is the one the module
    /// documents rather than, say, the assets directory.
    #[test]
    fn saves_land_in_a_directory_named_for_them() {
        let dir = FileStore::default_dir();
        assert_eq!(dir.file_name().and_then(|n| n.to_str()), Some("saves"));
    }
}
