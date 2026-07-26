//! The heads-up display: what the player needs to know without pausing.
//!
//! Drawn into the 640x360 internal canvas, in world-independent screen space
//! for the player's own bars and in world space for anything floating over an
//! entity. Everything here is rectangles rather than art, which is not a
//! placeholder apology — at this resolution a two-pixel border and a flat fill
//! read more clearly than a drawn frame would.
//!
//! Unlike [`crate::debug`], this is not diagnostic. It shows only what the
//! player is meant to know, which is why enemy health stays hidden until you
//! have actually hit something.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, Quad};

use crate::ecs::components::{Health, Position, Size, Team};
use crate::sim::Sim;

const PANEL: Color = Color::new(0.05, 0.04, 0.09, 0.85);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
const HEALTH: Color = Color::new(0.85, 0.22, 0.28, 1.0);
const HEALTH_LOW: Color = Color::new(0.95, 0.62, 0.15, 1.0);
const ENEMY_HEALTH: Color = Color::new(0.85, 0.30, 0.30, 1.0);
const EMPTY: Color = Color::new(0.16, 0.14, 0.22, 1.0);

/// Player bar geometry, in internal-canvas pixels.
const MARGIN: f32 = 6.0;
const BAR_W: f32 = 72.0;
const BAR_H: f32 = 7.0;
const BORDER_W: f32 = 1.0;
/// Below a quarter health the bar changes colour, which is readable from the
/// corner of the eye in a way a shrinking red bar is not.
const LOW_FRACTION: f32 = 0.25;

/// Floating enemy bar geometry.
const ENEMY_BAR_W: f32 = 24.0;
const ENEMY_BAR_H: f32 = 3.0;
/// How far above an enemy's collider its bar sits.
const ENEMY_BAR_LIFT: f32 = 7.0;

pub fn draw(canvas: &mut Canvas, sim: &Sim, camera_offset: Vec2) {
    draw_player_bars(canvas, sim);
    draw_enemy_bars(canvas, sim, camera_offset);
}

fn draw_player_bars(canvas: &mut Canvas, sim: &Sim) {
    let Some(health) = player_health(sim) else {
        return;
    };

    // One backing panel sized for two bars, so that adding the mana bar in
    // M3c does not move the health bar.
    let panel_h = BAR_H * 2.0 + MARGIN * 1.5;
    fill(
        canvas,
        Vec2::new(MARGIN - 2.0, MARGIN - 2.0),
        Vec2::new(BAR_W + 4.0, panel_h),
        PANEL,
    );

    bar(
        canvas,
        Vec2::new(MARGIN, MARGIN),
        health.fraction(),
        if health.fraction() <= LOW_FRACTION {
            HEALTH_LOW
        } else {
            HEALTH
        },
    );
}

fn draw_enemy_bars(canvas: &mut Canvas, sim: &Sim, camera_offset: Vec2) {
    for (_, (pos, size, health, team)) in sim
        .world
        .query::<(&Position, &Size, &Health, &Team)>()
        .iter()
    {
        // Nothing above an enemy you have not engaged, and nothing above a
        // corpse. A health bar is feedback on a fight in progress.
        if *team != Team::Enemy || !health.damaged() || health.dead() {
            continue;
        }

        let centre = pos.0.x + size.0.x / 2.0;
        let origin = Vec2::new(
            centre - ENEMY_BAR_W / 2.0,
            pos.0.y - ENEMY_BAR_LIFT - ENEMY_BAR_H,
        ) - camera_offset;

        fill(
            canvas,
            origin.floor() - Vec2::splat(1.0),
            Vec2::new(ENEMY_BAR_W + 2.0, ENEMY_BAR_H + 2.0),
            PANEL,
        );
        fill(
            canvas,
            origin.floor(),
            Vec2::new(ENEMY_BAR_W, ENEMY_BAR_H),
            EMPTY,
        );
        fill(
            canvas,
            origin.floor(),
            Vec2::new(ENEMY_BAR_W * health.fraction(), ENEMY_BAR_H),
            ENEMY_HEALTH,
        );
    }
}

/// A bordered bar: border, empty track, then the filled portion.
fn bar(canvas: &mut Canvas, origin: Vec2, fraction: f32, colour: Color) {
    let size = Vec2::new(BAR_W, BAR_H);
    fill(
        canvas,
        origin - Vec2::splat(BORDER_W),
        size + Vec2::splat(BORDER_W * 2.0),
        BORDER,
    );
    fill(canvas, origin, size, EMPTY);
    if fraction > 0.0 {
        fill(
            canvas,
            origin,
            Vec2::new((BAR_W * fraction).max(1.0).floor(), BAR_H),
            colour,
        );
    }
}

fn fill(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    canvas.draw(
        &Quad,
        DrawParam::new().dest(origin).scale(size).color(colour),
    );
}

fn player_health(sim: &Sim) -> Option<Health> {
    let mut query = sim.world.query::<(&Health, &Team)>();
    query
        .iter()
        .find(|(_, (_, team))| **team == Team::Player)
        .map(|(_, (health, _))| *health)
}
