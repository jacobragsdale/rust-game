//! Items: what you are carrying, what you are wearing, what that makes you
//! worth, and the screen you rearrange it on.
//!
//! **Why all of this is `Sim` and none of it is a scene.** The inventory
//! screen looks like UI and is not. Drinking a potion changes health; equipping
//! a helm changes maximum health; equipping a sword changes what the combo
//! does to a knight. Every one of those decides whether the next fight is
//! winnable, which makes them simulation wearing a menu's clothes. Put the
//! selection index in a scene and the largest system in the game becomes
//! untestable in one move — no tape can press a key that only a `ggez::Context`
//! can see. So the selection, the focused pane, and every consequence of
//! pressing `confirm` live here, driven by the same [`Action`]s a player
//! presses, and [`crate::scenes::inventory`] draws the result and decides
//! nothing. M5's dialogue inherits this arrangement, which is why it is set up
//! carefully on the simpler of the two.
//!
//! **Stats are `base + sum(modifiers)`, always.** [`effective`] rebuilds the
//! whole stat block from the base and whatever is equipped, and
//! [`derive_stats`] runs it once per tick into
//! [`DerivedStats`], which is what every other system reads. Nothing anywhere
//! adds 2 to a maximum and nothing anywhere subtracts it again — the second of
//! those is the one that eventually disagrees with the first, and the symptom
//! is a player whose maximum health is quietly wrong forever. PLAN.md sets the
//! rule; this module is where it is kept.
//!
//! **Pickups may be despawned. NPCs may not.** The never-despawn rule exists
//! because NPCs are addressed by *spawn index*: a tape says `knight.0`, and
//! removing a knight silently renumbers every later one, turning a passing
//! assertion into a passing assertion about a different entity. A pickup is
//! not addressed at all — a trace records how many exist and what was picked
//! up, never which entity it was — so nothing can be repointed by one going
//! away. Leaving collected items in the world as invisible husks would instead
//! be a permanent leak, one per drop. This is the same distinction
//! [`crate::systems::spell`] draws for projectiles, and for the same reason.
//!
//! Three places in the tick, and each is where it is on purpose:
//!
//! - [`collect_pickups`] runs in the resolve phase beside `combat::resolve`,
//!   once everything has finished moving — so walking over something is tested
//!   where the bodies actually ended the tick.
//! - [`drop_loot`] runs immediately after `combat::settle_dead`, on the tick
//!   the corpse died.
//! - [`derive_stats`] runs at the very end of [`crate::sim::Sim::step`], and
//!   *outside* the mode dispatch. It is not the world advancing — it
//!   recomputes a view of state that has already changed — so it is the one
//!   thing that still runs while a modal screen is up, which it has to be,
//!   because that screen is the only place equipment ever changes. Running it
//!   after the dispatch rather than before means the tick you put a helm on
//!   ends with the new maximum showing, and the controllers at the top of the
//!   next tick read a block that is already correct.

use std::sync::Arc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{ItemDef, ItemEffect, ItemKind, ItemTable, Slot, StatBlock, StatModifier};
use crate::ecs::components::{
    DerivedStats, Equipment, Health, Inventory, Loot, Mana, Pickup, Position, Size, Stats, Team,
};
use crate::ecs::spawn;
use crate::physics::Aabb;
use crate::sim::event::GameEvent;
use crate::sim::rng::Rng;
use crate::systems::input::{Action, ActionSet, PlayerInput};

/// Horizontal gap between two things dropped by the same corpse, in pixels.
///
/// Deterministic rather than scattered: a random spread would be one more
/// thing drawing from the seeded generator, and "which of the two drops did I
/// walk over first" is not a question worth making random.
const DROP_SPACING: f32 = 16.0;

// ---------------------------------------------------------------------------
// Derived stats
// ---------------------------------------------------------------------------

/// `base + sum(modifiers of everything currently equipped)`.
///
/// Pure, total, and cheap in the common case: with nothing equipped the base
/// `Arc` is handed straight back, so the overwhelming majority of entities and
/// ticks allocate nothing at all.
///
/// The order of the sum is fixed by [`Equipment`]'s `BTreeMap`. That matters
/// for the `f32` terms, where addition is not associative — summing the same
/// modifiers in a different order can give a different last bit, and a golden
/// trace would notice.
pub fn effective(
    base: &Arc<StatBlock>,
    equipment: &Equipment,
    items: &ItemTable,
) -> Arc<StatBlock> {
    if equipment.is_empty() {
        return base.clone();
    }

    let mut out = (**base).clone();
    for id in equipment.slots.values() {
        let Some(def) = items.get(id) else {
            continue;
        };
        match &def.kind {
            ItemKind::Weapon { damage, combo, .. } => {
                out.damage_bonus += damage;
                // The weapon's chain replaces the opener; the rest of it
                // follows `chain` in the attack table from there.
                if let Some(opener) = combo.first() {
                    out.attack.clone_from(opener);
                }
            }
            ItemKind::Equipment { modifiers, .. } => {
                for modifier in modifiers {
                    apply(&mut out, *modifier);
                }
            }
            // Not equippable, so it cannot be in a slot. Skipped rather than
            // rejected: the check that an item is wearable belongs at the
            // point it is put on, not at every read of it.
            ItemKind::Consumable { .. } => {}
        }
    }
    Arc::new(out)
}

fn apply(stats: &mut StatBlock, modifier: StatModifier) {
    match modifier {
        StatModifier::MaxHealth(amount) => stats.max_health += amount,
        StatModifier::MaxMana(amount) => stats.max_mana += amount,
        StatModifier::RunSpeed(amount) => stats.run_speed += amount,
        StatModifier::Damage(amount) => stats.damage_bonus += amount,
    }
}

