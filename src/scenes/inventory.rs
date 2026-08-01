//! The inventory overlay. It draws, and it decides nothing.
//!
//! Every piece of state on this screen — which pane has focus, where the
//! selection is, what `confirm` does to the item under it — lives on [`Sim`],
//! in [`crate::systems::inventory`], because using a potion changes health and
//! that is simulation rather than presentation. This file reads that state and
//! turns it into rectangles. If anything here ever decides something, a tape
//! stops being able to see it, and the largest system in the game becomes
//! untestable in one commit.
//!
//! **Why this is not a [`crate::scenes::Scene`] on the stack.** A stack entry
//! would be a second copy of "the inventory is open" living beside the sim's
//! mode, and the two would eventually disagree — the sim would freeze the
//! world while the stack drew the level, or the reverse. Instead
//! [`crate::scenes::adventure::AdventureScene`] calls [`draw`] whenever the sim
//! says the mode is [`crate::sim::Mode::Inventory`]. One source of truth, and
//! the overlay is exactly as transparent as the mode is.
//!
//! Item art does not exist yet, so a row's icon is a coloured quad keyed off
//! the item's kind. The `ItemDef` still names a sprite, so content authored
//! today does not have to be rewritten the day `assets/graphics/items/` lands —
//! only this file does.

use ggez::glam::Vec2;
use ggez::graphics::{Canvas, Color, DrawParam, PxScale, Quad, Text, TextFragment};

use crate::assets::{ItemKind, ItemTable, Slot};
use crate::ecs::components::{Equipment, Inventory};
use crate::sim::Sim;
use crate::systems::inventory::{self, Pane};

/// A scrim over the frozen world: dark enough to read against, light enough
/// that the fight you stepped out of is still visible behind it.
const SCRIM: Color = Color::new(0.02, 0.02, 0.05, 0.72);
const PANEL: Color = Color::new(0.07, 0.06, 0.11, 0.95);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
/// The outline on whichever pane has focus. Without it, moving the selection
/// between panes looks like the selection disappearing.
const FOCUS: Color = Color::new(0.95, 0.85, 0.35, 1.0);
const SELECTION: Color = Color::new(0.24, 0.22, 0.34, 1.0);
const TEXT: Color = Color::new(0.90, 0.90, 0.95, 1.0);
const DIM_TEXT: Color = Color::new(0.52, 0.52, 0.62, 1.0);

/// Item icons, by kind, until there is art.
const WEAPON_TINT: Color = Color::new(0.70, 0.76, 0.86, 1.0);
const EQUIPMENT_TINT: Color = Color::new(0.86, 0.72, 0.34, 1.0);
const CONSUMABLE_TINT: Color = Color::new(0.85, 0.28, 0.32, 1.0);

const MARGIN: f32 = 28.0;
const PANE_GAP: f32 = 10.0;
const ROW_H: f32 = 14.0;
const ROW_PAD: f32 = 4.0;
const ICON: f32 = 8.0;
const TITLE_H: f32 = 16.0;
const FONT: f32 = 10.0;
const BORDER_W: f32 = 1.0;

/// What colour a coloured quad should be for this item.
///
/// Public because [`crate::scenes::adventure`] draws pickups lying on the
/// floor with the same palette — a red square on the ground and a red square
/// in the bag are the same potion, and that only reads if one function owns
/// the mapping.
pub fn tint(items: &ItemTable, id: &str) -> Color {
    match items.get(id).map(|def| &def.kind) {
        Some(ItemKind::Weapon { .. }) => WEAPON_TINT,
        Some(ItemKind::Equipment { .. }) => EQUIPMENT_TINT,
        Some(ItemKind::Consumable { .. }) => CONSUMABLE_TINT,
        None => DIM_TEXT,
    }
}

/// Draw the overlay over the frozen world. `view` is the internal canvas size.
pub fn draw(canvas: &mut Canvas, sim: &Sim, view: Vec2) {
    fill(canvas, Vec2::ZERO, view, SCRIM);

    let size = Vec2::new(view.x - MARGIN * 2.0, view.y - MARGIN * 2.0);
    let origin = Vec2::splat(MARGIN);
    bordered(canvas, origin, size, PANEL);

    label(
        canvas,
        origin + Vec2::new(ROW_PAD, ROW_PAD),
        "INVENTORY",
        TEXT,
    );
    label(
        canvas,
        origin + Vec2::new(size.x - 152.0, ROW_PAD),
        "up/down select  left/right pane  enter use",
        DIM_TEXT,
    );

    let pane_w = (size.x - PANE_GAP) / 2.0;
    let pane_size = Vec2::new(pane_w, size.y - TITLE_H - ROW_PAD);
    let bag_at = origin + Vec2::new(0.0, TITLE_H);
    let gear_at = bag_at + Vec2::new(pane_w + PANE_GAP, 0.0);

    let screen = sim.screen();
    draw_bag(canvas, sim, bag_at, pane_size, screen.pane == Pane::Bag);
    draw_gear(canvas, sim, gear_at, pane_size, screen.pane == Pane::Gear);
}

