//! Scene stack (menus, gameplay, overlays), modeled after hump.gamestate from
//! SuperGame: scenes can be transparent overlays that draw (and optionally
//! update) the scene beneath them.

pub mod game_over;
pub mod level;
pub mod main_menu;
pub mod pause;

use ggez::graphics::{Canvas, Color};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::config::Config;
use crate::save::ScoreStore;

/// Shared state that outlives any single scene.
pub struct Resources {
    pub config: Config,
    pub score_store: Option<ScoreStore>,
    pub top_scores: Vec<f32>,
    /// Frame clear color; the level scene keeps this in sync with its palette.
    pub clear_color: Color,
}

impl Resources {
    pub fn record_score(&mut self, score: f32) {
        if let Some(store) = self.score_store.as_ref() {
            if let Err(err) = store.add_score(score) {
                eprintln!("Failed to save score: {err}");
            }
            if let Ok(scores) = store.top_scores(3) {
                self.top_scores = scores;
                return;
            }
        }

        // No database available: keep an in-memory leaderboard.
        self.top_scores.push(score);
        self.top_scores
            .sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        self.top_scores.truncate(3);
    }
}

pub enum Transition {
    None,
    Push(Box<dyn Scene>),
    Pop,
    /// Tear the whole stack down and start over (fresh level + main menu).
    Reset,
}

pub trait Scene {
    /// Runs once per fixed tick while this scene is active.
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition>;

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

    /// Overlay: the scene below keeps updating (e.g. the world keeps
    /// scrolling behind the game-over screen).
    fn updates_below(&self) -> bool {
        false
    }
}
