# CLAUDE.md

A 2D side-scroller in Rust (ggez + hecs). This file is a **map, not a manual**:
the reasoning behind every rule below is already written next to the code that
depends on it, so each entry points at where to read it rather than repeating
it. For *how to work* — running tapes, re-recording traces, authoring maps, the
done checklist — read
[`.claude/skills/supergame-dev/SKILL.md`](.claude/skills/supergame-dev/SKILL.md),
which this file does not duplicate.

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
| `hazard::tick_schedules`, `pendulum::advance` | geometry that owns itself deciding *where* it is this tick |
| `avatar::control`, `npc::think` | read input / AI, set a velocity and `Body` knobs |
| `mover::advance` | **last** decision: platforms move and carry riders |
| `body::rebuild_geometry` | **the one point per tick where collision geometry changes** |
| `body::move_bodies` | gravity, integration, collision — every body, one pass |
| `avatar::after_move` | landing, bounds, hazard death |
| `combat::resolve`, `spell::resolve_projectiles`, `inventory::collect_pickups`, `update_prompt`, `settle_dead`, `drop_loot` | tested at final positions |
| `animation::select_*`, `advance` | pick a clip, advance frames |

- Controllers only set a velocity and `Body` knobs (`gravity`, `fall_cap`,
  `ignore_one_way`, `frozen`). They never integrate or collide.
- Anything that *is* geometry owns a `Collider` and moves it in the decide
  phase; the rebuild picks it up the same tick and collision never learns it
  exists. [`src/systems/hazard.rs`](src/systems/hazard.rs) is the cheapest
  example; `mover::advance`'s slot is load-bearing in three directions at once,
  each argued in [`src/systems/mover.rs`](src/systems/mover.rs).
- Physics asks a `SolidQuery`, never a `Vec<SolidRect>`, so a query must stay a
  *subsequence* of the full list: reordering it moves the game
  ([`src/physics.rs`](src/physics.rs),
  [`src/systems/body.rs`](src/systems/body.rs)).

## `Sim` vs `scenes/`

**Gameplay logic lives in `Sim`; scenes draw and decide nothing.** A scene is
untestable by definition — no tape can press a key only a `ggez::Context` sees.

Inventory and dialogue are the two that feel like UI and are simulation: a
potion changes health, a reply takes an item and sets a quest flag. Both live
in [`src/systems/inventory.rs`](src/systems/inventory.rs) and
[`src/systems/dialogue.rs`](src/systems/dialogue.rs), which make the argument in
full; `scenes/inventory.rs` and `scenes/dialogue.rs` are overlays that read
state and emit rectangles. `Mode { Playing, Inventory, Dialogue }` on `Sim` is
the mechanism — a modal mode freezes the world but is still stepped, so a tape
drives the whole screen. Pause is deliberately *not* a mode: it stops the sim
being stepped at all.

## Verification

Everything runs headlessly; verify gameplay yourself rather than asking for a
playtest. The skill file has the workflow — in one line each:

- `cargo test` — 29 tapes and 29 golden traces replay here, plus the unit and
  property suites.
- `cargo run --bin sim -- --tape tapes/x.tape [--trace out.jsonl] [--geometry]`
  — one tape, with an event timeline.
- **`uv run .claude/skills/supergame-dev/scripts/trace_diff.py --ignore <new
  fields>` before ever re-recording.** Adding a probe field rewrites every
  baseline, which looks identical to a physics change. Zero remaining
  differences is the proof.
- `cargo run --bin sheet -- player` — traces record clip *names*, not which
  sheet cells they resolve to, so animation *mapping* is checkable only by eye.
- `SUPERGAME_DEBUG=1`, or F1 in game — colliders, sprite bounds, hitboxes
  ([`src/debug.rs`](src/debug.rs)).
- `Sim::fixture(&["..P...", "######"])` — a level and player inline in a test.

## Invariants

- **Never despawn an NPC.** They are addressed by spawn index (`knight.0`);
  removing one silently repoints every later assertion. Mark dead, freeze, stop
  colliding — `Sim::npcs` in [`src/sim/mod.rs`](src/sim/mod.rs).
  The two exceptions are **projectiles** and **pickups**, different for a
  stated reason: nothing addresses one, so nothing can be renumbered, and
  leaving them as corpses is the actual leak — see the module docs of
  [`src/systems/spell.rs`](src/systems/spell.rs) and
  [`src/systems/inventory.rs`](src/systems/inventory.rs).
- **`Body::frozen` stops movement dead; `Health::hitstun` only suppresses the
  controller.** Knockback needs the second — a hit takes away steering, not
  momentum ([`src/ecs/components.rs`](src/ecs/components.rs)).
- **`Sim::npcs()` sorts by entity id.** hecs iterates archetypes in creation
  order, so adding or removing a component mid-run reshuffles query order.
- **Fixture maps under `assets/maps/testbed*.ron` are frozen** — tapes encode
  their exact pixels. Add to them; do not move what exists (tapes/README.md).
- **`Aabb::overlaps` is strict**: sharing an edge is not an intersection. An
  inclusive test snaps bodies onto phantom ledges at every tile seam — the
  comment on it in [`src/physics.rs`](src/physics.rs) has the mechanism.
- **hecs components must be `Send + Sync`** — owned `String`, `Arc`, never `Rc`.
- **RON needs `implicit_some`**; it is enabled in `assets.rs::load_ron`, so
  content writes `sheet: "x"` rather than `Some("x")`.
- **The player sheet runs sequentially across rows** — frame *i* is at cell
  `(i % 7, i / 7)`. Authoring a clip per row silently points it at the wrong
  animation; [`assets/data/animations/player.ron`](assets/data/animations/player.ron)
  documents the verified layout.

## Determinism

Same seed, same run — or tapes and trace diffing are worth nothing.

- **`Sim::rng` is the only source of randomness** — no `thread_rng`, no clock.
  The generator is written out in [`src/sim/rng.rs`](src/sim/rng.rs) rather than
  taken from a crate, and two tests there enforce the rule.
- **`BTreeMap`, not `HashMap`, wherever iteration order can reach a float sum
  or an ordered output.** Both `Equipment` (deriving stats walks it and sums
  modifiers) and `Sim::flags` (serialized into every trace frame and every save)
  say why on the field, in `src/ecs/components.rs` and `src/sim/mod.rs`.
- **Closed form, not counted.** Fire schedules, movers and pendulums are pure
  functions of `Sim::tick`, never counters: a counter drifts, and drifts
  differently across the ticks hitstop skips, so `at(t) == at(t + period)` would
  stop being a fact. It is also what lets a save restore all three by restoring
  the tick alone ([`src/save.rs`](src/save.rs)).

## Balance in RON, mechanics in Rust

Code interprets; content decides — tuning moves a data file, not a literal. The
four balance tables, under `assets/data/` and loaded through
[`src/assets.rs`](src/assets.rs):

| File | Holds |
| --- | --- |
| `attacks.ron` | swing timing, reach, damage, and each link's `chain` — the combo is data, not a state machine in code |
| `spells.ron` | mana cost, cast time, cooldown, effect |
| `stats.ron` | every kind's movement, combat and AI numbers |
| `items/*.ron` | weapons, equipment, consumables, quest items |

Beside them: `dialogue/*.ron` (graphs, conditions, effects), `animations/*.ron`
(clip sets), `tilesets/*.ron` (autotiling rules). `tests/data.rs`
cross-references the lot, so a typo in a RON id fails the build rather than a
playthrough.
