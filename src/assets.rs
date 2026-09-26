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
    /// Drawing nudge for this clip alone, on top of the set's, measured facing
    /// right and mirrored facing left.
    ///
    /// Art packs rarely keep a body in the same place across animations: the
    /// knight's run is drawn ten pixels ahead of where its attack wind-up is,
    /// so with one offset for the whole set it lurched backwards every time
    /// it started a swing. A per-clip nudge puts each animation's body back
    /// over the collider it belongs to.
    #[serde(default)]
    pub offset: Option<(f32, f32)>,
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
    /// A colour every frame of this set is multiplied by, as 0-255 RGB.
    ///
    /// For art borrowed from another kind: the villager is drawn from the
    /// knight's sheets, and without a tint the thing you talk to and the thing
    /// that stabs you would be the same pixels. Content rather than a special
    /// case in the renderer, so the next borrowed look is a line of RON.
    #[serde(default)]
    pub tint: Option<(u8, u8, u8)>,
    /// Another clip set, by name, to start from: every clip and default this
    /// file does not give is that set's. A kind drawn from another's art —
    /// the villager, the mage, the warden are all the knight's sheets — is
    /// then a tint and whatever it does differently, rather than a copy of
    /// sixty lines that drifts from the original. One level only, so a chain
    /// can never loop.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub clips: HashMap<String, Clip>,
}

impl ClipSet {
    /// Fill in everything this set leaves out from `from`, the set it names
    /// as its `base`. What this set says wins, clip by clip.
    fn inherit(&mut self, from: ClipSet) {
        self.sheet = self.sheet.take().or(from.sheet);
        self.frame_size = self.frame_size.or(from.frame_size);
        self.offset = self.offset.or(from.offset);
        self.tint = self.tint.or(from.tint);
        for (name, clip) in from.clips {
            self.clips.entry(name).or_insert(clip);
        }
    }

    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.get(name)
    }

    /// Drawing nudge for this art, or none.
    pub fn offset(&self) -> (f32, f32) {
        self.offset.unwrap_or((0.0, 0.0))
    }

    /// The nudge for one clip: the set's plus the clip's own.
    pub fn offset_of(&self, clip: &Clip) -> (f32, f32) {
        let (sx, sy) = self.offset();
        let (cx, cy) = clip.offset.unwrap_or((0.0, 0.0));
        (sx + cx, sy + cy)
    }

    /// The set's tint as a colour to multiply by, white for none.
    pub fn tint(&self) -> ggez::graphics::Color {
        match self.tint {
            Some((r, g, b)) => ggez::graphics::Color::from_rgb(r, g, b),
            None => ggez::graphics::Color::WHITE,
        }
    }

    /// The pixel rectangle one frame of a clip occupies on its sheet.
    pub fn frame_rect(&self, clip: &Clip, frame: (u32, u32)) -> Rect {
        let (fw, fh) = self.frame_size_of(clip);
        Rect::new(frame.0 as f32 * fw, frame.1 as f32 * fh, fw, fh)
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

        // A clip with nothing in it passes every headless check — the
        // animator skips it — and then crashes the first draw that reaches it.
        let mut clips: Vec<(&String, &Clip)> = self.clips.iter().collect();
        clips.sort_by_key(|(clip_name, _)| *clip_name);
        for (clip_name, clip) in clips {
            anyhow::ensure!(
                !clip.frames.is_empty(),
                "clip set `{name}`: clip `{clip_name}` has no frames"
            );
            anyhow::ensure!(
                clip.fps > 0.0,
                "clip set `{name}`: clip `{clip_name}` has fps {}, which never advances",
                clip.fps
            );
            let (w, h) = self.frame_size_of(clip);
            anyhow::ensure!(
                w >= 1.0 && h >= 1.0,
                "clip set `{name}`: clip `{clip_name}` has a {w}x{h} frame"
            );
        }
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
        /// Flown at the nearest foe rather than straight ahead: a drone
        /// hanging in the air has nothing level with it to shoot at.
        #[serde(default)]
        aimed: bool,
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
    /// One line the inventory shows for the selected item. Optional, because
    /// a potion that says "Minor Health Potion" has already said most of it.
    #[serde(default)]
    pub description: String,
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
            ItemKind::Consumable { .. } | ItemKind::Carried => None,
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
    /// Worn in a slot, contributing [`StatModifier`]s for as long as it is —
    /// and, for a tome, the spell its wearer casts instead of their own.
    Equipment {
        slot: Slot,
        modifiers: Vec<StatModifier>,
        #[serde(default)]
        spell: Option<String>,
    },
    /// Only carried: a key a door asks for, the coin a merchant takes, a
    /// letter someone is waiting on. What it is *for* is written wherever it is
    /// asked for — a door's `locked:`, a reply's `HasItems` — so pressing
    /// `confirm` on one in the bag does nothing, rather than spending it the
    /// way it would spend an effect-less consumable.
    Carried,
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
    /// The spell the player casts. Empty is the spark they were born with —
    /// the player's own `spell` — and a tome worn here replaces it. Last, so
    /// every save and every sum that walked four slots walks them unchanged.
    Spell,
}

