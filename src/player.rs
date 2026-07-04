use crate::entity::Entity;
use ggez::graphics::{Canvas, Color};
use ggez::mint::{Point2, Vector2};
use ggez::winit::event::VirtualKeyCode;
use ggez::{self, graphics, Context, GameResult};
use std::time::Duration;

pub(super) struct Player {
    position: Point2<f32>,
    previous_position: Point2<f32>,
    velocity: Vector2<f32>,
    color: Color,
    max_speed: f32,
    ground_acceleration: f32,
    air_acceleration: f32,
    jump_speed: f32,
    gravity: f32,
    friction: f32,
    air_drag: f32,
    grounded_time: Duration,
    jump_delay: Duration,
    can_jump: bool,
    is_grounded: bool,
    was_grounded_last_frame: bool,
    world_width: f32,
    horizontal_input: f32,
    is_dead: bool,
    platform_velocity: f32,
}

impl Entity for Player {
    fn update(&mut self, ctx: &mut Context) {
        if self.is_dead {
            self.previous_position = self.position;
            self.velocity.y += self.gravity;
            self.position.y += self.velocity.y;
            return;
        }
        self.was_grounded_last_frame = self.is_grounded;
        self.handle_player_input(ctx);
        self.apply_friction();
        self.update_position(ctx);
    }

    fn draw(
        &mut self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        camera_offset: Vector2<f32>,
    ) -> GameResult {
        let rect = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            graphics::Rect::new(
                self.position.x - camera_offset.x,
                self.position.y - camera_offset.y,
                Self::SIZE,
                Self::SIZE,
            ),
            self.color,
        )?;

        canvas.draw(&rect, graphics::DrawParam::default());
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Player {
    pub(crate) const SIZE: f32 = 100.0;

    pub(crate) fn new(world_width: f32, spawn_position: Point2<f32>, color: Color) -> Self {
        let position = spawn_position;
        let previous_position = position;
        let velocity = Vector2 { x: 0.0, y: 0.0 };

        Player {
            position,
            previous_position,
            velocity,
            color,
            max_speed: 12.0,
            ground_acceleration: 12.0,
            air_acceleration: 8.0,
            jump_speed: 32.0,
            gravity: 1.3,
            friction: 3.0,
            air_drag: 0.85,
            grounded_time: Duration::new(0, 0),
            jump_delay: Duration::new(0, 100_000_000),
            can_jump: true,
            is_grounded: true,
            was_grounded_last_frame: false,
            world_width,
            horizontal_input: 0.0,
            is_dead: false,
            platform_velocity: 0.0,
        }
    }
    fn handle_player_input(&mut self, ctx: &mut Context) {
        self.horizontal_input = 0.0;

        // Move left and right
        if ctx.keyboard.is_key_pressed(VirtualKeyCode::Left)
            && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Right)
        {
            self.move_left();
            self.horizontal_input = -1.0;
        } else if ctx.keyboard.is_key_pressed(VirtualKeyCode::Right)
            && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Left)
        {
            self.move_right();
            self.horizontal_input = 1.0;
        }

