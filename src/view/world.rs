//! The world, as a list of things to draw: tiles, the hazards and platforms
//! that live in the level, what is lying on the floor, and everything with a
//! sprite.
//!
//! Every function here reads a [`Sim`] and writes a [`Frame`] and nothing
//! else, so the picture the game shows and the picture `cargo run --bin
//! render` writes are the same picture. Where art does not exist the
//! placeholders are drawn off the collider the physics actually uses rather
//! than off a second number stored beside it, so art and collision can never
//! quietly disagree.

use ggez::glam::Vec2;

use crate::assets::TilesetDef;
use crate::ecs::components::{
    AnimationState, Avatar, Checkpoint, Chest, Collider, Door, Fire, Gate, Health, Lever, Mover,
    Patrol, Pendulum, Pickup, Position, Projectile, Sign, Size, Sprite,
};
use crate::render::{Color, Frame, Rect};
use crate::sim::Sim;
use crate::systems::props;

/// How long the white hit flash lasts.
const FLASH_TICKS: u32 = 6;

/// Draw the world as seen from `offset`, into a picture of `view` size.
pub fn draw(frame: &mut Frame, sim: &Sim, tileset: &TilesetDef, offset: Vec2, view: Vec2) {
    tiles(frame, sim, tileset, offset, view);
    decor(frame, sim, tileset, offset);
    props(frame, sim, tileset, offset);
    hazards(frame, sim, offset);
    movers(frame, sim, offset);
    swings(frame, sim, offset);
    pickups(frame, sim, offset);
    sprites(frame, sim, offset);
}

/// The background layer, then the solid tiles over it — only the ones in
/// view, since a large map is thousands of tiles and a screen is a few hundred.
fn tiles(frame: &mut Frame, sim: &Sim, tileset: &TilesetDef, offset: Vec2, view: Vec2) {
    let level = &sim.level;
    let ts = level.tile_size;
    let x0 = (offset.x / ts).floor().max(0.0) as u32;
    let y0 = (offset.y / ts).floor().max(0.0) as u32;
    let x1 = (((offset.x + view.x) / ts).ceil().max(0.0) as u32 + 1).min(level.width);
    let y1 = (((offset.y + view.y) / ts).ceil().max(0.0) as u32 + 1).min(level.height);

    for layer in [&level.background, &level.tiles] {
        if layer.is_empty() {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let Some(tile) = layer[(y * level.width + x) as usize] else {
                    continue;
                };
                let dest = Vec2::new(x as f32 * ts, y as f32 * ts) - offset;
                frame.image(
                    &tileset.image,
                    tileset.tile_rect(tile),
                    dest.floor(),
                    false,
                    tileset.tint_color(),
                );
            }
        }
    }
}

/// Tileset art a map placed for looks, standing on the floor of its cell.
fn decor(frame: &mut Frame, sim: &Sim, tileset: &TilesetDef, offset: Vec2) {
    for piece in &sim.level.decor {
        tile_prop(
            frame,
            tileset,
            &piece.prop,
            piece.cell,
            sim.level.tile_size,
            offset,
        );
    }
}

/// A named tileset prop, its bottom row on the cell whose top-left is `cell`.
/// A name the tileset does not have draws nothing; `tests/data.rs` is where
/// that fails.
fn tile_prop(
    frame: &mut Frame,
    tileset: &TilesetDef,
    name: &str,
    cell: Vec2,
    ts: f32,
    offset: Vec2,
) {
    let Some(src) = tileset.prop_rect(name) else {
        return;
    };
    let dest = Vec2::new(cell.x, cell.y + ts - src.h) - offset;
    frame.image(
        &tileset.image,
        src,
        dest.floor(),
        false,
        tileset.tint_color(),
    );
}

/// One frame of a strip of pixel art whose frames are `size` wide.
fn strip(frame: &mut Frame, sheet: &str, index: u32, size: Vec2, at: Vec2) {
    frame.image(
        sheet,
        Rect::new(index as f32 * size.x, 0.0, size.x, size.y),
        at.floor(),
        false,
        Color::WHITE,
    );
}