impl Slot {
    /// Every slot, in the order the equipment pane lists them.
    pub const ALL: [Slot; 5] = [
        Slot::Head,
        Slot::Body,
        Slot::Weapon,
        Slot::Trinket,
        Slot::Spell,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Head => "Head",
            Slot::Body => "Body",
            Slot::Weapon => "Weapon",
            Slot::Trinket => "Trinket",
            Slot::Spell => "Spell",
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
/// replace.
///
/// **What Q-2 actually turned out to need**, having predicted `FlagAtLeast`:
/// that was already here, and so was `TakeItem` on the effect side. The one
/// thing missing was [`DialogueCondition::All`] — because the *return* leg of a
/// fetch quest is two questions at once ("have you taken the errand" and "are
/// you carrying the thing"), and one condition per choice can only ask one of
/// them. Without it a graph has to choose between offering "I have your helm."
/// to someone who was never asked for it, and duplicating the whole node.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum DialogueCondition {
    /// The player has at least one of this item, in the bag or worn.
    HasItem(String),
    /// The player has at least this many — `HasItems("coin", 10)` is a price.
    HasItems(String, u32),
    /// The inner condition does not hold: "has not been asked yet".
    Not(Box<DialogueCondition>),
    /// At least one of these holds. An empty list does not, which is the
    /// identity of an `any`.
    Any(Vec<DialogueCondition>),
    /// A quest flag is exactly this. An unset flag reads as 0.
    FlagEq(String, i64),
    /// A quest flag has reached at least this stage.
    FlagAtLeast(String, i64),
    /// Every one of these holds. Nested rather than a `Vec<DialogueCondition>`
    /// on the choice itself, because that would have changed the meaning of
    /// `condition:` in every graph already written — and because a combinator
    /// is where `Any` goes the day something needs one.
    ///
    /// An empty list holds, which is the identity a fold gives it and the same
    /// answer as no condition at all.
    All(Vec<DialogueCondition>),
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
    /// Add to a flag — a count rather than a stage: "the third bell rung".
    AddFlag(String, i64),
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
    /// rest of a combo is data: each attack names its own successor. Absent
    /// for a kind with no sword to swing — a bat bites by touching, a mage
    /// casts.
    #[serde(default)]
    pub attack: Option<String>,
    /// On the player's side: cannot be hurt by the player, is never hunted by
    /// an enemy's AI, and does not hunt. A villager. Everything else a map
    /// places is an enemy.
    #[serde(default)]
    pub friendly: bool,
    /// A hit dealt just by touching — a bat's bite, a slime's burn.
    #[serde(default)]
    pub contact: Option<ContactDef>,
    /// Carries a shield: a blow or a bolt from in front does nothing while it
    /// is ready — not swinging, not casting, not reeling — and throws the
    /// attacker back off it. From behind, from above, or while it is
    /// committed to a swing of its own, it is as open as anything. See
    /// `combat::guarded`.
    #[serde(default)]
    pub guard: bool,
    /// Too heavy to stagger: a blow still hurts and still shoves, but it does
    /// not stun — so a combo does not keep it helpless, and it swings back
    /// through yours. See `combat::apply_hit`.
    #[serde(default)]
    pub steadfast: bool,
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
    /// Present only for a kind that fights with the player's own kit — a
    /// rival champion. With an `avatar` group beside it, a map places one as
    /// an avatar with a brain at its controls rather than as something that
    /// walks a route.
    #[serde(default)]
    pub brain: Option<BrainStats>,
}

/// How a fighter with the player's kit decides what to press. Spelled
/// `BrainStats(...)` in the RON file; [`crate::systems::brain`] is what reads
/// it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "BrainStats")]
pub struct BrainStats {
    /// It wakes when the other side comes this close, centre to centre, and
    /// fights until one of them is down.
    pub sight: f32,
    /// Ticks between decisions. It holds what it chose in between, so this is
    /// its reaction time: 10 is a sixth of a second, about a person's; lower
    /// is harder.
    pub reaction: u32,
    /// Throws its spell at anything inside this gap and out of sword reach.
    /// Zero for a fighter that never casts.
    #[serde(default)]
    pub cast_range: f32,
    /// Takes to the air: jumps up to a target above it, and plunges onto one
    /// below.
    #[serde(default)]
    pub aerial: bool,
}

/// What touching one of these does to the other side. Spelled
/// `Contact(...)` in the RON file.
///
/// Dealt through the same `combat::apply_hit` a sword and a bolt go through,
/// so a bite respects i-frames and emits `damaged` like any other blow. After
/// landing one, a hostile kind backs off for its `ai.cooldown` — which is what
/// turns "stuck to the player" into a swoop.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Contact")]
pub struct ContactDef {
    pub damage: i32,
    /// Impulse on whatever it touches, pointing away from it.
    pub knockback: (f32, f32),
    pub hitstun: u32,
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
    /// One-way platforms are ignored for this long after pressing down on one.
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
    /// ...or this far above or below. Much looser than `sight_height`, which
    /// is roughly a body: a chase that ended the moment the player jumped was
    /// a chase any player could end at will.
    pub lose_height: f32,
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
    /// Flies: no gravity while alive, no ledges to turn at, and the player is
    /// seen in every direction rather than only in front. Dies and falls like
    /// anything else.
    #[serde(default)]
    pub flying: bool,
    /// Casts its `spell` at a player within this many pixels, and does not
    /// close further than this to do it. Zero for a kind that never casts.
    #[serde(default)]
    pub cast_range: f32,
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

/// One burst of particles: what a hit, a death or a landing looks like.
/// Spelled `Burst(...)` in `assets/data/effects.ron`.
///
/// Every range is picked from uniformly, per particle, off the view's own
/// generator — presentation never draws on [`crate::sim::rng`], so tuning an
/// effect cannot move a trace.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Burst")]
pub struct EffectDef {
    pub count: u32,
    /// Launch speed, px/s.
    pub speed: (f32, f32),
    /// Launch direction in degrees, screen-wise: 0 is right, 90 down, 270 up.
    pub angle: (f32, f32),
    /// How many ticks each particle lives. It fades over the second half.
    pub life: (u32, u32),
    /// Each particle is a square this many pixels across.
    pub size: (f32, f32),
    /// px/s², down. Negative rises — smoke, sparks off a flame.
    pub gravity: f32,
    /// Each particle is one of these, 0-255 RGB.
    pub colours: Vec<(u8, u8, u8)>,
}

/// Every effect, by the name the view asks for it by. Spelled
/// `Effects({...})` in the RON file; [`crate::view::fx::CUES`] lists the
/// names, and `tests/data.rs` checks each is here.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename = "Effects")]
pub struct EffectTable(pub HashMap<String, EffectDef>);

impl EffectTable {
    pub fn get(&self, name: &str) -> Option<&EffectDef> {
        self.0.get(name)
    }

