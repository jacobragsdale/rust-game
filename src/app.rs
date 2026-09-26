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
use crate::render::gpu::GpuRenderer;
use crate::render::Frame;
use crate::save::FileStore;
use crate::scenes::{main_menu::MainMenuScene, Resources, Scene, Transition};
use crate::sim::TICKS_PER_SECOND;
use crate::systems::input::InputLatch;

/// The most fixed ticks one frame may run to catch up.
///
/// The accumulator is otherwise unbounded: a hitch — a first frame that spent
/// a quarter of a second decoding PNGs, a window dragged between screens —
/// banks the missed time and pays it back as a burst of ticks the player never
/// saw, which in a platformer is a jump that happened off screen. Past this the
/// game runs slow for a frame instead, which is the lesser wrong.
const MAX_TICKS_PER_FRAME: u32 = 4;

pub struct App {
    scenes: Vec<Box<dyn Scene>>,
    resources: Resources,
    renderer: GpuRenderer,
    /// Frames drawn, for `SUPERGAME_CAPTURE`.
    frames: u32,
}

impl App {
    pub fn new(ctx: &mut Context, config: Config) -> Self {
        let mut resources = Resources {
            config,
            assets: Assets::new(),
            input: InputLatch::default(),
            saves: FileStore::new(FileStore::default_dir()),
        };
        let mut scenes = Self::initial_stack();

        // Dev shortcut: SUPERGAME_SCENE=adventure boots straight into a map,
        // skipping the menu. SUPERGAME_MAP picks which one — a testbed map is
        // often the only way to get a specific thing on screen to look at.
        //
        // SUPERGAME_PAUSE=1 opens the pause menu on top of it. (Headless, the
        // same picture is `cargo run --bin render -- --map <m> --pause`.)
        if std::env::var("SUPERGAME_SCENE").as_deref() == Ok("adventure") {
            let map = std::env::var("SUPERGAME_MAP")
                .unwrap_or_else(|_| resources.config.game.start_map.clone());
            match crate::scenes::adventure::AdventureScene::new(&mut resources, &map) {
                Ok(scene) => {
                    let overlay = std::env::var("SUPERGAME_PAUSE")
                        .is_ok_and(|v| v != "0")
                        .then(|| Box::new(scene.pause()) as Box<dyn Scene>);
                    scenes.push(Box::new(scene));
                    scenes.extend(overlay);
                }
                Err(err) => eprintln!("failed to boot into adventure: {err:#}"),
            }
        }

        App {
            scenes,
            resources,
            renderer: GpuRenderer::new(ctx),
            frames: 0,
        }
    }

    fn initial_stack() -> Vec<Box<dyn Scene>> {
        vec![Box::new(MainMenuScene::default())]
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
            Transition::Replace(scene) => {
                self.scenes = Self::initial_stack();
                self.scenes.push(scene);
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
        let mut ticks = 0;
        while ctx.time.check_update_time(TICKS_PER_SECOND) {
            ticks += 1;
            if ticks > MAX_TICKS_PER_FRAME {
                // Drain what is left of the backlog without running it.
                continue;
            }
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
        let mut frame = Frame::default();
        for scene in &self.scenes[start..] {
            scene.draw(&mut frame, &self.resources);
        }
        self.renderer
            .draw(ctx, &mut self.resources.assets, &frame)?;

        // Dev hook: SUPERGAME_CAPTURE=out.png saves the thirtieth frame exactly
        // as the GPU drew it and quits — a screenshot with no window manager,
        // no colour profile and no scaling in the way.
        self.frames += 1;
        if self.frames == 30 {
            if let Ok(path) = std::env::var("SUPERGAME_CAPTURE") {
                let image = self.renderer.capture(ctx)?;
                image
                    .save(&path)
                    .map_err(|e| ggez::GameError::CustomError(format!("{path}: {e}")))?;
                ctx.request_quit();
            }
        }

        let mut canvas = Canvas::from_frame(ctx, Color::BLACK);
        self.renderer.present(ctx, &mut canvas);
        canvas.finish(ctx)
    }

    fn key_down_event(&mut self, ctx: &mut Context, input: KeyInput, repeated: bool) -> GameResult {
        if repeated {
            return Ok(());
        }
        // Escape is not special here: it is `cancel`, and backs out of
        // whatever is open (see `ACTIONS`). Quitting is the title screen's.
        if let Some(key) = input.keycode {
            // Latch here, not in the tick loop: this is the only place that
            // sees every press exactly once. (`repeated` is filtered above,
            // so holding the key does not re-arm it.)
            self.resources.input.key_down(key);
            let top = self.scenes.len() - 1;
            let transition = self.scenes[top].key_down(ctx, &mut self.resources, key);
            self.apply(transition);
        }
        Ok(())
    }

    /// Losing the window pauses the game, the way tabbing away from any game
    /// should: the world is not left running with nobody at the keys.
    fn focus_event(&mut self, ctx: &mut Context, gained: bool) -> GameResult {
        // A capture is taken from a window launched from a terminal, which
        // rarely has focus — pausing it would capture the pause menu.
        if !gained && std::env::var_os("SUPERGAME_CAPTURE").is_none() {
            let top = self.scenes.len() - 1;
            let transition =
                self.scenes[top].key_down(ctx, &mut self.resources, VirtualKeyCode::Pause);
            self.apply(transition);
        }
        Ok(())
    }
}
