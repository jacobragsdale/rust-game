//! Asset cache: images (with optional color-key transparency), animation clip
//! sets, and tileset definitions. Data files are RON under `assets/data/`;
//! images live under `assets/graphics/`. Everything is cached on first use.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use anyhow::Context as _;
use ggez::glam::Vec2;
use ggez::graphics::{Image, ImageFormat, Rect};
use ggez::Context;
use serde::{Deserialize, Serialize};

/// A named animation: frames are (col, row) cells on a uniform grid.
///
/// `sheet` and `frame_size` default to the [`ClipSet`]'s, and override it when
/// present. The player is one atlas with one cell size, but art packs commonly
/// ship a file per animation with a different frame width each — the knight's
/// idle is 64px wide and its attack is 144 — and a set that could only name one
/// grid could not describe that at all.
#[derive(Clone, Debug, Deserialize)]
pub struct Clip {
    pub frames: Vec<(u32, u32)>,
    pub fps: f32,
    pub looping: bool,
    /// Image name (under `assets/graphics/`, without extension).
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub frame_size: Option<(f32, f32)>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClipSet {
    /// Default image for clips that do not name their own.
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub frame_size: Option<(f32, f32)>,
    /// Drawing nudge for art that is not bottom-aligned inside its own frame.
    ///
    /// Sprites are drawn standing on their collider, which assumes the artist
    /// put the character's feet at the bottom of the cell. The knight pack does
    /// not: every one of its animations leaves 20px of empty space below the
    /// feet, so without this the knight floats two thirds of a tile above the
    /// ground it is standing on.
    #[serde(default)]
    pub offset: Option<(f32, f32)>,
    pub clips: HashMap<String, Clip>,
}

impl ClipSet {
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    /// Drawing nudge for this art, or none.
    pub fn offset(&self) -> (f32, f32) {
        self.offset.unwrap_or((0.0, 0.0))
    }

    /// The sheet a clip's frames live on. The result borrows from whichever of
    /// the two provided it, hence the shared lifetime.
    pub fn sheet_of<'a>(&'a self, clip: &'a Clip) -> &'a str {
        clip.sheet
            .as_deref()
            .or(self.sheet.as_deref())
            .expect("clip set was validated on load")
    }

    /// The cell size a clip's frames are laid out on.
    pub fn frame_size_of(&self, clip: &Clip) -> (f32, f32) {
        clip.frame_size
            .or(self.frame_size)
            .expect("clip set was validated on load")
    }

    /// Normalized source rect for a frame of `clip`, given its sheet's size.
    pub fn src_rect(&self, clip: &Clip, frame: (u32, u32), sheet_w: f32, sheet_h: f32) -> Rect {
        let (fw, fh) = self.frame_size_of(clip);
        Rect::new(
            frame.0 as f32 * fw / sheet_w,
            frame.1 as f32 * fh / sheet_h,
            fw / sheet_w,
            fh / sheet_h,
        )
    }

    /// Every clip must resolve to a sheet and a frame size, one way or another.
    ///
    /// Checked once at load so that `sheet_of` and `frame_size_of` can be
    /// infallible everywhere else — a missing sheet is an authoring mistake in
    /// a RON file, not a condition the renderer should have to handle per frame.
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        let mut missing: Vec<String> = self
            .clips
            .iter()
            .filter(|(_, clip)| {
                clip.sheet.is_none() && self.sheet.is_none()
                    || clip.frame_size.is_none() && self.frame_size.is_none()
            })
            .map(|(clip_name, _)| clip_name.clone())
            .collect();
        missing.sort();

        anyhow::ensure!(
            missing.is_empty(),
            "clip set `{name}`: clips {missing:?} have no sheet or frame_size, \
             and the set does not provide a default"
        );
        Ok(())
    }
}

/// Where an attack's hitbox sits, and which way its knockback points.
///
/// [`HitboxAnchor::Facing`] is every sword swing: the box is measured from the
/// attacker's collider facing right and mirrored when facing left, and the
/// blow throws the victim the way the attacker is looking.
///
/// A plunge needed something the mirrored offset cannot express. A box
/// *underneath* the attacker is symmetric, and while a symmetric offset can be
/// contrived for one collider width — `offset.x = (size.x - w) / 2` happens to
/// mirror onto itself — it silently stops being centred the moment anything of
/// a different width performs the same attack. Naming the anchor says what was
/// meant instead of encoding it in an arithmetic coincidence.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub enum HitboxAnchor {
    /// Measured from the attacker's collider facing right, mirrored when
    /// facing left. Knockback follows the attacker's facing.
    #[default]
    Facing,
    /// Centred on the attacker and never mirrored, with `offset` as a nudge.
    /// Knockback points away from the attacker and up, so what you land on is
    /// thrown clear rather than through you.
    Down,
}

/// One attack's timing, reach, and effect. See `assets/data/attacks.ron`.
/// Spelled `AttackDef(...)` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "AttackDef")]
pub struct AttackDef {
    /// Animation clip to play on the attacker, from its own clip set.
    pub clip: String,
    /// Total ticks the attacker is committed for.
    pub duration: u32,
    /// `[start, end)` in ticks, during which the hitbox exists.
    pub active: (u32, u32),
    /// Extra ticks of commitment after the animation ends. A miss should cost
    /// something, or mashing is always the best play.
    #[serde(default)]
    pub recovery: u32,
    /// What pressing attack again turns this into, if anything.
    #[serde(default)]
    pub chain: Option<String>,
    /// How [`AttackDef::offset`] is read, and which way the blow throws.
    /// Defaults to [`HitboxAnchor::Facing`], which is every swing.
    #[serde(default)]
    pub anchor: HitboxAnchor,
    /// Hitbox position relative to the attacker's collider, facing right.
    pub offset: (f32, f32),
    pub size: (f32, f32),
    pub damage: i32,
    /// Impulse applied to whatever is hit, rightward.
    pub knockback: (f32, f32),
    /// Ticks the victim loses control for.
    pub hitstun: u32,
}

