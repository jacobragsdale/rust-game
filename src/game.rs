use crate::player::Player;
use ggez::event::EventHandler;
use ggez::graphics::{self, Color};
use ggez::{Context, GameResult};
use crate::config::Config;
use crate::entity::Entity;
use std::time::Instant;

pub struct Game {
    config: Config,
    entities: Vec<Box<dyn Entity>>
}

impl Game {
    pub fn new() -> Game {
        let config = Config::new("config.toml");
        
        // Create a vector of entities that need to be drawn and updated
        let mut entities: Vec<Box<dyn Entity>> = Vec::new();
        
        // Add the player and any others to the vec
        entities.push(Box::new(Player::new()));
        
        // Return the created struct
        Game { config, entities }
    }
}

impl EventHandler for Game {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        // Time the loop
        let start_time = Instant::now();
        
        for entity in &mut self.entities {
            entity.update(ctx); // Update each entity
        }
        
        if self.config.debug.debug {
            println!("Time take to update:{:?} entities {:?}", self.entities.len(), start_time.elapsed());
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        // Time the loop
        let start_time = Instant::now();
        
        let mut canvas = graphics::Canvas::from_frame(ctx, Color::BLACK);

        for entity in &mut self.entities {
            entity.draw(ctx, &mut canvas)?; // Draw each entity
        }

        if self.config.debug.debug {
            println!("Time take to draw:{:?} entities {:?}", self.entities.len(), start_time.elapsed());
        }

        canvas.finish(ctx)
    }
}