        // Jump!
        if ctx.keyboard.is_key_pressed(VirtualKeyCode::Up)
            && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Down)
        {
            self.jump();
        }
    }

    pub fn update_position(&mut self, ctx: &mut Context) {
        self.previous_position = self.position;
        self.is_grounded = false;

        // Update position based on velocity
        self.position.x += self.velocity.x;
        self.position.y += self.velocity.y;

        let delta = ctx.time.delta().as_secs_f32();
        self.position.x += self.platform_velocity * delta;

        // Apply gravity for the next frame
        self.velocity.y += self.gravity;

        if !self.is_grounded {
            self.platform_velocity = 0.0;
        }

        // Keep player inside horizontal bounds
        if self.position.x < 0.0 {
            self.position.x = 0.0;
            self.velocity.x = 0.0;
        } else if self.position.x > self.world_width - Self::SIZE {
            self.position.x = self.world_width - Self::SIZE;
            self.velocity.x = 0.0;
        }
    }

    pub fn apply_friction(&mut self) {
        if self.is_grounded && self.horizontal_input == 0.0 {
            if self.velocity.x > 0.0 {
                self.velocity.x -= self.friction;
                if self.velocity.x < self.friction {
                    self.velocity.x = 0.0;
                }
            }
            if self.velocity.x < 0.0 {
                self.velocity.x += self.friction;
                if self.velocity.x > -self.friction {
                    self.velocity.x = 0.0;
                }
            }
        }
    }

    pub fn jump(&mut self) {
        if self.can_jump && !self.is_dead {
            self.can_jump = false;
            self.is_grounded = false;
            self.velocity.y = -self.jump_speed;
            self.grounded_time = Duration::new(0, 0);
        }
    }

    pub fn move_left(&mut self) {
        let accel = if self.is_grounded {
            self.ground_acceleration
        } else {
            self.air_acceleration
        };
        self.velocity.x -= accel;
        if self.velocity.x < -self.max_speed {
            self.velocity.x = -self.max_speed;
        }
    }

    pub fn move_right(&mut self) {
        let accel = if self.is_grounded {
            self.ground_acceleration
        } else {
            self.air_acceleration
        };
        self.velocity.x += accel;
        if self.velocity.x > self.max_speed {
            self.velocity.x = self.max_speed;
        }
    }

    pub(crate) fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    pub(crate) fn resolve_collisions(&mut self, platforms: &[(graphics::Rect, f32)]) {
        if self.is_dead {
            return;
        }
        let mut landed_on_platform = false;
        let previous_rect = graphics::Rect::new(
            self.previous_position.x,
            self.previous_position.y,
            Self::SIZE,
            Self::SIZE,
        );

        for &(platform_rect, platform_speed) in platforms {
            let current_rect =
                graphics::Rect::new(self.position.x, self.position.y, Self::SIZE, Self::SIZE);

            if !current_rect.overlaps(&platform_rect) {
                continue;
            }

            let from_top = previous_rect.y + previous_rect.h <= platform_rect.y;
            let from_bottom = previous_rect.y >= platform_rect.y + platform_rect.h;
            let from_left = previous_rect.x + previous_rect.w <= platform_rect.x;
            let from_right = previous_rect.x >= platform_rect.x + platform_rect.w;

            if from_top {
                self.position.y = platform_rect.y - Self::SIZE;
                self.velocity.y = 0.0;
                self.is_grounded = true;
                landed_on_platform = true;
                self.platform_velocity = -platform_speed;
                if !self.was_grounded_last_frame {
                    self.grounded_time = Duration::new(0, 0);
                    self.can_jump = false;
                }
                continue;
            }

            if from_bottom {
                self.position.y = platform_rect.y + platform_rect.h;
                if self.velocity.y < 0.0 {
                    self.velocity.y = 0.0;
                }
                continue;
            }

            if from_left {
                self.position.x = platform_rect.x - Self::SIZE;
                if self.velocity.x > 0.0 {
                    self.velocity.x = 0.0;
                }
                continue;
            }

            if from_right {
                self.position.x = platform_rect.x + platform_rect.w;
                if self.velocity.x < 0.0 {
                    self.velocity.x = 0.0;
                }
                continue;
            }
        }

        if !landed_on_platform {
            self.is_grounded = false;
        }
    }

    pub(crate) fn finalize_update(&mut self, delta_time: Duration) {
        if self.is_dead {
            return;
        }

        if self.is_grounded {
            self.grounded_time += delta_time;
            if self.grounded_time >= self.jump_delay {
                self.can_jump = true;
            }
        } else if self.horizontal_input == 0.0 {
            self.velocity.x *= self.air_drag;
            if self.velocity.x.abs() < 0.5 {
                self.velocity.x = 0.0;
            }
        }
    }
    pub(crate) fn rect(&self) -> graphics::Rect {
        graphics::Rect::new(self.position.x, self.position.y, Self::SIZE, Self::SIZE)
    }

    pub(crate) fn center(&self) -> Point2<f32> {
        Point2 {
            x: self.position.x + Self::SIZE / 2.0,
            y: self.position.y + Self::SIZE / 2.0,
        }
    }

    pub(crate) fn bottom(&self) -> f32 {
        self.position.y + Self::SIZE
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.is_dead
    }

    pub(crate) fn mark_dead(&mut self) {
        if self.is_dead {
            return;
        }
        self.is_dead = true;
        self.can_jump = false;
        self.is_grounded = false;
        self.velocity.x = 0.0;
        self.horizontal_input = 0.0;
        self.platform_velocity = 0.0;
    }
}
