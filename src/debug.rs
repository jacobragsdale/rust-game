//! In-game debug overlay: draws what the simulation actually believes, on
//! top of what the artist drew.
//!
//! The distinction matters. A normal screenshot shows a knight standing on
//! bricks and tells you nothing about whether the collider matches the art,
//! whether that platform is registered as one-way, or where the hazard
//! rectangle really is. This overlay makes all of that visible, which is what
//! turns a screenshot into evidence.
//!
//! It renders into the 640x360 internal canvas rather than the window, so it
//! is pixel-aligned with the world and looks identical at any window size.
//!
//! Toggle with F1, or force it on at boot with `SUPERGAME_DEBUG=1`.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawMode, DrawParam, Mesh, MeshBuilder, Quad, Rect, Text};
use ggez::{Context, GameResult};

use crate::ecs::components::{AnimationState, Avatar, Body, Position, Size, Sprite, Velocity};
use crate::physics::Aabb;
use crate::sim::Sim;

const SOLID: Color = Color::new(0.25, 0.5, 1.0, 0.9);
const ONE_WAY: Color = Color::new(0.3, 0.9, 0.5, 0.9);
const HAZARD: Color = Color::new(1.0, 0.28, 0.28, 0.9);
const COLLIDER: Color = Color::new(1.0, 0.87, 0.24, 1.0);
const SPRITE_BOUNDS: Color = Color::new(1.0, 0.35, 1.0, 0.75);
const VELOCITY: Color = Color::new(0.35, 0.95, 1.0, 1.0);
const SPAWN: Color = Color::new(1.0, 1.0, 1.0, 0.8);

const STROKE: f32 = 1.0;
/// Velocity is up to ~900 px/s; scale it down to a readable arrow.
const VELOCITY_SCALE: f32 = 0.08;
const HUD_LINE_HEIGHT: f32 = 11.0;
const HUD_FONT_SIZE: f32 = 10.0;
const HUD_WIDTH: f32 = 168.0;
const LEGEND_CHAR_WIDTH: f32 = 5.5;
const LEGEND_GAP: f32 = 10.0;

pub struct DebugOverlay {
    pub enabled: bool,
}

