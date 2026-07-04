//! The gameplay scene: owns the hecs `World` and runs the system pipeline
//! each fixed tick. This is the ECS port of the old endless-runner `Game`.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, PxScale, Text, TextFragment};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};
use hecs::World;
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::app::TICK;
use crate::ecs::components::{Player, Position, Size, Velocity};
use crate::ecs::events::Events;
use crate::scenes::{game_over::GameOverScene, pause::PauseScene, Resources, Scene, Transition};
use crate::systems::{
    camera::{self, Camera},
    collision, hazards, input, player, render, scroll, spawn, worldgen,
};

#[derive(Clone, Copy)]
struct ColorScheme {
    background: Color,
    player: Color,
    platform: Color,
}

const COLOR_CHANGE_TICKS: u32 = 60; // 1 second
const BASE_SCROLL_SPEED: f32 = 420.0;
const SCROLL_GROWTH_PER_SEC: f32 = 25.0;

pub struct LevelScene {
    world: World,
    events: Events,
    camera: Camera,
    rng: StdRng,
    gen: worldgen::WorldGen,
    schemes: Vec<ColorScheme>,
    scheme_index: usize,
    ticks_since_color_change: u32,
    /// Ticks since the run started; drives the timer, scroll speed, and color
    /// cycling. Zero until the main-menu overlay pops.
    run_ticks: u32,
    /// Set once when the player dies; freezes the displayed/recorded time.
    final_ticks: Option<u32>,
    view: Vec2,
    debug: bool,
}

impl LevelScene {
    pub fn new(res: &mut Resources) -> Self {
        let view = Vec2::new(res.config.display.width, res.config.display.height);
        let mut scene = LevelScene {
            world: World::new(),
            events: Events::default(),
            camera: Camera::new(view, view),
            rng: StdRng::from_entropy(),
            gen: worldgen::WorldGen::new(view.y),
            schemes: color_schemes(),
            scheme_index: 0,
            ticks_since_color_change: 0,
            run_ticks: 0,
            final_ticks: None,
            view,
            debug: res.config.debug.debug,
        };
        scene.reset(res);
        scene
    }

    fn reset(&mut self, res: &mut Resources) {
        self.world.clear();
        self.events.clear();
        self.camera.reset();
        self.run_ticks = 0;
        self.final_ticks = None;
        self.scheme_index = 0;
        self.ticks_since_color_change = 0;

        let spawn_pos =
            worldgen::initial_world(&self.gen, &mut self.rng, self.view.x, &mut self.events);
        spawn::drain(&mut self.world, &mut self.events);
        self.world.spawn((
            Player::new(spawn_pos),
            Position(spawn_pos),
            Velocity(Vec2::ZERO),
            Size(Vec2::splat(Player::SIZE)),
        ));

        res.clear_color = self.scheme().background;
    }

    fn scheme(&self) -> ColorScheme {
        self.schemes[self.scheme_index]
    }

    fn scroll_speed(&self) -> f32 {
        BASE_SCROLL_SPEED + SCROLL_GROWTH_PER_SEC * self.run_ticks as f32 * TICK
    }

    fn elapsed_secs(&self) -> f32 {
        self.final_ticks.unwrap_or(self.run_ticks) as f32 * TICK
    }

    fn game_over(&self) -> bool {
        self.final_ticks.is_some()
    }
}

impl Scene for LevelScene {
    fn update(&mut self, ctx: &mut Context, res: &mut Resources) -> GameResult<Transition> {
        let debug_start = std::time::Instant::now();

        // The clock keeps running after death: the world keeps scrolling and
        // accelerating behind the game-over overlay, like the original.
        self.run_ticks += 1;
        self.ticks_since_color_change += 1;
        if self.ticks_since_color_change >= COLOR_CHANGE_TICKS {
            self.ticks_since_color_change = 0;
            self.scheme_index = (self.scheme_index + 1) % self.schemes.len();
        }
        res.clear_color = self.scheme().background;

        let speed = self.scroll_speed();
        let player_input = input::read(ctx);

        player::update(&mut self.world, player_input, self.view.x, TICK);
        scroll::update(&mut self.world, speed, TICK);
        worldgen::cleanup(&mut self.world, &self.gen);
        worldgen::ensure_buffer(
            &mut self.world,
            &self.gen,
            &mut self.rng,
            self.view.x,
            &mut self.events,
        );
        spawn::drain(&mut self.world, &mut self.events);
        collision::resolve_player(&mut self.world, speed);
        hazards::check(
            &mut self.world,
            &mut self.events,
            self.camera.world_height(),
        );

        if self.events.player_died && !self.game_over() {
            self.events.player_died = false;
            self.final_ticks = Some(self.run_ticks);
            player::mark_dead(&mut self.world);

            let final_secs = self.elapsed_secs();
            res.record_score(final_secs);

            if self.debug {
                println!(
                    "Time to update: {:?} entities {}",
                    debug_start.elapsed(),
                    self.world.len()
                );
            }
            return Ok(Transition::Push(Box::new(GameOverScene::new(final_secs))));
        }

        camera::follow_player(&mut self.world, &mut self.camera);

        if self.debug {
            println!(
                "Time to update: {:?} entities {}",
                debug_start.elapsed(),
                self.world.len()
            );
        }
        Ok(Transition::None)
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, _res: &mut Resources) -> GameResult {
        let scheme = self.scheme();
        render::draw_world(
            ctx,
            canvas,
            &mut self.world,
            self.camera.offset(),
            render::Palette {
                player: scheme.player,
                platform: scheme.platform,
            },
        )?;

        let timer_text = Text::new(
            TextFragment::new(format!("Time: {:.2}s", self.elapsed_secs()))
                .scale(PxScale::from(32.0))
                .color(Color::from_rgb(230, 230, 230)),
        );
        canvas.draw(
            &timer_text,
            ggez::graphics::DrawParam::default().dest(Vec2::new(20.0, 20.0)),
        );
        Ok(())
    }

    fn key_down(
        &mut self,
        _ctx: &mut Context,
        _res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            VirtualKeyCode::P => Transition::Push(Box::new(PauseScene)),
            _ => Transition::None,
        }
    }
}

fn color_schemes() -> Vec<ColorScheme> {
    vec![
        ColorScheme {
            background: Color::from_rgb(10, 12, 32),
            player: Color::from_rgb(255, 214, 10),
            platform: Color::from_rgb(240, 240, 240),
        },
        ColorScheme {
            background: Color::from_rgb(12, 36, 48),
            player: Color::from_rgb(252, 117, 106),
            platform: Color::from_rgb(242, 246, 250),
        },
        ColorScheme {
            background: Color::from_rgb(25, 8, 40),
            player: Color::from_rgb(51, 255, 221),
            platform: Color::from_rgb(235, 228, 255),
        },
        ColorScheme {
            background: Color::from_rgb(8, 38, 36),
            player: Color::from_rgb(255, 189, 89),
            platform: Color::from_rgb(225, 245, 239),
        },
        ColorScheme {
            background: Color::from_rgb(32, 10, 22),
            player: Color::from_rgb(135, 206, 250),
            platform: Color::from_rgb(245, 235, 225),
        },
    ]
}
