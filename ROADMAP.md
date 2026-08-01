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

Also done: **M1 through M7 below**, plus the engineering half of M8 — the `Body`
split, NPCs, the full melee kit and the shock spell, items and a modal inventory,
interaction and dialogue, quest flags with save/load, and live map geometry
(fire, moving platforms, swinging hazards). Cross-cutting: CI, and a root
`CLAUDE.md` indexing the invariants.

Not done: M8's remaining content — the "advanced world" and the story — and the
polish list (audio, particles, transitions). `TICKETS.md` carries the per-ticket
status.

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

## M1 — Split movement into intent and body ✅ done

`Body` component in `ecs/components.rs`, `systems/body.rs::move_bodies` moving
every `Position + Velocity + Size + Body` against geometry in one pass per
tick. The tick is now three phases — `avatar::control` decides a velocity and
sets the body's knobs, `move_bodies` moves everything, `avatar::after_move`
reacts to where things ended up.

Shipped with zero trace diffs, which was the exit criterion: the golden
baselines were recorded specifically so this refactor could prove it changed
the code and not the game.

Notes for what comes next:

- **`Body` carries per-tick knobs, not just state.** `gravity`, `max_fall`,
  `fall_cap`, `ignore_one_way`, `frozen` are written by a controller each tick;
  `grounded`, `on_solid`, `on_one_way`, `landed` are written by `move_bodies`.
  Variable jump height is "heavier gravity while rising", wall slide is a
  `fall_cap`, drop-through is `ignore_one_way`. Gravity is an absolute value
  rather than a scale on purpose — a multiplier would not have reproduced the
  old arithmetic bit for bit.
- **`frozen` is the freeze primitive.** The death freeze uses it; stuns,
  knockback recovery, and cutscenes should too. Phases 1 and 3 both consult it,
  because a body that did not move must not draw conclusions from stale
  contact either.
- **`landed` is a transition flag**, true for exactly one tick. Contact events
  need it; `grounded` alone cannot express touching down.

## M2 — NPCs ✅ done

Spawn registry (`ecs/spawn.rs`) turning `EntitySpawn.kind` into entities, a
`Patrol` controller writing to the shared `Body`, per-entity sprite sheets, and
NPCs in the trace and in tape assertions as `knight.0.x`.

The knight is harmless by design — it has no idea the player exists. That kept
the whole milestone verifiable without also inventing health: all nine testbed
baselines stayed byte-identical, and the two castle ones changed only by
gaining an `npcs` array, checked by stripping that key and diffing the rest.

Notes for what comes next:

- **`ClipSet` clips carry their own sheet and frame size.** The knight pack is
  seven files with four frame widths; the player is one atlas. Set-level values
  are defaults, validated at load so the renderer never sees a clip with no
  sheet. `ClipSet.offset` handles art that is not bottom-aligned in its cell —
  the knight's feet sit at row 44 of 64.
- **NPCs are addressed by kind and spawn index**, not by name. Map order
  decides it, `Sim::npcs` sorts by entity id to keep it stable while components
  come and go, and a test fails if that sort is removed. Named NPCs are an M5
  problem, when a quest needs one.
- **The seeded RNG was deferred to M3.** Patrol is fully deterministic, so an
  RNG here would have been dead code. What matters is the rule, not the
  plumbing: nothing may call `rand::thread_rng()`. The moment combat wants a
  crit roll, the RNG goes on `Sim` and is seeded per run.
- **Clip names are not assertable** — the tape language compares numbers only.
  Golden traces do record the clip, so an animation change is still caught.
- **Known art quirk**: the knight's jump/fall frames carry the character across
  the cell, so a jumping knight will visibly slide against a position that
  comes from physics. Harmless while it only walks; deal with it in M3.

## M3 — Combat ✅ done

**Size:** large. **Depends on:** M1, M2. All four sub-milestones below shipped;
M3c closed it.

