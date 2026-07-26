# Roadmap

Working order for the next stretch of SuperGame: NPCs, combat, dialogue, items,
quests, new maps, and live map features (fire, moving platforms, swinging
hazards).

This is the *sequencing* document — what to build next and what has to be true
first. [PLAN.md](PLAN.md) remains the design document: it holds the data
schemas (`ItemDef`, `NpcDef`, dialogue graphs, `SpellDef`), the architecture
rules, and the story premise. Where the two disagree about order, this file
wins; where they disagree about shape, PLAN.md wins. Known drift in PLAN.md is
listed at the bottom.

## Where things stand

Done: hecs ECS on a fixed 60 Hz timestep, scene stack, sprite/animation system
driven by RON clip sets, ASCII and Tiled map loading, and a full movement kit —
run, jump with coyote time and buffering and variable height, double jump, wall
slide and wall jump, drop-through platforms, hazard and fall death with respawn.

Also done, and the reason this roadmap can be written the way it is: a
verification harness that runs the real simulation headlessly. Input tapes,
gameplay events with `expect` assertions, golden traces checked on every
`cargo test`, inline ASCII fixtures, property sweeps over the collision system,
and an F1 debug overlay. See [tapes/README.md](tapes/README.md).

Not done: anything with a second entity in it.

## The organizing principle

The codebase is in good shape, but essentially every abstraction in it is
shaped like *one player*. Movement lives inside a `&mut Avatar` query. The
snapshot function panics if there is no avatar and returns exactly one
entity's state. Collision geometry is flattened once at load and never changes.
The renderer has a single `player_image`. Entity spawns are parsed out of maps
and dropped on the floor. Movement constants are `const`s on a component.

None of that is wrong today, and all of it has to change. So the milestones
below alternate: an enabling refactor, then the features it unlocks. The
refactors are not optional prep work — each one is the thing that makes the
next feature cheap instead of a rewrite.

Two rules that fall out of this, worth stating once:

**Gameplay logic goes in `Sim`, never in `scenes/`.** The entire premise of the
harness is that the real simulation runs headlessly. Anything that lives in a
scene is untestable by definition. This is most tempting — and most damaging —
for dialogue and inventory, which feel like UI but are simulation.

**A feature is not done until the harness can see it.** Each milestone below
names the harness work it needs. Build that first or alongside, not after; the
tooling is much harder to retrofit once there is content depending on the
current shape.

---

## M1 — Split movement into intent and body

**Size:** small. **No new features.** Do this first.

Add a `Body` component (velocity, grounded, prev_pos, gravity scale) and a
system that moves every `Position + Velocity + Size + Body` against geometry.
`avatar::update` shrinks to translating `PlayerInput` into velocity; anything
else that needs to fall and collide reuses the same system.

**Why first:** nothing else can start. An NPC that walks off a ledge needs
gravity, and today not one line of that is reachable from outside the avatar
query. It is also the cheapest moment to do it — the golden traces were
recorded specifically to make this refactor safe, and their value decays with
every feature built on the current shape.

**Exit criteria:** `cargo test` green **and zero trace diffs**. This is a pure
refactor, so any trace movement at all is a bug. That is a rare and very clean
success condition; take advantage of it.

## M2 — NPCs

**Size:** medium. **Unlocks:** everything downstream.

Spawn registry mapping `EntitySpawn.kind` to a spawn function, so the `K` in a
map and the `Npc(kind: "...")` entries finally do something. `NpcDef` in RON
(stats, clip set, AI behavior, loot table) per PLAN.md. A patrol AI that writes
to the same `Body` the player uses.

**Harness work — do it with this milestone, not after:**

- `Sim` holds `player: Entity`; `probe()` stops panicking and stops assuming.
- `Probe` becomes a keyed world snapshot (entities by name, plus globals), with
  tape assertions as path expressions: `player.x`, `npc.goblin.hp`. The
  hand-maintained `FIELD_NAMES`/`FLAG_NAMES` tables and their `match` arms do
  not survive contact with a second entity type.
- Deterministic RNG owned by `Sim`, seeded per run. AI jitter, crits, and loot
  rolls all want randomness, and one `rand::thread_rng()` anywhere destroys
  tapes, traces, and reproducibility together.
- A determinism guard against archetype reordering: hecs iterates archetypes in
  creation order, and adding or removing a component mid-run (a stun, a damage
  flash) moves an entity between archetypes and can change iteration order. The
  current determinism test cannot catch this because one entity has no order to
  get wrong. Test: spawn, despawn, and mutate components mid-run, and still
  reproduce a byte-identical trace.

