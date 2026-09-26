# CLAUDE.md

A 2D side-scroller in Rust (ggez + hecs). This file is a **map, not a manual**:
the reasoning behind every rule below is already written next to the code that
depends on it, so each entry points at where to read it rather than repeating
it. It is also the whole of the workflow — there is no separate skill file.

## Where to look first

1. **[ROADMAP.md](ROADMAP.md)** — what to build next and what must be true
   first. Supersedes PLAN.md's phase list; its "Known drift" section records
   where PLAN.md and the shipped code deliberately disagree.
2. **[TICKETS.md](TICKETS.md)** — that, broken into pickup-able work, each with
   acceptance criteria and its named tape.
3. **[PLAN.md](PLAN.md)** — the designs: RON schemas, architecture rules,
   story. Its *status* claims are stale; its designs are not.
4. **[tapes/README.md](tapes/README.md)** — the tape and trace language, the
   assertion paths, the events, and what each fixture map is for.

## The tick

`Sim::step` dispatches on the mode; `Sim::step_playing` is the world — decide,
move, react, with movement as one pass over every body. Read both doc comments
in [`src/sim/mod.rs`](src/sim/mod.rs) before adding a system: **a new system
goes into the phase it belongs to, never onto the end.**

| Phase | Belongs here |
| --- | --- |
| `combat::tick_timers`, `advance_attacks`, `spell::advance_casts` | i-frames, hitstun, swing and cast progress; a cast releases its bolt here |
| `hazard::tick_schedules`, `pendulum::advance`, `props::tick_gates` | geometry that owns itself deciding *where* — or whether — it is this tick |
| `brain::think`, `avatar::control`, `npc::think` | read input / AI — a rival's brain writes the input its avatar then reads — and set a velocity and `Body` knobs |
| `mover::advance` | **last** decision: platforms move and carry riders |
| `body::rebuild_geometry` | **the one point per tick where collision geometry changes** |
| `body::move_bodies` | gravity, integration, collision — every body, one pass |
| `avatar::after_move`, `combat::environmental_deaths` | landing, bounds, hazard and fall death — the player's, then everyone's |
| `combat::resolve`, `spell::resolve_projectiles`, `combat::contact_hits`, `inventory::collect_pickups`, `update_prompt`, `settle_dead`, `death_flags`, `touch_props`, `drop_loot` | tested at final positions: swings, bolts, bites, pickups, what is in reach, exits and checkpoints |
| `animation::select_*`, `advance` | pick a clip, advance frames |
| pending travel | an exit walked into this tick; the finished world is replaced last |

- Controllers only set a velocity and `Body` knobs (`gravity`, `fall_cap`,
  `ignore_one_way`, `frozen`). They never integrate or collide. A flyer is a
  controller setting `gravity` to zero while it lives.
- Anything that *is* geometry owns a `Collider` and moves it in the decide
  phase; the rebuild picks it up the same tick and collision never learns it
  exists. [`src/systems/hazard.rs`](src/systems/hazard.rs) is the cheapest
  example, a gate ([`src/systems/props.rs`](src/systems/props.rs)) the
  simplest; `mover::advance`'s slot is load-bearing in three directions at
  once, each argued in [`src/systems/mover.rs`](src/systems/mover.rs).
- Physics asks a `SolidQuery`, never a `Vec<SolidRect>`, so a query must stay a
  *subsequence* of the full list: reordering it moves the game
  ([`src/physics.rs`](src/physics.rs),
  [`src/systems/body.rs`](src/systems/body.rs)).
- Tick-driven geometry reads `Sim::world_tick()` (the tick minus time spent in
  menus), not `Sim::tick` — so opening the bag does not move a platform.

## `Sim`, `view/`, `scenes/`

**Gameplay logic lives in `Sim`; everything else draws and decides nothing.** A
scene is untestable by definition — no tape can press a key only a
`ggez::Context` sees.

- Inventory and dialogue feel like UI and are simulation: a potion changes
  health, a reply takes an item and sets a flag. Both live in
  [`src/systems/inventory.rs`](src/systems/inventory.rs) and
  [`src/systems/dialogue.rs`](src/systems/dialogue.rs); `Mode { Playing,
  Inventory, Dialogue }` on `Sim` freezes the world but is still stepped, so a
  tape drives the whole screen. Pause is deliberately *not* a mode.
- Doors, chests, levers, checkpoints and triggers (walk in, a conversation
  opens, once) are simulation too, and what they have done is a flag — see the
  module doc of [`src/systems/props.rs`](src/systems/props.rs). A trigger's
  dialogue opens at the end of the tick, after any travel.
- Drawing is data: a frame is a list of `Cmd`s built from a `Sim` with no
  graphics context ([`src/render/mod.rs`](src/render/mod.rs)), drawn by the GPU
  backend in the window and by the CPU backend headlessly.
  [`src/view/`](src/view/mod.rs) holds what advances with the sim but is not
  simulation — the camera, and in [`src/view/fx.rs`](src/view/fx.rs) particles,
  shake, fades, toasts and the map title, all *derived* from events and from
  diffing the world. The sim never reads any of it back.