/// The level's furniture. Every one is drawn from the state the sim keeps —
/// a chest from its world flag, a lever from the flag it throws, a checkpoint
/// from where the player will come back to — so the picture cannot show a
/// chest open that the game thinks is shut.
fn props(frame: &mut Frame, sim: &Sim, tileset: &TilesetDef, offset: Vec2) {
    let ts = sim.level.tile_size;
    for (_, (pos, door)) in sim.world.query::<(&Position, &Door)>().iter() {
        // The door's box is its lower two tiles; the art stands on the same
        // floor and may be taller.
        let cell = Vec2::new(pos.0.x, pos.0.y + ts);
        tile_prop(frame, tileset, &door.art, cell, ts, offset);
    }

    for (entity, (pos, gate)) in sim.world.query::<(&Position, &Gate)>().iter() {
        let box_ = gate.collider.aabb(pos.0);
        let at = (Vec2::new(box_.x, box_.y) - offset).floor();
        let closed = props::gate_closed(&sim.world, entity);
        // Raised, a gate is a few pixels of iron under the lintel.
        let height = if closed { box_.h } else { 6.0 };
        frame.rect(at, Vec2::new(box_.w, 4.0), Color::from_rgb(58, 54, 70));
        let bars = 4;
        for i in 0..bars {
            let x = at.x + 3.0 + i as f32 * (box_.w - 6.0) / (bars - 1) as f32;
            frame.rect(
                Vec2::new(x - 1.0, at.y),
                Vec2::new(3.0, height),
                Color::from_rgb(92, 88, 108),
            );
        }
        if closed {
            frame.rect(
                Vec2::new(at.x, at.y + box_.h * 0.5),
                Vec2::new(box_.w, 3.0),
                Color::from_rgb(74, 70, 88),
            );
        }
    }

    for (_, (pos, _)) in sim.world.query::<(&Position, &Sign)>().iter() {
        strip(frame, "props/sign", 0, props::SIGN_SIZE, pos.0 - offset);
    }
    for (_, (pos, lever)) in sim.world.query::<(&Position, &Lever)>().iter() {
        let thrown = sim.flag(&lever.flag) != 0;
        strip(
            frame,
            "props/lever",
            thrown as u32,
            props::LEVER_SIZE,
            pos.0 - offset,
        );
    }
    for (_, (pos, chest)) in sim.world.query::<(&Position, &Chest)>().iter() {
        let open = sim.flag(&chest.flag) != 0;
        strip(
            frame,
            "props/chest",
            open as u32,
            props::CHEST_SIZE,
            pos.0 - offset,
        );
    }

    // Lit is "this is where you would come back to", read off the sim rather
    // than remembered here: the flame follows the respawn point, whatever
    // moved it.
    let respawn = sim.respawn_point();
    let player = sim
        .stats
        .get("player")
        .map(|p| p.size())
        .unwrap_or_default();
    for (_, (pos, size, _)) in sim.world.query::<(&Position, &Size, &Checkpoint)>().iter() {
        let spot = Vec2::new(
            pos.0.x + size.0.x / 2.0 - player.x / 2.0,
            pos.0.y + size.0.y - player.y,
        );
        let lit = spot.distance_squared(respawn) < 1.0;
        let index = if lit {
            1 + (sim.world_tick() / 8 % 2) as u32
        } else {
            0
        };
        strip(
            frame,
            "props/checkpoint",
            index,
            props::CHECKPOINT_SIZE,
            pos.0 - offset,
        );
    }
}

