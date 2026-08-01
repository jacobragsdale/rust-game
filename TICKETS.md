# Tickets

Every item in [ROADMAP.md](ROADMAP.md), broken into work a single engineer (or
agent) can pick up cold and finish. The roadmap says *what and in what order*;
this says *what done looks like*.

Read [.claude/skills/supergame-dev/SKILL.md](.claude/skills/supergame-dev/SKILL.md)
before starting any ticket. Two rules from it govern every ticket here:

- **Gameplay logic goes in `Sim`, never in `scenes/`.** Scenes draw; they do not
  decide. This is most tempting for inventory and dialogue, which are the two
  features most likely to be gotten wrong.
- **A feature is not done until the harness can see it.** Every ticket names its
  tape, its events, and its probe fields. Build those with the feature.

## Conventions used by every ticket

**Testing** on every ticket means all of:

1. `cargo test` green.
2. `cargo clippy --all-targets` adds no new warnings.
3. Golden traces: run
   `uv run .claude/skills/supergame-dev/scripts/trace_diff.py --ignore <new probe fields>`
   and account for **every** remaining difference in the ticket's writeup before
   re-recording with `UPDATE_TRACES=1 cargo test --test traces`. A trace diff you
   cannot explain is a bug you have not found yet.
4. Any new tape asserts something that would fail if the feature were deleted.

**Out of scope** on every ticket: re-tuning existing movement constants,
reformatting files the ticket does not otherwise touch, and re-recording traces
for changes the ticket did not cause.

**Ticket status key:** `todo` / `in progress` / `done`.

---

# Workstream X — cross-cutting

## X-1 — Format the repo, lint it clean, and put it in CI

**Size:** S. **Depends on:** nothing. **Blocks:** everything (it rewrites files
every other ticket touches, so it goes first or it goes never).

### Context

There is no CI. The repo is not rustfmt-clean — pre-existing drift in
`physics.rs`, `debug.rs`, `probe.rs`, `tests/`, `bin/sim.rs`. Formatting inside a
feature diff buries the feature; this is why it gets a commit of its own.

### Implementation

- `cargo fmt --all` as one mechanical commit, no other change in it.
- `cargo clippy --all-targets` — fix what it reports. The known pre-existing
  `Assets`/`Default` warning is either fixed (`impl Default for Assets`) or
  `#[allow]`ed with a one-line reason.
- `.github/workflows/ci.yml`: one job on push and PR, stable toolchain, cached
  cargo registry and `target/`, running
  `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.

### Acceptance criteria

- `cargo fmt --all --check` exits 0.
- `cargo clippy --all-targets -- -D warnings` exits 0.
- The workflow file exists, and its three commands are the same ones a developer
  runs locally.
- **Zero trace diffs.** Formatting cannot change behavior; if a baseline moved,
  something else went in with it.

### Testing

`cargo test` before and after produce identical golden traces (`git diff traces/`
is empty).

---

## X-2 — Root `CLAUDE.md` pointing at the invariants

**Size:** S. **Depends on:** nothing. **Parallel with:** everything (docs only).

### Context

Hard-won design intent lives only in doc comments: the strict-overlap reasoning
in `physics.rs`, the `InputLatch` rationale in `input.rs`, the frozen-fixture
rule for testbed maps, the reason `Sim::npcs` sorts by entity id, the reason
corpses are never despawned. Every one of those was rediscovered the hard way at
least once.

### Implementation

Write `/CLAUDE.md` (repo root) — short, and a map rather than a manual. It points
at where the reasoning already lives rather than restating it.

Cover, with a one-line "why" and a file reference each:

- The three-phase tick and which phase new work belongs in.
- `Sim` vs `scenes/`: what may never move into a scene.
- Verification: tapes, golden traces, `trace_diff.py`, `sheet`, `SUPERGAME_DEBUG=1`.
- Invariants: never despawn an NPC; `frozen` vs `hitstun`; `Sim::npcs` ordering;
  frozen testbed maps; strict AABB overlap; components must be `Send + Sync`;
  RON `implicit_some`; the player sheet's sequential-across-rows layout.
- Content-vs-code line: balance in RON, mechanics in Rust.
- Where to look first: ROADMAP → TICKETS → PLAN → tapes/README.

### Acceptance criteria

- Under 150 lines. Every claim in it is true of the code today.
- Every file path and command in it exists and runs.
- It does not duplicate the skill file; where they overlap it defers.

### Testing

Run every command the document names. No code change, so no trace movement.

---

# Workstream H — harness and data plumbing

These three unblock M3c and everything after it. They are the "enabling refactor"
half of the roadmap's alternating pattern.

## H-1 — Inputs become a named action set

**Size:** M. **Depends on:** X-1. **Blocks:** C-1 (cast), I-5 (inventory keys),
D-3 (dialogue choices).

### Context

`PlayerInput` is a fixed struct of six bools and `tape::Keys` is a parallel fixed
struct of five. Every new action means editing both, plus the keyboard reader,
plus the tape parser, plus every literal `PlayerInput { .. }` in tests. M3c adds
`cast`, M4 adds `inventory` and `use`, M5 adds `interact` and choice selection.
Four features multiplying two hand-maintained structs is the shape of a mess.

### Implementation

- `src/systems/input.rs`: define one enumeration of actions and derive everything
  else from it.

  ```rust
  pub enum Action { Left, Right, Down, Up, Jump, Attack, Cast, Interact, Inventory, Confirm, Cancel }
  ```

  with, in one table beside it, each action's name as tapes spell it, its default
  keyboard bindings, and whether it is edge- or level-triggered.
- `PlayerInput` keeps its ergonomic accessors (`input.jump_pressed()`,
  `input.left()`) but is backed by two bitsets: `held` and `pressed`. Keep the
  field-style call sites compiling by whatever mechanism is least disruptive —
  accessor methods are fine, and a `PlayerInput::from_actions(&[Action])` builder
  keeps tests readable.
- `InputLatch` latches every edge-triggered action, not just jump and attack.
  Its existing rationale (presses and ticks are on different clocks) applies
  unchanged to every one-shot action; generalize the latch, do not add a second
  mechanism.
- `tape::Keys` becomes the same bitset. `parse_keys` resolves a token against the
  action table, so a new action is spelled in tapes with **no parser change**.
  Unknown key error message lists the table.
- `Tape::inputs()` derives `pressed` by diffing consecutive ticks for every
  edge-triggered action, exactly as it does for jump and attack today. The
  existing tests `jump_is_edge_triggered_but_stays_held` and
  `releasing_and_repressing_jump_fires_again` must pass unchanged.
- `up` becomes a real action (currently `Up`/`W` are jump keys only). Bind
  `Up`/`W` to both Jump and Up for now; M5's `interact` will want a direction-free
  key (`E`).

### Acceptance criteria

- Adding an action is: one enum variant, one row in the binding table. Nothing
  else. Prove it by adding `Cast` (bound to `K`) in this ticket, unused by
  gameplay, and writing a tape line `cast 1` that parses.
- `left`+`right` still cancel; jump and attack still edge-trigger.
- Every existing tape parses and passes with no edit.
- **Zero trace diffs.** This is a refactor; the game must not move.

### Testing

- Unit: every action round-trips name → `Action` → bitset → `PlayerInput`; the
  action table has no duplicate names or duplicate key bindings; edge-triggered
  actions fire once per press through `Tape::inputs()`.
- The full existing tape and trace suites, unchanged, are the real test.
- `tapes/README.md` key list regenerated from the action table's names, or at
  minimum updated by hand and covered by a test that the doc's list matches
  `Action::names()`.

---

## H-2 — A seeded RNG on `Sim`

**Size:** S. **Depends on:** X-1. **Blocks:** C-2 (spell variance), I-4 (loot
tables).

### Context

Deferred from M2 because patrol is deterministic and an RNG would have been dead
code. Loot drops need one first; crit rolls and hit variance follow. The rule
matters more than the plumbing: **nothing may call `rand::thread_rng()`**, or
tapes and golden traces stop meaning anything.

### Implementation

- New `src/sim/rng.rs`: a small, explicit, dependency-free generator — a 64-bit
  xorshift or PCG, written out, with its algorithm fixed by a unit test carrying
  hardcoded expected values. **Do not add the `rand` crate**: a golden trace
  recorded today must reproduce in three years, and that is a promise about a
  specific bit sequence, not about an API.
- `Rng` on `Sim`, seeded per run. `Sim::new` takes a seed (default a constant, so
  `Sim::fixture` and every existing tape stay reproducible);
  `Sim::load` keeps its signature by using the default.
- API: `next_u32`, `next_f32` (in `[0,1)`), `range(lo, hi)`, `chance(p)`,
  `pick(&[T])`. Every one deterministic given the seed.
- Tape directive `seed <n>` sets it, defaulting to the constant. Record the seed
  in the trace's first frame or in a header comment so a trace is reproducible
  from the file alone.
- A `#[test]` (or a CI grep step in X-1's workflow) fails if `thread_rng` or
  `rand::random` appears anywhere in `src/`.