/// Recompute [`DerivedStats`] for everything that can wear something, and push
/// the two derived numbers that are cached on components into them.
///
/// `Health::max` and `Mana::max` are caches, not state: they are *assigned*
/// the derived value every tick, never incremented or decremented, so they
/// cannot drift from it. Current values are clamped down and never raised —
/// putting a helm on at 3/5 leaves you at 3/7, which is the only reading that
/// does not turn equipment into a heal.
pub fn derive_stats(world: &mut World, items: &ItemTable) {
    for (_, (stats, equipment, derived, health, mana)) in world.query_mut::<(
        &Stats,
        &Equipment,
        &mut DerivedStats,
        Option<&mut Health>,
        Option<&mut Mana>,
    )>() {
        derived.0 = effective(&stats.0, equipment, items);

        if let Some(health) = health {
            health.max = derived.0.max_health;
            health.current = health.current.min(health.max);
        }
        if let Some(mana) = mana {
            mana.max = derived.0.max_mana;
            mana.current = mana.current.min(mana.max);
        }
    }
}

// ---------------------------------------------------------------------------
// Pickups and loot
// ---------------------------------------------------------------------------

/// Whoever the bag belongs to: the player-team entity carrying an
/// [`Inventory`]. Nothing else has one today, and nothing about this assumes
/// only one thing ever will — it takes the lowest entity id so the answer is
/// stable rather than whatever hecs iterated first.
pub fn holder(world: &World) -> Option<hecs::Entity> {
    world
        .query::<&Team>()
        .with::<&Inventory>()
        .iter()
        .filter(|(_, team)| **team == Team::Player)
        .map(|(entity, _)| entity)
        .min_by_key(|entity| entity.id())
}

/// Walk over an item and it goes in the bag.
///
/// Runs in the resolve phase, so the test is against where the bodies actually
/// finished the tick. A bag with no room leaves the item on the floor and says
/// so once — silence would be indistinguishable from the pickup not existing.
pub fn collect_pickups(world: &mut World, events: &mut Vec<GameEvent>) {
    let Some(holder) = holder(world) else {
        return;
    };
    let Some(reach) = entity_box(world, holder) else {
        return;
    };

    // Collected first because the loop below mutates the world, and sorted by
    // entity id because two drops in a heap must go into the bag in an order
    // that does not depend on hecs archetype iteration.
    let mut pickups: Vec<(hecs::Entity, String, u32, bool)> = world
        .query::<(&Pickup, &Position, &Size)>()
        .iter()
        .map(|(entity, (pickup, pos, size))| {
            let box_ = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
            (
                entity,
                pickup.item.clone(),
                pickup.count,
                reach.overlaps(&box_),
            )
        })
        .collect();
    pickups.sort_by_key(|(entity, ..)| entity.id());

    let mut collected: Vec<hecs::Entity> = Vec::new();
    for (entity, item, count, touching) in pickups {
        if !touching {
            // Stepping off re-arms the refusal, so making room and coming back
            // reports itself again.
            if let Ok(mut pickup) = world.get::<&mut Pickup>(entity) {
                pickup.refused = false;
            }
            continue;
        }

        let taken = world
            .get::<&mut Inventory>(holder)
            .map(|mut bag| bag.add(&item, count))
            .unwrap_or(false);

        if taken {
            events.push(GameEvent::PickedUp { item, count });
            collected.push(entity);
        } else if let Ok(mut pickup) = world.get::<&mut Pickup>(entity) {
            if !pickup.refused {
                pickup.refused = true;
                events.push(GameEvent::InventoryFull { item });
            }
        }
    }

    for entity in collected {
        // Allowed here, and for the reason set out in the module doc: nothing
        // addresses a pickup by index, so nothing can be renumbered by one
        // leaving.
        let _ = world.despawn(entity);
    }
}

/// Roll every dead thing's loot table once and put the results on the floor.
///
/// **The order of the rolls is part of the contract.** Corpses are taken in
/// entity-id order and each table is walked top to bottom, and every entry is
/// rolled whether or not it lands — so the sequence of numbers drawn from the
/// generator is a function of the tables alone. Rolling in hecs iteration
/// order, or skipping the roll for an entry that cannot drop, would make the
/// same seed produce different loot depending on which components happened to
/// have been added that run, which is exactly the kind of irreproducibility
/// tapes and golden traces exist to rule out.
pub fn drop_loot(world: &mut World, rng: &mut Rng) {
    let mut corpses: Vec<hecs::Entity> = world
        .query::<(&Health, &Loot)>()
        .iter()
        .filter(|(_, (health, loot))| health.dead() && !loot.dropped)
        .map(|(entity, _)| entity)
        .collect();
    corpses.sort_by_key(|entity| entity.id());

    for corpse in corpses {
        let Some((drops, origin, gravity, max_fall)) = drop_site(world, corpse) else {
            continue;
        };
        if let Ok(mut loot) = world.get::<&mut Loot>(corpse) {
            loot.dropped = true;
        }

        let mut placed = 0u32;
        for drop in &drops {
            let landed = rng.chance(drop.chance);
            if !landed || drop.count == 0 {
                continue;
            }
            let at = origin + Vec2::new(placed as f32 * DROP_SPACING, 0.0);
            spawn::pickup(world, &drop.item, drop.count, at, gravity, max_fall);
            placed += 1;
        }
    }
}

/// A corpse's table, where its drops start, and how heavily they fall.
///
/// Gravity is the corpse's own rather than a constant here: a drop should fall
/// the way the thing that dropped it did, and that is one fewer number to
/// invent.
fn drop_site(
    world: &World,
    corpse: hecs::Entity,
) -> Option<(Vec<crate::assets::LootDrop>, Vec2, f32, f32)> {
    let drops = world.get::<&Loot>(corpse).ok()?.drops.clone();
    let pos = world.get::<&Position>(corpse).ok()?.0;
    let size = world.get::<&Size>(corpse).ok()?.0;
    let stats = world.get::<&Stats>(corpse).ok()?.0.clone();
    let origin = Vec2::new(
        pos.x + (size.x - spawn::PICKUP_SIZE.x) / 2.0,
        pos.y + size.y - spawn::PICKUP_SIZE.y,
    );
    Some((drops, origin, stats.gravity, stats.max_fall))
}