/// Spikes and fires. Placeholder shapes — the original spike art was never
/// committed — but a fire's two states have to be unmistakable at a glance: an
/// invisible hazard is the bug this whole feature is most likely to ship.
fn hazards(frame: &mut Frame, sim: &Sim, offset: Vec2) {
    const SPIKE_W: f32 = 8.0;
    for hazard in &sim.level.hazards {
        let mut x = hazard.x;
        while x + SPIKE_W <= hazard.right() + 0.5 {
            frame.poly(
                vec![
                    Vec2::new(x, hazard.bottom()) - offset,
                    Vec2::new(x + SPIKE_W, hazard.bottom()) - offset,
                    Vec2::new(x + SPIKE_W / 2.0, hazard.y) - offset,
                ],
                Color::from_rgb(220, 220, 230),
            );
            x += SPIKE_W;
        }
    }

    for (entity, (pos, fire)) in sim.world.query::<(&Position, &Fire)>().iter() {
        let box_ = fire.collider.aabb(pos.0);
        let (left, right) = (box_.x - offset.x, box_.right() - offset.x);
        let (top, bottom) = (box_.y - offset.y, box_.bottom() - offset.y);
        let mid = (left + right) / 2.0;
        if crate::systems::hazard::is_lit(&sim.world, entity) {
            // A full-height flame in two layers, flickering on the world's
            // clock so it reads as fire and not as an orange triangle.
            let lick = if (sim.world_tick() / 6).is_multiple_of(2) {
                1.5
            } else {
                -1.5
            };
            frame.poly(
                vec![
                    Vec2::new(left, bottom),
                    Vec2::new(right, bottom),
                    Vec2::new(mid + lick, top),
                ],
                Color::from_rgb(240, 120, 30),
            );
            frame.poly(
                vec![
                    Vec2::new(left + box_.w * 0.25, bottom),
                    Vec2::new(right - box_.w * 0.25, bottom),
                    Vec2::new(mid - lick, top + box_.h * 0.35),
                ],
                Color::from_rgb(255, 226, 120),
            );
        } else {
            // Out: a low bed of coals, dark and only a few pixels tall. Same
            // footprint, nothing like the same silhouette.
            frame.rect(
                Vec2::new(left, bottom - 4.0),
                Vec2::new(box_.w, 4.0),
                Color::from_rgb(90, 40, 35),
            );
            frame.rect(
                Vec2::new(left + 2.0, bottom - 2.0),
                Vec2::new(box_.w - 4.0, 2.0),
                Color::from_rgb(150, 60, 35),
            );
        }
    }
}

/// Moving platforms, drawn from the collider the physics uses rather than
/// from the path — so if the two ever disagree, the platform visibly leaves
/// its own collision box behind instead of hiding the bug.
fn movers(frame: &mut Frame, sim: &Sim, offset: Vec2) {
    for (_, (pos, collider)) in sim
        .world
        .query::<(&Position, &Collider)>()
        .with::<&Mover>()
        .iter()
    {
        let box_ = collider.aabb(pos.0);
        let at = (Vec2::new(box_.x, box_.y) - offset).floor();
        frame.rect(at, Vec2::new(box_.w, box_.h), Color::from_rgb(96, 88, 104));
        // A bright lip along the walkable surface: the top edge is the only
        // part of a platform the player is really aiming at.
        frame.rect(
            at,
            Vec2::new(box_.w, 3.0_f32.min(box_.h)),
            Color::from_rgb(196, 186, 210),
        );
    }
}

/// Swinging hazards: a chain to the anchor and a spiked ball on the end. The
/// ball's radius is derived from its collider — the lethal box is the square
/// inscribed in the circle — so there is no second number that could be
/// edited out of step with the first.
fn swings(frame: &mut Frame, sim: &Sim, offset: Vec2) {
    for (_, (pos, collider, pendulum)) in sim
        .world
        .query::<(&Position, &Collider, &Pendulum)>()
        .iter()
    {
        let anchor = (pendulum.anchor - offset).floor();
        let centre = (pos.0 - offset).floor();
        let radius = collider.size.x * std::f32::consts::SQRT_2 / 2.0;

        frame.line(anchor, centre, 2.0, Color::from_rgb(120, 116, 128));
        frame.rect(
            anchor - Vec2::splat(4.0),
            Vec2::splat(8.0),
            Color::from_rgb(80, 76, 88),
        );
        // Spikes first, so the ball is drawn over their roots.
        const SPIKES: usize = 8;
        for i in 0..SPIKES {
            let angle = std::f32::consts::TAU * i as f32 / SPIKES as f32;
            let out = Vec2::new(angle.cos(), angle.sin());
            let across = Vec2::new(-out.y, out.x) * radius * 0.4;
            frame.poly(
                vec![
                    centre + out * radius * 1.55,
                    centre + across,
                    centre - across,
                ],
                Color::from_rgb(190, 186, 200),
            );
        }
        frame.circle(centre, radius, Color::from_rgb(64, 60, 72));
        // A highlight up and to the left, so the ball reads as a sphere and not
        // as a hole in the level.
        frame.circle(
            centre - Vec2::splat(radius * 0.3),
            radius * 0.35,
            Color::from_rgb(120, 114, 132),
        );
    }
}