impl AttackDef {
    pub fn is_active(&self, elapsed: u32) -> bool {
        elapsed >= self.active.0 && elapsed < self.active.1
    }

    /// The animation is over; the attacker is still committed until
    /// [`AttackDef::released`].
    pub fn finished(&self, elapsed: u32) -> bool {
        elapsed >= self.duration
    }

    /// Free to act again.
    pub fn released(&self, elapsed: u32) -> bool {
        elapsed >= self.duration + self.recovery
    }

    /// Does this attack continue into another?
    pub fn chains(&self) -> bool {
        self.chain.is_some()
    }

    /// The hitbox in world space for an attacker whose collider is at `pos`
    /// with size `size`, facing `facing_right`.
    pub fn hitbox(&self, pos: Vec2, size: Vec2, facing_right: bool) -> Rect {
        let (w, h) = self.size;
        let x = match self.anchor {
            HitboxAnchor::Facing if facing_right => pos.x + self.offset.0,
            // mirror the whole box about the collider's centre
            HitboxAnchor::Facing => pos.x + size.x - self.offset.0 - w,
            HitboxAnchor::Down => pos.x + (size.x - w) / 2.0 + self.offset.0,
        };
        Rect::new(x, pos.y + self.offset.1, w, h)
    }

    /// Knockback impulse, pointing away from an attacker facing `facing_right`.
    pub fn impulse(&self, facing_right: bool) -> Vec2 {
        let (x, y) = self.knockback;
        Vec2::new(if facing_right { x } else { -x }, y)
    }

    /// Knockback for a blow that landed on a target at `target_centre`, from
    /// an attacker centred at `attacker_centre`.
    ///
    /// Only [`HitboxAnchor::Down`] cares where the target was: a plunge lands
    /// on top of things, and "away" is the side of the impact the victim is
    /// standing on rather than the side the attacker is looking. A swing is
    /// unchanged — the geometry of a sword is that it throws whatever it
    /// reaches the way it was swung.
    pub fn impulse_on(&self, attacker_centre: f32, target_centre: f32, facing_right: bool) -> Vec2 {
        match self.anchor {
            HitboxAnchor::Facing => self.impulse(facing_right),
            HitboxAnchor::Down => {
                // Dead centre is a real tie; break it with the facing so the
                // result is never zero, which would read as "no knockback".
                let away = if target_centre > attacker_centre {
                    true
                } else if target_centre < attacker_centre {
                    false
                } else {
                    facing_right
                };
                self.impulse(away)
            }
        }
    }
}

/// Every attack in the game, by id. Spelled `Attacks({...})` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Attacks")]
pub struct AttackTable(pub HashMap<String, AttackDef>);

impl AttackTable {
    pub fn get(&self, id: &str) -> Option<&AttackDef> {
        self.0.get(id)
    }
}

/// One spell's cost, timing, and what it does. See `assets/data/spells.ron`.
/// Spelled `SpellDef(...)` in the RON file.
///
/// The same split [`AttackDef`] makes: this is balance, and the art it names
/// is a clip on the *caster's* own clip set rather than a sheet, so tuning a
/// spell never reopens the animation table and a second caster with different
/// art needs no second spell.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "SpellDef")]
pub struct SpellDef {
    /// Animation clip played on the caster, from its own clip set.
    pub clip: String,
    /// Mana spent the moment the cast starts, not when it lands.
    pub cost: i32,
    /// Ticks before this spell may be cast again.
    pub cooldown: u32,
    /// Ticks the caster is committed for before the effect appears.
    pub cast_ticks: u32,
    /// Extra ticks of commitment after the effect, the way an attack's
    /// `recovery` works: a spell that ends the instant it fires is free.
    #[serde(default)]
    pub recovery: u32,
    pub effect: SpellEffect,
}

impl SpellDef {
    /// Is this the tick the effect happens on? Exactly one tick per cast, so
    /// a long `recovery` cannot fire a second bolt.
    pub fn releases_at(&self, elapsed: u32) -> bool {
        elapsed == self.cast_ticks
    }

    /// The caster is free to act again.
    pub fn released(&self, elapsed: u32) -> bool {
        elapsed >= self.cast_ticks + self.recovery
    }
}

/// What a spell does when it goes off.
///
/// An enum with one variant from the start, because PLAN.md names `Aoe` and
/// `Buff` as the next two and a struct that has to be widened later is worse
/// than an enum that is trivially extended.
#[derive(Clone, Debug, Deserialize)]
pub enum SpellEffect {
    /// A bolt launched from the caster, travelling the way they face.
    Projectile {
        /// Travel speed in px/s. Gravity does not apply.
        speed: f32,
        damage: i32,
        /// Ticks it survives over open ground.
        lifetime: u32,
        /// Its collider, which is smaller than its art.
        size: (f32, f32),
        /// Clip to draw it with, from the *caster's* clip set — so the bolt's
        /// art travels with whoever throws it.
        clip: String,
        knockback: (f32, f32),
        hitstun: u32,
        /// Carry on through whatever it hits, rather than expiring on contact.
        #[serde(default)]
        pierces: bool,
    },
}

/// Every spell in the game, by id. Spelled `Spells({...})` in the RON file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename = "Spells")]
pub struct SpellTable(pub HashMap<String, SpellDef>);

impl SpellTable {
    pub fn get(&self, id: &str) -> Option<&SpellDef> {
        self.0.get(id)
    }

