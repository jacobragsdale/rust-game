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

use crate::ecs::components::{Casting, Health, Mana, Position, Size, Team};
use crate::sim::Sim;

const PANEL: Color = Color::new(0.05, 0.04, 0.09, 0.85);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
const HEALTH: Color = Color::new(0.85, 0.22, 0.28, 1.0);
const HEALTH_LOW: Color = Color::new(0.95, 0.62, 0.15, 1.0);
/// The mana bar has no low-mana colour, deliberately. Running out of mana is a
/// resource state, not an emergency: the spell simply does not come out, and a
/// bar that turned orange at one point left would be shouting about it.
const MANA: Color = Color::new(0.27, 0.50, 0.92, 1.0);
/// The same blue, dimmed, while a cast is on cooldown. Without some readout of
/// the cooldown, "why didn't that go off?" has no answer on screen at all.
const MANA_COOLING: Color = Color::new(0.16, 0.28, 0.52, 1.0);
const ENEMY_HEALTH: Color = Color::new(0.85, 0.30, 0.30, 1.0);
const EMPTY: Color = Color::new(0.16, 0.14, 0.22, 1.0);

/// Player bar geometry, in internal-canvas pixels.
const MARGIN: f32 = 6.0;
const BAR_W: f32 = 72.0;
const BAR_H: f32 = 7.0;
const BORDER_W: f32 = 1.0;
/// Vertical gap between the health bar and the mana bar. Chosen so both bars
/// and their borders sit inside the panel that was already being drawn for
/// two of them — `MARGIN` and `BAR_H` are untouched, so the health bar has not
/// moved by a pixel.
const BAR_GAP: f32 = 3.0;
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

    // One backing panel sized for two bars, which is what it was already sized
    // for: the mana bar landed in space that had been reserved for it, so the
    // health bar did not move.
    let panel_h = BAR_H * 2.0 + MARGIN * 1.5;
    fill(
        canvas,
        Vec2::new(MARGIN - 2.0, MARGIN - 2.0),
        Vec2::new(BAR_W + 4.0, panel_h),
        PANEL,
    );

    bar(
        canvas,
        HEALTH_ORIGIN,
        health.fraction(),
        if health.fraction() <= LOW_FRACTION {
            HEALTH_LOW
        } else {
            HEALTH
        },
    );

    // Only for someone who has a pool at all. A player with no magic yet
    // should see one bar, not an empty second one.
    let Some((mana, cooling)) = player_mana(sim) else {
        return;
    };
    if mana.max <= 0 {
        return;
    }
    bar(
        canvas,
        MANA_ORIGIN,
        mana.fraction(),
        if cooling { MANA_COOLING } else { MANA },
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

/// Where the health bar's top-left corner is. The mana bar's is
/// [`MANA_ORIGIN`], directly below it.
///
/// Named constants rather than literals inside `draw_player_bars` so that
/// `the_mana_bar_did_not_move_the_health_bar` can check the layout without a
/// graphics context — this file is otherwise untestable, and "did the health
/// bar move?" is exactly the kind of question a screenshot answers badly.
const HEALTH_ORIGIN: Vec2 = Vec2::new(MARGIN, MARGIN);
const MANA_ORIGIN: Vec2 = Vec2::new(MARGIN, MARGIN + BAR_H + BAR_GAP);

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

/// The player's pool, and whether a cast is on cooldown right now.
fn player_mana(sim: &Sim) -> Option<(Mana, bool)> {
    let mut query = sim.world.query::<(&Mana, &Team, Option<&Casting>)>();
    query
        .iter()
        .find(|(_, (_, team, _))| **team == Team::Player)
        .map(|(_, (mana, _, casting))| (*mana, casting.is_some_and(|c| c.cooldown > 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The height of the panel both bars are drawn on. Written out as the
    /// expression `draw_player_bars` uses, because the point is that it did
    /// not change: it was sized for two bars from the day the health bar
    /// shipped, specifically so that this ticket would not move it.
    fn panel() -> (f32, f32) {
        let top = MARGIN - 2.0;
        (top, top + BAR_H * 2.0 + MARGIN * 1.5)
    }

    /// The vertical extent of a bar including its border.
    fn extent(origin: Vec2) -> (f32, f32) {
        (origin.y - BORDER_W, origin.y + BAR_H + BORDER_W)
    }

    /// The acceptance criterion, checked rather than eyeballed: the mana bar
    /// went into space that was already reserved, so the health bar's pixels
    /// are exactly where they were.
    #[test]
    fn the_mana_bar_did_not_move_the_health_bar() {
        assert_eq!(HEALTH_ORIGIN, Vec2::new(MARGIN, MARGIN));
        assert_eq!(MARGIN, 6.0);
        assert_eq!(BAR_W, 72.0);
        assert_eq!(BAR_H, 7.0);
    }

    #[test]
    fn the_mana_bar_sits_below_the_health_bar_without_touching_it() {
        let (_, health_bottom) = extent(HEALTH_ORIGIN);
        let (mana_top, _) = extent(MANA_ORIGIN);
        assert!(
            mana_top >= health_bottom,
            "the bars' borders overlap: health ends at {health_bottom}, mana starts at {mana_top}"
        );
        assert_eq!(MANA_ORIGIN.x, HEALTH_ORIGIN.x, "and they are left-aligned");
    }

    #[test]
    fn both_bars_fit_inside_the_panel_that_was_already_being_drawn() {
        let (panel_top, panel_bottom) = panel();
        for origin in [HEALTH_ORIGIN, MANA_ORIGIN] {
            let (top, bottom) = extent(origin);
            assert!(
                top >= panel_top && bottom <= panel_bottom,
                "a bar at {origin:?} spans {top}..{bottom}, outside the panel's \
                 {panel_top}..{panel_bottom}"
            );
        }
    }

    /// Running out of mana is a resource state, not an emergency: there is no
    /// low-mana colour, and the only thing that changes the bar's colour is
    /// the cooldown — without which "why didn't that cast" is invisible.
    fn channels(colour: Color) -> [f32; 3] {
        [colour.r, colour.g, colour.b]
    }

    #[test]
    fn the_only_second_mana_colour_is_the_cooldown_one() {
        let (full, cooling) = (channels(MANA), channels(MANA_COOLING));
        assert_ne!(full, cooling);
        assert!(
            full.iter().zip(&cooling).all(|(a, b)| b < a),
            "the cooldown treatment should be the same blue, dimmed: \
             {full:?} vs {cooling:?}"
        );
    }
}
