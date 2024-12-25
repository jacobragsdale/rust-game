use std::time::Duration;
use ggez::{self, Context, GameResult, graphics};
use ggez::graphics::{Canvas, Color};
use ggez::mint::{Point2, Vector2};

pub(super) struct Player {
    position: Point2<f32>,
    size: f32,
    entity: graphics::Rect,
    velocity: Vector2<f32>,
    color: Color,
    max_speed: f32,
    acceleration: f32,
    jump_speed: f32,
    gravity: f32,
    friction: f32,
    grounded_time: Duration,
    jump_delay: Duration,
    can_jump: bool,
    is_grounded: bool
}

impl Player {
    pub fn new(position: Point2<f32>, size: f32, velocity: Vector2<f32>) -> Self{
        Player {
            position,
            velocity,
            size,
            entity: graphics::Rect::new(position.x, position.y, size, size),
            color: Color::BLUE,
            max_speed: 20.0,
            acceleration: 2.0,
            jump_speed: 40.0,
            gravity: 2.5,
            friction: 1.0,
            grounded_time: Duration::new(0, 0),
            jump_delay: Duration::new(0, 100_000_000),
            can_jump: false,
            is_grounded: false
        }
    }

    pub fn update_position(&mut self, ctx: &mut Context, _other: &Player) {
        let delta_time = ctx.time.delta().as_secs_f32();
        
        // Update position based on velocity
        self.position.x += self.velocity.x * delta_time;
        self.position.y += self.velocity.y * delta_time;

        // Update the entity rectangle position
        self.entity.x = self.position.x;
        self.entity.y = self.position.y;

        // Basic screen bounds checking
        if self.position.y >= 1080.0 - self.size {
            self.position.y = 1080.0 - self.size;
            self.velocity.y = 0.0;
            self.is_grounded = true;
        }
        
        if self.position.x < 0.0 {
            self.position.x = 0.0;
            self.velocity.x = 0.0;
        } else if self.position.x > 1920.0 - self.size {
            self.position.x = 1920.0 - self.size;
            self.velocity.x = 0.0;
        }
    }

    pub fn apply_friction(&mut self){
        if self.is_grounded{
            if self.velocity.x > 0.0 {
                self.velocity.x -= self.friction;
            }
            if self.velocity.x < 0.0 {
                self.velocity.x += self.friction;
            }
        }
    }

    pub fn apply_gravity(&mut self){
        self.velocity.y += self.gravity;
    }

    pub fn jump(&mut self){
        if self.can_jump {
            self.can_jump = false;
            self.is_grounded = false;
            self.velocity.y = -self.jump_speed;
            self.grounded_time = Duration::new(0, 0);
        }
    }

    pub fn move_left(&mut self){
        self.velocity.x = self.velocity.x - self.acceleration;
        if self.velocity.x < -self.max_speed {
            self.velocity.x = -self.max_speed;
        }
    }

    pub fn move_right(&mut self){
        self.velocity.x = self.velocity.x + self.acceleration;
        if self.velocity.x > self.max_speed {
            self.velocity.x = self.max_speed;
        }
    }

    pub fn update_color(&mut self){
        if self.color == Color::BLUE {
            self.color = Color::RED
        }
        else if self.color == Color::RED {
            self.color = Color::YELLOW
        }
        else if self.color == Color::YELLOW {
            self.color = Color::GREEN
        }
        else if self.color == Color::GREEN {
            self.color = Color::MAGENTA
        }
        else if self.color == Color::MAGENTA {
            self.color = Color::CYAN
        }
        else if self.color == Color::CYAN {
            self.color = Color::BLUE
        }
        else {
            self.color = Color::BLUE
        }

        println!("{:?}", self.color);
    }

    pub fn draw(&self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let rect = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            graphics::Rect::new(
                self.position.x,
                self.position.y,
                self.size,
                self.size
            ),
            Color::WHITE,
        )?;
        
        canvas.draw(&rect, graphics::DrawParam::default());
        Ok(())
    }
}