/// How many items are lying in the world, for the trace.
///
/// A count rather than a probe each, for the reason `projectiles` is one: what
/// a tape wants to say is "the kill dropped something", and a line per item
/// per tick makes a trace unreadable long before it makes it informative.
pub fn pickup_count(world: &World) -> usize {
    world.query::<&Pickup>().iter().count()
}

fn entity_box(world: &World, entity: hecs::Entity) -> Option<Aabb> {
    let pos = world.get::<&Position>(entity).ok()?.0;
    let size = world.get::<&Size>(entity).ok()?.0;
    Some(Aabb::new(pos.x, pos.y, size.x, size.y))
}

// ---------------------------------------------------------------------------
// The screen, which is state and not drawing
// ---------------------------------------------------------------------------

/// Which half of the inventory screen the selection is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    /// What you are carrying.
    #[default]
    Bag,
    /// What you are wearing, one row per [`Slot`] whether or not it is filled
    /// — an empty slot has to be visible, or "what can I still put on?" is
    /// unanswerable.
    Gear,
}

impl Pane {
    /// How a tape spells it.
    pub fn name(self) -> &'static str {
        match self {
            Pane::Bag => "bag",
            Pane::Gear => "gear",
        }
    }
}

/// Everything the inventory screen remembers between ticks.
///
/// Kept across a close and reopen on purpose: coming back to where you left
/// off is what a player expects after stepping out to fight something, and
/// resetting to the top would make a bag of twelve things tedious to work
/// through. The selection is clamped on open rather than trusted, since the
/// bag may have shrunk in between.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub pane: Pane,
    pub selection: usize,
}

/// Advance the inventory screen by one tick. Returns whether it should close.
///
/// `newly_held` is this tick's directions minus last tick's. Directions are
/// level-triggered because movement needs them held, and a menu needs one step
/// per press — so the difference is taken rather than the raw state. This is
/// *not* a second [`crate::systems::input::InputLatch`]: that one exists
/// because presses and rendered frames are on different clocks and is upstream
/// of everything here. This one exists because a held key is not a repeat.
pub fn step_screen(
    world: &mut World,
    items: &ItemTable,
    screen: &mut Screen,
    input: PlayerInput,
    newly_held: ActionSet,
    events: &mut Vec<GameEvent>,
) -> bool {
    // Both close it: the key that opened it toggles, and cancel is what a
    // player reaches for.
    if input.pressed(Action::Inventory) || input.pressed(Action::Cancel) {
        return true;
    }

    if newly_held.contains(Action::Left) {
        screen.pane = Pane::Bag;
    } else if newly_held.contains(Action::Right) {
        screen.pane = Pane::Gear;
    }

    let rows = row_count(world, screen.pane);
    if newly_held.contains(Action::Up) {
        screen.selection = screen.selection.saturating_sub(1);
    }
    if newly_held.contains(Action::Down) {
        screen.selection += 1;
    }
    clamp(screen, rows);

    if input.pressed(Action::Confirm) {
        if let Some(holder) = holder(world) {
            confirm(world, items, screen, holder, events);
            // Using the last potion in a stack takes a row away underneath the
            // selection; equipping moves one from one pane to the other.
            clamp(screen, row_count(world, screen.pane));
        }
    }

    false
}

/// Pull the selection back inside the screen. Called on open as well as after
/// every change, because the bag can shrink while the screen is shut.
pub fn clamp_selection(world: &World, screen: &mut Screen) {
    clamp(screen, row_count(world, screen.pane));
}

fn clamp(screen: &mut Screen, rows: usize) {
    screen.selection = screen.selection.min(rows.saturating_sub(1));
}

/// How many rows the given pane has right now.
pub fn row_count(world: &World, pane: Pane) -> usize {
    match pane {
        Pane::Bag => holder(world)
            .and_then(|entity| {
                world
                    .get::<&Inventory>(entity)
                    .ok()
                    .map(|bag| bag.slots.len())
            })
            .unwrap_or(0),
        Pane::Gear => Slot::ALL.len(),
    }
}

fn confirm(
    world: &mut World,
    items: &ItemTable,
    screen: &Screen,
    holder: hecs::Entity,
    events: &mut Vec<GameEvent>,
) {
    match screen.pane {
        Pane::Bag => {
            let Some(id) = world.get::<&Inventory>(holder).ok().and_then(|bag| {
                bag.slots
                    .get(screen.selection)
                    .map(|stack| stack.id.clone())
            }) else {
                return;
            };
            let Some(def) = items.get(&id).cloned() else {
                return;
            };
            // One button, and the item decides what it means. There is nothing
            // a player could want `confirm` to do to a potion other than drink
            // it, or to a helm other than put it on.
            if def.is_consumable() {
                use_item(world, holder, &def, events);
            } else {
                equip(world, holder, &def, events);
            }
        }
        Pane::Gear => {
            let Some(slot) = Slot::ALL.get(screen.selection).copied() else {
                return;
            };
            unequip(world, holder, slot, events);
        }
    }
}