### Acceptance criteria

- Two `Sim`s with the same seed produce identical value sequences; different
  seeds differ.
- The existing determinism test (`simulation_is_deterministic`) still passes.
- **Zero trace diffs** — nothing consumes the RNG yet.

### Testing

Unit tests for the algorithm's fixed sequence, uniformity of `range` over a large
sample, and reproducibility across two `Sim` instances.

---

## H-3 — Movement and combat constants move to RON as a `Stats` component

**Size:** M. **Depends on:** X-1. **Blocks:** I-3 (equipment modifiers need a
base to modify), and every enemy after the knight.

### Context

`Avatar::RUN_SPEED` and friends are `const`s on a component; `spawn.rs` has
`KNIGHT_SPEED`, `KNIGHT_HEALTH`, `KNIGHT_GRAVITY`. Enemies need per-entity stats
anyway, and M4's equipment computes `base + sum(modifiers)` — which needs a base
that is data. Tuning without a recompile is the other half of the win.

### Implementation

- `assets/data/stats.ron`: one entry per entity kind, `"player"` included.

  ```ron
  Stats({
    "player": StatBlock(
      width: 20.0, height: 34.0,
      run_speed: 200.0, accel: 1500.0, decel: 1800.0,
      jump_speed: 520.0, double_jump_speed: 470.0,
      gravity: 1400.0, low_jump_gravity: 1800.0, max_fall: 900.0,
      wall_slide_speed: 70.0, wall_jump_push: 260.0, wall_jump_speed: 480.0,
      coyote_ticks: 6, wall_coyote_ticks: 6, jump_buffer_ticks: 6,
      max_air_jumps: 1, drop_ticks: 8, death_ticks: 36,
      slide_ticks: 20, slide_speed: 300.0, slide_cooldown: 20,
      max_health: 5, iframe_ticks: 30,
    ),
    "knight": StatBlock( ... ),
  })
  ```

- `Stats` component holding a resolved `StatBlock` (by value, or `Arc<StatBlock>`
  — it must be `Send + Sync`). `spawn::player` and `spawn::entity` read the table
  and attach it.
- `Avatar`'s consts are deleted and every read goes through `Stats`. Tests that
  reference `Avatar::HEIGHT` and `Avatar::MAX_HEALTH` (there are many, in
  `avatar.rs`, `sim/mod.rs`, `tests/levels.rs`, `spawn.rs`) go through a test
  helper that loads the real table — **not** through invented numbers. Combat
  numbers are content; a test that invents its own is not testing the game.
- Knight consts in `spawn.rs` follow the same path.
- Values in the RON file are **exactly** today's constants, to the last decimal.
  This ticket moves numbers; it does not tune them.
- `tests/data.rs` (N-1) later checks every kind in `spawn::KINDS` has a stat
  block. If N-1 has not landed, add a focused test here instead.

### Acceptance criteria

- No gameplay constant remains in `src/ecs/components.rs` or `src/ecs/spawn.rs`.
- Editing `run_speed` in the RON file and running one tape changes the outcome,
  with no recompile of a `const`.
- **Zero trace diffs.** The numbers did not change, so the game did not.

### Testing

- Unit: the table loads; every advertised kind resolves; a missing kind is an
  error naming the kind.
- The whole existing tape and trace suite is the regression net. Any diff here
  means a value was transcribed wrong — find it, do not re-record.

---

# Workstream C — M3c, the shock spell (finishes M3)

## C-1 — Mana, `SpellDef`, and casting

**Size:** M. **Depends on:** H-1, H-3.

### Context