Design the snapshot format *with* the second entity in hand, not before it.

**Exit criteria:** a knight patrols the castle map, is visible in a trace, and a
tape can assert about it by name. Two runs of the same tape are byte-identical.

## M3 — Combat

**Size:** large. **Depends on:** M1, M2.

Health and mana components, i-frames, knockback, attack states driving timed
hitbox queries, a three-hit ground combo, air attacks, the shock spell as the
first `SpellDef` projectile, enemy death and despawn, and a health/mana HUD.

**Harness work:** combat is almost entirely event-shaped, which is why events
landed in Tier 1. Add `Damaged`, `Died`, `Attacked`, `SpellCast`, `Blocked`
with enough payload to assert on. Tapes need attack and cast inputs, which
means `Keys` and `PlayerInput` stop being fixed structs and become a named
action set — one definition feeding the keyboard reader, the tape parser, and
eventually rebinding.

While here, consider moving movement and combat constants out of `Avatar`
consts into a `Stats` component loaded from RON. Enemies need per-entity stats
anyway, and tuning without a recompile is worth losing `Avatar::JUMP_SPEED` as
a compile-time constant.

**Exit criteria:** fight the knight, take damage, die, respawn — all of it
asserted by a tape, with the damage numbers coming from data files.

## M4 — Items and inventory

**Size:** medium. **Depends on:** M2 (pickups are entities), M3 (loot drops).

`ItemDef` pipeline per PLAN.md: inventory component, pickups, consumables,
equipment slots with stat modifiers, weapon defs feeding melee damage. Stats
computed as `base + sum(modifiers)`, never mutated in place.

**Architectural note:** the inventory screen is the first *modal* state — it
pauses the world but is itself simulation, since using a potion changes game
state. This is where `Sim` gets a mode enum (`Playing | Inventory | …`) and
`step` dispatches on it. Getting this pattern established here, on the simpler
of the two modal features, is deliberate: dialogue inherits it.

**Exit criteria:** kill the knight, loot drops, pick it up, equip it, stats
change — asserted headlessly, no window.

## M5 — Interaction and dialogue

**Size:** medium. **Depends on:** M2, M4 (mode enum).

`Interactable` component and a "press E" prompt, friendly NPC archetype, a
dialogue graph in RON (nodes, speakers, lines, choices with conditions and
effects), and a dialogue mode in `Sim` with a thin overlay scene that only
draws it.

**The one thing not to get wrong:** dialogue belongs in `Sim`. It is modal, so
a scene feels like the natural home, but choices mutate quest state and
inventory — it is simulation wearing a UI hat. Put it in a scene and it becomes
the single largest system in the game that cannot be tested without a human.

**Harness work:** tapes need a way to select dialogue choices, and events for
`DialogueOpened`, `ChoiceTaken`, `DialogueClosed`.

**Exit criteria:** a tape walks an NPC conversation, takes a branch gated on an
inventory item, and asserts the resulting state change.

## M6 — Quests

**Size:** small once M5 lands. **Depends on:** M5.

Quest flags as global sim state, dialogue conditions and effects reading and
writing them, a fetch quest as proof, and persistence so progress survives a
save/load.

Quests are mostly flags plus dialogue, which is why they are cheap here and
would have been expensive earlier. They surface in the world snapshot as
globals, so `expect quest.rescue.stage == 2` works without new machinery.

**Exit criteria:** accept a quest, complete it, get a reward, and have it
survive save and load.

## M7 — Live map features

**Size:** medium. **Depends on:** M1. **Parallelizable** — independent of the
combat/dialogue chain, so it can slot in wherever there is appetite.

Fire (a hazard that toggles on a cycle), moving platforms, and swinging
hazards. All three need the same thing: **geometry that changes every tick**.

This is the deepest change to the physics layer on the roadmap:

- Colliders owned by entities, not just flattened from the level at load.
- **Rider carry** — standing on a moving platform has to transport you. This is
  the genuinely tricky part and the source of most platformer physics bugs;
  budget accordingly and write property tests, not just examples.
- A broadphase (uniform grid). The current linear scan over `Vec<SolidRect>` is
  fine for one body and wasteful for thirty NPCs plus projectiles on a large
  map. Introduce the interface early even if the naive implementation stays.

Fire is the cheap one — a hazard rect with a schedule — and is a good first
slice to prove entity-owned colliders before taking on rider carry.