## Making things

Content is data; a new thing should be a file, not a code change.

| To add | Write | Checked by |
| --- | --- | --- |
| an enemy or NPC | a block in `assets/data/stats.ron` and `assets/data/animations/<kind>.ron`. What it *is* follows from what the block has: `friendly`, an `attack`, a `spell` + `ai.cast_range`, a `contact` hit, `ai.flying`, `guard` (a shield that turns blows from the front; a plunge goes through), `steadfast` (hurt and shoved, never staggered) — see `spawn::entity` in [`src/ecs/spawn.rs`](src/ecs/spawn.rs) | `tests/data.rs`, `tests/assets.rs`, `tests/enemies.rs` |
| a rival (a duel) | a block with `avatar:` and `brain:` (sight, reaction, cast range, aerial) and a clip set with the player's clips (`base: "player"`). It fights with the player's own controller and combo, flown by [`src/systems/brain.rs`](src/systems/brain.rs); a death flag on its `Npc` makes the win stick | `tests/enemies.rs`, `tests/assets.rs` |
| borrowed art | a clip set with `base: "knight"`, a `tint`, and only the clips that differ | `ClipSet::base` in [`src/assets.rs`](src/assets.rs) |
| sprite art | text: `Pixels(palette, frames)` in `assets/graphics/**/<name>.ron`, usable anywhere a PNG name is | `PixelArt` in [`src/assets.rs`](src/assets.rs) |
| a map | an ASCII grid plus an entity list — `Npc`, `Door`, `Exit`, `Spawn`, `Chest`, `Item`, `Checkpoint`, `Sign`, `Lever`, `Gate`, `Trigger`, `Decor`, `Fire`, `Platform`, `Swing`, and a `name:`. `%` in the grid is a false wall: drawn as stone, walked through | module doc of [`src/level/ascii.rs`](src/level/ascii.rs); `tests/levels.rs`, `tests/data.rs`, `tests/render.rs` |
| an area's look | a tileset in `assets/data/tilesets/`: an atlas and its autotile rules. `tint` re-lights another tileset's atlas (`beacon`, `ark_deep`, `ark_heart`) and `clear` is the colour behind everything | `tests/render.rs`; look at it with `render --level` |
| an item | `assets/data/items/*.ron` and a 12x12 icon under `assets/graphics/items/`. A tome is equipment in the `Spell` slot, and its `spell:` is what `cast` throws while it is worn | `tests/data.rs` |
| a conversation, a quest, a shop | `assets/data/dialogue/*.ron`: conditions (`HasItem`, `HasItems`, `FlagEq`, `All`, `Any`, `Not`, …) gate replies, effects (`GiveItem`, `TakeItem`, `SetFlag`, `AddFlag`) are atomic. [`pedlar.ron`](assets/data/dialogue/pedlar.ron) is a shop | `tests/data.rs` (graphs connected, every flag read is written) |
| an effect | a `Burst` in `assets/data/effects.ron` | `tests/data.rs` |

## Verification

Everything runs headlessly; verify gameplay yourself rather than asking for a
playtest.

- `cargo test` — 39 tapes and their golden traces replay here, plus the unit,
  property, content and render suites.
- `cargo run --bin sim -- --tape tapes/x.tape [--trace out.jsonl]` — one tape,
  with an event timeline. `--geometry` prints a map's rects, platforms and NPCs
  by the index a tape uses. `--fight warden.0` plays a fight after the tape,
  the player flown by the rivals' brain, and prints the inputs that won as tape
  lines — how every fight in a real-level tape was written, and how to
  re-record one. It does not platform: write the climb by hand and record
  from the top.
- `cargo run --bin render -- --tape tapes/x.tape --at 120` (or `--every 60`,
  or `--map maps/x.ron --level --debug`) — the frame the game would draw, as a
  PNG under `target/render/`. **Look at it** after anything visual.
- `cargo run --bin sheet -- <kind>`, `--image <name>` — traces record clip
  *names*, not which sheet cells they resolve to, so animation *mapping* is
  checkable only by eye.
- `SUPERGAME_DEBUG=1`, or F1 in game — colliders, sprite bounds, hitboxes.
- `Sim::fixture(&["..P...", "######"])` — a level and player inline in a test;
  `tests/enemies.rs` shows placing any kind on one.

**Golden traces.** They catch a refactor that quietly changed the game. Never
re-record one you have not accounted for: adding a probe field rewrites every
baseline, which looks identical to a physics change. Prove it first —
`TRACE_IGNORE=field,other cargo test --test traces` drops those fields from
both sides, and zero remaining differences is the proof — then
`UPDATE_TRACES=1 cargo test --test traces`. When the change is not a whole
field (a new NPC in one map), diff old against new with exactly the addition
stripped, and say so.

**Writing a tape.** Balance and timing by probing, never by arithmetic: run
variants through `sim --trace -`, look, pick. Assert outcomes, not timings, for
anything involving two entities. `reload` in a tape saves and loads mid-run.