**M3a ✅ done** — you can kill the knight. Attack as a latched action input,
`Health` with i-frames and hitstun, hitbox timing and balance in
`assets/data/attacks.ron`, combat resolved after `move_bodies`, knockback,
death, and the HUD. The player's health bar is in; the mana bar's space is
reserved and it arrives with the spell in M3c.

**M3b ✅ done** — the knight fights back. `Hostile` layered on `Patrol` for the
Patrol → Chase → Attack → Return state machine, the knight's own attack, the
player dying into the respawn that already existed, and `expect
knight.damaged` / `expect player.damaged` now that the unqualified count means
nothing.

Notes: sight is a box *in front*, not a radius, so walking up behind a
patrolling knight works. Being hit interrupts your swing, which is what makes
an exchange readable — whoever connects first wins it, both ways. Hazards kill
outright rather than dealing damage, so spikes stay lethal at any health.

**M3-melee ✅ done** — the full melee kit, and a correctness fix underneath it.
The Adventurer sheet's animations run *sequentially across rows*, not one per
row; the clip set had been authored by row, which put `wall_slide` on the
spell-cast frames, started `hurt` on the last frame of attack 3, and made the
attack clip span a wall-slide frame and an attack-2 frame. Rewritten from the
verified layout, which also turned up `die`, `slide`, both air attacks, and
the `cast` frames M3c needs.

On top of that: a three-hit ground combo (each attack names its successor in
`attacks.ron` — no state machine in code), a separate air attack, the slide
(down+jump while running), attack recovery so a miss costs something, a white
hit flash, and four ticks of hitstop on every connect.

Two things worth carrying forward. I-frames are per-entity now: the player's
window is long as a mercy, an enemy's is short enough that every link of a
combo lands — with one shared value only the first hit of a combo ever landed.
And golden traces cannot see an animation remapping at all: they record clip
*names* and frame indices, not which cells those point at, so all three
mapping fixes were verifiable only by eye.

**M3c ✅ done** — the shock spell, and the plunge, which finish M3. `Mana` with
an integer regeneration accumulator, `Casting` beside `Attacking`, `SpellDef`
in `assets/data/spells.ron` with `effect` an enum from the start, and the mana
bar in the space the HUD had been reserving for it. `SpellCast` and
`CastFailed` both, because "nothing happened" and "you were out of mana" are
the same absence in a trace otherwise.

The projectile is the first entity spawned mid-run, and the first thing in the
game that may be *despawned*: the never-despawn rule protects entities
addressed by spawn index, and nothing addresses a bolt — a trace records how
many exist, never which. `systems/spell.rs` holds both the cast and the bolt,
and says why in its module doc.

Two things worth carrying forward. A spell's art is a clip on the *caster's*
clip set rather than a sheet named in the balance file — that keeps
`spells.ron` free of PNGs, and it is also what gets the bolt drawn at all,
since the scene uploads textures once when the level loads and a projectile
does not exist yet. And the plunge needed a hitbox anchor (`Down`) rather than
a cleverly symmetric offset: an offset can be *made* to mirror onto itself for
one collider width, and stops being centred the moment anything else performs
the same attack.

Also from the original scope: moving stats to RON (H-3, done) and the plunge
attack (frames 94-108, a ready/loop/impact sequence, now three clips).

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
asserted by a tape, with the damage numbers coming from data files. ✅ Met:
`knight_kill`, `knight_fights`, `spell_kill`, `spell_cast` and `plunge` cover
the melee kill, the death and respawn, the ranged kill, the resource, and the
last of the melee kit — nineteen tapes and nineteen golden traces in all.

## M4 — Items and inventory ✅ done

**Size:** medium. **Depends on:** M2 (pickups are entities), M3 (loot drops).

`ItemDef` pipeline per PLAN.md: inventory component, pickups, consumables,
equipment slots with stat modifiers, weapon defs feeding melee damage. Stats
computed as `base + sum(modifiers)`, never mutated in place.