    /// Every spell id, sorted, for error messages and for the fixture clip set.
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.0.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// The shipped table — the counterpart of [`StatTable::shipped`], and
    /// what every [`crate::sim::Sim`] casts from.
    ///
    /// Read once per process rather than once per call, because `Sim::new`
    /// and the fixture clip set both want it and a headless test run builds
    /// hundreds of sims. Panics if it does not load: spells are content, and
    /// a table that does not parse is a broken build rather than a condition
    /// to degrade around.
    pub fn shipped() -> Arc<SpellTable> {
        static SHIPPED: OnceLock<Arc<SpellTable>> = OnceLock::new();
        SHIPPED
            .get_or_init(|| {
                Assets::new()
                    .spells()
                    .expect("assets/data/spells.ron should load")
            })
            .clone()
    }
}

/// One item, under the id every other file names it by. See
/// `assets/data/items/*.ron`. Spelled `ItemDef(...)` in the RON files.
///
/// Ids are the currency of every system after this one — a loot table, a
/// dialogue effect, a quest reward — which is why `tests/data.rs` insists they
/// are unique and that every reference resolves.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "ItemDef")]
pub struct ItemDef {
    pub id: String,
    /// What the inventory screen calls it.
    pub name: String,
    /// Image name under `assets/graphics/`, without extension.
    ///
    /// May name art that does not exist yet: a pickup on the floor and a row
    /// in the bag both fall back to a coloured quad, so content authored today
    /// does not have to be rewritten the day the art arrives. This is also why
    /// `tests/data.rs` checks `sheet:` and `image:` for existence but not
    /// `sprite:`.
    pub sprite: String,
    pub kind: ItemKind,
}

impl ItemDef {
    /// Which equipment slot this occupies, or `None` for something that is
    /// only ever consumed. A weapon's slot is implied by its kind; anything
    /// else says which one it wants.
    pub fn slot(&self) -> Option<Slot> {
        match &self.kind {
            ItemKind::Weapon { .. } => Some(Slot::Weapon),
            ItemKind::Equipment { slot, .. } => Some(*slot),
            ItemKind::Consumable { .. } => None,
        }
    }

    /// Can this be drunk, eaten or otherwise spent?
    pub fn is_consumable(&self) -> bool {
        matches!(self.kind, ItemKind::Consumable { .. })
    }
}

/// What an item *is*, which decides what `Confirm` does to it in the bag.
#[derive(Clone, Debug, Deserialize)]
pub enum ItemKind {
    /// Held in [`Slot::Weapon`]. `damage` is added to every melee hit on top
    /// of the attack's own, and `combo` replaces the bare-handed chain's
    /// opener — the rest of the chain is still `chain` in `attacks.ron`, so a
    /// weapon that reuses the standard swings names them and nothing else
    /// changes.
    Weapon {
        damage: i32,
        combo: Vec<String>,
        /// Swing rate multiplier. **Not read yet**: attack timing is the
        /// attack table's, and making a weapon swing faster means either its
        /// own entries in `attacks.ron` (which `combo` already expresses) or a
        /// multiplier threaded through `AttackDef` — a decision M4 does not
        /// need to make. It is in the schema because PLAN.md puts it there and
        /// because adding a field to shipped content later is the expensive
        /// direction.
        speed: f32,
    },
    /// Spent from the bag for an immediate effect.
    Consumable { effects: Vec<ItemEffect> },
    /// Worn in a slot, contributing [`StatModifier`]s for as long as it is.
    Equipment {
        slot: Slot,
        modifiers: Vec<StatModifier>,
    },
}

/// Where a piece of equipment is worn.
///
/// `Ord` is not decoration: [`crate::ecs::components::Equipment`] keys a
/// `BTreeMap` on this so that summing modifiers walks the slots in a fixed
/// order. A `HashMap` would sum the same floats in an order that varies run to
/// run, and float addition is not associative — which is exactly the kind of
/// invisible nondeterminism a golden trace exists to catch and nobody enjoys
/// hunting.
#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
pub enum Slot {
    #[default]
    Head,
    Body,
    Weapon,
    Trinket,
}

impl Slot {
    /// Every slot, in the order the equipment pane lists them.
    pub const ALL: [Slot; 4] = [Slot::Head, Slot::Body, Slot::Weapon, Slot::Trinket];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Head => "Head",
            Slot::Body => "Body",
            Slot::Weapon => "Weapon",
            Slot::Trinket => "Trinket",
        }
    }
}

/// What using a consumable does. An enum from the start for the reason
/// [`SpellEffect`] is one: the second variant should cost nothing to add.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub enum ItemEffect {
    /// Restore health, clamped to the maximum.
    Heal(i32),
    /// Restore mana, clamped to the pool.
    RestoreMana(i32),
}

/// One term of `base + sum(modifiers)`.
///
/// Deliberately additive and deliberately dumb. A modifier that multiplied, or
/// that depended on what else was equipped, would make the order equipment was
/// put on in observable — and the whole point of recomputing from the base
/// every tick is that it cannot be.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub enum StatModifier {
    MaxHealth(i32),
    MaxMana(i32),
    RunSpeed(f32),
    /// Added to every melee hit, exactly as a weapon's own `damage` is.
    Damage(i32),
}

/// One line of a loot table: what may drop, how many, and how often.
///
/// Rolled against [`crate::sim::rng::Rng`] and nothing else — see
/// [`crate::systems::inventory::drop_loot`] for why the order of the roll is
/// part of the contract.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "LootDrop")]
pub struct LootDrop {
    /// An id from `assets/data/items/`.
    pub item: String,
    pub count: u32,
    /// Probability in `[0, 1]`. `1.0` always drops and `0.0` never does.
    pub chance: f32,
}

