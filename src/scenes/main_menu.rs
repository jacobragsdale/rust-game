//! Transparent start overlay: the freshly generated world sits frozen behind
//! it until the player presses Up.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Text, TextFragment};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::scenes::{Resources, Scene, Transition};

pub struct MainMenuScene;

impl Scene for MainMenuScene {
    fn update(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        Ok(Transition::None)
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, res: &mut Resources) -> GameResult {
        let title = Text::new(
            TextFragment::new("SuperGame")
                .scale(PxScale::from(96.0))
                .color(Color::from_rgb(240, 240, 240)),
        );
        let prompt = Text::new(
            TextFragment::new("Press Up to Start")
                .scale(PxScale::from(40.0))
                .color(Color::from_rgb(220, 220, 220)),
        );

        let view_w = res.config.display.width;
        let view_h = res.config.display.height;
        let title_size = title.measure(ctx)?;
        let prompt_size = prompt.measure(ctx)?;

        canvas.draw(
            &title,
            DrawParam::default().dest(Vec2::new((view_w - title_size.x) / 2.0, view_h * 0.3)),
        );
        canvas.draw(
            &prompt,
            DrawParam::default().dest(Vec2::new(
                (view_w - prompt_size.x) / 2.0,
                view_h * 0.3 + title_size.y + 40.0,
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
            VirtualKeyCode::Up => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draws_below(&self) -> bool {
        true
    }
}
