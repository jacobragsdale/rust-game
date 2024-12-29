use crate::player::Player;
use ggez::event::EventHandler;
use ggez::graphics::{self, Color};
use ggez::{Context, GameResult};
use crate::entity::Entity;

const DEBUG: bool = false;

pub struct Game {
    entities: Vec<Box<dyn Entity>>
}

impl Game {
    pub fn new() -> Game {
        // Create a vector of entities that need to be drawn and updated
        let mut entities: Vec<Box<dyn Entity>> = Vec::new();
        
        // Add the player and any others to the vec
        entities.push(Box::new(Player::new()));
        
        // Return the created struct
        Game { entities }
    }
}

impl EventHandler for Game {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        use std::time::Instant;

        // Start the timer
        let start_time = Instant::now();

        // The loop you want to measure
        for entity in &mut self.entities {
            entity.update(ctx); // Update each entity
        }

        // Calculate the elapsed time
        let elapsed = start_time.elapsed();
        
        if DEBUG {
            println!("Time elapsed: {:?}", elapsed);
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);

        for entity in &mut self.entities {
            entity.draw(ctx, &mut canvas)?; // Draw each entity
        }

        canvas.finish(ctx)
    }
}