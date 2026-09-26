//! In-game debug overlay: draws what the simulation actually believes, on
//! top of what the artist drew.
//!
//! The distinction matters. A normal screenshot shows a knight standing on
//! bricks and tells you nothing about whether the collider matches the art,
//! whether that platform is registered as one-way, or where the hazard
//! rectangle really is. This overlay makes all of that visible, which is what
//! turns a screenshot into evidence.
//!
//! It draws into the same [`Frame`] as the world, so it is pixel-aligned with
//! it — and so `cargo run --bin render -- --debug` shows it too, which is how
//! an agent gets the evidence without a window.
//!
//! Toggle with F1, or force it on at boot with `SUPERGAME_DEBUG=1`.

use ggez::glam::Vec2;

use crate::ecs::components::{
    AnimationState, Avatar, Body, Pendulum, Position, Size, Sprite, Velocity,
};
use crate::physics::Aabb;
use crate::render::{font, Color, Frame};
use crate::sim::Sim;

const SOLID: Color = Color::new(0.25, 0.5, 1.0, 0.9);
const ONE_WAY: Color = Color::new(0.3, 0.9, 0.5, 0.9);
const HAZARD: Color = Color::new(1.0, 0.28, 0.28, 0.9);
/// Inset outline on geometry an entity owns, rather than the map.
const ENTITY_OWNED: Color = Color::new(1.0, 0.6, 0.15, 1.0);
const COLLIDER: Color = Color::new(1.0, 0.87, 0.24, 1.0);
const SPRITE_BOUNDS: Color = Color::new(1.0, 0.35, 1.0, 0.75);
const VELOCITY: Color = Color::new(0.35, 0.95, 1.0, 1.0);
const SPAWN: Color = Color::new(1.0, 1.0, 1.0, 0.8);
const BACKDROP: Color = Color::new(0.0, 0.0, 0.0, 0.65);

const STROKE: f32 = 1.0;
/// Velocity is up to ~900 px/s; scale it down to a readable arrow.
const VELOCITY_SCALE: f32 = 0.08;
const LEGEND_GAP: f32 = 8.0;

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

    /// Draw the overlay. `offset` is the camera's world offset; `view` is the
    /// size of the picture.
    pub fn draw(&self, frame: &mut Frame, sim: &Sim, offset: Vec2, view: Vec2) {
        if !self.enabled {
            return;
        }
        draw_geometry(frame, sim, offset, view);
        draw_readout(frame, sim);
        draw_legend(frame, view);
    }
}

fn draw_geometry(frame: &mut Frame, sim: &Sim, offset: Vec2, view: Vec2) {
    let visible = Aabb::new(offset.x, offset.y, view.x, view.y);

    // Collision geometry, read from `Sim::geometry` rather than from the
    // level. The level is only half the world now — a fire's collider, a
    // moving platform's, anything an entity owns — and drawing the half
    // that never changes would make exactly the geometry most likely to be
    // wrong the geometry you cannot see.
    let solids = sim.geometry.rects();
    let owned_from = solids.len() - sim.geometry.entity_rect_count();
    for (index, solid) in solids.iter().enumerate() {
        if !solid.rect.overlaps(&visible) {
            continue;
        }
        let color = if solid.one_way { ONE_WAY } else { SOLID };
        outline(frame, &solid.rect, offset, color);
        if index >= owned_from {
            mark_entity_owned(frame, &solid.rect, offset);
        }
    }

    let hazards = sim.geometry.hazards();
    let owned_from = hazards.len() - sim.geometry.entity_hazard_count();
    for (index, hazard) in hazards.iter().enumerate() {
        if !hazard.overlaps(&visible) {
            continue;
        }
        outline(frame, hazard, offset, HAZARD);
        if index >= owned_from {
            mark_entity_owned(frame, hazard, offset);
        }
    }

    // A swinging hazard's whole arc, not just where the ball is now.
    //
    // Every other collider in this overlay is a box you can see all of at
    // once. A pendulum's danger is a *path* — the ball is somewhere harmless
    // for most of its cycle and lethal for a fraction of it — so the box
    // alone tells you nothing about the thing the map actually placed.
    // Drawing the arc is what makes "this swing is buried in a wall" visible;
    // `every_swinging_hazard_has_an_arc_clear_of_the_level` in
    // `tests/levels.rs` is the same check without a picture.
    for (_, pendulum) in sim.world.query::<&Pendulum>().iter() {
        const STEPS: usize = 32;
        let points: Vec<Vec2> = (0..=STEPS)
            .map(|step| {
                let t = step as f32 / STEPS as f32;
                pendulum.at_angle(pendulum.amplitude * (2.0 * t - 1.0)) - offset
            })
            .collect();
        for pair in points.windows(2) {
            frame.line(pair[0], pair[1], STROKE, ENTITY_OWNED);
        }
    }

    // Exits are invisible in the game — the level art around one is what
    // shows the way — so this is the one place they can be seen, as a dashed
    // box in the one-way colour.
    for (_, (pos, size, _)) in sim
        .world
        .query::<(&Position, &Size, &crate::ecs::components::Exit)>()
        .iter()
    {
        let box_ = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        outline(frame, &box_, offset, ONE_WAY);
        mark_entity_owned(frame, &box_, offset);
    }
    // Named arrival points, where a door on another map puts the player.
    for (id, at) in &sim.level.spawns {
        let at = *at - offset;
        frame.rect(at, Vec2::new(4.0, 1.0), SPAWN);
        frame.text(at + Vec2::new(0.0, -10.0), id, SPAWN);
    }

    // Where the player comes back to, as a small cross — the checkpoint or
    // the door they came in by, not just the map's `P`.
    let spawn = sim.respawn_point() - offset;
    frame.rect(spawn - Vec2::new(3.0, 0.0), Vec2::new(7.0, 1.0), SPAWN);
    frame.rect(spawn - Vec2::new(0.0, 3.0), Vec2::new(1.0, 7.0), SPAWN);

    // Per-entity: the collider the physics uses, and the sprite drawn for
    // it. When these two disagree, the game looks subtly wrong in a way no
    // amount of staring at the art will explain.
    for (entity, (pos, size, sprite, anim, vel)) in sim
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
        let collider = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        outline(frame, &collider, offset, COLLIDER);

        // The frame box is per clip, so the outline has to follow the clip
        // actually playing — otherwise it would be drawn at the wrong size
        // for every entity whose animations differ in frame width.
        if let (Some(sprite), Some(anim)) = (sprite, anim) {
            if let Some(clip) = sprite.clips.clip(&anim.clip) {
                let (fw, fh) = sprite.clips.frame_size_of(clip);
                let facing = crate::view::faces_right(&sim.world, entity);
                let origin = sprite.draw_origin(pos.0, size.0, clip, facing);
                outline(
                    frame,
                    &Aabb::new(origin.x, origin.y, fw, fh),
                    offset,
                    SPRITE_BOUNDS,
                );
            }
        }

        if vel.0.length_squared() > 1.0 {
            let centre = pos.0 + size.0 / 2.0 - offset;
            frame.line(centre, centre + vel.0 * VELOCITY_SCALE, STROKE, VELOCITY);
        }
    }
}