/// One conversation, as a graph of nodes. See `assets/data/dialogue/*.ron`.
/// Spelled `Dialogue(...)` in the RON files.
///
/// This is *content*: what is said, what may be said back, and what saying it
/// does. Walking the graph is [`crate::systems::dialogue`], and it is a
/// simulation system rather than a scene for the reason set out there.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Dialogue")]
pub struct DialogueGraph {
    /// What an `Interactable` names this conversation by. Unique across every
    /// file, the same way an item id is.
    pub id: String,
    /// The node the conversation opens on. Checked at load.
    pub start: String,
    pub nodes: HashMap<String, DialogueNode>,
}

impl DialogueGraph {
    pub fn node(&self, id: &str) -> Option<&DialogueNode> {
        self.nodes.get(id)
    }

    /// Every node id, sorted, for error messages.
    pub fn node_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.nodes.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }
}

/// One thing an NPC says, and what may be said back.
///
/// `lines` is a list rather than a string because a speech is paged: `confirm`
/// walks to the next line, and the choices are offered once the last one has
/// been read. A node with no choices is an end of the conversation.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Node")]
pub struct DialogueNode {
    pub speaker: String,
    pub lines: Vec<String>,
    #[serde(default)]
    pub choices: Vec<DialogueChoice>,
}

/// One reply, where it leads, and what it costs or gives.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Choice")]
pub struct DialogueChoice {
    pub text: String,
    /// The node this leads to. `None` ends the conversation — which is what
    /// "Goodbye." is.
    #[serde(default)]
    pub next: Option<String>,
    /// When this may be offered at all. A choice whose condition fails is
    /// **hidden**, not greyed out; see [`crate::systems::dialogue`] for why.
    #[serde(default)]
    pub condition: Option<DialogueCondition>,
    /// What taking it does, applied in order, before the conversation moves on.
    #[serde(default)]
    pub effects: Vec<DialogueEffect>,
}

/// When a choice may be offered.
///
/// An enum from the start, with the flag variants present before there is much
/// to point them at, because the shape is what later tickets extend rather than
/// replace — Q-2's quest branches are `FlagAtLeast` and nothing else new.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum DialogueCondition {
    /// The player is carrying at least one of this item.
    HasItem(String),
    /// A quest flag is exactly this. An unset flag reads as 0.
    FlagEq(String, i64),
    /// A quest flag has reached at least this stage.
    FlagAtLeast(String, i64),
}

/// What taking a choice does.
///
/// Every one of these goes through the system that owns the state it touches —
/// `GiveItem` through the same [`crate::ecs::components::Inventory`] a pickup
/// lands in, `Heal` through the same clamp a potion uses. Nothing here writes
/// to a component that some other system also writes to by a different route.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum DialogueEffect {
    /// Set a quest flag to a stage.
    SetFlag(String, i64),
    /// Put items in the bag, exactly as walking over them would.
    GiveItem(String, u32),
    /// Take items out of it. Does nothing if they are not there.
    TakeItem(String, u32),
    /// Restore health, clamped to the derived maximum.
    Heal(i32),
}

/// Every conversation in the game, by id. Assembled from every `.ron` file
/// under `assets/data/dialogue/`, each of which holds one `Dialogue(...)`.
///
/// A file per graph, for the reason items get a directory: conversations are
/// the content type that grows without bound, and one file per graph is one
/// diff per rewrite. Ids are global regardless of the file, so a filename is an
/// organizing convenience and never part of a graph's identity.
#[derive(Clone, Debug, Default)]
pub struct DialogueTable(HashMap<String, Arc<DialogueGraph>>);

impl DialogueTable {
    pub fn get(&self, id: &str) -> Option<&Arc<DialogueGraph>> {
        self.0.get(id)
    }

    /// Every graph id, sorted, for error messages.
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.0.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// The shipped table, read once per process — the counterpart of
    /// [`ItemTable::shipped`] and [`SpellTable::shipped`].
    ///
    /// Panics if it does not load, for the same reason those do: dialogue is
    /// content, and a graph that does not parse — or that points at a node it
    /// does not define — is a broken build rather than something to degrade
    /// around at runtime.
    pub fn shipped() -> Arc<DialogueTable> {
        static SHIPPED: OnceLock<Arc<DialogueTable>> = OnceLock::new();
        SHIPPED
            .get_or_init(|| {
                Assets::new()
                    .dialogue()
                    .expect("assets/data/dialogue should load")
            })
            .clone()
    }
}

/// Every item in the game, by id. Assembled from every `.ron` file under
/// `assets/data/items/`, each of which holds one `ItemDef(...)` or a list of
/// them.
///
/// A directory rather than one file because items are the content type that
/// grows without bound, and a thousand-line `items.ron` is a merge conflict
/// waiting to happen. Ids are global regardless of which file they live in, so
/// a file is an organizing convenience and never part of an item's identity.
#[derive(Clone, Debug, Default)]
pub struct ItemTable(HashMap<String, ItemDef>);

impl ItemTable {
    pub fn get(&self, id: &str) -> Option<&ItemDef> {
        self.0.get(id)
    }

    /// Every item id, sorted, for error messages.
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.0.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// What to call an item on screen, falling back to the id so that content
    /// naming something the table does not define is visible rather than
    /// blank.
    pub fn label<'a>(&'a self, id: &'a str) -> &'a str {
        self.get(id).map_or(id, |def| def.name.as_str())
    }

    /// The shipped table, the counterpart of [`StatTable::shipped`] and
    /// [`SpellTable::shipped`], read once per process.
    ///
    /// Panics if it does not load: items are content, and a table that does
    /// not parse is a broken build rather than something to degrade around.
    pub fn shipped() -> Arc<ItemTable> {
        static SHIPPED: OnceLock<Arc<ItemTable>> = OnceLock::new();
        SHIPPED
            .get_or_init(|| {
                Assets::new()
                    .items()
                    .expect("assets/data/items should load")
            })
            .clone()
    }
}

