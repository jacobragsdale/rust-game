use crate::config::Config;
use crate::entity::Entity;
use crate::platform::Platform;
use crate::player::Player;
use crate::score::ScoreStore;
use crate::spike_ball::SpikedBall;
use ggez::event::EventHandler;
use ggez::graphics::{self, Color, PxScale, Text, TextFragment};
use ggez::mint::{Point2, Vector2};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};
use rand::{rngs::ThreadRng, Rng};
use std::cmp::Ordering;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct ColorScheme {
    background: Color,
    player: Color,
    platform: Color,
}

pub struct Game {
    config: Config,
    entities: Vec<Box<dyn Entity>>,
    player_index: usize,
    camera: Camera,
    color_schemes: Vec<ColorScheme>,
    current_scheme_index: usize,
    background_color: Color,
    player_color: Color,
    platform_color: Color,
    color_change_interval: Duration,
    last_color_change: Instant,
    score_store: Option<ScoreStore>,
    top_scores: Vec<f32>,
    current_run_score: Option<f32>,
    game_started: bool,
    base_platform_speed: f32,
    speed_growth_per_sec: f32,
    spawn_margin: f32,
    platform_height: f32,
    platform_width_range: (f32, f32),
    platform_gap_range: (f32, f32),
    platform_y_levels: Vec<f32>,
    rng: ThreadRng,
    run_start: Instant,
    last_run_duration: Option<Duration>,
}

impl Game {
    pub fn new() -> Game {
        let config = Config::new("config.toml");
        let world_width = config.display.width;
        let world_height = config.display.height;
        let camera = Camera::new(
            config.display.width,
            config.display.height,
            world_width,
            world_height,
        );
        let color_schemes = Self::build_color_schemes();
        let initial_scheme = color_schemes.first().copied().unwrap_or(ColorScheme {
            background: Color::BLACK,
            player: Color::WHITE,
            platform: Color::from_rgb(200, 200, 200),
        });
        let score_store = ScoreStore::new("scores.db")
            .map(Some)
            .unwrap_or_else(|err| {
                eprintln!("Failed to open score database: {}", err);
                None
            });
        let top_scores = score_store
            .as_ref()
            .and_then(|store| store.top_scores(3).ok())
            .unwrap_or_default();
        let mut game = Game {
            config,
            entities: Vec::new(),
            player_index: 0,
            camera,
            color_schemes,
            current_scheme_index: 0,
            background_color: initial_scheme.background,
            player_color: initial_scheme.player,
            platform_color: initial_scheme.platform,
            color_change_interval: Duration::from_secs(1),
            last_color_change: Instant::now(),
            score_store,
            top_scores,
            current_run_score: None,
            game_started: false,
            base_platform_speed: 420.0,
            speed_growth_per_sec: 25.0,
            spawn_margin: 400.0,
            platform_height: 30.0,
            platform_width_range: (180.0, 360.0),
            platform_gap_range: (220.0, 360.0),
            platform_y_levels: vec![
                world_height - 150.0,
                world_height - 300.0,
                world_height - 450.0,
            ],
            rng: rand::thread_rng(),
            run_start: Instant::now(),
            last_run_duration: None,
        };

        game.reset_world();
        game
    }
}

impl EventHandler for Game {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        // Time the loop
        let start_time = Instant::now();
        let delta_time = ctx.time.delta();

        if !self.game_started && ctx.keyboard.is_key_just_pressed(VirtualKeyCode::Up) {
            self.game_started = true;
            self.run_start = Instant::now();
            self.last_color_change = Instant::now();
        }

        let current_speed = if self.game_started { self.current_platform_speed() } else { 0.0 };
        self.update_platform_speeds(current_speed);
        if self.game_started {
            self.update_color_cycle();
        }

        for entity in &mut self.entities {
            entity.update(ctx); // Update each entity
        }

        self.cleanup_platforms();
        self.ensure_platform_buffer();
        self.handle_collisions(delta_time);

        if self.player_is_dead() && self.restart_key_just_pressed(ctx) {
            self.reset_world();
        }

