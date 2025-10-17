use crate::config::Config;
use crate::entity::Entity;
use crate::platform::Platform;
use crate::player::Player;
use ggez::event::EventHandler;
use ggez::graphics::{self, Color, PxScale, Text, TextFragment};
use ggez::mint::{Point2, Vector2};
use ggez::{Context, GameResult};
use std::time::{Duration, Instant};

pub struct Game {
    config: Config,
    entities: Vec<Box<dyn Entity>>,
    player_index: usize,
    platform_indices: Vec<usize>,
    camera: Camera,
}

impl Game {
    pub fn new() -> Game {
        let config = Config::new("config.toml");
        let world_width = config.display.width * 3.0;
        let world_height = config.display.height;
        let camera = Camera::new(
            config.display.width,
            config.display.height,
            world_width,
            world_height,
        );

        let platform_specs = vec![
            (200.0, world_height - 150.0, 300.0, 30.0),
            (650.0, world_height - 350.0, 280.0, 30.0),
            (1100.0, world_height - 500.0, 260.0, 30.0),
            (1550.0, world_height - 320.0, 320.0, 30.0),
            (2000.0, world_height - 200.0, 240.0, 30.0),
            (2450.0, world_height - 420.0, 220.0, 30.0),
            (2900.0, world_height - 280.0, 360.0, 30.0),
            (3350.0, world_height - 460.0, 260.0, 30.0),
            (3800.0, world_height - 240.0, 300.0, 30.0),
            (4250.0, world_height - 380.0, 220.0, 30.0),
            (4700.0, world_height - 520.0, 280.0, 30.0),
            (5150.0, world_height - 300.0, 320.0, 30.0),
            (5600.0, world_height - 180.0, 160.0, 30.0),
        ];

        let spawn_position = platform_specs
            .first()
            .map(|(x, y, w, _)| Point2 {
                x: x + (w - Player::SIZE) / 2.0,
                y: y - Player::SIZE,
            })
            .unwrap_or(Point2 {
                x: 0.0,
                y: world_height - Player::SIZE,
            });

        // Create a vector of entities that need to be drawn and updated
        let mut entities: Vec<Box<dyn Entity>> = Vec::new();

        // Add the player and any others to the vec
        let player_index = entities.len();
        entities.push(Box::new(Player::new(world_width, spawn_position)));

        // Add static platforms the player can interact with
        let mut platform_indices = Vec::new();

        for &(x, y, w, h) in &platform_specs {
            let idx = entities.len();
            entities.push(Box::new(Platform::new(x, y, w, h)));
            platform_indices.push(idx);
        }

        // Return the created struct
        Game {
            config,
            entities,
            player_index,
            platform_indices,
            camera,
        }
    }
}

impl EventHandler for Game {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        // Time the loop
        let start_time = Instant::now();
        let delta_time = ctx.time.delta();

        for entity in &mut self.entities {
            entity.update(ctx); // Update each entity
        }

        self.handle_collisions(delta_time);

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

        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);
        let camera_offset = self.camera.offset();

        for entity in &mut self.entities {
            entity.draw(ctx, &mut canvas, camera_offset)?; // Draw each entity
        }

        if let Some(player) = self.entities[self.player_index]
            .as_any()
            .downcast_ref::<Player>()
        {
            if player.is_dead() {
                let text = Text::new(
                    TextFragment::new("You Died.")
                        .scale(PxScale::from(96.0))
                        .color(Color::from_rgb(200, 20, 20)),
                );
                let text_size = text.measure(ctx)?;
                let position = Point2 {
                    x: (self.config.display.width - text_size.x) / 2.0,
                    y: (self.config.display.height - text_size.y) / 2.0,
                };

                canvas.draw(&text, graphics::DrawParam::default().dest(position));
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
        let platform_rects: Vec<graphics::Rect> = self
            .platform_indices
            .iter()
            .map(|&idx| {
                self.entities[idx]
                    .as_any()
                    .downcast_ref::<Platform>()
                    .expect("Entity at index is not a Platform")
                    .rect()
            })
            .collect();

        if let Some(player) = self.entities[self.player_index]
            .as_any_mut()
            .downcast_mut::<Player>()
        {
            if !player.is_dead() {
                player.resolve_collisions(&platform_rects);
                player.finalize_update(delta_time);

                if player.bottom() > self.camera.world_height() {
                    player.mark_dead();
                } else {
                    self.camera.follow_player(player);
                }
            }
        }
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
}