/// Spend one of `def` and apply what it does.
fn use_item(world: &mut World, holder: hecs::Entity, def: &ItemDef, events: &mut Vec<GameEvent>) {
    let ItemKind::Consumable { effects } = &def.kind else {
        return;
    };

    // Paid for first: an effect that cannot apply (no mana pool to top up) is
    // still a potion drunk, and the alternative — apply, then try to charge —
    // is how an item gets used for free.
    let spent = world
        .get::<&mut Inventory>(holder)
        .map(|mut bag| bag.remove(&def.id, 1))
        .unwrap_or(false);
    if !spent {
        return;
    }

    for effect in effects {
        match effect {
            ItemEffect::Heal(amount) => {
                if let Ok(mut health) = world.get::<&mut Health>(holder) {
                    // Clamped to the derived maximum, so a potion can never
                    // put you above what your equipment says you can hold.
                    health.current = (health.current + amount).min(health.max);
                }
            }
            ItemEffect::RestoreMana(amount) => {
                if let Ok(mut mana) = world.get::<&mut Mana>(holder) {
                    mana.current = (mana.current + amount).min(mana.max);
                }
            }
        }
    }

    events.push(GameEvent::ItemUsed {
        item: def.id.clone(),
    });
}

/// Move one of `def` out of the bag and into its slot, putting whatever was
/// already there back.
fn equip(world: &mut World, holder: hecs::Entity, def: &ItemDef, events: &mut Vec<GameEvent>) {
    let Some(slot) = def.slot() else {
        return;
    };
    let (Ok(mut bag), Ok(mut gear)) = (
        world.get::<&mut Inventory>(holder),
        world.get::<&mut Equipment>(holder),
    ) else {
        return;
    };
    if !bag.remove(&def.id, 1) {
        return;
    }

    if let Some(previous) = gear.slots.remove(&slot) {
        if !bag.add(&previous, 1) {
            // A full bag and nowhere for what was already worn. Put both back
            // rather than deleting one of them to make the swap fit — losing
            // an item silently is worse than refusing loudly.
            bag.add(&def.id, 1);
            gear.slots.insert(slot, previous.clone());
            events.push(GameEvent::InventoryFull { item: previous });
            return;
        }
        events.push(GameEvent::Unequipped {
            item: previous,
            slot,
        });
    }

    gear.slots.insert(slot, def.id.clone());
    events.push(GameEvent::Equipped {
        item: def.id.clone(),
        slot,
    });
}