        if self.config.debug.debug {
            println!(
                "Time take to update:{:?} entities {:?}",
                self.entities.len(),
                start_time.elapsed()
            );
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Time the loop
        let start_time = Instant::now();

        let mut canvas = graphics::Canvas::from_frame(ctx, self.background_color);
        let camera_offset = self.camera.offset();

        for entity in &mut self.entities {
            entity.draw(ctx, &mut canvas, camera_offset)?; // Draw each entity
        }

        let elapsed = if !self.game_started {
            Duration::ZERO
        } else {
            self.last_run_duration
                .unwrap_or_else(|| self.run_start.elapsed())
        };
        let timer_text = Text::new(
            TextFragment::new(format!("Time: {:.2}s", elapsed.as_secs_f32()))
                .scale(PxScale::from(32.0))
                .color(Color::from_rgb(230, 230, 230)),
        );
        canvas.draw(
            &timer_text,
            graphics::DrawParam::default().dest(Point2 { x: 20.0, y: 20.0 }),
        );

        if let Some(player) = self.entities[self.player_index]
            .as_any()
            .downcast_ref::<Player>()
        {
            if player.is_dead() {
                let death_text = Text::new(
                    TextFragment::new("You Died.")
                        .scale(PxScale::from(96.0))
                        .color(Color::from_rgb(200, 20, 20)),
                );
                let death_size = death_text.measure(ctx)?;
                let death_position = Point2 {
                    x: (self.config.display.width - death_size.x) / 2.0,
                    y: (self.config.display.height - death_size.y) / 2.0,
                };

                canvas.draw(
                    &death_text,
                    graphics::DrawParam::default().dest(death_position),
                );

                let score_text = Text::new(
                    TextFragment::new(format!("Final Time: {:.2}s", elapsed.as_secs_f32()))
                        .scale(PxScale::from(48.0))
                        .color(Color::from_rgb(220, 220, 220)),
                );
                let score_size = score_text.measure(ctx)?;
                let score_position = Point2 {
                    x: (self.config.display.width - score_size.x) / 2.0,
                    y: death_position.y + death_size.y + 20.0,
                };

                canvas.draw(
                    &score_text,
                    graphics::DrawParam::default().dest(score_position),
                );

                let mut next_y = score_position.y + score_size.y + 40.0;

                if let Some(current) = self.current_run_score {
                    let current_text = Text::new(
                        TextFragment::new(format!("Current Run: {:.2}s", current))
                            .scale(PxScale::from(36.0))
                            .color(Color::from_rgb(235, 235, 235)),
                    );
                    let current_size = current_text.measure(ctx)?;
                    let current_position = Point2 {
                        x: (self.config.display.width - current_size.x) / 2.0,
                        y: next_y,
                    };

                    canvas.draw(
                        &current_text,
                        graphics::DrawParam::default().dest(current_position),
                    );
                    next_y += current_size.y + 20.0;
                }

                if !self.top_scores.is_empty() {
                    let header_text = Text::new(
                        TextFragment::new("Top Runs")
                            .scale(PxScale::from(36.0))
                            .color(Color::from_rgb(240, 240, 240)),
                    );
                    let header_size = header_text.measure(ctx)?;
                    let header_position = Point2 {
                        x: (self.config.display.width - header_size.x) / 2.0,
                        y: next_y,
                    };
                    canvas.draw(
                        &header_text,
                        graphics::DrawParam::default().dest(header_position),
                    );
                    next_y += header_size.y + 12.0;

                    for (index, score) in self.top_scores.iter().take(3).enumerate() {
                        let entry_text = Text::new(
                            TextFragment::new(format!("{}. {:.2}s", index + 1, score))
                                .scale(PxScale::from(28.0))
                                .color(Color::from_rgb(220, 220, 220)),
                        );
                        let entry_size = entry_text.measure(ctx)?;
                        let entry_position = Point2 {
                            x: (self.config.display.width - entry_size.x) / 2.0,
                            y: next_y,
                        };
                        canvas.draw(
                            &entry_text,
                            graphics::DrawParam::default().dest(entry_position),
                        );
                        next_y += entry_size.y + 8.0;
                    }
                }

                let prompt_text = Text::new(
                    TextFragment::new("Press Space to Play Again")
                        .scale(PxScale::from(32.0))
                        .color(Color::from_rgb(235, 235, 235)),
                );
                let prompt_size = prompt_text.measure(ctx)?;
                let prompt_position = Point2 {
                    x: (self.config.display.width - prompt_size.x) / 2.0,
                    y: next_y + 20.0,
                };
                canvas.draw(
                    &prompt_text,
                    graphics::DrawParam::default().dest(prompt_position),
                );
            }
        }

        if self.config.debug.debug {
            println!(
                "Time take to draw:{:?} entities {:?}",
                self.entities.len(),
                start_time.elapsed()
            );
        }

        canvas.finish(ctx)
    }
}

