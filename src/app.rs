//! Top-level ggez event handler: owns the scene stack and shared resources,
//! and runs the simulation on a fixed 60 Hz timestep so gameplay behaves the
//! same at any frame rate.

use ggez::event::EventHandler;
use ggez::graphics::{Canvas, Color};
use ggez::input::keyboard::KeyInput;
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::assets::Assets;
use crate::config::Config;
use crate::scenes::{main_menu::MainMenuScene, Resources, Scene, Transition};
use crate::sim::TICKS_PER_SECOND;
use crate::systems::input::InputLatch;

pub struct App {
    scenes: Vec<Box<dyn Scene>>,
    resources: Resources,
}

impl App {
    pub fn new(ctx: &mut Context, config: Config) -> Self {
        let mut resources = Resources {
            config,
            assets: Assets::new(),
            input: InputLatch::default(),
        };
        let mut scenes = Self::initial_stack();

        // Dev shortcut: SUPERGAME_SCENE=adventure boots straight into a map,
        // skipping the menu. SUPERGAME_MAP picks which one — a testbed map is
        // often the only way to get a specific thing on screen to look at.
        if std::env::var("SUPERGAME_SCENE").as_deref() == Ok("adventure") {
            let map = std::env::var("SUPERGAME_MAP")
                .unwrap_or_else(|_| "maps/castle.ron".to_string());
            match crate::scenes::adventure::AdventureScene::new(ctx, &mut resources, &map) {
                Ok(scene) => scenes.push(Box::new(scene)),
                Err(err) => eprintln!("failed to boot into adventure: {err:#}"),
            }
        }

        App { scenes, resources }
    }

    fn initial_stack() -> Vec<Box<dyn Scene>> {
        vec![Box::new(MainMenuScene)]
    }

    fn apply(&mut self, transition: Transition) {
        match transition {
            Transition::None => return,
            Transition::Push(scene) => self.scenes.push(scene),
            Transition::Pop => {
                self.scenes.pop();
                assert!(!self.scenes.is_empty(), "popped the last scene");
            }
            Transition::Reset => {
                self.scenes = Self::initial_stack();
            }
        }
        // The key that changed scenes must not also be a jump: unpausing with
        // a jump key held, or a stray press while the menu is up, should not
        // launch the player the moment the level resumes.
        self.resources.input.clear();
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

        let mut canvas = Canvas::from_frame(ctx, Color::BLACK);
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
                // Latch here, not in the tick loop: this is the only place
                // that sees every press exactly once. (`repeated` is filtered
                // above, so holding the key does not re-arm it.)
                self.resources.input.key_down(key);
                let top = self.scenes.len() - 1;
                let transition = self.scenes[top].key_down(ctx, &mut self.resources, key);
                self.apply(transition);
            }
            None => {}
        }
        Ok(())
    }
}