    /// Ranges the right way round, and something to draw.
    fn validate(&self) -> anyhow::Result<()> {
        let mut names: Vec<&String> = self.0.keys().collect();
        names.sort();
        for name in names {
            let def = &self.0[name];
            anyhow::ensure!(
                def.count > 0 && !def.colours.is_empty(),
                "effect `{name}` has no particles, or no colours to draw them in"
            );
            anyhow::ensure!(
                def.speed.0 <= def.speed.1
                    && def.angle.0 <= def.angle.1
                    && def.life.0 <= def.life.1
                    && def.size.0 <= def.size.1
                    && def.life.0 > 0
                    && def.size.0 > 0.0,
                "effect `{name}`: every range is (low, high), and a particle needs life and size"
            );
        }
        Ok(())
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
    /// Named pieces of the atlas a map can place by name — `Decor(prop:
    /// "torch")`, a door's `art:` — as `(column, row, width, height)` in
    /// tiles. The same rule autotiling keeps: maps never contain tile indices,
    /// so repainting a tileset never means editing a map.
    #[serde(default)]
    pub props: HashMap<String, (u32, u32, u32, u32)>,
    /// Multiplies every tile, prop and piece of decor drawn from this set:
    /// the same stone in another light. Absent is the art as painted.
    #[serde(default)]
    pub tint: Option<(u8, u8, u8)>,
    /// What shows where nothing is drawn — a night sky, the dark of a hold.
    /// Absent is the view's default dusk.
    #[serde(default)]
    pub clear: Option<(u8, u8, u8)>,
}

impl TilesetDef {
    /// The colour everything drawn from this set is multiplied by.
    pub fn tint_color(&self) -> crate::render::Color {
        self.tint.map_or(crate::render::Color::WHITE, |(r, g, b)| {
            crate::render::Color::from_rgb(r, g, b)
        })
    }