/// Everything one kind of entity is made of, numerically. See
/// `assets/data/stats.ron`. Spelled `StatBlock(...)` in the RON file.
///
/// The flat fields are what *anything* alive needs: a box, a weight, hit
/// points, a walking pace, and something to swing. The two groups below are
/// the parts only some kinds have — steering a player through a jump, or
/// hunting one — and they are `Option` rather than defaulted so that a kind
/// which forgets them fails loudly at the first read instead of quietly
/// running on zeroes.
///
/// M4's equipment computes `base + sum(modifiers)`; this is the base.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "StatBlock")]
pub struct StatBlock {
    /// Collider width and height. The sprite is aligned to this box, not the
    /// other way round — see [`crate::ecs::components::Sprite::draw_origin`].
    pub width: f32,
    pub height: f32,
    /// Ground speed: the player's run, an NPC's patrol pace.
    pub run_speed: f32,
    pub gravity: f32,
    /// Terminal velocity.
    pub max_fall: f32,
    pub max_health: i32,
    /// How long invulnerability lasts after a hit. Per-kind because the two
    /// sides want opposite things: the player's is a mercy window, an enemy's
    /// must be shorter than the gap between combo links or only the first hit
    /// of a combo ever lands.
    pub iframe_ticks: u32,
    /// The attack this kind opens with, from `assets/data/attacks.ron`. The
    /// rest of a combo is data: each attack names its own successor.
    pub attack: String,
    /// Flat damage added to every melee hit this kind lands, on top of the
    /// attack's own.
    ///
    /// Zero bare-handed. A weapon's `damage` is a modifier on this, which is
    /// what makes "the sword hits harder" a derived stat rather than a second
    /// damage path — and therefore something that unequipping undoes exactly.
    pub damage_bonus: i32,
    /// How many distinct stacks this kind can carry.
    ///
    /// Zero is a kind with no bag at all, and is what [`crate::ecs::spawn`]
    /// reads to decide whether to give it an
    /// [`crate::ecs::components::Inventory`] — the same arrangement `max_mana`
    /// has. Capacity is a stat because it is balance: a bigger bag is a reward,
    /// and a reward should not be a recompile.
    pub inventory_slots: u32,
    /// What this kind leaves behind when it dies, rolled once against
    /// [`crate::sim::rng::Rng`] on the tick of death.
    pub loot: Vec<LootDrop>,
    /// Size of the mana pool. Zero means a kind that never casts, and is what
    /// [`crate::ecs::spawn`] reads to decide whether to give it a
    /// [`crate::ecs::components::Mana`] at all — no pool, no component, no
    /// mana bar, and no archetype it would otherwise be dragged into.
    pub max_mana: i32,
    /// Mana regenerated per tick, in thousandths of a point.
    ///
    /// Fixed point rather than a float because a tape asserts "back to full",
    /// and a float accumulator makes the tick that happens on depend on how
    /// rounding fell over the preceding second. Integers make it exact:
    /// `1000 / mana_regen` ticks per point, forever, on every machine.
    pub mana_regen: u32,
    /// The spell this kind casts, from `assets/data/spells.ron`. Absent for
    /// anything that does not cast.
    #[serde(default)]
    pub spell: Option<String>,
    /// What pressing `Interact` beside one of these offers. Absent for
    /// anything there is nothing to say to — which is every kind but the
    /// villager today.
    #[serde(default)]
    pub interact: Option<InteractDef>,
    /// Present only for a kind the player drives.
    #[serde(default)]
    pub avatar: Option<AvatarStats>,
    /// Present only for a kind that walks a route and hunts.
    #[serde(default)]
    pub ai: Option<AiStats>,
}

/// The knobs [`crate::systems::avatar`] steers with. Spelled
/// `AvatarStats(...)` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "AvatarStats")]
pub struct AvatarStats {
    pub accel: f32,
    pub decel: f32,
    /// Jump clears 3 tiles up and ~4.5 tiles across at full run speed.
    pub jump_speed: f32,
    pub double_jump_speed: f32,
    /// Extra gravity while rising with the jump key released: tap = short
    /// hop, hold = full jump.
    pub low_jump_gravity: f32,
    pub max_air_jumps: u8,
    /// Jump grace after walking off a ledge (6 ticks is 100 ms at 60 Hz).
    pub coyote_ticks: u32,
    /// How early a jump press may land and still count.
    pub jump_buffer_ticks: u32,
    /// Fall speed cap while pressed against a wall.
    pub wall_slide_speed: f32,
    /// Horizontal kick away from the wall on a wall jump.
    pub wall_jump_push: f32,
    pub wall_jump_speed: f32,
    /// Jump grace after leaving a wall. Wall contact is often a single tick —
    /// clipping a corner, or bouncing off on the way up — and without a grace
    /// window the wall jump only exists while a slide is held.
    pub wall_coyote_ticks: u32,
    /// One-way platforms are ignored for this long after down+jump.
    pub drop_ticks: u32,
    /// How long a slide lasts, matching the `slide` clip.
    pub slide_ticks: u32,
    /// Slides start faster than a run and bleed off across their length.
    pub slide_speed: f32,
    /// Ticks after a slide before another may start, so it is a move rather
    /// than a faster way to walk.
    pub slide_cooldown: u32,
    /// Death freeze before respawning.
    pub death_ticks: u32,
    /// What a press in mid-air performs instead of the ground combo.
    pub air_attack: String,
    /// What down+attack in mid-air performs instead of the air attack.
    pub plunge_attack: String,
    /// How long the plunge hangs before it drops. The hover is what makes it
    /// read as a decision rather than as a faster fall — it is the tell the
    /// thing underneath you gets.
    pub plunge_hover_ticks: u32,
    /// How fast the plunge falls, in px/s. Deliberately its own number rather
    /// than `max_fall`: the drop should outrun an ordinary fall visibly.
    pub plunge_speed: f32,
    /// Ticks rooted on landing, matching the `plunge_impact` clip.
    ///
    /// The plunge's recovery lives here rather than in `attacks.ron` because
    /// it starts when the ground arrives, and nothing in an attack's fixed
    /// timeline knows when that is.
    pub plunge_impact_ticks: u32,
}

