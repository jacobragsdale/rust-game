//! Death overlay. The level keeps updating underneath (the world scrolls on,
//! the corpse falls), matching the original behavior. Space starts a new run.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Text, TextFragment};
use ggez::winit::event::VirtualKeyCode;
use ggez::{Context, GameResult};

use crate::scenes::{Resources, Scene, Transition};

pub struct GameOverScene {
    final_secs: f32,
}

impl GameOverScene {
    pub fn new(final_secs: f32) -> Self {
        GameOverScene { final_secs }
    }
}

impl Scene for GameOverScene {
    fn update(&mut self, _ctx: &mut Context, _res: &mut Resources) -> GameResult<Transition> {
        Ok(Transition::None)
    }

    fn draw(&mut self, ctx: &mut Context, canvas: &mut Canvas, res: &mut Resources) -> GameResult {
        let view_w = res.config.display.width;
        let view_h = res.config.display.height;
        let centered = |size: Vec2, y: f32| Vec2::new((view_w - size.x) / 2.0, y);

        let death_text = Text::new(
            TextFragment::new("You Died.")
                .scale(PxScale::from(96.0))
                .color(Color::from_rgb(200, 20, 20)),
        );
        let death_size = Vec2::from(death_text.measure(ctx)?);
        let death_pos = centered(death_size, (view_h - death_size.y) / 2.0);
        canvas.draw(&death_text, DrawParam::default().dest(death_pos));

        let score_text = Text::new(
            TextFragment::new(format!("Final Time: {:.2}s", self.final_secs))
                .scale(PxScale::from(48.0))
                .color(Color::from_rgb(220, 220, 220)),
        );
        let score_size = Vec2::from(score_text.measure(ctx)?);
        let mut next_y = death_pos.y + death_size.y + 20.0;
        canvas.draw(
            &score_text,
            DrawParam::default().dest(centered(score_size, next_y)),
        );
        next_y += score_size.y + 40.0;

        if !res.top_scores.is_empty() {
            let header = Text::new(
                TextFragment::new("Top Runs")
                    .scale(PxScale::from(36.0))
                    .color(Color::from_rgb(240, 240, 240)),
            );
            let header_size = Vec2::from(header.measure(ctx)?);
            canvas.draw(
                &header,
                DrawParam::default().dest(centered(header_size, next_y)),
            );
            next_y += header_size.y + 12.0;

            for (index, score) in res.top_scores.iter().take(3).enumerate() {
                let entry = Text::new(
                    TextFragment::new(format!("{}. {:.2}s", index + 1, score))
                        .scale(PxScale::from(28.0))
                        .color(Color::from_rgb(220, 220, 220)),
                );
                let entry_size = Vec2::from(entry.measure(ctx)?);
                canvas.draw(
                    &entry,
                    DrawParam::default().dest(centered(entry_size, next_y)),
                );
                next_y += entry_size.y + 8.0;
            }
        }

        let prompt = Text::new(
            TextFragment::new("Press Space to Play Again")
                .scale(PxScale::from(32.0))
                .color(Color::from_rgb(235, 235, 235)),
        );
        let prompt_size = Vec2::from(prompt.measure(ctx)?);
        canvas.draw(
            &prompt,
            DrawParam::default().dest(centered(prompt_size, next_y + 20.0)),
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
            VirtualKeyCode::Space => Transition::Reset,
            _ => Transition::None,
        }
    }

    fn draws_below(&self) -> bool {
        true
    }

    fn updates_below(&self) -> bool {
        true
    }
}
