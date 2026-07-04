//! Top-level ggez event handler: owns the scene stack and shared resources,
//! and runs the simulation on a fixed 60 Hz timestep so gameplay behaves the
//! same at any frame rate.

use ggez::event::EventHandler;
use ggez::graphics::Canvas;
use ggez::input::keyboard::KeyInput;
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::assets::Assets;
use crate::config::Config;
use crate::save::ScoreStore;
use crate::scenes::{level::LevelScene, main_menu::MainMenuScene, Resources, Scene, Transition};

pub const TICKS_PER_SECOND: u32 = 60;
pub const TICK: f32 = 1.0 / TICKS_PER_SECOND as f32;

pub struct App {
    scenes: Vec<Box<dyn Scene>>,
    resources: Resources,
}

impl App {
    pub fn new(ctx: &mut Context, config: Config) -> Self {
        let score_store = ScoreStore::new("scores.db")
            .map(Some)
            .unwrap_or_else(|err| {
                eprintln!("Failed to open score database: {err}");
                None
            });
        let top_scores = score_store
            .as_ref()
            .and_then(|store| store.top_scores(3).ok())
            .unwrap_or_default();

        let mut resources = Resources {
            config,
            score_store,
            top_scores,
            clear_color: ggez::graphics::Color::BLACK,
            assets: Assets::new(),
        };
        let mut scenes = Self::initial_stack(&mut resources);

        // Dev shortcut: SUPERGAME_SCENE=adventure boots straight into the
        // castle map, skipping the menu.
        if std::env::var("SUPERGAME_SCENE").as_deref() == Ok("adventure") {
            match crate::scenes::adventure::AdventureScene::new(
                ctx,
                &mut resources,
                "maps/castle.ron",
            ) {
                Ok(scene) => scenes.push(Box::new(scene)),
                Err(err) => eprintln!("failed to boot into adventure: {err:#}"),
            }
        }

        App { scenes, resources }
    }

    /// A fresh run: the level underneath, the "press Up" menu on top.
    fn initial_stack(resources: &mut Resources) -> Vec<Box<dyn Scene>> {
        vec![
            Box::new(LevelScene::new(resources)),
            Box::new(MainMenuScene),
        ]
    }

    fn apply(&mut self, transition: Transition) {
        match transition {
            Transition::None => {}
            Transition::Push(scene) => self.scenes.push(scene),
            Transition::Pop => {
                self.scenes.pop();
                assert!(!self.scenes.is_empty(), "popped the last scene");
            }
            Transition::Reset => {
                self.scenes = Self::initial_stack(&mut self.resources);
            }
        }
    }

    /// Index of the deepest scene that should be processed, honoring the
    /// overlay flags of everything stacked above it.
    fn first_active(&self, below: impl Fn(&dyn Scene) -> bool) -> usize {
        let mut start = self.scenes.len() - 1;
        while start > 0 && below(self.scenes[start].as_ref()) {
            start -= 1;
        }
        start
    }
}

impl EventHandler for App {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        while ctx.time.check_update_time(TICKS_PER_SECOND) {
            let start = self.first_active(|s| s.updates_below());
            let top = self.scenes.len() - 1;
            let mut transition = Transition::None;
            for i in start..=top {
                let t = self.scenes[i].update(ctx, &mut self.resources)?;
                // Only the top scene may drive stack transitions.
                if i == top {
                    transition = t;
                }
            }
            self.apply(transition);
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let start = self.first_active(|s| s.draws_below());
        // Offscreen passes must finish before the frame canvas opens.
        for i in start..self.scenes.len() {
            self.scenes[i].pre_draw(ctx, &mut self.resources)?;
        }

        let mut canvas = Canvas::from_frame(ctx, self.resources.clear_color);
        for i in start..self.scenes.len() {
            self.scenes[i].draw(ctx, &mut canvas, &mut self.resources)?;
        }
        canvas.finish(ctx)
    }

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, repeated: bool) -> GameResult {
        if repeated {
            return Ok(());
        }
        match input.keycode {
            Some(VirtualKeyCode::Escape) => ctx.request_quit(),
            Some(key) => {
                let top = self.scenes.len() - 1;
                let transition = self.scenes[top].key_down(ctx, &mut self.resources, key);
                self.apply(transition);
            }
            None => {}
        }
        Ok(())
    }
}