/// What a kind offers the player standing next to it. Spelled
/// `Interact(...)` in the RON file.
///
/// Content rather than code, so a second talking NPC is a stat block and a
/// dialogue file. `prompt` is the word the HUD shows and the word a tape reads
/// with `assert prompt == talk`, so it is deliberately one token.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Interact")]
pub struct InteractDef {
    pub prompt: String,
    /// A graph id from `assets/data/dialogue/`.
    pub dialogue: String,
}

/// The knobs [`crate::systems::npc`] walks and hunts with. Spelled
/// `AiStats(...)` in the RON file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "AiStats")]
pub struct AiStats {
    /// How far ahead it notices the player. Sight is a box in front of it,
    /// not a radius: walking up behind a patrolling knight should work.
    pub sight: f32,
    /// Vertical tolerance on that box. Roughly a body height, so a player on
    /// the platform above is not "in front of" anything.
    pub sight_height: f32,
    /// Gives up once the player is this far away — wider than `sight`, so an
    /// enemy at the edge of its vision does not flicker between states.
    pub lose: f32,
    /// Close enough to swing.
    pub reach: f32,
    /// Ticks between swings. Long enough that closing in, landing a hit and
    /// backing out is a plan rather than a gamble.
    pub cooldown: u32,
    /// How close to home counts as home.
    pub home_slack: f32,
    /// Chasing is faster than strolling, but still much slower than the
    /// player runs: backing off has to work.
    pub chase_multiplier: f32,
    /// How far ahead of the collider to look for a wall, or for missing
    /// floor. A little under half a tile: far enough to stop before the edge,
    /// close enough that a one-tile ledge is still walkable.
    pub lookahead: f32,
    /// How far below the feet counts as "there is still floor here".
    pub floor_probe: f32,
}

impl StatBlock {
    /// The player-steering group, which a kind the player drives must have.
    pub fn avatar(&self) -> &AvatarStats {
        self.avatar
            .as_ref()
            .expect("an entity with an `Avatar` needs an `avatar` group in assets/data/stats.ron")
    }

    /// The walking-and-hunting group, which a kind that patrols must have.
    pub fn ai(&self) -> &AiStats {
        self.ai
            .as_ref()
            .expect("an entity with a `Patrol` needs an `ai` group in assets/data/stats.ron")
    }

    /// The collider box this kind occupies.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }
}

/// Every kind's numbers, by the name a map (or `spawn`) calls it.
/// Spelled `Stats({...})` in the RON file.
///
/// Blocks come out behind an `Arc` because every entity of a kind shares one:
/// thirty knights are thirty pointers, not thirty copies of a struct with two
/// `String`s in it. It also means a block is `Send + Sync`, which a component
/// must be.
#[derive(Clone, Debug, Default)]
pub struct StatTable(HashMap<String, Arc<StatBlock>>);

impl StatTable {
    /// The block for `kind`, or an error naming it and everything that does
    /// resolve — the same contract `spawn::entity` gives an unknown kind.
    pub fn get(&self, kind: &str) -> anyhow::Result<Arc<StatBlock>> {
        match self.0.get(kind) {
            Some(block) => Ok(block.clone()),
            None => anyhow::bail!(
                "assets/data/stats.ron has no stat block for `{kind}` (it defines: {})",
                self.kinds().join(", ")
            ),
        }
    }

    /// Every kind the table defines, sorted, for error messages.
    pub fn kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = self.0.keys().map(String::as_str).collect();
        kinds.sort_unstable();
        kinds
    }

    /// The shipped table, read straight off disk.
    ///
    /// [`Assets::stats`] is the cached path the game itself uses; this is for
    /// tests and for `Sim::fixture`, which have no asset cache to hand.
    /// Combat and movement numbers are content, and a test that invents its
    /// own is not testing the game — so this panics rather than offering a
    /// fallback: a table that does not load is a broken build.
    pub fn shipped() -> Arc<StatTable> {
        Assets::new()
            .stats()
            .expect("assets/data/stats.ron should load")
    }
}

impl<'de> Deserialize<'de> for StatTable {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "Stats")]
        struct Raw(HashMap<String, StatBlock>);

        let Raw(blocks) = Raw::deserialize(de)?;
        Ok(StatTable(
            blocks
                .into_iter()
                .map(|(kind, block)| (kind, Arc::new(block)))
                .collect(),
        ))
    }
}

/// How a logical map cell maps onto tiles of the atlas, chosen by looking at
/// a solid cell's neighbors. All values are 0-based tile indices.
/// Spelled `Rules(...)` in the RON files.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Rules")]
pub struct AutotileRules {
    pub solid_top_left: u32,
    pub solid_top: u32,
    pub solid_top_right: u32,
    pub solid_left: u32,
    pub solid_fill: u32,
    pub solid_right: u32,
    pub platform: u32,
    /// Variants scattered over empty cells for texture.
    pub background: Vec<u32>,
}

/// Spelled `Tileset(...)` in the RON files.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Tileset")]
pub struct TilesetDef {
    /// Image name (under `assets/graphics/`, without extension).
    pub image: String,
    pub tile_size: u32,
    pub columns: u32,
    /// Pixels of exactly this color become transparent (e.g. magenta keys).
    pub transparent_color: Option<(u8, u8, u8)>,
    pub rules: AutotileRules,
}