**Done means:** `cargo fmt --all --check`, `cargo clippy --all-targets -- -D
warnings` and `cargo test` clean (CI runs exactly these); every trace change
accounted for; a tape or test that fails if the feature is deleted; colliders
moved in the decide phase; anything modal behind a `Mode`; anything visual
looked at with `render` or `sheet`; ROADMAP.md and TICKETS.md updated.

## Invariants

- **The player is the `Avatar` without a `Brain`.** A rival is an avatar too —
  the same controller, body and combo — so anything that means *the player*
  asks `avatar::player` or queries `.without::<&Brain>()`: the HUD, the camera,
  saves, pickups, the probe ([`src/systems/avatar.rs`](src/systems/avatar.rs)).
- **Never despawn an NPC.** They are addressed by spawn index (`knight.0`);
  removing one silently repoints every later assertion. Mark dead, freeze, stop
  colliding — `Sim::npcs` in [`src/sim/mod.rs`](src/sim/mod.rs).
  The two exceptions are **projectiles** and **pickups**, different for a
  stated reason: nothing addresses one, so nothing can be renumbered, and
  leaving them as corpses is the actual leak — see the module docs of
  [`src/systems/spell.rs`](src/systems/spell.rs) and
  [`src/systems/inventory.rs`](src/systems/inventory.rs).
- **A save holds the player and the flags, never the rest of the world.**
  Loading, or leaving a map and coming back, rebuilds the map from its file and
  its flags: an opened chest stays open, an ordinary enemy comes back. An NPC
  whose death must stick carries a death flag (`Npc(…, flag: "quest.x")`) —
  [`src/save.rs`](src/save.rs), `spawn::customise`.
- **`Body::frozen` stops movement dead; `Health::hitstun` only suppresses the
  controller.** Knockback needs the second — a hit takes away steering, not
  momentum ([`src/ecs/components.rs`](src/ecs/components.rs)).
- **Anything whose order is observable is sorted by entity id.** hecs iterates
  archetypes in creation order, so adding or removing a component mid-run
  reshuffles query order: `Sim::npcs()`, hits, swings, casts, bites.
- **Fixture maps under `assets/maps/testbed*.ron` are frozen** — tapes encode
  their exact pixels. Add to them; do not move what exists (tapes/README.md).
  The adventure maps are not frozen, but twelve tapes walk them: add, don't
  move.
- **`Aabb::overlaps` is strict**: sharing an edge is not an intersection. An
  inclusive test snaps bodies onto phantom ledges at every tile seam — the
  comment on it in [`src/physics.rs`](src/physics.rs) has the mechanism.
- **hecs components must be `Send + Sync`** — owned `String`, `Arc`, never `Rc`.
- **RON needs `implicit_some`**; `assets.rs::load_ron` enables it, and maps are
  read through it too, so content writes `sheet: "x"` rather than `Some("x")`.
- **The player sheet runs sequentially across rows** — frame *i* is at cell
  `(i % 7, i / 7)`. Authoring a clip per row silently points it at the wrong
  animation; [`assets/data/animations/player.ron`](assets/data/animations/player.ron)
  documents the verified layout.

## Determinism

Same seed, same run — or tapes and trace diffing are worth nothing.

- **`Sim::rng` is the only source of randomness in the simulation** — no
  `thread_rng`, no clock. The generator is written out in
  [`src/sim/rng.rs`](src/sim/rng.rs) rather than taken from a crate, and two
  tests there enforce the rule. The view has its own, so the look of a spark
  can never shift a loot roll.
- **`BTreeMap`, not `HashMap`, wherever iteration order can reach a float sum
  or an ordered output.** Both `Equipment` (deriving stats walks it and sums
  modifiers) and `Sim::flags` (serialized into every trace frame and every save)
  say why on the field, in `src/ecs/components.rs` and `src/sim/mod.rs`.
- **Closed form, not counted.** Fire schedules, movers and pendulums are pure
  functions of the world clock, never counters: a counter drifts, and drifts
  differently across the ticks hitstop skips, so `at(t) == at(t + period)` would
  stop being a fact. It is also what lets a save restore all three by restoring
  the clock alone ([`src/save.rs`](src/save.rs)).

## Balance in RON, mechanics in Rust

Code interprets; content decides — tuning moves a data file, not a literal.
Under `assets/data/`, loaded through [`src/assets.rs`](src/assets.rs):

| File | Holds |
| --- | --- |
| `attacks.ron` | swing timing, reach, damage, and each link's `chain` — the combo is data, not a state machine in code |
| `spells.ron` | mana cost, cast time, cooldown, the projectile |
| `stats.ron` | every kind's body, combat and AI numbers — and, by what it has, what the kind *is* |
| `items/*.ron` | weapons, equipment, consumables, carried things (coins, keys) |
| `effects.ron` | what a hit, a death, a landing look like |

Beside them: `dialogue/*.ron` (graphs, conditions, effects), `animations/*.ron`
(clip sets), `tilesets/*.ron` (autotiling rules and prop art). `tests/data.rs`
cross-references the lot, so a typo in a RON id fails the build rather than a
playthrough.