The mana bar's space is already reserved in the HUD (`hud.rs` sizes its panel for
two bars). PLAN.md specifies `SpellDef`: mana cost, cooldown, cast time, effect.
The Adventurer sheet's `cast` frames were identified during M3-melee and are
waiting.

### Implementation

- `assets/data/spells.ron`, mirroring `attacks.ron`'s shape and comment density:

  ```ron
  Spells({
    "shock": SpellDef(
      clip: "cast",
      cost: 2,
      cooldown: 45,          // ticks
      cast_ticks: 12,        // committed before the projectile appears
      recovery: 6,
      effect: Projectile(
        speed: 320.0,
        damage: 2,
        lifetime: 90,
        size: (16.0, 16.0),
        sheet: "shockSpell",
        knockback: (120.0, -40.0),
        hitstun: 12,
      ),
    ),
  })
  ```

  `effect` is an enum from the start (`Projectile { .. }`), because `Aoe` and
  `Buff` are named in PLAN.md and a struct that has to be widened later is worse
  than an enum with one variant now.
- `Mana` component: `current`, `max`, `regen` (per-tick fixed-point or a
  fractional accumulator — pick one and document why). On the player only, for
  now, but nothing about it is player-specific.
- `Casting` component alongside `Attacking`, same shape and same reasons
  (a component with `None` inside, never an added/removed marker — archetype
  churn is what `Sim::npcs` has to defend against).
- `avatar::control` handles the `Cast` action: refuse if `Casting`/`Attacking` is
  busy, if mana is short, or if the cooldown is live. On success, deduct mana,
  set the cooldown, start the cast.
- `systems/combat.rs` (or a new `systems/spell.rs` — justify the choice in the
  module doc) advances casts in the timer phase and spawns the projectile at the
  cast's release tick.
- `GameEvent::SpellCast { spell }` and `GameEvent::CastFailed { spell, reason }`.
  The failure event is not decoration: "nothing happened" and "you were out of
  mana" are indistinguishable in a trace otherwise, and that ambiguity will cost
  an afternoon.
- `animation::select_avatar_clip`: a cast outranks movement and loses to death,
  the same way an attack does. Add `cast` to `AVATAR_CLIPS`.

### Acceptance criteria

- Casting with full mana spends it, plays the `cast` clip, and emits `SpellCast`.
- Casting at zero mana emits `CastFailed`, spends nothing, and does not animate.
- The cooldown blocks a second cast and expires on schedule.
- Mana regenerates to full, at a documented rate, and never exceeds `max`.
- Costs, cooldowns and damage all come from `spells.ron` — no number in Rust.

### Testing

- Unit, in `Sim::fixture`: cast → mana drops; cast again immediately → refused;
  wait out the cooldown → allowed; mana regen reaches max and stops.
- Probe gains `mana` and `cast_cooldown`; declared to `trace_diff.py --ignore
  mana cast_cooldown` when re-recording.
- Tape `tapes/spell_cast.tape` on `testbed_arena.ron`:
  `expect shock.spell_cast == 1`, `assert mana < 5`, and after regen
  `assert mana == 5`.

---

## C-2 — The projectile entity

**Size:** M. **Depends on:** C-1, H-2.

### Context

The first entity in the game that is neither the player nor an NPC: it is spawned
mid-run, it collides, it damages, and it goes away. `move_bodies` was written to
serve exactly this ("bodies with no controller at all — dropped items,
projectiles, gibs — still move").

### Implementation

- `spawn::projectile(world, origin, dir, &SpellDef, team)`: `Position`,
  `Velocity`, `Size`, `Body` with `gravity: 0.0`, `Team` inherited from the
  caster, `Sprite`, `AnimationState`, plus:
  - `Projectile { damage, knockback, hitstun, pierces: bool, source: Entity }`
  - `Lifetime { ticks: u32 }`
- New `systems/projectile.rs`, running in the resolve phase beside `combat`:
  test each projectile's AABB against opposing `Team` entities with
  `Health::vulnerable()`, apply damage/knockback/hitstun through the *same* code
  path `combat::resolve` uses (extract a shared `apply_hit` helper — two damage
  paths that can drift is exactly the bug `Health` was made shared to avoid), and
  emit `Damaged`.
- Expiry: on lifetime reaching zero, on hitting level geometry, or on connecting
  (unless `pierces`).
- **Despawning is allowed here, and only here.** The never-despawn rule exists
  because NPCs are addressed by spawn index in tapes and traces; projectiles are
  not addressed at all. Write that distinction into the module doc so the next
  person does not have to re-derive it. Projectiles must *not* appear in
  `Sim::npcs()` — they have no `Patrol`, so they will not, but add a test that
  says so, because a future refactor of `npcs()` could sweep them in and silently
  renumber every knight in every tape.
- Projectiles are counted in the trace as a single `projectiles` integer, not as
  per-entity probes. A line per projectile per tick makes traces unreadable, and
  what a tape wants to assert is "the bolt existed" and "it hit".

### Acceptance criteria

- A cast bolt travels in the caster's facing direction at the speed in the RON.
- It damages an enemy once, applies knockback, and disappears.
- It stops at a wall.
- It expires by lifetime over open ground.
- It cannot damage its own team.
- The knight's kill by spell emits the same `Damaged`/`Died` events a sword kill
  does, with the same payload shape.

### Testing

- Unit: travel, wall stop, lifetime expiry, friendly-fire immunity, one-hit-only.
- Property: over a sweep of spawn positions and speeds, a projectile never ends a
  tick inside a solid and never survives past its lifetime.
- Tape `tapes/spell_kill.tape`: kill the arena knight with spells alone —
  `expect knight.died == 1`, `expect no player.damaged`, `assert knight.0.dead`.
  Assert the *outcome*, not the tick it lands on.

---

## C-3 — The mana bar, and the spell in the harness

**Size:** S. **Depends on:** C-1.

### Context

The HUD panel is already sized for two bars specifically so this does not move
the health bar (`hud.rs`, `draw_player_bars`).

### Implementation

- Second bar under the health bar, in the reserved space, blue, same border and
  fill treatment. A distinct low-mana colour is **not** wanted: mana running out
  is a resource state, not an emergency.
- Cooldown readability: dim the bar, or draw a thin sweep, while the spell is on
  cooldown. Cheap, and without it "why didn't that cast" is invisible.
- `Probe`: `mana`, `mana_max`, `cast_cooldown`, `casting` (bool), `projectiles`
  (count). Add to `FIELD_NAMES`/`FLAG_NAMES` beside the accessors — the
  `advertised_names_all_resolve` test enforces the pairing.
- `tapes/README.md`: document `cast`, the new fields, and the new events.

### Acceptance criteria

- The health bar's pixels do not move (compare a screenshot before and after, or
  reason from the constants — `MARGIN`, `BAR_W`, `BAR_H` unchanged).