    /// The pixel rectangle a named prop occupies on the tileset's image.
    pub fn prop_rect(&self, name: &str) -> Option<Rect> {
        let &(col, row, w, h) = self.props.get(name)?;
        let ts = self.tile_size as f32;
        Some(Rect::new(
            col as f32 * ts,
            row as f32 * ts,
            w as f32 * ts,
            h as f32 * ts,
        ))
    }

    /// The pixel rectangle a tile index occupies on the tileset's image.
    pub fn tile_rect(&self, tile: u32) -> Rect {
        let ts = self.tile_size as f32;
        Rect::new(
            (tile % self.columns) as f32 * ts,
            (tile / self.columns) as f32 * ts,
            ts,
            ts,
        )
    }

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
    effects: Option<Arc<EffectTable>>,
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
            effects: None,
        }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base
    }

    /// Decode the image called `name` to RGBA, applying an optional color key.
    ///
    /// An image is `assets/graphics/{name}.png` or, failing that,
    /// `assets/graphics/{name}.ron` — pixel art written as text, see
    /// [`PixelArt`]. One namespace for both, so a clip set, an item's `sprite:`
    /// or a tileset names an image without knowing which kind it is, and art
    /// drawn as text today can be replaced by a PNG tomorrow without touching
    /// anything that names it.
    ///
    /// Split out of [`Assets::image`] because it needs no graphics context,
    /// which lets asset checks inspect exactly the pixels the game uploads.
    pub fn decode_image(
        &self,
        name: &str,
        color_key: Option<(u8, u8, u8)>,
    ) -> anyhow::Result<image::RgbaImage> {
        let path = self.base.join("graphics").join(format!("{name}.png"));
        let pixels = self.base.join("graphics").join(format!("{name}.ron"));
        if !path.exists() && pixels.exists() {
            let art: PixelArt = load_ron(&pixels)?;
            return art
                .to_image()
                .with_context(|| format!("invalid pixel art {}", pixels.display()));
        }
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
        let read = |name: &str| -> anyhow::Result<ClipSet> {
            load_ron(
                &self
                    .base
                    .join("data/animations")
                    .join(format!("{name}.ron")),
            )
        };
        let mut set = read(name)?;
        if let Some(base) = set.base.clone() {
            let from =
                read(&base).with_context(|| format!("clip set `{name}` is built on `{base}`"))?;
            anyhow::ensure!(
                from.base.is_none(),
                "clip set `{name}` is built on `{base}`, which is built on another in turn — \
                 one level only"
            );
            set.inherit(from);
        }
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

    /// Load `assets/data/effects.ron`. One table for the whole game.
    pub fn effects(&mut self) -> anyhow::Result<Arc<EffectTable>> {
        if let Some(table) = &self.effects {
            return Ok(table.clone());
        }
        let path = self.base.join("data/effects.ron");
        let table: EffectTable = load_ron(&path)?;
        table
            .validate()
            .with_context(|| path.display().to_string())?;
        let table = Arc::new(table);
        self.effects = Some(table.clone());
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

/// An image drawn as text: a palette of characters and the frames they paint.
/// Spelled `Pixels(...)` in `assets/graphics/**/*.ron`.
///
/// This is the art pipeline for an author who cannot open a paint program.
/// Every pixel is a character an agent can write, diff and review, and the
/// result is an ordinary image to everything downstream — a clip set names it
/// as `sheet: "bat"` exactly as it would name a PNG. `cargo run --bin sheet --
/// --image <name>` draws one large enough to look at.
///
/// ```ron
/// Pixels(
///     palette: { 'k': (24, 20, 37), 'r': (190, 38, 51) },
///     frames: [
///         [".kk.", "krrk", "krrk", ".kk."],
///     ],
/// )
/// ```
///
/// `.` and space are transparent in every palette. Frames are laid left to
/// right in one strip, so frame *i* is at cell `(i, 0)` of a clip whose
/// `frame_size` is one frame's size. Every frame and every row must be the same
/// size, and a character missing from the palette is an error naming it —
/// a typo in art should fail the load, not draw a hole.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Pixels")]
pub struct PixelArt {
    pub palette: HashMap<char, (u8, u8, u8)>,
    pub frames: Vec<Vec<String>>,
}

impl PixelArt {
    /// Paint the frames into one strip.
    pub fn to_image(&self) -> anyhow::Result<image::RgbaImage> {
        let first = self.frames.first().context("pixel art has no frames")?;
        let height = first.len();
        let width = first.first().map_or(0, |row| row.chars().count());
        anyhow::ensure!(width > 0 && height > 0, "a frame must have pixels in it");

        let mut image = image::RgbaImage::new((width * self.frames.len()) as u32, height as u32);
        for (index, frame) in self.frames.iter().enumerate() {
            anyhow::ensure!(
                frame.len() == height,
                "frame {index} is {} rows tall; frame 0 is {height}",
                frame.len()
            );
            for (y, row) in frame.iter().enumerate() {
                anyhow::ensure!(
                    row.chars().count() == width,
                    "frame {index}, row {y} is {} wide; frame 0 is {width}: {row:?}",
                    row.chars().count()
                );
                for (x, ch) in row.chars().enumerate() {
                    if ch == '.' || ch == ' ' {
                        continue;
                    }
                    let Some(&(r, g, b)) = self.palette.get(&ch) else {
                        let mut known: Vec<char> = self.palette.keys().copied().collect();
                        known.sort_unstable();
                        anyhow::bail!(
                            "frame {index}, row {y}, column {x}: `{ch}` is not in the palette \
                             (it has {known:?}; `.` and space are transparent)"
                        );
                    };
                    image.put_pixel(
                        (index * width + x) as u32,
                        y as u32,
                        image::Rgba([r, g, b, 255]),
                    );
                }
            }
        }
        Ok(image)
    }
}

/// Parse a RON data file.
///
/// `implicit_some` is on so that an optional field can be written as
/// `sheet: "knight/knightIdle"` rather than `sheet: Some("knight/knightIdle")`.
/// These files are hand-authored content; making every optional field announce
/// its optionality is noise for whoever is writing the twentieth NPC.
pub(crate) fn load_ron<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
) -> anyhow::Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(text: &str) -> PixelArt {
        ron::from_str(text).expect("pixel art parses")
    }

    #[test]
    fn pixel_art_paints_its_frames_into_one_strip() {
        let image = art(r#"Pixels(
                palette: { 'r': (255, 0, 0), 'b': (0, 0, 255) },
                frames: [["r.", ".r"], ["b.", " b"]],
            )"#)
        .to_image()
        .unwrap();
        assert_eq!(image.dimensions(), (4, 2), "two 2x2 frames side by side");
        assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0[3], 0, "`.` is transparent");
        assert_eq!(
            image.get_pixel(3, 1).0,
            [0, 0, 255, 255],
            "frame 1 at x 2..4"
        );
        assert_eq!(image.get_pixel(2, 1).0[3], 0, "space is transparent");
    }