**Architectural note:** the inventory screen is the first *modal* state — it
pauses the world but is itself simulation, since using a potion changes game
state. This is where `Sim` gets a mode enum (`Playing | Inventory | …`) and
`step` dispatches on it. Getting this pattern established here, on the simpler
of the two modal features, is deliberate: dialogue inherits it.

Shipped as: `Mode { Playing, Inventory, Dialogue }` on `Sim` with `step`
dispatching on it, `assets/data/items/*.ron` behind an `ItemTable`, `Inventory`
/ `Equipment` / `DerivedStats` components, pickups as bodies that fall,
seeded loot rolls on the tick of death, and `scenes/inventory.rs` as an overlay
that draws and decides nothing. `Dialogue` is in the enum unused, so M5 does
not have to reopen the dispatch.

Two things worth knowing before M5 inherits this. **Derived stats are
recomputed at the end of every tick, in every mode and outside the dispatch** —
recomputing a view of state is not the world advancing, and the modal screen is
the only place equipment ever changes. And **a modal tick runs no world system
at all**, including the tick the mode is entered and the tick it is left, which
is what makes a detour through the inventory cost the run exactly nothing.

**Exit criteria:** kill the knight, loot drops, pick it up, equip it, stats
change — asserted headlessly, no window. ✅ Met: `tapes/inventory_use.tape`
is that chain end to end in one run, with `tapes/loot.tape` covering the
walk-over the melee kill cannot (the knockback drops the corpse underfoot) and
`tapes/inventory_frozen.tape` measuring the freeze against the same run with
the detour deleted — twenty-three tapes and twenty-three golden traces in all.

## M5 — Interaction and dialogue ✅ done

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

Shipped as: `Interactable { prompt, target }` with the proximity check in the
resolve phase and the answer cached on `Sim`, so the prompt drawn on screen and
the thing the key does are one answer to one question; dialogue graphs under
`assets/data/dialogue/` with `HasItem` / `FlagEq` / `FlagAtLeast` conditions and
`SetFlag` / `GiveItem` / `TakeItem` / `Heal` effects, validated at *load* so a
dangling `next` names its graph and node instead of dead-ending a player; a
`Conversation` on `Sim` driving `Mode::Dialogue` through the dispatch M4 left
open; and `scenes/dialogue.rs` as an overlay that draws and decides nothing.

Two decisions worth knowing before M6 builds on them. **A reply whose condition
fails is hidden, not greyed out** — the filtered list is the only list, which is
what makes `dialogue_choices` a count of what the player can actually do and
therefore what makes a gated branch unlocking visible to a tape at all. And **a
villager is on the player's team and does carry `Health`**: the player cannot
kill the quest giver, because friendly fire is already impossible in
`combat::resolve` and so needs no special case that could later be forgotten; a
knight still can, which is why the proximity check skips the dead.

Tape support needed no parser work: `interact`, `up`, `down`, `confirm` and
`cancel` were already in the action table, so a tape selects a reply by pressing
the keys a player presses rather than through a bespoke `choose 2` directive.

M6 inherited a `flags` map on `Sim` that `SetFlag` already wrote and the
conditions already read, with nothing about flags in a baseline; Q-1's whole job
was surfacing it, and holding it back to here is why exactly one baseline moved
when it did.

**Exit criteria:** a tape walks an NPC conversation, takes a branch gated on an
inventory item, and asserts the resulting state change. ✅ Met:
`tapes/dialogue_branch.tape` visits the same node twice — three replies with an
empty bag, four once Runa's draught is in it — takes the branch that did not
exist the first time, and asserts the draught leaving the bag and the charm
arriving in it. `tapes/interact_prompt.tape` covers the prompt appearing,
clearing, being announced once per approach, and doing nothing at all when there
is nobody in reach — twenty-six tapes and twenty-six golden traces in all.

## M6 — Quests ✅ done

**Size:** small once M5 lands. **Depends on:** M5.

Quest flags as global sim state, dialogue conditions and effects reading and
writing them, a fetch quest as proof, and persistence so progress survives a
save/load.

