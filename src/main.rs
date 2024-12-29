mod player;
mod config;
mod entity;
mod game;

use crate::game::Game;
use ggez::event::{self};
use ggez::ContextBuilder;

fn main() {
    let conf = config::Config::new("config.toml");
    
    let (ctx, event_loop) =
        ContextBuilder::new("jacobs_game", "Jacob Ragsdale")
            .window_setup(ggez::conf::WindowSetup::default().title("Jacob's game"))
            .window_mode(ggez::conf::WindowMode::default().dimensions(conf.display.width, 
                                                                      conf.display.height))
            .build().expect("Failed to create game context");

    let my_game = Game::new();
    event::run(ctx, event_loop, my_game);
}