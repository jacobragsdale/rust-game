mod app;
mod assets;
mod config;
mod ecs;
mod level;
mod physics;
mod scenes;
mod systems;

use ggez::event;
use ggez::ContextBuilder;

use crate::app::App;
use crate::config::Config;

fn main() -> anyhow::Result<()> {
    let config = Config::load("config.toml")?;

    let (mut ctx, event_loop) = ContextBuilder::new("supergame", "Jacob Ragsdale")
        .window_setup(ggez::conf::WindowSetup::default().title("SuperGame"))
        .window_mode(
            ggez::conf::WindowMode::default()
                .dimensions(config.display.width, config.display.height),
        )
        .build()?;

    let app = App::new(&mut ctx, config);
    event::run(ctx, event_loop, app)
}
