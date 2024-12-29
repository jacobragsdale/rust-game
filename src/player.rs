use std::time::Duration;
use ggez::{self, Context, GameResult, graphics};
use ggez::graphics::{Canvas, Color};
use ggez::mint::{Point2, Vector2};
use ggez::winit::event::VirtualKeyCode;
use crate::entity::Entity;

pub(super) struct Player {
    entity: graphics::Rect,
    size: f32,
    position: Point2<f32>,
    velocity: Vector2<f32>,
    color: Color,
    max_speed: f32,
    acceleration: f32,
    jump_speed: f32,
    gravity: f32,
    friction: f32,
    grounded_time: Duration,
    jump_delay: Duration,
    color_delay: Duration,
    color_time: Duration,
    can_jump: bool,
    is_grounded: bool,
    colors: Vec<Color>,
}

impl Entity for Player {
    fn update(&mut self, ctx: &mut Context) {
        self.handle_player_input(ctx);
        self.apply_friction();
        self.update_position(ctx);
    }

    
    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas) -> GameResult {
        let rect = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            graphics::Rect::new(
                self.position.x,
                self.position.y,
                self.size,
                self.size
            ),
            self.color,
        )?;
        
        canvas.draw(&rect, graphics::DrawParam::default());
        Ok(())
    }
}

impl Player {
    pub(crate) fn new() -> Self{
        let colors = vec![
            Color::BLUE,
            Color::RED,
            Color::YELLOW,
            Color::GREEN,
            Color::MAGENTA,
            Color::CYAN,
            Color::WHITE,
        ];
        let position = Point2{ x: 0.0, y: 0.0, };
        let velocity = Vector2{ x: 0.0, y: 0.0 };
        let entity = graphics::Rect::new(0.0, 0.0, 10.0, 10.0);

        Player {
            entity,
            position,
            velocity,
            size: 100.0,
            color: colors[0],
            max_speed: 20.0,
            acceleration: 20.0,
            jump_speed: 40.0,
            gravity: 2.0,
            friction: 1.0,
            grounded_time: Duration::new(0, 0),
            jump_delay: Duration::new(0, 100_000_000),
            color_delay: Duration::new(0, 100_000_000),
            color_time: Duration::new(0, 0),
            can_jump: false,
            is_grounded: false,
            colors
        }
    }
    fn handle_player_input(&mut self, ctx: &mut Context) {
        // Move left and right
        if ctx.keyboard.is_key_pressed(VirtualKeyCode::Left) && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Right) {
            self.move_left();
        }
        else if ctx.keyboard.is_key_pressed(VirtualKeyCode::Right) && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Left) {
            self.move_right();
        }
        
        // Jump!
        if ctx.keyboard.is_key_pressed(VirtualKeyCode::Up) && !ctx.keyboard.is_key_pressed(VirtualKeyCode::Down)  {
            self.jump();
        }
        
        // Change Color
        if ctx.keyboard.is_key_just_pressed(VirtualKeyCode::Space){
            self.update_color();
        }
    }
    
    pub fn update_position(&mut self, ctx: &mut Context) {
        // Update position based on velocity
        self.position.x += self.velocity.x;
        self.position.y += self.velocity.y;

        // Update the entity rectangle position
        self.entity.x = self.position.x;
        self.entity.y = self.position.y;

        // Apply gravity
        self.velocity.y += self.gravity;

        // Keep player on the screen
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

        let delta_time = ctx.time.delta();
        self.color_time += delta_time;
        if self.is_grounded {
            self.grounded_time += delta_time;
        }
        if self.grounded_time >= self.jump_delay {
            self.can_jump = true;
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

    pub fn update_color(&mut self) {
        if self.color_time > self.color_delay {
            let index = self.colors.iter().position(|&c| c == self.color).unwrap();
            self.color = self.colors[(index + 1) % self.colors.len()];
            self.color_time = Duration::new(0, 0);
        }
    }
}