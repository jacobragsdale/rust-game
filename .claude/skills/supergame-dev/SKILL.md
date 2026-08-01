---
name: supergame-dev
description: "Build and verify gameplay in the SuperGame Rust platformer. Use whenever editing src/, assets/, tapes/ or traces/ — movement, combat, NPCs, maps, animation, HUD — even if testing isn't mentioned. Not for read-only code questions."
---

# SuperGame development

A 2D side-scroller in Rust (ggez + hecs). The whole simulation runs headlessly,
so you verify gameplay yourself with input tapes and recorded traces instead of
asking Jacob to play. Do that for every change — a passing `cargo test` alone
does not mean the game still behaves the same.

## Orient before writing code

1. **`ROADMAP.md`** — what to build next, in what order, and what each
   milestone must be true before it starts. It supersedes PLAN.md's phase list,
   and its "Known drift" section records where PLAN.md and the code disagree.
2. **`TICKETS.md`** — the roadmap broken into pickup-able work, each with
   acceptance criteria, its named tape, and a **Status**.
3. **`PLAN.md`** — the designs: item/dialogue/spell RON schemas, story, and
   architecture rules. Its status claims are stale; its designs are not.
4. **`tapes/README.md`** — the tape and trace language you will be writing in.
5. **`CLAUDE.md`** — the invariants, each pointing at the doc comment that
   argues it.

## The tick

`Sim::step` dispatches on the sim's `Mode`; `Sim::step_playing` runs fixed 60 Hz
phases in this order. A new system goes into the phase it belongs to, not onto
the end. Both doc comments in `src/sim/mod.rs` are the authority.

| Phase | What belongs here |
| --- | --- |
| mode entry (`Inventory`, `Interact`), then hitstop | opening a screen or a conversation freezes the tick it happens on, and returns |
| `combat::tick_timers`, `advance_attacks`, `spell::advance_casts` | count down i-frames, hitstun, swing/cast progress; a cast that reaches its release tick spawns its bolt here, before anything moves |
| `hazard::tick_schedules`, `pendulum::advance` | geometry that owns itself deciding *where* — or whether — its collider is this tick |
| `avatar::control` | read input, decide the player's velocity and body knobs |
| `npc::think` | AI decides an NPC's velocity |
| `mover::advance` | **last** decision of the tick: platforms move and carry their riders. The slot is load-bearing in three directions — see `src/systems/mover.rs` |
| `body::rebuild_geometry` | **the one point per tick where collision geometry changes** — entity-owned colliders are read at the positions the controllers just decided |
| `body::move_bodies` | gravity, integration, collision — **every** body, one pass |
| `avatar::after_move` | react to where the body ended up: landing, bounds, death |
| `combat::resolve`, `spell::resolve_projectiles`, `inventory::collect_pickups`, `update_prompt`, `settle_dead`, `drop_loot` | tested at final positions: hitboxes, bolts, walking over a drop, what is in reach, and the loot a kill leaves |
| `animation::select_*`, `advance` | pick a clip, advance frames |

Then, **outside the dispatch and in every mode**, `inventory::derive_stats`
recomputes `base + sum(modifiers)`, and the tick counter advances — through
hitstop and through a modal screen alike.

Controllers only ever set a velocity and a few `Body` knobs (`gravity`,
`fall_cap`, `ignore_one_way`, `frozen`). They never integrate or collide.

Anything that *is* geometry — a moving platform, a fire on a cycle — owns a
`Collider` and moves it in the decide phase; `rebuild_geometry` picks it up the
same tick, and collision never learns it exists. The physics asks a
`SolidQuery` ("every solid overlapping this box"), never a `Vec<SolidRect>`, so
a query must stay a *subsequence* of the full list: resolution walks what it is
handed, and reordering it moves the game.

## Verify without playing