- `assert mana == 3`, `assert casting`, `assert projectiles == 1` all parse and
  evaluate.
- `expect shock.spell_cast >= 1` parses (spell id as the event subject, the way
  `attacked` carries the attack id).

### Testing

Probe name/accessor pairing tests; one tape exercising each new assertion form.
Look at the HUD on screen — `SUPERGAME_SCENE=adventure SUPERGAME_MAP=maps/testbed_arena.ron cargo run`.

---

## C-4 — The plunge attack

**Size:** S. **Depends on:** C-1 (only for sequencing; technically independent).

### Context

Left over from M3's original scope. Frames 94–108 of the Adventurer sheet are a
long ready/loop/impact sequence — the last unused piece of the melee kit.

### Implementation

- Three clips in `player.ron`: `plunge_ready`, `plunge_loop` (looping),
  `plunge_impact`. Derive the frame ranges from the verified sequential layout
  (`frame i` is at cell `(i % 7, i / 7)`); confirm with
  `cargo run --bin sheet -- player --clip plunge_loop`.
- Input: `down` + `attack` while airborne. Distinct from the air attack, which is
  `attack` alone.
- Mechanic: brief hover during `ready`, then a fast fall with a live hitbox
  underneath, then an impact with recovery on landing. Timing and damage in
  `attacks.ron` — add a `plunge` entry with a `Down` hitbox anchor if the current
  facing-mirrored `offset` cannot express "below".
- The knockback on impact points *away and up* from the point of contact.

### Acceptance criteria

- Down+attack in the air plunges; attack alone in the air still does the existing
  air attack; down+attack on the ground still does the ground combo.
- The plunge lands a hit on something below and has recovery on impact.
- The three clips play in sequence, and the loop actually loops during a long
  fall.

### Testing

- Tape `tapes/plunge.tape`: jump over the arena knight, plunge, `expect
  knight.damaged >= 1`, `assert clip == plunge_impact` at the landing.
- Visual check with `sheet` that the three clips point at the right frames —
  traces cannot see animation mapping.

---

# Workstream L — M7, live map features

Independent of the combat/dialogue chain. The deepest change to the physics layer
on the roadmap.

## L-1 — Colliders owned by entities, and a broadphase interface

**Size:** L. **Depends on:** X-1. **Blocks:** L-2, L-3, L-4.

### Context

`Sim::geometry` is flattened once at load from `level.solids` and `level.one_way`
and never changes. Fire, moving platforms and swinging hazards all need the same
thing: geometry that changes every tick. The current linear scan is fine for one
body and wasteful for thirty NPCs plus projectiles.

### Implementation

- `Collider { rect_offset: Vec2, size: Vec2, kind: ColliderKind }` where
  `ColliderKind` is `Solid | OneWay | Hazard`. An entity with a `Position` and a
  `Collider` contributes geometry.
- Introduce a `Geometry` type that owns both the static level rects and the
  per-tick entity-owned ones, rebuilt (or incrementally updated) at a **single
  defined point in the tick**, before `move_bodies`. Name that point in the tick
  table in the skill file and in `Sim::step`'s doc comment.
- `Geometry` exposes the query the physics layer actually needs — "every solid
  overlapping this AABB" — rather than a `&[SolidRect]`. This is the interface
  change that matters; the implementation behind it may stay a linear scan in
  this ticket.
- Then put a uniform grid behind it. Cell size one or two tiles. Static rects
  bucketed once at load; dynamic ones rebucketed per tick.
- `physics::resolve_move`, `touching_wall`, `wall_contact` take the query
  interface instead of a slice. Their semantics do not change — in particular the
  strict-overlap rule and its phantom-ledge reasoning survive verbatim.

### Acceptance criteria

- **Zero trace diffs.** A refactor of how geometry is stored must not move a
  pixel. This is the exit criterion, exactly as it was for M1.
- `tests/physics_diagnostics.rs` passes unchanged.
- An entity with a `Solid` collider blocks a body; removing the entity unblocks
  it, within the same run.
- Ordering: geometry from entities is assembled in a stable order (entity id),
  because `resolve_move` iterates solids and its result must not depend on
  hecs archetype order. The existing `resolution_is_independent_of_solid_order`
  property test is the guard; extend it to cover entity-owned rects.

### Testing

- The full existing suite, with zero diffs, is the primary test.
- New: a fixture where a solid entity is spawned mid-run and a body then collides
  with it; a benchmark-shaped test (or a comment with measured numbers) showing
  the grid actually reduces the work for thirty bodies.

---

## L-2 — Fire: a hazard that toggles on a cycle

**Size:** S. **Depends on:** L-1.

### Context

The cheap one, and deliberately first: it proves entity-owned colliders with no
rider-carry complexity in the way.

### Implementation

- Map legend: `F` places a fire emitter. Entity list form
  `Fire(cell: (x, y), period: 120, duty: 60, phase: 0)` for authored timing;
  the grid character uses defaults.
- `Hazard` collider kind plus a `Schedule { period, duty, phase }` component. The
  hazard's collider exists only while the schedule says on.
- Hazard death currently reads `level.hazards` in `avatar::after_move`. Move that
  to consult the unified geometry so a static spike and a lit fire kill by the
  same code path. Keep `DeathCause::Hazard`.
- Rendering: a placeholder is acceptable (the spikes are placeholder triangles
  today) but it must be visibly on/off — an invisible hazard is a bug report.

### Acceptance criteria

- A lit fire kills; an unlit fire does not.
- The cycle is deterministic and phase-authorable: two fires with different
  `phase` alternate.
- `expect died` fires with `cause: hazard` for fire exactly as for spikes.
- Existing spike behaviour is byte-identical (spike tapes and traces unchanged).

### Testing

