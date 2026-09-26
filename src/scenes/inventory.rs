//! The inventory overlay. It draws, and it decides nothing.
//!
//! Which pane has focus, where the selection is, and what `confirm` does to
//! the thing under it all live on [`Sim`], in [`crate::systems::inventory`],
//! because drinking a potion changes health and wearing a helm changes how
//! much of it you can have. This file reads that state and turns it into a
//! picture.
//!
//! Like [`crate::scenes::dialogue`], this is not a stack entry: the sim's mode
//! is the one record of "the bag is open", and [`crate::view::View`] draws
//! this whenever it says so.

use ggez::glam::Vec2;

use crate::assets::{ItemTable, Slot};
use crate::ecs::components::{Equipment, Inventory};
use crate::render::{font, Color, Frame, Rect};
use crate::sim::Sim;
use crate::systems::inventory::{self, Pane};

/// Much darker than the dialogue scrim: the bag is a screen of its own, and
/// the world behind it is only there to say it is paused.
const SCRIM: Color = Color::new(0.02, 0.02, 0.05, 0.72);
const PANEL: Color = Color::new(0.07, 0.06, 0.11, 0.95);
const BORDER: Color = Color::new(0.55, 0.55, 0.68, 1.0);
const FOCUS: Color = Color::new(0.95, 0.85, 0.35, 1.0);
const SELECTION: Color = Color::new(0.24, 0.22, 0.34, 1.0);
const SLOT: Color = Color::new(0.12, 0.11, 0.18, 1.0);
const TEXT: Color = Color::new(0.90, 0.90, 0.95, 1.0);
const DIM_TEXT: Color = Color::new(0.52, 0.52, 0.62, 1.0);

const MARGIN: f32 = 28.0;
const PANE_GAP: f32 = 10.0;
const ROW_H: f32 = 16.0;
const ROW_PAD: f32 = 4.0;
const TITLE_H: f32 = 16.0;
/// Room at the bottom of the panel for the selected item's description.
const FOOTER_H: f32 = 26.0;
const HINT: &str = "up/down select   left/right pane   enter use   I or Esc close";

/// Draw an item's icon with its top-left at `at`.
///
/// Every item names a sprite, `tests/data.rs` insists it exists and is
/// [`crate::ecs::spawn::PICKUP_SIZE`] square, and this draws its first frame —
/// the same picture the item shows lying on the floor.
pub fn icon(frame: &mut Frame, items: &ItemTable, id: &str, at: Vec2) {
    let size = crate::ecs::spawn::PICKUP_SIZE;
    match items.get(id) {
        Some(def) => frame.image(
            &def.sprite,
            Rect::new(0.0, 0.0, size.x, size.y),
            at.floor(),
            false,
            Color::WHITE,
        ),
        // Content naming an item nothing defines: visible, not blank.
        None => frame.rect(at, size, DIM_TEXT),
    }
}

pub fn draw(frame: &mut Frame, sim: &Sim, view: Vec2) {
    frame.rect(Vec2::ZERO, view, SCRIM);

    let size = view - Vec2::splat(MARGIN * 2.0);
    let origin = Vec2::splat(MARGIN);
    frame.panel(origin, size, PANEL, BORDER);

    frame.text(origin + Vec2::splat(ROW_PAD), "INVENTORY", TEXT);
    // Right-aligned by its measured width, so it can never run off the panel.
    frame.text(
        Vec2::new(
            origin.x + size.x - ROW_PAD - font::width(HINT),
            origin.y + ROW_PAD,
        ),
        HINT,
        DIM_TEXT,
    );

    let pane_w = (size.x - PANE_GAP) / 2.0;
    let pane_size = Vec2::new(pane_w, size.y - TITLE_H - ROW_PAD - FOOTER_H);
    let bag_at = origin + Vec2::new(0.0, TITLE_H);
    let gear_at = bag_at + Vec2::new(pane_w + PANE_GAP, 0.0);

    let screen = sim.screen();
    let selected = match screen.pane {
        Pane::Bag => draw_bag(frame, sim, bag_at, pane_size, true),
        Pane::Gear => {
            draw_bag(frame, sim, bag_at, pane_size, false);
            None
        }
    };
    let worn = draw_gear(frame, sim, gear_at, pane_size, screen.pane == Pane::Gear);
    let about = selected.or(worn);

    // What the thing under the selection is, in the item's own words.
    if let Some(def) = about.and_then(|id| sim.items.get(&id)) {
        let footer = Vec2::new(origin.x + ROW_PAD, origin.y + size.y - FOOTER_H + 2.0);
        frame.text(footer, &def.name, FOCUS);
        let line = font::wrap(&def.description, size.x - ROW_PAD * 2.0)
            .into_iter()
            .next()
            .unwrap_or_default();
        frame.text(footer + Vec2::new(0.0, font::LINE_H), &line, DIM_TEXT);
    }
}