impl Game {
    fn handle_collisions(&mut self, delta_time: Duration) {
        if self.player_is_dead() {
            return;
        }

        let platform_hits: Vec<(graphics::Rect, f32)> = self
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .as_any()
                    .downcast_ref::<Platform>()
                    .map(|p| (p.rect(), p.speed()))
            })
            .collect();

        let spike_centers: Vec<(f32, f32)> = self
            .entities
            .iter()
            .filter_map(|e| e.as_any().downcast_ref::<SpikedBall>())
            .map(|b| (b.x(), b.y()))
            .collect();

        if let Some(player) = self.entities[self.player_index]
            .as_any_mut()
            .downcast_mut::<Player>()
        {
            player.resolve_collisions(&platform_hits);
            player.finalize_update(delta_time);

            let player_rect = player.rect();
            let spike_hit = spike_centers.iter().any(|&(bx, by)| {
                let cx = bx.clamp(player_rect.x, player_rect.x + player_rect.w);
                let cy = by.clamp(player_rect.y, player_rect.y + player_rect.h);
                let dx = bx - cx;
                let dy = by - cy;
                dx * dx + dy * dy <= SpikedBall::TOTAL_RADIUS * SpikedBall::TOTAL_RADIUS
            });
            let fell = player.bottom() > self.camera.world_height();

            if spike_hit || fell {
                player.mark_dead();
                if self.last_run_duration.is_none() {
                    let run_duration = self.run_start.elapsed();
                    self.last_run_duration = Some(run_duration);
                    self.record_score(run_duration.as_secs_f32());
                }
            } else {
                self.camera.follow_player(player);
            }
        }
    }

    fn reset_world(&mut self) {
        self.entities.clear();
        self.player_index = 0;
        self.camera.reset();
        self.rng = rand::thread_rng();
        self.run_start = Instant::now();
        self.last_run_duration = None;
        self.game_started = false;
        self.last_color_change = Instant::now();
        self.current_run_score = None;

        let start_platform_width = 320.0;
        let start_platform_x = 260.0;
        let start_platform_y = self
            .platform_y_levels
            .first()
            .copied()
            .unwrap_or(self.camera.world_height() - 150.0);
        let spawn_position = Point2 {
            x: start_platform_x + (start_platform_width - Player::SIZE) / 2.0,
            y: start_platform_y - Player::SIZE,
        };

        self.entities.push(Box::new(Player::new(
            self.camera.world_width(),
            spawn_position,
            self.player_color,
        )));

        self.entities.push(Box::new(Platform::new(
            start_platform_x,
            start_platform_y,
            start_platform_width,
            self.platform_height,
            self.current_platform_speed(),
            self.platform_color,
        )));

        let mut right_edge = start_platform_x + start_platform_width;
        let target_edge = self.config.display.width + self.spawn_margin;
        while right_edge < target_edge {
            let gap = self.random_gap();
            let width = self.random_platform_width();
            let y = self.random_platform_y();
            let x = right_edge + gap;
            self.entities.push(Box::new(Platform::new(
                x,
                y,
                width,
                self.platform_height,
                self.current_platform_speed(),
                self.platform_color,
            )));
            if self.rng.gen_bool(0.1) {
                let ball_x = self.rng.gen_range(x..=(x + width));
                let ball_y = self.rng.gen_range((y - 200.0)..=(y - 50.0));
                self.entities.push(Box::new(SpikedBall::new(
                    ball_x,
                    ball_y,
                    self.current_platform_speed(),
                )));
            }
            right_edge = x + width;
        }

        self.apply_colors_to_entities();
    }

    fn player_is_dead(&self) -> bool {
        self.entities[self.player_index]
            .as_any()
            .downcast_ref::<Player>()
            .map(|player| player.is_dead())
            .unwrap_or(false)
    }

    fn restart_key_just_pressed(&self, ctx: &Context) -> bool {
        ctx.keyboard.is_key_just_pressed(VirtualKeyCode::Space)
    }

    fn cleanup_platforms(&mut self) {
        let margin = self.spawn_margin;
        self.entities.retain(|entity| {
            if entity.as_any().downcast_ref::<Player>().is_some() {
                return true;
            }
            if let Some(platform) = entity.as_any().downcast_ref::<Platform>() {
                return !platform.is_offscreen(margin);
            }
            if let Some(ball) = entity.as_any().downcast_ref::<SpikedBall>() {
                return !ball.is_offscreen(margin);
            }
            true
        });
    }

    fn ensure_platform_buffer(&mut self) {
        let target_edge = self.config.display.width + self.spawn_margin;
        let mut right_edge = self
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .as_any()
                    .downcast_ref::<Platform>()
                    .map(|platform| platform.right_edge())
            })
            .fold(0.0, f32::max);

        if right_edge == 0.0 {
            right_edge = self.config.display.width * 0.25;
        }

        while right_edge < target_edge {
            let gap = self.random_gap();
            let width = self.random_platform_width();
            let y = self.random_platform_y();
            let x = right_edge + gap;
            self.entities.push(Box::new(Platform::new(
                x,
                y,
                width,
                self.platform_height,
                self.current_platform_speed(),
                self.platform_color,
            )));
            if self.rng.gen_bool(0.1) {
                let ball_x = self.rng.gen_range(x..=(x + width));
                let ball_y = self.rng.gen_range((y - 200.0)..=(y - 50.0));
                self.entities.push(Box::new(SpikedBall::new(
                    ball_x,
                    ball_y,
                    self.current_platform_speed(),
                )));
            }
            right_edge = x + width;
        }
    }

    fn update_platform_speeds(&mut self, speed: f32) {
        for entity in &mut self.entities {
            if let Some(platform) = entity.as_any_mut().downcast_mut::<Platform>() {
                platform.set_speed(speed);
            } else if let Some(ball) = entity.as_any_mut().downcast_mut::<SpikedBall>() {
                ball.set_speed(speed);
            }
        }
    }

    fn current_platform_speed(&self) -> f32 {
        let elapsed = self.run_start.elapsed().as_secs_f32();
        self.base_platform_speed + self.speed_growth_per_sec * elapsed
    }

    fn random_gap(&mut self) -> f32 {
        self.rng
            .gen_range(self.platform_gap_range.0..=self.platform_gap_range.1)
    }

    fn random_platform_width(&mut self) -> f32 {
        self.rng
            .gen_range(self.platform_width_range.0..=self.platform_width_range.1)
    }

    fn random_platform_y(&mut self) -> f32 {
        let index = self.rng.gen_range(0..self.platform_y_levels.len());
        self.platform_y_levels[index]
    }

    fn update_color_cycle(&mut self) {
        if self.last_color_change.elapsed() >= self.color_change_interval {
            self.advance_color_scheme();
            self.last_color_change = Instant::now();
        }
    }

    fn advance_color_scheme(&mut self) {
        if self.color_schemes.is_empty() {
            return;
        }

        self.current_scheme_index = (self.current_scheme_index + 1) % self.color_schemes.len();
        let scheme = self.color_schemes[self.current_scheme_index];
        self.background_color = scheme.background;
        self.player_color = scheme.player;
        self.platform_color = scheme.platform;

        self.apply_colors_to_entities();
    }

    fn apply_colors_to_entities(&mut self) {
        if let Some(player) = self.entities.get_mut(self.player_index) {
            if let Some(player) = player.as_any_mut().downcast_mut::<Player>() {
                player.set_color(self.player_color);
            }
        }

        for entity in &mut self.entities {
            if let Some(platform) = entity.as_any_mut().downcast_mut::<Platform>() {
                platform.set_color(self.platform_color);
            }
        }
    }

    fn record_score(&mut self, score: f32) {
        self.current_run_score = Some(score);

        if let Some(store) = self.score_store.as_ref() {
            if let Err(err) = store.add_score(score) {
                eprintln!("Failed to save score: {}", err);
            }
            if let Ok(scores) = store.top_scores(3) {
                self.top_scores = scores;
                return;
            }
        }

        self.top_scores.push(score);
        self.top_scores
            .sort_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));
        self.top_scores.truncate(3);
    }

    fn build_color_schemes() -> Vec<ColorScheme> {
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
}