fn draw_readout(frame: &mut Frame, sim: &Sim) {
    let mut lines: Vec<String> = Vec::new();
    for (_, (avatar, body, pos, vel, anim)) in sim
        .world
        .query::<(&Avatar, &Body, &Position, &Velocity, &AnimationState)>()
        .iter()
    {
        lines.push(format!(
            "tick {}  world {}  map {}",
            sim.tick,
            sim.world_tick(),
            sim.map_name()
        ));
        lines.push(format!("pos {:.2} {:.2}", pos.0.x, pos.0.y));
        lines.push(format!("vel {:.2} {:.2}", vel.0.x, vel.0.y));
        lines.push(format!("clip {}[{}]", anim.clip, anim.frame));
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

    let width = lines.iter().map(|l| font::width(l)).fold(0.0, f32::max) + 6.0;
    frame.rect(
        Vec2::new(2.0, 2.0),
        Vec2::new(width, lines.len() as f32 * font::LINE_H + 4.0),
        BACKDROP,
    );
    for (row, line) in lines.iter().enumerate() {
        frame.text(
            Vec2::new(5.0, 4.0 + row as f32 * font::LINE_H),
            line,
            Color::WHITE,
        );
    }
}

/// A colour key along the bottom edge. Without it, a screenshot of the
/// overlay is a pile of coloured boxes whose meaning has to be recalled from
/// the source.
fn draw_legend(frame: &mut Frame, view: Vec2) {
    const ITEMS: [(&str, Color); 7] = [
        ("solid", SOLID),
        ("oneway", ONE_WAY),
        ("hazard", HAZARD),
        ("entity-geo", ENTITY_OWNED),
        ("collider", COLLIDER),
        ("sprite", SPRITE_BOUNDS),
        ("vel", VELOCITY),
    ];
    let y = view.y - font::LINE_H - 2.0;
    frame.rect(
        Vec2::new(2.0, y - 2.0),
        Vec2::new(view.x - 4.0, font::LINE_H + 3.0),
        BACKDROP,
    );
    let mut x = 5.0;
    for (label, color) in ITEMS {
        x += frame.text(Vec2::new(x, y), label, color) + LEGEND_GAP;
    }
}

fn outline(frame: &mut Frame, rect: &Aabb, offset: Vec2, color: Color) {
    frame.outline(
        Vec2::new(rect.x - offset.x, rect.y - offset.y).floor(),
        Vec2::new(rect.w, rect.h).round(),
        STROKE,
        color,
    );
}

/// A second, inset outline marking a rect as belonging to an entity rather
/// than to the map.
///
/// Which half a rect came from is the question the overlay exists to answer
/// for M7: a fire that never lights and a platform frozen at one end of its
/// path both look exactly like level geometry until you can see that the game
/// thinks they are entities.
fn mark_entity_owned(frame: &mut Frame, rect: &Aabb, offset: Vec2) {
    const INSET: f32 = 2.0;
    if rect.w <= INSET * 2.0 || rect.h <= INSET * 2.0 {
        return;
    }
    let inner = Aabb::new(
        rect.x + INSET,
        rect.y + INSET,
        rect.w - INSET * 2.0,
        rect.h - INSET * 2.0,
    );
    outline(frame, &inner, offset, ENTITY_OWNED);
}

/// Coyote uses `u32::MAX`-ish sentinels for "no jump available".
fn display_ticks(ticks: u32) -> String {
    if ticks > 999 {
        "-".to_string()
    } else {
        ticks.to_string()
    }
}