| Tool | Use it for |
| --- | --- |
| `cargo test` | everything; 29 tapes and 29 golden traces replay here |
| `cargo run --bin sim -- --tape tapes/x.tape` | one tape, with an event timeline |
| `... --trace out.jsonl` | tick-by-tick JSONL for inspection |
| `... --map maps/x.ron --geometry` | a map's collision rects, to write assertions against |
| `cargo run --bin sheet -- player` | one PNG per animation clip, to check art by eye |
| `Sim::fixture(&["..P...", "######"])` | a level and player inline in a test — no files |
| `SUPERGAME_DEBUG=1`, or F1 in game | colliders, sprite bounds, hitboxes and a state HUD drawn over the world — this is what catches art that does not match its collider |

**Authoring a map.** ASCII grids in `assets/maps/*.ron`: `#` solid, `=` one-way
platform, `^` spikes, `F` fire, `P` player spawn, `K` knight, `.` empty.
Anything with no character goes in the entity list instead — `Npc(kind:,
cell:)` for a villager, `Door(cell:, to:)`, and, because a character can say
where a thing is but not where it goes, `Platform(from:, to:, …)`,
`Swing(anchor:, length:, amplitude:, period:, …)` and `Fire(cell:, period:,
duty:, phase:)` for a fire off the house cycle. `src/level/ascii.rs` documents
every field and its default. Tile art is chosen by autotiling, so maps never
contain tile indices. `tests/levels.rs` checks every shipped map for
unreachable platforms, spawns inside geometry, and platform paths or swing arcs
that run into the level.

**Writing a tape.** Keys are the 11 actions in `ACTIONS` (`src/systems/input.rs`)
— `left right up down jump attack cast interact inventory confirm cancel`, plus
`wait` — combinable with `+`, and the table in `tapes/README.md` is generated
from that list and checked against it. `assert` samples state (`assert hp == 5`,
`assert knight.0.dead`, `assert clip == die`, `assert mode == inventory`,
`assert item.minor_potion.count == 2`, `assert quest.helm.stage == 1`); `expect`
counts events cumulatively (`expect knight.damaged == 3`). NPCs are
`<kind>.<index>` in spawn order.

**Assert outcomes, not timings, for anything involving two entities.** Hit
timing falls out of knockback, chase speed, i-frames, cooldown and hitstop at
once — pinning a tick in a tape encodes five tunables and breaks when any is
touched. The tape says the fight is winnable; the golden trace holds the detail.

**Balance by probing, not guessing.** Write a throwaway script that runs several
input patterns through `sim --trace` and prints the hit counts, pick the one
that plays well, then write the tape around it. A first pass at enemy numbers
has been off by 10-to-1.

## Trace discipline

Golden traces are what catch a refactor that quietly changed the game — proven:
bumping the player's jump speed by one (then an `Avatar` const, now
`stats.ron`) left every tape assertion passing and failed 7 of the 11 traces
that existed at the time.

**Never re-record a trace you have not accounted for.** Adding a probe field
rewrites all 29 baselines, which looks identical to a physics change. Run:

```bash
uv run .claude/skills/supergame-dev/scripts/trace_diff.py --ignore <new fields>
```

Zero remaining differences is the proof that only your new columns moved. Then,
and only then:

```bash
UPDATE_TRACES=1 cargo test --test traces
```

**Traces cannot see animation *mapping*.** They record clip names and frame
indices, not which sheet cells those resolve to. A clip pointing at the wrong
art produces byte-identical traces. Check that with `sheet`, by eye.

## Invariants

The repo's `CLAUDE.md` is the index of these, with a file reference for the
reasoning behind each. The short forms:

- **Gameplay logic lives in `Sim`, never in `scenes/`.** Anything in a scene is
  untestable by definition. This is most tempting for dialogue and inventory,
  which feel like UI and are simulation — both are `Mode`s on `Sim`, with the
  scene drawing and deciding nothing.
- **Never despawn an NPC.** They are addressed by spawn index in traces and
  tapes; removing one renumbers every later `knight.<n>` and silently repoints
  assertions. Mark dead, freeze, stop colliding. Projectiles and pickups are
  the two documented exceptions — nothing addresses one, so nothing can be
  renumbered by one leaving.
- **`Body::frozen` stops movement dead; `Health::hitstun` only suppresses the
  controller.** Knockback needs the second — a hit takes away steering, not
  momentum.