- Fixture: walk into an unlit fire (survive), wait for it to light (die).
- Tape `tapes/fire_timing.tape` on a new `testbed_fire.ron`: cross during the
  gap, `expect no died`; then wait and stand in it, `expect died == 1`.
- New testbed map — new file, so no frozen-fixture concern.

---

## L-3 — Moving platforms and rider carry

**Size:** L. **Depends on:** L-1. **The hard one on this roadmap.**

### Context

Standing on a moving platform has to transport you. This is the source of most
platformer physics bugs: riders jitter, sink, get left behind at direction
changes, or get crushed. Budget accordingly and write property tests, not just
examples.

### Implementation

- `Mover` component: a path (two waypoints is enough; a list is not much harder),
  speed, and easing (linear is fine, and is what the property tests will assume).
- Platform motion is applied in `move_bodies`' phase, **before** rider bodies
  integrate. Order is the whole ticket: decide it, write it down in `Sim::step`,
  and make the test suite depend on it.
- Rider detection: a body is riding if it is grounded and its feet were resting
  on that platform's top at the *start* of the tick. Use `Body::prev_pos` — it
  exists for exactly this class of question.
- Carry: add the platform's delta to the rider's position directly, not to its
  velocity. Adding to velocity leaks momentum into the jump arc and makes the
  jump height depend on which way the platform was going.
- Leaving: stepping or jumping off must not inherit the platform's velocity
  (that is a design choice — state it in the doc comment either way, and test the
  one you chose).
- Crush: a rider pressed between a moving platform and a solid must not tunnel or
  get stuck. Push out along the shallowest axis, reusing the existing
  depenetration fallback rather than inventing a second one.
- One-way moving platforms: a rider can still drop through with `down`.

### Acceptance criteria