**Exit criteria:** a tape rides a moving platform across a gap and arrives; the
property sweeps in `tests/physics_diagnostics.rs` still pass with dynamic
geometry in the mix.

## M8 — Content: new maps

**Size:** ongoing. **Depends on:** M2 for spawns; benefits from everything.

The village hub, the dungeon, the "advanced world" — plus the story from the
SuperGame premise. Mostly authoring, not engineering, which is the goal.

**The engineering part that makes it safe** is a `tests/data.rs` that
cross-references every content file: every dialogue node's `next` target
exists, every quest reward is a real item id, every `NpcDef` names a clip set
that loads, every map's entity kinds are registered. This extends the pattern
in `tests/assets.rs` and `tests/levels.rs` and kills the entire class of
typo-in-a-RON-id bugs that otherwise dominates content work. Cheap to write,
and it pays back the first time a map references a deleted NPC.

Every new map also wants a smoke tape — spawn is on solid ground, the level is
traversable, nothing is unreachable.

---

## Cross-cutting

Work that does not belong to one milestone.

**Renderer.** Currently one `player_image` and a hardcoded avatar query. NPCs
need per-entity sprite sheets, which means `Sprite` carries a sheet handle, the
scene keeps an image cache, and draw order becomes explicit z-layers. Needed by
M2, so do it there; just do not be surprised when it also needs doing for
items, projectiles, and UI.

**Save system.** PLAN.md specifies rusqlite. Needed by M6, useful from M4.
Whatever it stores has to be reconstructible into a `Sim`, which is a good
forcing function for keeping game state out of scenes.

**CI and formatting.** No CI today. `cargo test` plus `clippy -D warnings` plus
`fmt --check` in a GitHub action. The repo is not currently rustfmt-clean
(pre-existing drift in `physics.rs`, `debug.rs`, `probe.rs`, `tests/`,
`bin/sim.rs`), so that wants one mechanical formatting commit of its own rather
than being smuggled into a feature diff.

**CLAUDE.md.** A lot of hard-won design intent lives only in doc comments — the
strict-overlap reasoning in `physics.rs`, the `JumpLatch` rationale in
`input.rs`, the frozen-fixture rule for testbed maps. A root CLAUDE.md pointing
at the invariants would save rediscovering them every session.

## Harness capability by milestone

The through-line. Each row is tooling that must exist before the feature it
serves, not after.

| Milestone | Harness work |
| --- | --- |
| M1 Body split | none — the point is that the traces already cover it |
| M2 NPCs | world-snapshot `Probe`, path assertions, seeded RNG, ordering guard |
| M3 Combat | combat events, action-set inputs (attack/cast) in tapes |
| M4 Items | inventory in the snapshot, pickup/equip events |
| M5 Dialogue | choice selection in tapes, dialogue events |
| M6 Quests | quest flags as snapshot globals |
| M7 Map features | property tests for dynamic geometry and rider carry |
| M8 Content | `tests/data.rs` cross-reference validation, per-map smoke tapes |

## Known drift in PLAN.md

PLAN.md was written before Phases 0–3a shipped and is stale in these specific
ways. Its designs are still good; its status claims are not.

- The endless runner it treats as the architecture's canary was deleted
  (commit `e876092`); adventure mode is the only mode.
- It says level design happens in Tiled. ASCII RON maps are the primary
  authoring format now, because they are readable and writable by an agent; the
  `.tmx` loader still exists for the legacy castle level.
- It folds movement and combat into one Phase 3. Movement (3a) shipped;
  combat is M3 here.
- It has no notion of the enabling refactors in M1 and M2, because the
  one-player shape of the code was not visible until there was a reason to add
  a second entity.
- Its event queues were removed in `e876092` and are back in a different form:
  `GameEvent` exists for verification, and is the natural substrate for the
  "events over direct calls" rule it asks for.

## Open questions

- **Movement feel has never been play-tested.** The harness proves correctness
  and cannot say anything about feel. The constants in `ecs/components.rs` are
  unjudged, and M3 is a natural moment to tune them — ideally before combat
  timing is balanced against them.
- **Asset licensing** for the itch.io packs (Adventurer sprite, knight pack,
  castle tiles) is still unverified. Blocking for public distribution only.
- **Scope of the RPG layer.** PLAN.md's item/equipment system is fairly deep.
  Worth deciding at M4 whether that depth is wanted or whether a lighter
  inventory serves the game better.