fn draw_bag(canvas: &mut Canvas, sim: &Sim, origin: Vec2, size: Vec2, focused: bool) {
    outline(canvas, origin, size, if focused { FOCUS } else { BORDER });
    label(canvas, origin + Vec2::splat(ROW_PAD), "Bag", DIM_TEXT);

    let Some(holder) = inventory::holder(&sim.world) else {
        return;
    };
    let Ok(bag) = sim.world.get::<&Inventory>(holder) else {
        return;
    };
    let gear = sim.world.get::<&Equipment>(holder).ok();
    let selection = sim.screen().selection;

    if bag.slots.is_empty() {
        label(
            canvas,
            origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H),
            "(empty)",
            DIM_TEXT,
        );
        return;
    }

    for (index, stack) in bag.slots.iter().enumerate() {
        let at = origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H * (index as f32 + 1.0));
        if focused && index == selection {
            fill(
                canvas,
                at - Vec2::new(ROW_PAD / 2.0, 1.0),
                Vec2::new(size.x - ROW_PAD, ROW_H),
                SELECTION,
            );
        }
        fill(
            canvas,
            at + Vec2::new(0.0, 1.0),
            Vec2::splat(ICON),
            tint(&sim.items, &stack.id),
        );

        // The worn marker matters: an equipped weapon is still listed here
        // (it came out of the bag, so it is not — but a stack of two swords
        // with one worn is), and "why can I not equip this again" needs an
        // answer on screen.
        let worn = gear.as_ref().is_some_and(|gear| gear.holds(&stack.id));
        let text = format!(
            "{}{}{}",
            sim.items.label(&stack.id),
            if stack.count > 1 {
                format!(" x{}", stack.count)
            } else {
                String::new()
            },
            if worn { "  [worn]" } else { "" },
        );
        label(canvas, at + Vec2::new(ICON + 4.0, 0.0), &text, TEXT);
    }
}

fn draw_gear(canvas: &mut Canvas, sim: &Sim, origin: Vec2, size: Vec2, focused: bool) {
    outline(canvas, origin, size, if focused { FOCUS } else { BORDER });
    label(canvas, origin + Vec2::splat(ROW_PAD), "Worn", DIM_TEXT);

    // Cloned rather than borrowed: the borrow would be held across the whole
    // loop below, and a draw pass has no business pinning a component.
    let gear = inventory::holder(&sim.world)
        .and_then(|holder| sim.world.get::<&Equipment>(holder).ok())
        .map(|gear| Equipment::clone(&gear));
    let selection = sim.screen().selection;

    // Every slot, filled or not: an empty slot has to be visible, or "what can
    // I still put on?" has no answer.
    for (index, slot) in Slot::ALL.iter().enumerate() {
        let at = origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H * (index as f32 + 1.0));
        if focused && index == selection {
            fill(
                canvas,
                at - Vec2::new(ROW_PAD / 2.0, 1.0),
                Vec2::new(size.x - ROW_PAD, ROW_H),
                SELECTION,
            );
        }

        let worn = gear.as_ref().and_then(|gear| gear.get(*slot));
        if let Some(id) = worn {
            fill(
                canvas,
                at + Vec2::new(0.0, 1.0),
                Vec2::splat(ICON),
                tint(&sim.items, id),
            );
        }
        let text = match worn {
            Some(id) => format!("{}: {}", slot.label(), sim.items.label(id)),
            None => format!("{}: —", slot.label()),
        };
        label(
            canvas,
            at + Vec2::new(ICON + 4.0, 0.0),
            &text,
            if worn.is_some() { TEXT } else { DIM_TEXT },
        );
    }
}

fn label(canvas: &mut Canvas, at: Vec2, text: &str, colour: Color) {
    let text = Text::new(
        TextFragment::new(text)
            .scale(PxScale::from(FONT))
            .color(colour),
    );
    canvas.draw(&text, DrawParam::default().dest(at.floor()));
}

/// A filled box with a one-pixel border, the same treatment the HUD's bars use.
fn bordered(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    fill(
        canvas,
        origin - Vec2::splat(BORDER_W),
        size + Vec2::splat(BORDER_W * 2.0),
        BORDER,
    );
    fill(canvas, origin, size, colour);
}

/// Four thin rectangles rather than a stroked mesh: a `Quad` needs no context.
fn outline(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    fill(canvas, origin, Vec2::new(size.x, BORDER_W), colour);
    fill(
        canvas,
        origin + Vec2::new(0.0, size.y - BORDER_W),
        Vec2::new(size.x, BORDER_W),
        colour,
    );
    fill(canvas, origin, Vec2::new(BORDER_W, size.y), colour);
    fill(
        canvas,
        origin + Vec2::new(size.x - BORDER_W, 0.0),
        Vec2::new(BORDER_W, size.y),
        colour,
    );
}

fn fill(canvas: &mut Canvas, origin: Vec2, size: Vec2, colour: Color) {
    canvas.draw(
        &Quad,
        DrawParam::new()
            .dest(origin.floor())
            .scale(size)
            .color(colour),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three kinds have to be told apart at a glance, on the floor and in
    /// the bag alike — which is the whole of what a coloured quad can convey.
    #[test]
    fn every_item_kind_gets_a_distinct_placeholder_colour() {
        let items = ItemTable::shipped();
        let channels = |c: Color| [c.r, c.g, c.b];
        let mut seen: Vec<[f32; 3]> = Vec::new();
        for id in items.ids() {
            let colour = channels(tint(&items, id));
            if !seen.contains(&colour) {
                seen.push(colour);
            }
        }
        assert!(
            seen.len() >= 3,
            "the shipped items should span at least the three kinds: {seen:?}"
        );
        assert_ne!(
            channels(tint(&items, "no_such_item")),
            channels(CONSUMABLE_TINT),
            "an unknown id should not be mistaken for a potion"
        );
    }
}
