mod player;
mod config;
mod entity;
mod game;

use crate::game::Game;
use ggez::event::{self};
use ggez::ContextBuilder;

fn main() {
    let conf = config::Config::new("config.toml");

    let (mut ctx, event_loop) =
        ContextBuilder::new("jacobs_game", "Jacob Ragsdale")
            .window_setup(ggez::conf::WindowSetup::default().title("Jacob's game"))
            .window_mode(ggez::conf::WindowMode::default().dimensions(1920.0, 1080.0))
            .build().expect("Failed to create game context");

    let my_game = Game::new(&mut ctx);
    event::run(ctx, event_loop, my_game);
}