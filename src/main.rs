mod app;
mod config;
mod ecs;
mod physics;
mod save;
mod scenes;
mod systems;

use ggez::event;
use ggez::ContextBuilder;

use crate::app::App;
use crate::config::Config;

fn main() -> anyhow::Result<()> {
    let config = Config::load("config.toml")?;

    let (ctx, event_loop) = ContextBuilder::new("supergame", "Jacob Ragsdale")
        .window_setup(ggez::conf::WindowSetup::default().title("SuperGame"))
        .window_mode(
            ggez::conf::WindowMode::default()
                .dimensions(config.display.width, config.display.height),
        )
        .build()?;

    let app = App::new(config);
    event::run(ctx, event_loop, app)
}