struct Camera {
    offset: Vector2<f32>,
    viewport_width: f32,
    viewport_height: f32,
    world_width: f32,
    world_height: f32,
}

impl Camera {
    fn new(viewport_width: f32, viewport_height: f32, world_width: f32, world_height: f32) -> Self {
        Camera {
            offset: Vector2 { x: 0.0, y: 0.0 },
            viewport_width,
            viewport_height,
            world_width,
            world_height,
        }
    }

    fn follow_player(&mut self, player: &Player) {
        let center = player.center();
        let half_width = self.viewport_width / 2.0;
        let half_height = self.viewport_height / 2.0;

        let max_x_offset = (self.world_width - self.viewport_width).max(0.0);
        let max_y_offset = (self.world_height - self.viewport_height).max(0.0);

        let desired_x = center.x - half_width;
        let desired_y = center.y - half_height;

        self.offset.x = desired_x.clamp(0.0, max_x_offset);
        self.offset.y = desired_y.clamp(0.0, max_y_offset);
    }

    fn offset(&self) -> Vector2<f32> {
        self.offset
    }

    fn world_height(&self) -> f32 {
        self.world_height
    }

    fn world_width(&self) -> f32 {
        self.world_width
    }

    fn reset(&mut self) {
        self.offset = Vector2 { x: 0.0, y: 0.0 };
    }
}