/// Take whatever is in `slot` off and put it back in the bag.
fn unequip(world: &mut World, holder: hecs::Entity, slot: Slot, events: &mut Vec<GameEvent>) {
    let (Ok(mut bag), Ok(mut gear)) = (
        world.get::<&mut Inventory>(holder),
        world.get::<&mut Equipment>(holder),
    ) else {
        return;
    };
    let Some(id) = gear.get(slot).map(str::to_string) else {
        return;
    };
    if !bag.has_room_for(&id) {
        events.push(GameEvent::InventoryFull { item: id });
        return;
    }

    gear.slots.remove(&slot);
    bag.add(&id, 1);
    events.push(GameEvent::Unequipped { item: id, slot });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::StatTable;

    fn items() -> Arc<ItemTable> {
        ItemTable::shipped()
    }

    fn player_stats() -> Arc<StatBlock> {
        StatTable::shipped().get("player").unwrap()
    }

    fn worn(pairs: &[(Slot, &str)]) -> Equipment {
        let mut gear = Equipment::default();
        for (slot, id) in pairs {
            gear.slots.insert(*slot, (*id).to_string());
        }
        gear
    }

    // --- the bag ---------------------------------------------------------

    #[test]
    fn two_of_the_same_item_are_one_stack_of_two() {
        let mut bag = Inventory::new(4);
        assert!(bag.add("minor_potion", 1));
        assert!(bag.add("minor_potion", 1));
        assert_eq!(bag.used_slots(), 1, "one stack, not two");
        assert_eq!(bag.count("minor_potion"), 2);
    }

    #[test]
    fn capacity_counts_stacks_so_a_familiar_item_always_fits() {
        let mut bag = Inventory::new(2);
        assert!(bag.add("a", 1));
        assert!(bag.add("b", 1));
        assert!(bag.is_full());

        assert!(!bag.add("c", 1), "a third kind does not fit");
        assert_eq!(bag.count("c"), 0);
        assert!(
            bag.add("a", 5),
            "but more of something already carried always does"
        );
        assert_eq!(bag.count("a"), 6);
    }

    #[test]
    fn removing_the_last_of_a_stack_frees_its_slot() {
        let mut bag = Inventory::new(1);
        bag.add("a", 2);
        assert!(!bag.remove("a", 3), "cannot take more than is there");
        assert!(bag.remove("a", 2));
        assert_eq!(bag.used_slots(), 0);
        assert!(bag.add("b", 1), "the slot came back");
    }

    // --- derived stats ---------------------------------------------------

    #[test]
    fn nothing_equipped_derives_exactly_the_base() {
        let base = player_stats();
        let derived = effective(&base, &Equipment::default(), &items());
        assert_eq!(derived.max_health, base.max_health);
        assert!(
            Arc::ptr_eq(&derived, &base),
            "the base should be handed back rather than rebuilt"
        );
    }

    #[test]
    fn a_helm_raises_max_health_by_exactly_its_modifier() {
        let base = player_stats();
        let derived = effective(&base, &worn(&[(Slot::Head, "knight_helm")]), &items());
        assert_eq!(derived.max_health, base.max_health + 2);
        assert_eq!(base.max_health, 5, "and the base was not touched");
    }

    #[test]
    fn a_weapon_contributes_damage_and_replaces_the_combos_opener() {
        let base = player_stats();
        let table = items();
        let derived = effective(&base, &worn(&[(Slot::Weapon, "iron_sword")]), &table);
        let ItemKind::Weapon { damage, combo, .. } = &table.get("iron_sword").unwrap().kind else {
            panic!("iron_sword should be a weapon");
        };
        assert_eq!(derived.damage_bonus, base.damage_bonus + damage);
        assert_eq!(&derived.attack, &combo[0]);
    }

    /// The test the whole `DerivedStats` split exists for. Over a long random
    /// sequence of equips and unequips, the derived block must always equal
    /// `base + sum(modifiers of what is on right now)` — computed here the
    /// slow, obvious way. In-place mutation passes the first few steps of this
    /// and then drifts, which is precisely the bug it cannot be allowed to be.
    #[test]
    fn derived_stats_never_drift_over_random_equip_sequences() {
        let table = items();
        let base = player_stats();
        let equippable: Vec<&str> = table
            .ids()
            .into_iter()
            .filter(|id| table.get(id).is_some_and(|def| def.slot().is_some()))
            .collect();
        assert!(
            equippable.len() >= 2,
            "this test needs something to put on: {equippable:?}"
        );

        let mut rng = Rng::new(20_240_401);
        let mut gear = Equipment::default();

        for step in 0..1_000 {
            let id = *rng.pick(&equippable).expect("non-empty");
            let slot = table.get(id).unwrap().slot().unwrap();
            if rng.chance(0.5) {
                gear.slots.insert(slot, id.to_string());
            } else {
                gear.slots.remove(&slot);
            }

            let derived = effective(&base, &gear, &table);

            // The independent answer: start from the base and add up whatever
            // happens to be on, with no memory of what any earlier step did.
            let mut expected_health = base.max_health;
            let mut expected_mana = base.max_mana;
            let mut expected_speed = base.run_speed;
            let mut expected_damage = base.damage_bonus;
            for worn_id in gear.slots.values() {
                match &table.get(worn_id).unwrap().kind {
                    ItemKind::Weapon { damage, .. } => expected_damage += damage,
                    ItemKind::Equipment { modifiers, .. } => {
                        for modifier in modifiers {
                            match modifier {
                                StatModifier::MaxHealth(a) => expected_health += a,
                                StatModifier::MaxMana(a) => expected_mana += a,
                                StatModifier::RunSpeed(a) => expected_speed += a,
                                StatModifier::Damage(a) => expected_damage += a,
                            }
                        }
                    }
                    ItemKind::Consumable { .. } => {}
                }
            }

            assert_eq!(derived.max_health, expected_health, "step {step}");
            assert_eq!(derived.max_mana, expected_mana, "step {step}");
            assert_eq!(derived.run_speed, expected_speed, "step {step}");
            assert_eq!(derived.damage_bonus, expected_damage, "step {step}");

            // And with nothing on, it is the base to the last bit.
            if gear.is_empty() {
                assert_eq!(derived.max_health, base.max_health, "step {step}");
                assert_eq!(derived.run_speed, base.run_speed, "step {step}");
            }
        }
    }

    /// A hundred cycles of the same helm, which is the shape the ticket names.
    #[test]
    fn a_hundred_equip_unequip_cycles_leave_no_residue() {
        let table = items();
        let base = player_stats();
        let mut gear = Equipment::default();

        for _ in 0..100 {
            gear.slots.insert(Slot::Head, "knight_helm".to_string());
            assert_eq!(
                effective(&base, &gear, &table).max_health,
                base.max_health + 2
            );
            gear.slots.remove(&Slot::Head);
            assert_eq!(effective(&base, &gear, &table).max_health, base.max_health);
        }
    }

    /// An id no item file defines contributes nothing rather than panicking.
    /// `tests/data.rs` is where a typo in content is caught; the simulation's
    /// job is to keep running.
    #[test]
    fn an_unknown_equipped_id_contributes_nothing() {
        let base = player_stats();
        let derived = effective(&base, &worn(&[(Slot::Head, "no_such_item")]), &items());
        assert_eq!(derived.max_health, base.max_health);
    }

    // --- through a real sim ----------------------------------------------

    use crate::assets::LootDrop;
    use crate::ecs::components::{Health, Loot, Position};
    use crate::sim::{GameEvent, Mode, Sim};
    use crate::systems::input::Action;

    const OPEN_BAG: PlayerInput = PlayerInput::from_actions(&[Action::Inventory]);
    const CONFIRM: PlayerInput = PlayerInput::from_actions(&[Action::Confirm]);
    const DOWN: PlayerInput = PlayerInput::from_actions(&[Action::Down]);

    /// A settled player on flat ground, with `extra` rows of grid above.
    fn grounded_sim() -> Sim {
        let mut sim = Sim::fixture(&[
            "....................",
            "..P.................",
            "####################",
        ]);
        for _ in 0..30 {
            sim.step(PlayerInput::default());
            if sim.probe().grounded {
                break;
            }
        }
        assert!(sim.probe().grounded);
        sim
    }

    fn player(sim: &Sim) -> hecs::Entity {
        holder(&sim.world).expect("the fixture player carries a bag")
    }

    /// Step until `pred` holds, or fail saying it never did.
    fn step_until(sim: &mut Sim, limit: u32, input: PlayerInput, pred: impl Fn(&Sim) -> bool) {
        for _ in 0..limit {
            sim.step(input);
            if pred(sim) {
                return;
            }
        }
        panic!("condition never held within {limit} ticks");
    }

    // --- pickups ----------------------------------------------------------

    #[test]
    fn walking_over_an_item_puts_it_in_the_bag_and_says_so() {
        let mut sim = grounded_sim();
        let at = sim.probe();
        spawn::pickup(
            &mut sim.world,
            "minor_potion",
            1,
            Vec2::new(at.x + 60.0, at.y),
            1400.0,
            900.0,
        );
        assert_eq!(pickup_count(&sim.world), 1);

        step_until(&mut sim, 120, PlayerInput::holding(&[Action::Right]), |s| {
            s.events().iter().any(|e| {
                matches!(e, GameEvent::PickedUp { item, count }
                    if item == "minor_potion" && *count == 1)
            })
        });

        assert_eq!(pickup_count(&sim.world), 0, "and it left the world");
        assert_eq!(
            sim.world
                .get::<&Inventory>(player(&sim))
                .unwrap()
                .count("minor_potion"),
            1
        );
    }

    /// Two of the same item are one stack of two, end to end.
    #[test]
    fn two_pickups_of_one_item_stack() {
        let mut sim = grounded_sim();
        let at = sim.probe();
        for offset in [40.0, 90.0] {
            spawn::pickup(
                &mut sim.world,
                "minor_potion",
                1,
                Vec2::new(at.x + offset, at.y),
                1400.0,
                900.0,
            );
        }

        step_until(&mut sim, 200, PlayerInput::holding(&[Action::Right]), |s| {
            s.probe().pickups == 0
        });

        let bag = sim.world.get::<&Inventory>(player(&sim)).unwrap();
        assert_eq!(bag.count("minor_potion"), 2);
        assert_eq!(bag.used_slots(), 1, "one stack, not two");
    }

    /// A full bag leaves the item where it is and reports why — once, not once
    /// per tick of standing on it.
    #[test]
    fn a_full_bag_refuses_a_pickup_and_leaves_it_in_the_world() {
        let mut sim = grounded_sim();
        let holder = player(&sim);
        {
            let mut bag = sim.world.get::<&mut Inventory>(holder).unwrap();
            for index in 0..bag.capacity {
                assert!(bag.add(&format!("filler_{index}"), 1));
            }
            assert!(bag.is_full());
        }

        let at = sim.probe();
        spawn::pickup(
            &mut sim.world,
            "minor_potion",
            1,
            Vec2::new(at.x + 60.0, at.y),
            1400.0,
            900.0,
        );

        let mut refusals = 0;
        for _ in 0..120 {
            sim.step(PlayerInput::holding(&[Action::Right]));
            refusals += sim
                .events()
                .iter()
                .filter(|e| matches!(e, GameEvent::InventoryFull { .. }))
                .count();
        }

        assert_eq!(pickup_count(&sim.world), 1, "still on the floor");
        assert_eq!(
            sim.world
                .get::<&Inventory>(holder)
                .unwrap()
                .count("minor_potion"),
            0
        );
        assert_eq!(refusals, 1, "reported once, not once per tick");
    }

    // --- loot -------------------------------------------------------------

    /// A corpse carrying this table, dropped into a settled sim.
    fn corpse_with(sim: &mut Sim, drops: Vec<LootDrop>) -> hecs::Entity {
        let at = sim.probe();
        let pos = Vec2::new(at.x + 200.0, at.y);
        let stats = StatTable::shipped().get("knight").unwrap();
        let mut health = Health::new(3, stats.iframe_ticks);
        health.current = 0;
        sim.world.spawn((
            health,
            Loot::new(drops),
            Position(pos),
            crate::ecs::components::Size(stats.size()),
            Stats(stats),
        ))
    }

    fn drop(item: &str, chance: f32) -> LootDrop {
        LootDrop {
            item: item.to_string(),
            count: 1,
            chance,
        }
    }

    #[test]
    fn a_certain_drop_always_lands_and_an_impossible_one_never_does() {
        let mut sim = grounded_sim();
        corpse_with(
            &mut sim,
            vec![drop("knight_helm", 1.0), drop("minor_potion", 0.0)],
        );
        sim.step(PlayerInput::default());

        let dropped: Vec<String> = sim
            .world
            .query::<&Pickup>()
            .iter()
            .map(|(_, p)| p.item.clone())
            .collect();
        assert_eq!(dropped, vec!["knight_helm".to_string()]);
    }

    /// A corpse rolls once, however long it lies there. Without the `dropped`
    /// flag this would produce a new helm every tick forever, and a corpse is
    /// never despawned.
    #[test]
    fn a_corpse_drops_once_and_not_once_per_tick() {
        let mut sim = grounded_sim();
        corpse_with(&mut sim, vec![drop("knight_helm", 1.0)]);
        for _ in 0..120 {
            sim.step(PlayerInput::default());
        }
        assert_eq!(pickup_count(&sim.world), 1);
    }

    /// Drops land on the floor rather than floating: they have a `Body`, so
    /// they fall through exactly the code the player falls through.
    #[test]
    fn drops_fall_to_the_ground() {
        let mut sim = grounded_sim();
        let corpse = corpse_with(&mut sim, vec![drop("knight_helm", 1.0)]);
        // Well above the floor, so falling is the only way down.
        sim.world.get::<&mut Position>(corpse).unwrap().0.y -= 90.0;
        sim.step(PlayerInput::default());

        let start = sim
            .world
            .query::<(&Pickup, &Position)>()
            .iter()
            .map(|(_, (_, pos))| pos.0.y)
            .next()
            .expect("something dropped");

        for _ in 0..120 {
            sim.step(PlayerInput::default());
        }
        let (resting, grounded) = sim
            .world
            .query::<(&Pickup, &Position, &crate::ecs::components::Body)>()
            .iter()
            .map(|(_, (_, pos, body))| (pos.0.y, body.grounded))
            .next()
            .expect("still there");
        assert!(resting > start, "it fell");
        assert!(grounded, "and landed rather than hovering");
    }

    /// The property the whole seeded-RNG rule exists for: the same seed and
    /// the same fight produce the same drops, every time.
    #[test]
    fn the_same_seed_always_drops_the_same_loot() {
        let roll = |seed: u64| -> Vec<String> {
            let mut sim = grounded_sim();
            sim.reseed(seed);
            corpse_with(
                &mut sim,
                vec![
                    drop("knight_helm", 0.5),
                    drop("minor_potion", 0.5),
                    drop("iron_sword", 0.5),
                ],
            );
            sim.step(PlayerInput::default());
            let mut dropped: Vec<String> = sim
                .world
                .query::<&Pickup>()
                .iter()
                .map(|(_, p)| p.item.clone())
                .collect();
            dropped.sort();
            dropped
        };

        assert_eq!(
            roll(crate::sim::DEFAULT_SEED),
            roll(crate::sim::DEFAULT_SEED)
        );
        // Not a claim that *every* other seed differs — that would be a claim
        // about the generator, which `rng.rs` already pins — only that the
        // seed is what the answer depends on.
        let sample: Vec<Vec<String>> = (0..40).map(roll).collect();
        assert!(
            sample.iter().any(|d| d != &sample[0]),
            "forty seeds all rolled the same loot: {:?}",
            sample[0]
        );
    }

    /// A quarter-chance entry lands about a quarter of the time. Not a test of
    /// the generator (`rng.rs` owns that) but of the plumbing between it and a
    /// loot table — a `chance` read as a fraction of something else, or rolled
    /// once for the whole table, would show up here.
    #[test]
    fn a_quarter_chance_lands_in_a_sane_band_over_a_hundred_rolls() {
        let mut landed = 0;
        for seed in 0..200u64 {
            let mut sim = grounded_sim();
            sim.reseed(seed);
            corpse_with(&mut sim, vec![drop("minor_potion", 0.25)]);
            sim.step(PlayerInput::default());
            landed += pickup_count(&sim.world);
        }
        assert!(
            (30..=70).contains(&landed),
            "{landed} of 200 quarter-chance rolls landed"
        );
    }

    /// Every entry is rolled whether or not it can land, so the sequence of
    /// numbers a table draws depends on the table and not on its outcomes.
    /// Otherwise adding a `chance: 0.0` line to a knight would silently change
    /// what every knight after it dropped.
    #[test]
    fn an_impossible_entry_still_consumes_its_roll() {
        let sample = |drops: Vec<LootDrop>| {
            let mut sim = grounded_sim();
            corpse_with(&mut sim, drops);
            sim.step(PlayerInput::default());
            sim.rng.clone()
        };

        let without = sample(vec![drop("knight_helm", 1.0)]);
        let with = sample(vec![drop("knight_helm", 1.0), drop("minor_potion", 0.0)]);
        assert_ne!(
            without, with,
            "the impossible entry was skipped instead of rolled"
        );
    }

    // --- the screen -------------------------------------------------------

    /// Open the screen on a sim whose player is carrying `stock`.
    fn screen_sim(stock: &[(&str, u32)]) -> Sim {
        let mut sim = grounded_sim();
        {
            let holder = holder(&sim.world).unwrap();
            let mut bag = sim.world.get::<&mut Inventory>(holder).unwrap();
            for (id, count) in stock {
                assert!(bag.add(id, *count));
            }
        }
        sim.step(OPEN_BAG);
        assert_eq!(sim.mode(), Mode::Inventory);
        sim
    }

    #[test]
    fn drinking_a_potion_heals_decrements_and_removes_the_empty_stack() {
        let mut sim = screen_sim(&[("minor_potion", 2)]);
        let holder = player(&sim);
        sim.world.get::<&mut Health>(holder).unwrap().current = 1;

        sim.step(CONFIRM);
        assert_eq!(sim.probe().hp, 3, "healed by the item's own number");
        assert_eq!(
            sim.events(),
            [GameEvent::ItemUsed {
                item: "minor_potion".to_string()
            }]
        );
        assert_eq!(
            sim.world
                .get::<&Inventory>(holder)
                .unwrap()
                .count("minor_potion"),
            1
        );

        sim.step(PlayerInput::default());
        sim.step(CONFIRM);
        assert_eq!(
            sim.world.get::<&Inventory>(holder).unwrap().used_slots(),
            0,
            "the empty stack left the bag"
        );
    }

    #[test]
    fn a_potion_cannot_heal_past_the_maximum() {
        let mut sim = screen_sim(&[("minor_potion", 1)]);
        let full = sim.probe().hp;
        sim.step(CONFIRM);
        assert_eq!(sim.probe().hp, full, "clamped, not overhealed");
        assert_eq!(sim.probe().hp, sim.probe().hp_max);
    }

    /// Equipping raises the maximum by exactly the modifier and clamps the
    /// current value rather than topping it up; unequipping puts it back with
    /// no drift.
    #[test]
    fn equipping_moves_the_maximum_and_clamps_rather_than_heals() {
        let mut sim = screen_sim(&[("knight_helm", 1)]);
        let holder = player(&sim);
        sim.world.get::<&mut Health>(holder).unwrap().current = 3;
        let base_max = sim.probe().hp_max;

        sim.step(CONFIRM);
        assert_eq!(sim.probe().hp_max, base_max + 2);
        assert_eq!(sim.probe().hp, 3, "equipment is not a heal");
        assert!(sim.events().contains(&GameEvent::Equipped {
            item: "knight_helm".to_string(),
            slot: Slot::Head
        }));

        // Fill up at the new maximum, then take it off: the maximum returns to
        // the base and the current value is clamped down to it.
        sim.world.get::<&mut Health>(holder).unwrap().current = base_max + 2;
        sim.step(PlayerInput::default());
        // Move to the worn pane and take it off.
        sim.step(PlayerInput::from_actions(&[Action::Right]));
        assert_eq!(sim.screen().pane, Pane::Gear);
        sim.step(CONFIRM);

        assert_eq!(sim.probe().hp_max, base_max);
        assert_eq!(sim.probe().hp, base_max, "clamped to the base maximum");
        assert_eq!(
            sim.world
                .get::<&Inventory>(holder)
                .unwrap()
                .count("knight_helm"),
            1,
            "and it went back in the bag"
        );
    }

    /// I-3's other acceptance criterion: the melee combo hits harder with a
    /// weapon, and the number comes out of the `ItemDef`.
    #[test]
    fn equipping_a_weapon_makes_the_combo_hit_harder() {
        let sword_damage = match &items().get("iron_sword").expect("shipped").kind {
            ItemKind::Weapon { damage, .. } => *damage,
            other => panic!("iron_sword should be a weapon, not {other:?}"),
        };

        let first_hit = |weapon: Option<&str>| -> i32 {
            // Knight in the neighbouring cell, so the opener connects without
            // the walk-in tuning its own timing into this test.
            let mut sim =
                Sim::fixture(&["................", "..PK............", "################"]);
            for _ in 0..30 {
                sim.step(PlayerInput::default());
                if sim.probe().grounded {
                    break;
                }
            }
            if let Some(id) = weapon {
                let holder = holder(&sim.world).unwrap();
                sim.world
                    .get::<&mut Equipment>(holder)
                    .unwrap()
                    .slots
                    .insert(Slot::Weapon, id.to_string());
                // Let the derived block catch up before the swing.
                sim.step(PlayerInput::default());
            }

            // Swing until something connects; the first blow that lands is the
            // one under test, whichever tick it falls on.
            for tick in 0..240 {
                let input = if tick % 30 == 0 {
                    PlayerInput::from_actions(&[Action::Attack])
                } else {
                    PlayerInput::default()
                };
                sim.step(input);
                if let Some(amount) = sim.events().iter().find_map(|e| match e {
                    GameEvent::Damaged { who, amount, .. } if who == "knight" => Some(*amount),
                    _ => None,
                }) {
                    return amount;
                }
            }
            panic!("never landed a hit on the knight");
        };

        let bare = first_hit(None);
        let armed = first_hit(Some("iron_sword"));
        assert_eq!(
            armed,
            bare + sword_damage,
            "the sword's own damage should be what changed"
        );
    }

    // --- selection --------------------------------------------------------

    #[test]
    fn the_selection_cannot_leave_the_list() {
        // Empty bag: there is nowhere to go, and nothing to fall off.
        let mut sim = screen_sim(&[]);
        for _ in 0..5 {
            sim.step(DOWN);
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.screen().selection, 0);

        // Two rows: down twice stops at the second, up runs out at the first.
        let mut sim = screen_sim(&[("minor_potion", 1), ("knight_helm", 1)]);
        for _ in 0..5 {
            sim.step(DOWN);
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.screen().selection, 1);
        for _ in 0..5 {
            sim.step(PlayerInput::from_actions(&[Action::Up]));
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.screen().selection, 0);
    }

    /// A held direction is one step, not one per tick — otherwise a menu would
    /// be unusable at 60 Hz.
    #[test]
    fn holding_a_direction_moves_the_selection_once() {
        let mut sim = screen_sim(&[("a", 1), ("b", 1), ("c", 1)]);
        for _ in 0..20 {
            sim.step(PlayerInput::holding(&[Action::Down]));
        }
        assert_eq!(sim.screen().selection, 1, "one step for one press");

        sim.step(PlayerInput::default());
        sim.step(DOWN);
        assert_eq!(
            sim.screen().selection,
            2,
            "releasing and pressing moves again"
        );
    }

    /// Documented behaviour, so it is a decision rather than an accident:
    /// closing and reopening comes back where you left off, clamped if the bag
    /// shrank in the meantime.
    #[test]
    fn closing_and_reopening_keeps_the_selection_and_clamps_it() {
        let mut sim = screen_sim(&[("a", 1), ("b", 1), ("c", 1)]);
        sim.step(DOWN);
        sim.step(PlayerInput::default());
        sim.step(DOWN);
        assert_eq!(sim.screen().selection, 2);

        sim.step(PlayerInput::default());
        sim.step(OPEN_BAG);
        assert_eq!(sim.mode(), Mode::Playing);
        sim.step(PlayerInput::default());
        sim.step(OPEN_BAG);
        assert_eq!(sim.screen().selection, 2, "back where it was");

        // Empty the bag while the screen is shut, and reopening pulls the
        // selection back inside it.
        sim.step(OPEN_BAG);
        {
            let holder = player(&sim);
            let mut bag = sim.world.get::<&mut Inventory>(holder).unwrap();
            bag.slots.clear();
            assert!(bag.add("only", 1));
        }
        sim.step(PlayerInput::default());
        sim.step(OPEN_BAG);
        assert_eq!(sim.screen().selection, 0);
    }

    /// Left and right move between the two panes, and the selection is pulled
    /// inside whichever one it lands in.
    #[test]
    fn the_panes_are_switched_with_left_and_right() {
        let mut sim = screen_sim(&[("a", 1)]);
        assert_eq!(sim.screen().pane, Pane::Bag);

        sim.step(PlayerInput::from_actions(&[Action::Right]));
        assert_eq!(sim.screen().pane, Pane::Gear);
        assert_eq!(row_count(&sim.world, Pane::Gear), Slot::ALL.len());

        sim.step(PlayerInput::default());
        sim.step(PlayerInput::from_actions(&[Action::Down]));
        sim.step(PlayerInput::default());
        sim.step(PlayerInput::from_actions(&[Action::Down]));
        assert_eq!(sim.screen().selection, 2);

        // Back to a one-row bag: the selection cannot stay at row two.
        sim.step(PlayerInput::default());
        sim.step(PlayerInput::from_actions(&[Action::Left]));
        assert_eq!(sim.screen().pane, Pane::Bag);
        assert_eq!(sim.screen().selection, 0);
    }

    /// Confirming an empty slot, or an empty bag, does nothing at all — no
    /// event, no panic, no half-applied state.
    #[test]
    fn confirming_nothing_does_nothing() {
        let mut sim = screen_sim(&[]);
        sim.step(CONFIRM);
        assert!(sim.events().is_empty());

        sim.step(PlayerInput::default());
        sim.step(PlayerInput::from_actions(&[Action::Right]));
        sim.step(PlayerInput::default());
        sim.step(CONFIRM);
        assert!(
            sim.events().is_empty(),
            "an empty worn slot is not an error"
        );
    }
}