- A player standing on a horizontally moving platform arrives with it, with no
  drift and no jitter (position delta per tick equals the platform's, exactly).
- A vertically moving platform carries a standing player both up and down, and
  going down does not leave the player briefly airborne.
- Jumping off a moving platform gives the documented result, consistently.
- A rider is never left inside the platform.
- Existing static-geometry behaviour is unchanged: **zero trace diffs** on the
  existing 15 tapes.

### Testing

- **Property sweeps in `tests/physics_diagnostics.rs`, which is the exit
  criterion the roadmap names.** Over a sweep of platform speeds, directions and
  rider positions:
  - a grounded rider's x-delta equals the platform's x-delta every tick;
  - no body ends a tick inside a solid, static or moving;
  - a rider never becomes airborne while the platform moves horizontally;
  - reversing direction does not displace a rider.
- Tape `tapes/platform_ride.tape` on a new `testbed_mover.ron`: ride across a gap
  and arrive — the roadmap's stated exit criterion — `assert x > <far side>`,
  `expect no died`.

---

## L-4 — Swinging hazards

**Size:** M. **Depends on:** L-1, L-3 (reuses moving-collider machinery).

### Implementation

- `Pendulum { anchor: Vec2, length: f32, amplitude: f32, period: u32, phase: u32 }`,
  evaluated as a closed-form function of `sim.tick` — **not** integrated. A
  numerically integrated pendulum drifts, and drift makes golden traces
  meaningless. Closed form is deterministic forever and trivially reversible.
- Its collider is a `Hazard`, so it kills through the same path fire and spikes do.
- Rendering: a line to the anchor and a filled circle is enough; the SuperGame
  original was a spiked ball.

### Acceptance criteria

- The swing is periodic and exactly reproducible from `tick` alone (evaluate at
  tick *t* and at *t + period*; identical).
- Touching it kills, with `DeathCause::Hazard`.
- It does not block movement — a hazard is not a solid.

### Testing

- Unit: closed-form position matches a hand-computed value; periodicity holds
  over several periods with no accumulated error.
- Tape `tapes/swing_dodge.tape`: time a run under the swinging ball, survive;
  a second tape or a later section runs into it, `expect died == 1`.

---

# Workstream I — M4, items and inventory

## I-1 — `Sim` gets a mode, and `step` dispatches on it

**Size:** M. **Depends on:** H-1.

### Context

The inventory screen is the first *modal* state: it pauses the world but is
itself simulation, because using a potion changes game state. Establishing this
pattern here, on the simpler of the two modal features, is deliberate — dialogue
inherits it. Getting it wrong means the largest system in the game ends up in a
scene where no test can reach it.

### Implementation

- `Mode { Playing, Inventory, Dialogue }` on `Sim` (add `Dialogue` now, unused,
  so D-2 does not have to reopen this).
- `Sim::step` dispatches: `Playing` runs the tick as it does today; a modal mode
  runs only the systems that mode needs (input handling, its own state machine,
  animation *not* advancing) and leaves the world otherwise frozen.
- Mode transitions are events: `ModeChanged { from, to }`.
- The tick counter keeps advancing in every mode, exactly as it does during
  hitstop, so traces and tapes stay aligned with wall-clock time.
- `Probe` gains `mode` as a text field, so `assert mode == inventory` works.
- `PauseScene` is **not** folded into this. Pause is a scene concern — it stops
  the sim being stepped at all, and nothing about it is simulation.

### Acceptance criteria

- Entering the inventory freezes the world: positions, velocities, timers and
  animation frames are all identical on exit to what they were on entry.
- The tick counter still advances while modal.
- `assert mode == playing` / `== inventory` work in tapes.
- **Zero trace diffs** — no tape enters a mode yet.

### Testing

- Unit: enter modal, step 60 ticks, exit; every probe field except `tick` and
  `mode` is unchanged.
- Tape: open and close the inventory mid-run and confirm the player's position is
  what it would have been without the detour.

---

## I-2 — `ItemDef`, `Inventory`, and pickups

**Size:** M. **Depends on:** I-1, H-2.

### Implementation

- `assets/data/items/*.ron` per PLAN.md's schema:

  ```ron
  ItemDef(id: "iron_sword", name: "Iron Sword", sprite: "items/iron_sword",
          kind: Weapon(damage: 2, combo: ["player_slash1"], speed: 1.0))
  ItemDef(id: "minor_potion", name: "Minor Health Potion", sprite: "items/potion_red",
          kind: Consumable(effects: [Heal(2)]))
  ItemDef(id: "knight_helm", name: "Knight Helm", sprite: "items/helm",
          kind: Equipment(slot: Head, modifiers: [MaxHealth(2)]))
  ```

  Loaded into an `ItemTable` on `Assets`, cached like `AttackTable`.
- `Inventory { slots: Vec<ItemStack> }` component; `ItemStack { id, count }`.
  Capacity is a stat, not a magic number.
- Pickups are entities: `Position`, `Size`, `Body` (they fall and land — this is
  what `move_bodies` was generalized for), `Pickup { item, count }`, `Sprite`.
- Pickup on overlap with a `Team::Player` entity, in the resolve phase.
  `GameEvent::PickedUp { item, count }`.
- Pickups **may** be despawned on collection, for the same reason projectiles
  may: they are not addressed by index in any tape. Say so in the module doc.
- Sprite art: if `assets/graphics/items/` does not exist, a coloured quad is an
  acceptable placeholder — but the `ItemDef` still names a sprite, so content is
  not rewritten when art arrives.

### Acceptance criteria

- Walking over a pickup adds it to the inventory and emits `PickedUp`.
- Stacking works: two of the same item is one stack of two.
- A full inventory refuses the pickup and leaves it in the world (emit
  `PickupFailed` or equivalent — silence is untestable).
- Every item id in every data file resolves (N-1 enforces this repo-wide).

### Testing

- Unit: stacking, capacity, refusal.
- Probe: `inventory_count`, plus a way to ask about a specific item. Suggested
  path syntax, matching the existing `<kind>.<index>` convention:
  `assert item.minor_potion.count == 2`. Extend `ProbePath` rather than
  inventing a parallel mechanism.
- Tape `tapes/pickup.tape`: walk over a potion, `expect picked_up == 1`,
  `assert item.minor_potion.count == 1`.

---

## I-3 — Equipment, derived stats, and consumables

**Size:** M. **Depends on:** I-2, H-3.

### Context

PLAN.md is explicit and it is the right rule: stats are always computed as
`base + sum(modifiers)`, **never mutated in place**. Mutating in place means
unequipping has to know what equipping did, and that is how stat corruption bugs
are born.

### Implementation

- `Equipment { slots: HashMap<Slot, ItemId> }`, `Slot { Head, Body, Weapon, Trinket }`.
- Derived stats: a function `effective(base: &StatBlock, equipment: &Equipment,
  items: &ItemTable) -> StatBlock`. Everything that reads a stat reads the derived
  value. Compute it once per tick into a `DerivedStats` component rather than at
  every read site.
- Consumables apply their effects and decrement the stack. `Heal` clamps to max
  health. Using an item is a mode action in the inventory, and it is simulation —
  it belongs in `Sim`.
- Equipping a weapon changes which attack ids the combo uses: `Avatar::ATTACK`'s
  role moves onto the equipped weapon's `combo` list, falling back to the current
  bare-handed chain when nothing is equipped.
- `GameEvent::Equipped { item, slot }`, `Unequipped`, `ItemUsed { item }`.

### Acceptance criteria

- Equipping a `MaxHealth(+2)` helm raises max health by exactly 2; unequipping
  returns it to base, with no drift over a hundred equip/unequip cycles.
- Current health is clamped, never silently raised, when max changes.
- Drinking a potion heals, decrements the stack, and removes the stack at zero.
- Equipping a weapon changes the damage the melee combo deals, sourced from the
  `ItemDef`.

### Testing

- Property: over random equip/unequip sequences, derived stats always equal
  `base + sum(modifiers of currently equipped)`. This is the test that catches
  in-place mutation.
- Tape `tapes/equip.tape`: pick up a sword, equip it, hit the knight, and assert
  the damage number changed (`expect knight.damaged`, and the knight dies in
  fewer hits than bare-handed).

---

## I-4 — Loot drops

**Size:** S. **Depends on:** I-2, H-2.

### Implementation

- `Loot { table: Vec<(ItemId, u32 /*count*/, f32 /*chance*/)> }` on enemies,
  authored in the entity's stat/def RON, not in code.
- On death (`combat::settle_dead`, or the tick `Died` fires), roll the table with
  `Sim`'s seeded RNG and spawn pickups at the corpse.
- Rolls consume the RNG in a defined order (entity id order), or the same seed
  produces different drops depending on hecs iteration order — which would make
  the whole thing non-reproducible.

### Acceptance criteria

- The same seed always produces the same drops from the same fight.
- A `chance: 1.0` entry always drops; `0.0` never does.
- Drops land on the floor rather than floating (they have a `Body`).

### Testing

- Unit: fixed seed, fixed table, expected drops; a hundred rolls of a `0.25`
  entry land within a sane band.
- Tape `tapes/loot.tape`: kill the arena knight, `expect picked_up >= 1` after
  walking over the drop.

---

## I-5 — The inventory screen

**Size:** M. **Depends on:** I-1, I-2, I-3.

### Context

The scene draws. It does not decide. Selection index, use, equip, and drop are
all `Sim` state, driven by actions, so a tape can walk the whole UI.

### Implementation

- `Sim` holds inventory-mode state: selection index, and which pane (bag vs
  equipment) has focus.
- Actions: `Inventory` toggles the mode; `Up`/`Down`/`Left`/`Right` move the
  selection; `Confirm` uses or equips; `Cancel` closes.
- `scenes/inventory.rs`: a transparent overlay that reads that state and draws a
  grid. No logic beyond layout.
- The overlay draws over a frozen world, which is already true because I-1
  freezes it.

### Acceptance criteria

- A tape can open the inventory, move the selection, use a potion, and close —
  and the health change is asserted headlessly, with no window.
- The selection cannot go out of bounds on an empty or partial inventory.
- Closing and reopening preserves the selection (or resets it — choose, document,
  and test).

### Testing

Tape `tapes/inventory_use.tape` — the M4 exit criterion end to end: kill the
knight, loot drops, pick it up, equip it, stats change, all asserted headlessly.

---

# Workstream D — M5, interaction and dialogue

## D-1 — `Interactable` and the prompt

**Size:** S. **Depends on:** H-1 (the `Interact` action).

### Implementation

- `Interactable { prompt: String, target: InteractTarget }` where the target is
  `Dialogue(graph_id)` for now and `Door`/`Chest` later.
- Proximity check in the resolve phase sets a `nearest_interactable` on `Sim`.
  Nearest, not first-found, and stable when two are equidistant (break ties by
  entity id) — flicker between two prompts is a bug that only shows up in the
  one map that has two NPCs standing together.
- `GameEvent::InteractPrompted { target }` and `Interacted { target }`.
- HUD draws the prompt above the interactable. Drawing only; the decision of
  *what* to prompt is in `Sim`.

### Acceptance criteria

- Walking near an NPC sets the prompt; walking away clears it.
- Pressing `Interact` with no target does nothing and emits nothing.
- `assert prompt == talk` (text probe field) works in a tape.

### Testing

Fixture with two interactables: the nearer one wins, deterministically, from
both sides.

---

## D-2 — Dialogue graphs and dialogue mode

**Size:** L. **Depends on:** D-1, I-1 (the mode enum).

### Context

**The one thing not to get wrong on this roadmap.** Dialogue is modal, so a scene
feels like the natural home, but choices mutate quest state and inventory. It is
simulation wearing a UI hat. In a scene it becomes the single largest system in
the game that cannot be tested without a human.

### Implementation

- `assets/data/dialogue/*.ron`:

  ```ron
  Dialogue(
    id: "elder_intro",
    start: "greet",
    nodes: {
      "greet": Node(
        speaker: "Village Elder",
        lines: ["You're not from around here."],
        choices: [
          Choice(text: "Who are you?", next: "who"),
          Choice(text: "I have your amulet.", next: "reward",
                 condition: HasItem("amulet"),
                 effects: [TakeItem("amulet", 1), SetFlag("quest.amulet.stage", 2)]),
          Choice(text: "Goodbye.", next: None),
        ],
      ),
    },
  )
  ```

- `condition` and `effects` are enums from the start: `HasItem`, `FlagEq`,
  `FlagAtLeast`; `SetFlag`, `GiveItem`, `TakeItem`, `Heal`. Q-2 adds to them; the
  shape does not change.
- Dialogue state on `Sim`: current graph, current node, and the *filtered* choice
  list (choices whose conditions fail are hidden, not greyed — decide, document,
  test).
- Entering dialogue sets `Mode::Dialogue`; the world freezes exactly as I-1
  established.
- Advancing a line, selecting a choice, and closing are all actions.
- Effects apply through the same systems everything else uses — `GiveItem` goes
  through the inventory code path, not a direct `Vec::push`.

### Acceptance criteria

- A conversation walks from `start` to an end node and closes.
- A choice gated on an item is absent without the item and present with it.
- An effect that gives an item results in an item in the inventory, and emits
  `PickedUp`-equivalent bookkeeping.
- A dangling `next` target is a **load-time** error naming the graph and the node
  (N-1 makes this a test over all shipped content).

### Testing

- Unit: graph traversal, condition filtering, effect application.
- Probe: `dialogue_node` (text), `dialogue_choices` (count), `dialogue_selection`.

---

## D-3 — Dialogue in the harness, and the overlay

**Size:** M. **Depends on:** D-2.

### Implementation

- Events: `DialogueOpened { graph }`, `ChoiceTaken { node, index }`,
  `DialogueClosed { graph }`.
- Tape support for choice selection. Prefer reusing the action set —
  `down 1` / `confirm 1` — over a bespoke `choose 2` directive, so the tape
  exercises the same input path a player does. If a direct form is added for
  readability, it must compile down to the same actions.
- `scenes/dialogue.rs`: transparent overlay, speaker name, wrapped lines,
  choices with the selection highlighted. Draw only.

### Acceptance criteria

- The M5 exit criterion: a tape walks an NPC conversation, takes a branch gated
  on an inventory item, and asserts the resulting state change.
- `expect elder_intro.dialogue_opened == 1` and `expect choice_taken == 3` work.
- Text longer than the box wraps rather than overflowing.

### Testing

`tapes/dialogue_branch.tape` — run it twice against maps that differ only in
whether the player starts with the gating item, and assert the branch differs.

---

## D-4 — A friendly NPC

**Size:** S. **Depends on:** D-1, H-3.

### Implementation

- A `villager` kind in `spawn::KINDS` with a stat block, a clip set, an
  `Interactable`, and **no** `Hostile`. `Patrol` without `Hostile` already works
  and is exactly the "blacksmith who paces but will not stab you" case the
  `Hostile`/`Patrol` split was designed for — say so in the spawn arm's comment.
- Art: reuse the knight sheets recoloured, or a placeholder. Note the choice in
  the clip set file.

### Acceptance criteria

- A villager patrols, can be talked to, and cannot be damaged by the player
  (`Team` choice: give villagers a neutral team or omit `Health` — decide and
  document; "the player can murder the quest giver" is a design decision, not an
  accident).
- It appears in `npc_probes` in spawn order like any other NPC.

### Testing

Fixture with a knight and a villager: the knight chases, the villager does not.

---

# Workstream Q — M6, quests and persistence

## Q-1 — Quest flags as sim globals

**Size:** S. **Depends on:** D-2.

### Implementation

- `Flags(HashMap<String, i64>)` on `Sim`. Integers rather than booleans because
  quest *stages* are the common case and a boolean forces `quest.x.started` plus
  `quest.x.done` plus their interaction.
- Surfaced in the snapshot as globals, so `expect quest.rescue.stage == 2` works
  with no new machinery. Extend `ProbePath` with a `flag.<name>` or bare
  dotted-path form — pick one and make the error message list what exists.
- Flags appear in the trace only when non-empty, the same way `npcs` and `events`
  do, so existing baselines do not churn.

### Acceptance criteria

- Setting, reading, and comparing flags works from dialogue effects and
  conditions.
- An unset flag reads as 0 rather than erroring — quests check stages before they
  start.
- **Zero trace diffs** on existing tapes (no flags set means no key emitted).

### Testing

Unit tests on the flag store; a tape asserting a flag set by a dialogue effect.

---

## Q-2 — A fetch quest, end to end

**Size:** M. **Depends on:** Q-1, I-2, D-2.

### Implementation

- Content, not engine: a dialogue graph that sets `quest.amulet.stage = 1`, an
  item to find, a return branch gated on `HasItem`, and a reward via `GiveItem`.
- Whatever this exposes as missing in the condition/effect enums is the real
  engineering work — expect `FlagAtLeast` and `TakeItem` to be it.

### Acceptance criteria

The M6 exit criterion: accept a quest, complete it, get a reward — asserted by a
tape, on a real map.

### Testing

`tapes/quest_fetch.tape`, and the same run replayed after a save/load round trip
(Q-3) with identical results.

---

## Q-3 — Save and load

**Size:** L. **Depends on:** I-2, Q-1.

### Context

PLAN.md specifies rusqlite. **This ticket deviates deliberately:** the save is a
serde `SaveState` behind a storage trait, with a RON file backend. The
requirement that actually matters — "whatever it stores has to be reconstructible
into a `Sim`" — is unchanged, and a file backend keeps save tests headless,
fast, and diffable in a commit, with no C build dependency. A rusqlite backend
can be added behind the same trait if and when there is a reason. **Flag this to
the user rather than burying it.**

### Implementation

- `SaveState`: player position and stats, health/mana, inventory, equipment,
  flags, current map, and the RNG seed and stream position. The seed matters:
  loading a save and getting different loot than you would have is a bug report
  that will take a day to find.
- What is deliberately *not* saved: NPC positions, in-flight projectiles,
  animation frames. Document the choice — "you reload at the map's spawn state"
  is a legitimate design, but it has to be a decision rather than an oversight.
- `Sim::save() -> SaveState` and `Sim::load_save(SaveState, ...)`.
- A version field, checked on load, with a clear error on mismatch.

### Acceptance criteria

- Save, quit, load: inventory, equipment, flags, health and mana all survive.
- A round trip through the file produces a `Sim` that steps identically to one
  that was never saved (compare traces of N ticks from both).
- A corrupt or version-mismatched save fails with a clear error rather than a
  panic or a silently wrong world.

### Testing

- **The strongest test available here:** run a tape, save, load, run the *same*
  remaining tape ticks from both sims, and diff the traces. Identical or the save
  is lossy.
- Unit: version mismatch, corrupt file, missing file.

---

# Workstream N — M8, content

## N-1 — `tests/data.rs`: cross-reference every content file

**Size:** M. **Depends on:** nothing (extend it as each data type lands).
**Parallel with:** everything.

### Context

This kills the entire class of typo-in-a-RON-id bugs that otherwise dominates
content work. Cheap to write, and it pays back the first time a map references a
deleted NPC. Write the frame now and extend it per workstream.

### Implementation

One test file, one test per relationship, each failing with the file, the id, and
what was expected:

- Every map's entity kinds are in `spawn::KINDS`.
- Every kind in `KINDS` has a clip set that loads and a stat block (H-3).
- Every clip in every clip set resolves to a sheet that exists on disk, with
  frames inside the sheet's bounds.
- Every attack's `clip` exists in the clip set of every entity that can use it.
- Every attack's `chain` target exists in the attack table.
- Every spell's clip and sheet exist (C-1).
- Every item id referenced by a loot table, dialogue effect, or map exists (I-2).
- Every dialogue node's `next` target exists; every graph's `start` exists; no
  node is unreachable from `start` (D-2).
- Every quest reward is a real item id (Q-2).

### Acceptance criteria

- Deleting a clip from `player.ron` fails a test that names the clip and the
  attack that wanted it. Verify by actually doing it, then reverting.
- The tests skip cleanly (not fail) for content types that do not exist yet, so
  this can land early and grow.

### Testing

It is the test. Confirm each check by breaking the data on purpose and reading
the failure message — a check whose failure message does not name the file and
the id is not finished.

---

## N-2 — The village hub

**Size:** M. **Depends on:** D-4, N-1.

### Implementation

- `assets/maps/village.ron`: a hub with a safe spawn, an elder to talk to
  (D-2's graph), a shop-shaped building (no shop mechanics yet), and an exit
  toward the dungeon.
- Story per the SuperGame premise: a medieval fighter, a solar flare, aliens
  recruiting pre-electricity warriors, and magic as the thing the aliens lack.
  The elder's dialogue is where the premise is first stated.
- Smoke tape: spawn is on solid ground, the level is traversable end to end,
  nothing is unreachable.

### Acceptance criteria

- `tests/levels.rs` passes on the new map (no unreachable platforms, no spawn
  inside geometry).
- The smoke tape walks from spawn to the exit.
- No hostile NPCs in the hub.

### Testing

`tapes/village_smoke.tape`. Look at it on screen once —
`SUPERGAME_SCENE=adventure SUPERGAME_MAP=maps/village.ron cargo run`.

---

## N-3 — The dungeon

**Size:** M. **Depends on:** L-2, L-3, C-2, N-1.

### Implementation

- `assets/maps/dungeon.ron`: the first level that uses the full kit — knights,
  fire, a moving platform, a spell-gated or platform-gated route, and the quest
  item from Q-2 at the end.
- Difficulty curve: the first knight is fightable head-on; the second is placed
  so that walking up behind it is the better play, which teaches the sight-box
  rule the AI already implements.

### Acceptance criteria

- Traversable from spawn to the quest item with the abilities the player has.
- Every hazard is survivable with correct play (prove it with the tape).
- `tests/levels.rs` and `tests/data.rs` pass.

### Testing

`tapes/dungeon_run.tape`: a full traversal — `expect no died`,
`expect knight.died >= 1`, and a final `assert x > <exit>`.

---

# Sequencing summary

| Wave | Tickets | Why they can run together |
| --- | --- | --- |
| 0 | X-1 | Rewrites every file; must land alone, first. |
| 1 | H-1, H-2 · X-2 · N-1 | Core input/RNG vs docs vs a new test file — disjoint. |
| 2 | H-3 · L-1 | Stats data vs geometry storage; both core, kept apart by file. |
| 3 | C-1, C-2, C-3, C-4 · L-2, L-3 | Combat vs physics; different systems, different data. |
| 4 | I-1…I-5 · L-4, N-2 | Items/mode vs hazards/content. |
| 5 | D-1…D-4 · Q-3 | Dialogue needs the mode enum; save is independent. |
| 6 | Q-1, Q-2 · N-3 | Quests need dialogue; the dungeon needs the full kit. |
| 7 | Validation | Full suite, clippy, fmt, trace accounting, roadmap update. |

The chain H → C → I → D → Q is genuinely sequential: each one's enabling refactor
is the next one's foundation. Workstream L (physics) and workstream N (content
tests, maps) are the parallel capacity, which is exactly what ROADMAP.md predicts
when it marks M7 "parallelizable" and M8 "mostly authoring".
