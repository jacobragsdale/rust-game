//! Pause overlay: freezes the level underneath, pops on P.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Text, TextFragment};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::scenes::{Resources, Scene, Transition};

pub struct PauseScene;

impl Scene for PauseScene {
    fn update(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        Ok(Transition::None)
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, res: &mut Resources) -> GameResult {
        let text = Text::new(
            TextFragment::new("Paused - P to resume")
                .scale(PxScale::from(48.0))
                .color(Color::from_rgb(235, 235, 235)),
        );
        let size = text.measure(ctx)?;
        canvas.draw(
            &text,
            DrawParam::default().dest(Vec2::new(
                (res.config.display.width - size.x) / 2.0,
                (res.config.display.height - size.y) / 2.0,
            )),
        );
        Ok(())
    }

    fn key_down(
        &mut self,
        _ctx: &mut Context,
        _res: &mut Resources,
        key: VirtualKeyCode,
    ) -> Transition {
        match key {
            VirtualKeyCode::P => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draws_below(&self) -> bool {
        true
    }
}
