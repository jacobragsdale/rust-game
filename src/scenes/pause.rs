//! Pause overlay: freezes the level underneath, pops on P — and the one place
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

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Text, TextFragment};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::save::{SaveError, SaveState, SaveStore};
use crate::scenes::{adventure::AdventureScene, Resources, Scene, Transition};

/// The slot the pause menu reads and writes.
///
/// One slot, named, rather than a list to pick from: a save browser is a
/// screen of its own, and [`crate::save::SaveStore::slots`] already returns
/// what it would list when somebody writes it. Everything below is unchanged
/// by that day arriving.
pub const SLOT: &str = "slot1";

/// A row of the menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Resume,
    Save,
    Load,
}

impl Entry {
    const ALL: [Entry; 3] = [Entry::Resume, Entry::Save, Entry::Load];

    fn label(self) -> &'static str {
        match self {
            Entry::Resume => "Resume",
            Entry::Save => "Save",
            Entry::Load => "Load",
        }
    }
}

pub struct PauseScene {
    /// The run as it stood when the menu opened, or why it could not be
    /// captured. Taken by [`crate::scenes::adventure::AdventureScene`] on the
    /// tick the world stopped: a paused scene does not update the one below
    /// it, so this *is* the state at the moment `Save` is chosen, and holding
    /// the snapshot rather than the sim is what keeps the menu unable to
    /// change the world it is describing.
    snapshot: Result<SaveState, SaveError>,
    selection: usize,
    /// What the last action did, shown under the menu. A failure that is only
    /// printed to stderr is a failure the player never sees — the store's
    /// errors already name their cause, so this shows them verbatim.
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

    /// Write the snapshot into the slot, saying what happened either way.
    fn save(&mut self, res: &Resources) {
        self.status = Some(match &self.snapshot {
            Ok(state) => match res.saves.write(SLOT, state) {
                Ok(()) => format!("Saved to `{SLOT}`."),
                Err(err) => err.to_string(),
            },
            // A run with no map file behind it — `Sim::fixture`'s case, which
            // the game itself cannot reach, but the message says so rather
            // than the row doing nothing.
            Err(err) => err.to_string(),
        });
    }

    /// Read the slot and rebuild the world from it.
    ///
    /// Returns the transition that replaces the stack, or `None` having put
    /// the reason on screen. Both failure paths — nothing saved yet, a save
    /// from another version, a file that is not a save — arrive here as a
    /// [`SaveError`] that already names its cause.
    fn load(&mut self, ctx: &mut Context, res: &mut Resources) -> Option<Transition> {
        let state = match res.saves.read(SLOT) {
            Ok(state) => state,
            Err(err) => {
                self.status = Some(err.to_string());
                return None;
            }
        };
        match AdventureScene::from_save(ctx, res, &state) {
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

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, res: &mut Resources) -> GameResult {
        let (view_w, view_h) = (res.config.display.width, res.config.display.height);

        let title = Text::new(
            TextFragment::new("Paused")
                .scale(PxScale::from(48.0))
                .color(Color::from_rgb(235, 235, 235)),
        );
        let title_size = title.measure(ctx)?;
        let mut y = view_h * 0.28;
        canvas.draw(
            &title,
            DrawParam::default().dest(Vec2::new((view_w - title_size.x) / 2.0, y)),
        );
        y += title_size.y + 24.0;

        for (index, entry) in Entry::ALL.iter().enumerate() {
            let selected = index == self.selection;
            let label = if selected {
                format!("> {} <", entry.label())
            } else {
                entry.label().to_string()
            };
            let text = Text::new(TextFragment::new(label).scale(PxScale::from(32.0)).color(
                if selected {
                    Color::from_rgb(255, 225, 120)
                } else {
                    Color::from_rgb(190, 190, 190)
                },
            ));
            let size = text.measure(ctx)?;
            canvas.draw(
                &text,
                DrawParam::default().dest(Vec2::new((view_w - size.x) / 2.0, y)),
            );
            y += size.y + 10.0;
        }

        y += 16.0;
        let footer = self
            .status
            .clone()
            .unwrap_or_else(|| "Up/Down to choose, Enter to pick, P to resume".to_string());
        let footer = Text::new(
            TextFragment::new(footer)
                .scale(PxScale::from(22.0))
                .color(Color::from_rgb(170, 170, 180)),
        );
        let size = footer.measure(ctx)?;
        canvas.draw(
            &footer,
            DrawParam::default().dest(Vec2::new((view_w - size.x) / 2.0, y)),
        );
        Ok(())
    }

    fn key_down(
        &mut self,
        ctx: &mut Context,
        res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            VirtualKeyCode::P => Transition::Pop,
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
                Entry::Load => self.load(ctx, res).unwrap_or(Transition::None),
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