/// The bag pane. Returns the id under the selection, if it has focus.
fn draw_bag(
    frame: &mut Frame,
    sim: &Sim,
    origin: Vec2,
    size: Vec2,
    focused: bool,
) -> Option<String> {
    frame.outline(origin, size, 1.0, if focused { FOCUS } else { BORDER });
    frame.text(origin + Vec2::splat(ROW_PAD), "Bag", DIM_TEXT);

    let holder = inventory::holder(&sim.world)?;
    let bag = sim.world.get::<&Inventory>(holder).ok()?;
    let gear = sim.world.get::<&Equipment>(holder).ok();
    let selection = sim.screen().selection;

    if bag.slots.is_empty() {
        frame.text(
            origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H),
            "(empty)",
            DIM_TEXT,
        );
        return None;
    }

    let mut under = None;
    for (index, stack) in bag.slots.iter().enumerate() {
        let at = origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H * (index as f32 + 1.0));
        if focused && index == selection {
            frame.rect(
                at - Vec2::new(ROW_PAD / 2.0, 2.0),
                Vec2::new(size.x - ROW_PAD, ROW_H),
                SELECTION,
            );
            under = Some(stack.id.clone());
        }
        frame.rect(at - Vec2::splat(1.0), Vec2::splat(14.0), SLOT);
        icon(frame, &sim.items, &stack.id, at);

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
        frame.text(at + Vec2::new(18.0, 2.0), &text, TEXT);
    }
    under
}

/// The equipment pane. Returns the id worn in the selected slot, if it has
/// focus and there is one.
fn draw_gear(
    frame: &mut Frame,
    sim: &Sim,
    origin: Vec2,
    size: Vec2,
    focused: bool,
) -> Option<String> {
    frame.outline(origin, size, 1.0, if focused { FOCUS } else { BORDER });
    frame.text(origin + Vec2::splat(ROW_PAD), "Worn", DIM_TEXT);

    let gear = inventory::holder(&sim.world)
        .and_then(|holder| sim.world.get::<&Equipment>(holder).ok())
        .map(|gear| Equipment::clone(&gear));
    let selection = sim.screen().selection;

    let mut under = None;
    for (index, slot) in Slot::ALL.iter().enumerate() {
        let at = origin + Vec2::new(ROW_PAD, ROW_PAD + ROW_H * (index as f32 + 1.0));
        let worn = gear.as_ref().and_then(|gear| gear.get(*slot));
        if focused && index == selection {
            frame.rect(
                at - Vec2::new(ROW_PAD / 2.0, 2.0),
                Vec2::new(size.x - ROW_PAD, ROW_H),
                SELECTION,
            );
            under = worn.map(str::to_string);
        }
        frame.rect(at - Vec2::splat(1.0), Vec2::splat(14.0), SLOT);
        if let Some(id) = worn {
            icon(frame, &sim.items, id, at);
        }
        let text = match worn {
            Some(id) => format!("{}: {}", slot.label(), sim.items.label(id)),
            None => format!("{}: -", slot.label()),
        };
        frame.text(
            at + Vec2::new(18.0, 2.0),
            &text,
            if worn.is_some() { TEXT } else { DIM_TEXT },
        );
    }
    under
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::VIEW;

    /// The key hint is right-aligned by its measured width, so it starts
    /// inside the panel and ends inside it — it used to be placed by an
    /// estimate of letter width and ran off the edge of the screen.
    #[test]
    fn the_hint_fits_inside_the_panel() {
        let size = VIEW - Vec2::splat(MARGIN * 2.0);
        let start = MARGIN + size.x - ROW_PAD - font::width(HINT);
        assert!(
            start > MARGIN + font::width("INVENTORY") + 8.0,
            "overlaps the title"
        );
        assert!(start + font::width(HINT) <= MARGIN + size.x);
    }
}