impl DebugOverlay {
    /// Off unless `SUPERGAME_DEBUG` is set to something truthy. Screenshot
    /// and capture workflows set it so the overlay is on without a keypress.
    pub fn from_env() -> Self {
        let enabled = matches!(
            std::env::var("SUPERGAME_DEBUG").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        DebugOverlay { enabled }
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Draw the overlay into the internal canvas. `offset` is the camera's
    /// world offset; `view` is the internal canvas size.
    pub fn draw(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        sim: &Sim,
        offset: Vec2,
        view: Vec2,
    ) -> GameResult {
        if !self.enabled {
            return Ok(());
        }
        self.draw_geometry(ctx, canvas, sim, offset, view)?;
        self.draw_hud(canvas, sim, view);
        Ok(())
    }

    fn draw_geometry(
        &self,
        ctx: &mut Context,
        canvas: &mut Canvas,
        sim: &Sim,
        offset: Vec2,
        view: Vec2,
    ) -> GameResult {
        let visible = Aabb::new(offset.x, offset.y, view.x, view.y);
        // The spawn cross below is unconditional, so the mesh is never empty.
        let mut mb = MeshBuilder::new();

        // Level collision geometry, in the colors the legend names.
        let layers = [
            (&sim.level.solids, SOLID),
            (&sim.level.one_way, ONE_WAY),
            (&sim.level.hazards, HAZARD),
        ];
        for (rects, color) in layers {
            for rect in rects.iter() {
                if !rect.overlaps(&visible) {
                    continue;
                }
                mb.rectangle(DrawMode::stroke(STROKE), screen_rect(rect, offset), color)?;
            }
        }

        // The player's spawn point, as a small cross.
        let spawn = sim.level.player_spawn - offset;
        mb.line(
            &[
                Vec2::new(spawn.x - 3.0, spawn.y),
                Vec2::new(spawn.x + 3.0, spawn.y),
            ],
            STROKE,
            SPAWN,
        )?;
        mb.line(
            &[
                Vec2::new(spawn.x, spawn.y - 3.0),
                Vec2::new(spawn.x, spawn.y + 3.0),
            ],
            STROKE,
            SPAWN,
        )?;

        // Per-entity: the collider the physics uses, and the sprite drawn for
        // it. When these two disagree, the game looks subtly wrong in a way
        // no amount of staring at the art will explain.
        for (_, (pos, size, sprite, anim, vel)) in sim
            .world
            .query::<(
                &Position,
                &Size,
                Option<&Sprite>,
                Option<&AnimationState>,
                &Velocity,
            )>()
            .iter()
        {
            let collider = Rect::new(pos.0.x - offset.x, pos.0.y - offset.y, size.0.x, size.0.y);
            mb.rectangle(DrawMode::stroke(STROKE), collider, COLLIDER)?;

            // The frame box is per clip now, so the outline has to follow the
            // clip actually playing — otherwise it would be drawn at the wrong
            // size for every entity whose animations differ in frame width.
            if let (Some(sprite), Some(anim)) = (sprite, anim) {
                if let Some(clip) = sprite.clips.clip(&anim.clip) {
                    let (fw, fh) = sprite.clips.frame_size_of(clip);
                    let origin = sprite.draw_origin(pos.0, size.0, (fw, fh)) - offset;
                    mb.rectangle(
                        DrawMode::stroke(STROKE),
                        Rect::new(origin.x, origin.y, fw, fh),
                        SPRITE_BOUNDS,
                    )?;
                }
            }

            if vel.0.length_squared() > 1.0 {
                let center = pos.0 + size.0 / 2.0 - offset;
                mb.line(&[center, center + vel.0 * VELOCITY_SCALE], STROKE, VELOCITY)?;
            }
        }

        let mesh = Mesh::from_data(ctx, mb.build());
        canvas.draw(&mesh, DrawParam::default());
        Ok(())
    }

    fn draw_hud(&self, canvas: &mut Canvas, sim: &Sim, view: Vec2) {
        let mut lines: Vec<String> = Vec::new();

        for (_, (avatar, body, pos, vel, anim)) in sim
            .world
            .query::<(&Avatar, &Body, &Position, &Velocity, &AnimationState)>()
            .iter()
        {
            lines.push(format!("tick  {}", sim.tick));
            lines.push(format!("pos   {:8.2} {:8.2}", pos.0.x, pos.0.y));
            lines.push(format!("vel   {:8.2} {:8.2}", vel.0.x, vel.0.y));
            lines.push(format!("clip  {}[{}]", anim.clip, anim.frame));
            lines.push(format!(
                "air {}  coyote {}/{}  buf {}",
                avatar.air_jumps,
                display_ticks(avatar.coyote_ticks),
                display_ticks(avatar.wall_coyote_ticks),
                avatar.jump_buffer
            ));
            lines.push(format!(
                "{}{}{}{}",
                if body.grounded {
                    "grounded "
                } else {
                    "airborne "
                },
                if avatar.wall_sliding {
                    "wallslide "
                } else {
                    ""
                },
                if body.on_one_way_only() {
                    "oneway "
                } else {
                    ""
                },
                if avatar.dead() { "DEAD" } else { "" },
            ));
        }

        if lines.is_empty() {
            lines.push("no avatar in world".to_string());
        }

        let mut text = Text::new(lines.join("\n"));
        text.set_scale(HUD_FONT_SIZE);

        // Dark backdrop so the readout stays legible over bright tiles.
        canvas.draw(
            &Quad,
            DrawParam::new()
                .dest(Vec2::new(2.0, 2.0))
                .scale(Vec2::new(
                    HUD_WIDTH,
                    lines.len() as f32 * HUD_LINE_HEIGHT + 6.0,
                ))
                .color(Color::new(0.0, 0.0, 0.0, 0.65)),
        );
        canvas.draw(
            &text,
            DrawParam::new()
                .dest(Vec2::new(5.0, 5.0))
                .color(Color::WHITE),
        );

        self.draw_legend(canvas, view);
    }

    /// A color key along the bottom edge. Without it, a screenshot of the
    /// overlay is a pile of colored boxes whose meaning has to be recalled
    /// from the source.
    fn draw_legend(&self, canvas: &mut Canvas, view: Vec2) {
        const ITEMS: [(&str, Color); 6] = [
            ("solid", SOLID),
            ("oneway", ONE_WAY),
            ("hazard", HAZARD),
            ("collider", COLLIDER),
            ("sprite", SPRITE_BOUNDS),
            ("vel", VELOCITY),
        ];

        let y = view.y - HUD_LINE_HEIGHT - 3.0;
        canvas.draw(
            &Quad,
            DrawParam::new()
                .dest(Vec2::new(2.0, y - 2.0))
                .scale(Vec2::new(view.x - 4.0, HUD_LINE_HEIGHT + 4.0))
                .color(Color::new(0.0, 0.0, 0.0, 0.65)),
        );

        let mut x = 5.0;
        for (label, color) in ITEMS {
            let mut text = Text::new(label);
            text.set_scale(HUD_FONT_SIZE);
            canvas.draw(&text, DrawParam::new().dest(Vec2::new(x, y)).color(color));
            // The default font is proportional, so this is an estimate; it
            // only has to keep the labels from colliding.
            x += label.len() as f32 * LEGEND_CHAR_WIDTH + LEGEND_GAP;
        }
    }
}

/// Coyote uses `u32::MAX`-ish sentinels for "no jump available".
fn display_ticks(ticks: u32) -> String {
    if ticks > 999 {
        "-".to_string()
    } else {
        ticks.to_string()
    }
}

fn screen_rect(rect: &Aabb, offset: Vec2) -> Rect {
    Rect::new(rect.x - offset.x, rect.y - offset.y, rect.w, rect.h)
}