Quests are mostly flags plus dialogue, which is why they are cheap here and
would have been expensive earlier. They surface in the world snapshot as
globals, so `assert quest.rescue.stage == 2` works without new machinery.

Shipped as: the `flags` map M5 left on `Sim` surfaced as `quest.<name>.<field>`
assertion paths and a `flags` key in the trace that is **absent when nothing has
been set** — so adding it churned exactly one baseline,
`traces/dialogue_branch.jsonl`, on the one tape that already set a flag; a
`SaveState::flags` field added by the additive recipe `src/save.rs` documents
(`#[serde(default)]`, no version bump); `DialogueCondition::All`, which is the
one thing the condition enum turned out to be missing, because the return leg of
a fetch quest asks two questions at once; `assets/maps/testbed_quest.ron` and
`tapes/quest_fetch.tape`; and save and load reachable from the pause menu, which
Q-3 had left as a library nobody could call.

Two things worth knowing before M8 builds on them. **`quest.` is a reserved
root** rather than "any dotted path that is not a probe field": flags are
created by writing one, so there is no list to check a path against, and a bare
fallback would turn every mistyped path in every tape into a flag quietly
reading 0. And **a flag is the sanctioned way to make the world remember
anything across a load** — a save deliberately carries no NPCs, so a boss that
stays dead is `quest.boss.stage` and not a saved entity.

**Exit criteria:** accept a quest, complete it, get a reward, and have it
survive save and load. ✅ Met: `tapes/quest_fetch.tape` kills the knight that
carries the helm Runa asks for, takes the errand (`quest.helm.stage` 0 → 1),
fetches the helm off the floor, hands it over for her band (→ 2), and asks again
to find the reply gone; `tests/save.rs` saves at a dozen points mid-run and
replays both sims into identical traces with the flags in the frame, so a stage
that failed to survive a save shows up as the tick the two runs diverged —
twenty-seven tapes and twenty-seven golden traces in all.

## M7 — Live map features ✅ done

**Size:** medium. **Depends on:** M1. **Parallelizable** — independent of the
combat/dialogue chain, so it slotted in where there was appetite.
Entity colliders, the broadphase, fire, moving platforms and swinging
hazards have all landed.

Fire (a hazard that toggles on a cycle), moving platforms, and swinging
hazards. All three need the same thing: **geometry that changes every tick**.

This is the deepest change to the physics layer on the roadmap:

- ✅ Colliders owned by entities, not just flattened from the level at load.
- ✅ **Rider carry** — standing on a moving platform has to transport you. This
  was the genuinely tricky part and the source of most platformer physics bugs.
- ✅ A broadphase (uniform grid), behind `SolidQuery`.

Fire was the cheap one — a hazard rect with a schedule — and proved
entity-owned colliders before rider carry was attempted.

**Exit criteria:** ✅ `tapes/platform_ride.tape` rides a platform across a
spiked gap and arrives; the property sweeps in `tests/physics_diagnostics.rs`
pass with dynamic geometry in the mix, and six new ones cover carry itself.
✅ `tapes/swing_dodge.tape` times a run under a swinging ball, survives, then
stands in its arc and dies to it.

Swinging hazards (L-4) closed the milestone and cost almost nothing, which is
the point: the ball is a `Position`, a `Hazard` collider and a closed form.
Nothing in the physics, the geometry rebuild or the death check changed to
accept it. `Pendulum::at` is `anchor + length * (sin theta, cos theta)` with
`theta(t) = amplitude * cos(2*pi * (t + phase) / period)`, and the tick reaches
the cosine only as an integer remainder — which is what makes `at(t)` and
`at(t + period)` bit-identical rather than merely close. An integrated pendulum
would have been worse than an integrated platform: explicit Euler gains energy
every cycle, so the authored amplitude would not be the amplitude the ball
reached, and it would drift wider the longer a trace ran.

