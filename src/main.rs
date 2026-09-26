//! The game binary: opens a window and hands control to the scene stack.
//! Everything it uses lives in the `supergame` library, which the headless
//! `sim` binary and the integration tests share.

use ggez::event;
use ggez::ContextBuilder;

use supergame::app::App;
use supergame::config::Config;

fn main() -> anyhow::Result<()> {
    let config = Config::load("config.toml")?;

    let (mut ctx, event_loop) = ContextBuilder::new("supergame", "Jacob Ragsdale")
        .window_setup(ggez::conf::WindowSetup::default().title("SuperGame"))
        .window_mode(ggez::conf::WindowMode {
            // Logical points, not physical pixels: on a 2x display the
            // physical size would open a window half the intended size.
            logical_size: Some(ggez::winit::dpi::LogicalSize::new(
                config.display.width,
                config.display.height,
            )),
            resizable: true,
            ..ggez::conf::WindowMode::default()
        })
        .build()?;

    let app = App::new(&mut ctx, config);
    event::run(ctx, event_loop, app)
}