/// Items lying on the floor, drawn with their own icons from the collider
/// they are collected on.
fn pickups(frame: &mut Frame, sim: &Sim, offset: Vec2) {
    for (_, (pos, pickup)) in sim.world.query::<(&Position, &Pickup)>().iter() {
        let at = (pos.0 - offset).floor();
        crate::scenes::inventory::icon(frame, &sim.items, &pickup.item, at);
    }
}

/// Everything with a sprite, back to front: NPCs, then the player, then
/// anything thrown — sorted rather than drawn in hecs's archetype order, which
/// shuffles as components come and go and would make who is in front of whom
/// flicker.
fn sprites(frame: &mut Frame, sim: &Sim, offset: Vec2) {
    let mut order: Vec<(u8, u32, hecs::Entity)> = sim
        .world
        .query::<&Sprite>()
        .iter()
        .map(|(entity, _)| {
            let layer = if sim.world.get::<&Projectile>(entity).is_ok() {
                2
            } else if sim.world.get::<&Avatar>(entity).is_ok() {
                1
            } else {
                0
            };
            (layer, entity.id(), entity)
        })
        .collect();
    order.sort_unstable();

    for (_, _, entity) in order {
        let Ok(mut query) = sim
            .world
            .query_one::<(&Position, &Size, &Sprite, &AnimationState)>(entity)
        else {
            continue;
        };
        let Some((pos, size, sprite, anim)) = query.get() else {
            continue;
        };
        let Some(clip) = sprite.clips.clip(&anim.clip) else {
            continue;
        };
        let Some(&cell) = clip
            .frames
            .get(anim.frame.min(clip.frames.len().saturating_sub(1)))
        else {
            continue;
        };
        let facing = super::faces_right(&sim.world, entity);
        let dest = (sprite.draw_origin(pos.0, size.0, clip, facing) - offset).floor();
        frame.image(
            sprite.clips.sheet_of(clip),
            sprite.clips.frame_rect(clip, cell),
            dest,
            !facing,
            tint(sim, entity, sprite),
        );
    }
}

/// What colour to draw an entity's sprite: its art's own tint (a villager
/// drawn from the knight's sheets is recoloured in its clip set), times the
/// hit flash.
fn tint(sim: &Sim, entity: hecs::Entity, sprite: &Sprite) -> Color {
    let base = sprite.clips.tint();
    // The knight's art has no hurt animation, so without this a hit changes
    // nothing about it except a number. Driven off the tail of the i-frame
    // window so it flashes on the hit rather than for the whole invulnerable
    // period — and only while there are i-frames at all, or a kind whose
    // window is shorter than the flash would glow forever.
    let flashing = sim
        .world
        .get::<&Health>(entity)
        .is_ok_and(|h| h.iframes > 0 && h.iframes + FLASH_TICKS > h.iframe_ticks && !h.dead());
    if flashing {
        Color::new(base.r * 3.0, base.g * 3.0, base.b * 3.0, base.a)
    } else {
        base
    }
}

/// Everything a patroller's facing and a projectile's heading answer, in one
/// place for the renderer and the debug overlay alike.
pub(crate) fn facing(world: &hecs::World, entity: hecs::Entity) -> bool {
    if let Ok(avatar) = world.get::<&Avatar>(entity) {
        return avatar.facing_right;
    }
    if let Ok(patrol) = world.get::<&Patrol>(entity) {
        return patrol.dir >= 0.0;
    }
    // A bolt faces the way it flies.
    if let Ok(vel) = world.get::<&crate::ecs::components::Velocity>(entity) {
        return vel.0.x >= 0.0;
    }
    true
}