    #[test]
    fn a_character_missing_from_the_palette_names_itself() {
        let err = art(r#"Pixels(palette: { 'r': (1, 2, 3) }, frames: [["rq"]])"#)
            .to_image()
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("`q`") && text.contains("column 1"), "{text}");
    }

    /// An empty clip passes every headless check — the animator skips it —
    /// and then crashes the first draw that reaches it, so it fails at load.
    #[test]
    fn a_clip_with_no_frames_or_no_speed_is_rejected_at_load() {
        for clip in [
            "Clip(frames: [], fps: 8.0, looping: true)",
            "Clip(frames: [(0, 0)], fps: 0.0, looping: true)",
            "Clip(frames: [(0, 0)], fps: 8.0, looping: true, frame_size: (0.0, 4.0))",
        ] {
            let set: ClipSet = ron::Options::default()
                .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
                .from_str(&format!(
                    r#"ClipSet(sheet: "x", frame_size: (8.0, 8.0), clips: {{ "idle": {clip} }})"#
                ))
                .unwrap();
            assert!(set.validate("test").is_err(), "{clip} was accepted");
        }
    }

    #[test]
    fn ragged_frames_are_rejected() {
        assert!(art(r#"Pixels(palette: {}, frames: [["..", "..."]])"#)
            .to_image()
            .is_err());
        assert!(
            art(r#"Pixels(palette: {}, frames: [[".."], ["..", ".."]])"#)
                .to_image()
                .is_err()
        );
    }
}