impl TilesetDef {
    /// Normalized source rect for a tile index.
    pub fn src_rect(&self, tile: u32, sheet_w: f32, sheet_h: f32) -> Rect {
        let ts = self.tile_size as f32;
        let col = tile % self.columns;
        let row = tile / self.columns;
        Rect::new(
            col as f32 * ts / sheet_w,
            row as f32 * ts / sheet_h,
            ts / sheet_w,
            ts / sheet_h,
        )
    }
}

pub struct Assets {
    base: PathBuf,
    images: HashMap<String, Image>,
    clip_sets: HashMap<String, Arc<ClipSet>>,
    tilesets: HashMap<String, Rc<TilesetDef>>,
    attacks: Option<Arc<AttackTable>>,
    spells: Option<Arc<SpellTable>>,
    stats: Option<Arc<StatTable>>,
    items: Option<Arc<ItemTable>>,
    dialogue: Option<Arc<DialogueTable>>,
}

impl Default for Assets {
    fn default() -> Self {
        Assets::new()
    }
}

impl Assets {
    pub fn new() -> Self {
        // Prefer ./assets (running from the repo root), fall back to the
        // crate directory (running the binary from elsewhere during dev).
        let cwd = PathBuf::from("assets");
        let base = if cwd.is_dir() {
            cwd
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
        };
        Assets::rooted(base)
    }

    /// An asset cache reading from `base` instead of the shipped `assets/`.
    ///
    /// Exists so that a test can point the loaders at content written to be
    /// wrong — a dialogue graph with a dangling `next`, say. The alternative is
    /// committing broken files into `assets/`, where every other check would
    /// have to learn to ignore them.
    pub fn rooted(base: impl Into<PathBuf>) -> Self {
        Assets {
            base: base.into(),
            images: HashMap::new(),
            clip_sets: HashMap::new(),
            tilesets: HashMap::new(),
            attacks: None,
            spells: None,
            stats: None,
            items: None,
            dialogue: None,
        }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base
    }

    /// Decode `assets/graphics/{name}.png` to RGBA, applying an optional
    /// color key.
    ///
    /// Split out of [`Assets::image`] because it needs no graphics context,
    /// which lets asset checks inspect exactly the pixels the game uploads.
    pub fn decode_image(
        &self,
        name: &str,
        color_key: Option<(u8, u8, u8)>,
    ) -> anyhow::Result<image::RgbaImage> {
        let path = self.base.join("graphics").join(format!("{name}.png"));
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read image {}", path.display()))?;
        let decoded = image::load_from_memory(&bytes)
            .with_context(|| format!("failed to decode image {}", path.display()))?;
        let mut rgba = decoded.to_rgba8();

        if let Some((r, g, b)) = color_key {
            for pixel in rgba.pixels_mut() {
                if pixel[0] == r && pixel[1] == g && pixel[2] == b {
                    *pixel = image::Rgba([0, 0, 0, 0]);
                }
            }
        }

        Ok(rgba)
    }

    /// Load `assets/graphics/{name}.png`, applying an optional color key.
    /// Images are cheap to clone (shared GPU handle).
    pub fn image(
        &mut self,
        ctx: &mut Context,
        name: &str,
        color_key: Option<(u8, u8, u8)>,
    ) -> anyhow::Result<Image> {
        if let Some(image) = self.images.get(name) {
            return Ok(image.clone());
        }

        let rgba = self.decode_image(name, color_key)?;
        let (w, h) = rgba.dimensions();
        let image = Image::from_pixels(ctx, rgba.as_raw(), ImageFormat::Rgba8UnormSrgb, w, h);
        self.images.insert(name.to_string(), image.clone());
        Ok(image)
    }

    /// Load `assets/data/animations/{name}.ron`.
    pub fn clip_set(&mut self, name: &str) -> anyhow::Result<Arc<ClipSet>> {
        if let Some(set) = self.clip_sets.get(name) {
            return Ok(set.clone());
        }
        let path = self
            .base
            .join("data/animations")
            .join(format!("{name}.ron"));
        let set: ClipSet = load_ron(&path)?;
        set.validate(name)?;
        let set = Arc::new(set);
        self.clip_sets.insert(name.to_string(), set.clone());
        Ok(set)
    }

    /// Load `assets/data/attacks.ron`. One table for the whole game.
    pub fn attacks(&mut self) -> anyhow::Result<Arc<AttackTable>> {
        if let Some(table) = &self.attacks {
            return Ok(table.clone());
        }
        let table: AttackTable = load_ron(&self.base.join("data/attacks.ron"))?;
        let table = Arc::new(table);
        self.attacks = Some(table.clone());
        Ok(table)
    }

    /// Load `assets/data/spells.ron`. One table for the whole game.
    pub fn spells(&mut self) -> anyhow::Result<Arc<SpellTable>> {
        if let Some(table) = &self.spells {
            return Ok(table.clone());
        }
        let table: SpellTable = load_ron(&self.base.join("data/spells.ron"))?;
        let table = Arc::new(table);
        self.spells = Some(table.clone());
        Ok(table)
    }

    /// Load `assets/data/stats.ron`. One table for the whole game.
    pub fn stats(&mut self) -> anyhow::Result<Arc<StatTable>> {
        if let Some(table) = &self.stats {
            return Ok(table.clone());
        }
        let table: StatTable = load_ron(&self.base.join("data/stats.ron"))?;
        let table = Arc::new(table);
        self.stats = Some(table.clone());
        Ok(table)
    }