- **`Sim::npcs()` sorts by entity id.** hecs iterates archetypes in creation
  order, so adding or removing a component mid-run reshuffles query order.
  Never rely on raw query order where the order is observable.
- **Fixture maps under `assets/maps/testbed*.ron` are frozen** — tapes encode
  their exact pixels. Add to them; do not move what exists.
- **Balance and content go in RON**, code holds mechanics. Attacks in
  `assets/data/attacks.ron`, spells in `spells.ron`, every kind's movement and
  combat numbers in `stats.ron`, items in `items/*.ron`; frames in the clip
  sets. H-3b finished the job — `src/ecs/components.rs` holds no balance
  constant at all now, so there is no mirror left to drift.
- **Determinism**: `Sim::rng` is the only randomness, and `BTreeMap` over
  `HashMap` anywhere iteration order can reach a float sum or a serialized
  output (`Equipment`, `Sim::flags`).

## Gotchas that have already cost time

- **hecs components must be `Send + Sync`** — use `Arc`, never `Rc`.
- **RON needs the `implicit_some` extension** for bare `Option` values; it is
  enabled in `assets.rs::load_ron`, so write `sheet: "x"` not `Some("x")`.
- **The player sprite sheet runs sequentially across rows** — frame *i* is at
  cell `(i % 7, i / 7)`. Authoring a clip per row silently points it at the
  wrong animation. `player.ron` documents the real layout.
- **Screenshots grab whichever window is frontmost.** Raise the game and delay
  *inside one shell command*, then verify the capture really is the game:
  `osascript -e 'tell application "System Events" to set frontmost of (first
  process whose name contains "supergame") to true' -e 'delay 1.5'` then
  `screencapture -x -R<x>,<y>,<w>,<h> out.png`. Retry on failure.
- **Keystroke injection into the window does not work here.** To see something
  specific, boot into a map that shows it:
  `SUPERGAME_SCENE=adventure SUPERGAME_MAP=maps/testbed_arena.ron SUPERGAME_DEBUG=1 cargo run`
  Controls, for telling Jacob how to try it: arrows or WASD, jump on
  `Up`/`W`/`Space`, attack on `J`/`X`, cast on `K`, talk on `E`, bag on `I`
  (`Enter` confirms, `Backspace` backs out), `P` pause, `M` menu, `F1` overlay,
  `Esc` quit. Down+jump while running slides; down alone on a platform drops
  through; attack again mid-swing continues the combo; down+attack in the air
  is the plunge.
- **The repo is rustfmt-clean and CI enforces it** (`.github/workflows/ci.yml`
  runs `cargo fmt --all --check`). Run `cargo fmt --all` before committing;
  there is no longer any pre-existing drift for it to bury.

## Before saying it is done

- [ ] the three commands CI runs are clean: `cargo fmt --all --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`
- [ ] every trace difference accounted for via `trace_diff.py`, then re-recorded
- [ ] a tape covers the new behaviour, and it asserts something that would fail
      if the feature were removed
- [ ] anything that owns a collider moves it in the *decide* phase, so
      `rebuild_geometry` picks it up the same tick
- [ ] anything modal lives in `Sim` behind a `Mode`, never in a scene, so a tape
      can drive it
- [ ] anything visual checked on screen, or with `sheet` for animation data
- [ ] `ROADMAP.md` updated if a milestone moved, and the ticket's **Status** in
      `TICKETS.md` with it

Commits and pushes go through the **git-ops** skill.

## Example

Adding a new probe field (`stamina`) so tapes can assert on it:

```bash
# 1. add the field to Probe::new, FIELD_NAMES and Probe::field
# 2. prove nothing else moved
uv run .claude/skills/supergame-dev/scripts/trace_diff.py --ignore stamina
#    -> "No undeclared changes."   (if any tape DIFFERS on another field, stop
#       and explain it before going further)
# 3. re-record and confirm
UPDATE_TRACES=1 cargo test --test traces
cargo test
```

## Bundled resources

- `scripts/trace_diff.py` — **run** before re-recording any golden trace, to
  prove a diff is only the fields you added. `--help` documents the flags.