Notes from rider carry, for anything else that owns a collider:

- **A mover's position is a closed form of `Sim::tick`**, like `Schedule`, not
  an integration. A counter drifts, and drifts differently across the ticks
  hitstop skips; `Mover::at(t)` is the same answer forever. The leg is a whole
  number of *ticks* rather than a speed, so the period is an integer and
  `at(t) == at(t + period)` is a fact rather than an approximation.
- **Ordering is the whole feature.** `mover::advance` is the last thing before
  `rebuild_geometry`: after the controllers (so drop-through is visible), before
  the rebuild (so a rider does not collide against last tick's rect), and before
  `move_bodies` (so the ride and the rider's own velocity resolve together).
  `Sim::step`'s doc comment says so, and the sweeps depend on it.
- **Carry adds to position, and leaving inherits nothing.** Velocity carry leaks
  the platform's momentum into the jump arc; a body with upward velocity is not
  a passenger, so a jump off a platform is the same jump either way.
- **Crush has no clean answer and does not need one.** A body wedged between an
  advancing platform and a wall has nowhere legal to be. The depenetration
  fallback keeps it from tunnelling and from staying stuck, and
  `tests/levels.rs` refuses to ship a map whose platform path dead-ends into
  geometry, which is the only way an author reaches that state on purpose.

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

Shipped so far: `tests/data.rs` (N-1), which cross-references entity kinds, clip
sets, stat blocks, sheet cells, images, attack ids and chains, spell clips, item
ids, loot tables and dialogue graph connectivity and reachability; the village
hub (N-2) with `tapes/village_smoke.tape`; and the dungeon (N-3) with
`tapes/dungeon_run.tape`, a full traversal that claims the level is completable
and its hazards survivable with correct play. The "advanced world" and the story
are the part still open, which is why this milestone stays `ongoing` rather than
closing.

---

## Cross-cutting

Work that does not belong to one milestone.

**Renderer.** ✅ mostly done in M2. `Sprite` carries a clip set that names its
own sheet, and `AdventureScene` uploads every distinct sheet the world
references once at level load, keyed by name — which is also why a projectile's
art is a clip on its *caster's* set rather than a PNG named in `spells.ron`: a
bolt does not exist yet when the textures go up. Still outstanding: explicit
z-layers for draw order, and item art (inventory rows are coloured quads keyed
off the item's kind today).

**Save system.** ✅ done in Q-3/Q-3b. PLAN.md specifies rusqlite; what shipped
is a serde `SaveState` behind a `SaveStore` trait with a RON file backend, and
the deviation is argued at the top of `src/save.rs`. The requirement that
actually mattered — whatever it stores has to be reconstructible into a `Sim` —
is unchanged, and it did prove a good forcing function for keeping game state
out of scenes.

**CI and formatting.** ✅ done in X-1. `.github/workflows/ci.yml` runs
`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and
`cargo test` on push and PR — the same three commands a developer runs locally.
The repo is rustfmt-clean; the mechanical formatting commit happened first and
on its own, as planned.

**CLAUDE.md.** ✅ done in X-2. A root `CLAUDE.md` indexes the invariants and
points at where each one's reasoning already lives — the strict-overlap comment
on `Aabb::overlaps` in `physics.rs`, the `InputLatch` rationale in `input.rs`,
the frozen-fixture rule for testbed maps, the tick's one geometry-rebuild point.
It is a map, not a manual: it defers to
`.claude/skills/supergame-dev/SKILL.md` for workflow and does not restate it.

## Harness capability by milestone

The through-line. Each row is tooling that must exist before the feature it
serves, not after.

| Milestone | Harness work |
| --- | --- |
| ~~M1 Body split~~ | ~~none — the traces already cover it~~ (held: zero diffs) |
| ~~M2 NPCs~~ | ~~snapshot `Probe`, path assertions, ordering guard~~ (RNG moved to M3) |
| ~~M3 Combat~~ | ~~combat events, action-set inputs (attack/cast) in tapes~~ (H-1's `ACTIONS` table now feeds the keyboard reader, the tape parser and `tapes/README.md` from one list; H-2's seeded `Sim::rng`) |
| ~~M4 Items~~ | ~~inventory in the snapshot, pickup/equip events~~ (`item.<id>.count`, `mode`, six item events) |
| ~~M5 Dialogue~~ | ~~choice selection in tapes, dialogue events~~ (no parser work needed — `interact`/`up`/`down`/`confirm` were already actions; `prompt`, `dialogue_node`, `dialogue_choices`, `dialogue_selection` and five events) |
| ~~M6 Quests~~ | ~~quest flags as snapshot globals~~ (`quest.<name>.<field>` paths, a `flags` key omitted when empty, and the same map in `SaveState`) |
| ~~M7 Map features~~ | ~~property tests for dynamic geometry and rider carry~~ (six sweeps in `tests/physics_diagnostics.rs`) |
| ~~M8 Content~~ | ~~`tests/data.rs` cross-reference validation, per-map smoke tapes~~ (18 cross-reference tests; `castle_spawn`, `village_smoke`, `dungeon_run`) |

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
- **It specifies rusqlite for saves.** Q-3 deliberately shipped a serde
  `SaveState` behind a `SaveStore` trait with a RON file backend instead: the
  requirement that matters is "reconstructible into a `Sim`", and a file keeps
  save tests headless, fast and diffable in a commit with no C build dependency.
  A rusqlite backend is one more `impl SaveStore` and nothing above that line
  changes. `src/save.rs` makes the argument, and `MemoryStore` exists partly to
  keep the seam honest.
- **It has `Equipment` as `HashMap<Slot, ItemId>`.** It is a `BTreeMap`, and
  that is load-bearing rather than a preference: deriving stats walks it and
  sums modifiers, so hash order would reach a float and make a golden trace
  depend on hash seeding. `Sim::flags` is a `BTreeMap` for the same reason.
- **Its Phase 3 gives the knight a "death anim, despawn".** Nothing despawns an
  NPC here: they are addressed by spawn index in tapes and traces, so a corpse
  is marked dead and frozen instead. Projectiles and pickups *are* despawned,
  and `src/systems/spell.rs` and `src/systems/inventory.rs` say why they are
  different.
- **Its `NpcDef` per file under `data/npcs/` never happened.** Per-kind numbers
  are one `assets/data/stats.ron` table keyed by kind, with `ecs/spawn.rs`'s
  `KINDS` as the registry — one file to open when tuning, and `tests/data.rs`
  cross-references it against the clip sets and the maps.
- Its generic `Cooldowns` component (rule 5) was not needed: the timers that
  exist are typed fields on `Attacking`, `Casting` and `Health`, which the probe
  can name individually. `Casting::cooldown`'s doc names the day that changes —
  the second entry in `spells.ron`.

## Open questions

- **Movement feel has never been play-tested.** The harness proves correctness
  and cannot say anything about feel. The constants in `ecs/components.rs` are
  unjudged, and M3 is a natural moment to tune them — ideally before combat
  timing is balanced against them.
- **Asset licensing** for the itch.io packs (Adventurer sprite, knight pack,
  castle tiles) is still unverified. Blocking for public distribution only.
- ~~**Scope of the RPG layer.** PLAN.md's item/equipment system is fairly deep.
  Worth deciding at M4 whether that depth is wanted or whether a lighter
  inventory serves the game better.~~ Decided in M4: PLAN.md's schema shipped
  as written, minus two things. There is no *drop* action — no key a player
  would guess is free, and dropping into a frozen world raises a
  pick-it-straight-back-up problem M4 does not need to solve. And
  `ItemKind::Weapon`'s `speed` is authored but unread; making a weapon swing
  faster is either its own entries in `attacks.ron` (which `combo` already
  expresses) or a multiplier threaded through `AttackDef`, and neither was
  needed to meet the exit criterion.