    /// Load every `.ron` file under `assets/data/items/` into one table.
    ///
    /// Files are read in sorted order so that a duplicate id always blames the
    /// same pair of files, whatever order the filesystem hands them back in. A
    /// file may hold a single `ItemDef(...)` or a list of them; both shapes
    /// appear in PLAN.md and neither is worth forcing content into.
    pub fn items(&mut self) -> anyhow::Result<Arc<ItemTable>> {
        if let Some(table) = &self.items {
            return Ok(table.clone());
        }

        let dir = self.base.join("data/items");
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("failed to read items directory {}", dir.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "ron"))
            .collect();
        paths.sort();

        let mut items: HashMap<String, ItemDef> = HashMap::new();
        let mut sources: HashMap<String, PathBuf> = HashMap::new();
        for path in paths {
            let defs: Vec<ItemDef> = match load_ron::<Vec<ItemDef>>(&path) {
                Ok(defs) => defs,
                // A single definition is the other legal shape; report the
                // list error if it is neither, since that is the one content
                // is more likely to have been aiming at.
                Err(list_error) => match load_ron::<ItemDef>(&path) {
                    Ok(def) => vec![def],
                    Err(_) => return Err(list_error),
                },
            };
            for def in defs {
                if let Some(first) = sources.insert(def.id.clone(), path.clone()) {
                    anyhow::bail!(
                        "item id `{}` is defined in both {} and {} — ids are how every \
                         other file names an item, so they have to be unique",
                        def.id,
                        first.display(),
                        path.display(),
                    );
                }
                items.insert(def.id.clone(), def);
            }
        }

        let table = Arc::new(ItemTable(items));
        self.items = Some(table.clone());
        Ok(table)
    }

    /// Load every `.ron` file under `assets/data/dialogue/` into one table,
    /// checking as it goes that each graph actually holds together.
    ///
    /// **A dangling `next` is a load-time error naming the graph and the
    /// node**, not a conversation that dead-ends in front of a player. The
    /// alternative — resolving targets when a choice is taken — turns a
    /// one-character typo into a branch that silently closes the conversation,
    /// which is indistinguishable from a branch that was meant to. Same for a
    /// `start` that names nothing: the conversation would open on nothing at
    /// all.
    ///
    /// What is *not* checked here is reachability. A node nothing points at is
    /// writing that ships and is never read — a content mistake rather than a
    /// broken graph, so it fails in `tests/data.rs` where the whole corpus is
    /// visible, rather than stopping a map from loading.
    ///
    /// Files are read in sorted order so a duplicate id always blames the same
    /// pair of files, whatever order the filesystem hands them back in.
    pub fn dialogue(&mut self) -> anyhow::Result<Arc<DialogueTable>> {
        if let Some(table) = &self.dialogue {
            return Ok(table.clone());
        }

        let dir = self.base.join("data/dialogue");
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("failed to read dialogue directory {}", dir.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "ron"))
            .collect();
        paths.sort();

        let mut graphs: HashMap<String, Arc<DialogueGraph>> = HashMap::new();
        let mut sources: HashMap<String, PathBuf> = HashMap::new();
        for path in paths {
            let graph: DialogueGraph = load_ron(&path)?;
            validate_dialogue(&graph, &path)?;
            if let Some(first) = sources.insert(graph.id.clone(), path.clone()) {
                anyhow::bail!(
                    "dialogue graph `{}` is defined in both {} and {} — ids are how an \
                     NPC names a conversation, so they have to be unique",
                    graph.id,
                    first.display(),
                    path.display(),
                );
            }
            graphs.insert(graph.id.clone(), Arc::new(graph));
        }

        let table = Arc::new(DialogueTable(graphs));
        self.dialogue = Some(table.clone());
        Ok(table)
    }

    /// Load `assets/data/tilesets/{name}.ron`.
    pub fn tileset(&mut self, name: &str) -> anyhow::Result<Rc<TilesetDef>> {
        if let Some(def) = self.tilesets.get(name) {
            return Ok(def.clone());
        }
        let path = self.base.join("data/tilesets").join(format!("{name}.ron"));
        let def: TilesetDef = load_ron(&path)?;
        let def = Rc::new(def);
        self.tilesets.insert(name.to_string(), def.clone());
        Ok(def)
    }
}

/// Every way a dialogue graph can fail to hold together, reported against the
/// file it was read from.
///
/// Both checks are about *edges* of the graph, which is the only part of a
/// conversation no type can enforce: `start` and `next` are strings, and a
/// string that names nothing parses perfectly.
fn validate_dialogue(graph: &DialogueGraph, path: &std::path::Path) -> anyhow::Result<()> {
    let known = graph.node_ids().join(", ");

    anyhow::ensure!(
        graph.nodes.contains_key(&graph.start),
        "{}: dialogue graph `{}` starts at node `{}`, which it does not define \
         (nodes: {known})",
        path.display(),
        graph.id,
        graph.start,
    );

    // Sorted, so two runs blame the same node first.
    for node_id in graph.node_ids() {
        let node = &graph.nodes[node_id];
        for (index, choice) in node.choices.iter().enumerate() {
            let Some(next) = &choice.next else {
                continue; // ending the conversation is a legitimate target
            };
            anyhow::ensure!(
                graph.nodes.contains_key(next),
                "{}: dialogue graph `{}`, node `{node_id}`: choice {index} \
                 (`{}`) leads to `{next}`, which the graph does not define \
                 (nodes: {known})",
                path.display(),
                graph.id,
                choice.text,
            );
        }
    }

    Ok(())
}

/// Parse a RON data file.
///
/// `implicit_some` is on so that an optional field can be written as
/// `sheet: "knight/knightIdle"` rather than `sheet: Some("knight/knightIdle")`.
/// These files are hand-authored content; making every optional field announce
/// its optionality is noise for whoever is writing the twentieth NPC.
fn load_ron<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> anyhow::Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))
}
