# Input tapes

A tape is a scripted keypress sequence played through the real simulation with
no window and no GPU. Every tape here is a regression test: `cargo test` replays
all of them, and a failure names the tick, the line, and the actual value.

Adding coverage for a movement feature means writing a tape, not writing Rust.

## Running one

```bash
cargo run --bin sim -- --tape tapes/wall_jump.tape
```

Add `--trace out.jsonl` to record every tick, or `--trace -` for stdout.

`--geometry` prints a map's collision rectangles, which is how you find the
coordinates to write assertions against.

## Golden traces

Every tape has a recorded trace in `traces/`, and `cargo test` checks that it
still reproduces it. A tape's assertions cover whatever the author thought to
write down; a trace covers every probe field and every event on every tick, so
it catches the changes assertions walk past — a landing that shifts by one
tick, a jump arc that moves two pixels, an event that stops firing.

```bash
cargo test --test traces                    # check
UPDATE_TRACES=1 cargo test --test traces    # re-record after an intended change
```

A failure names the tick and the fields that moved:

```
jump: behavior changed
  first difference at trace line 7
    vy: -496.66666 -> -497.66666
    y: 341.72223 -> 341.70557
```

Re-recording is deliberate: the diff lands in the commit, where it can be read.

## Format

```
map maps/testbed.ron   # which map to run on — required
seed 1234              # random seed, optional (see below)

right 40               # hold right for 40 ticks (60 ticks = 1 second)
right+jump 1           # combine keys with '+'
wait 30                # hold nothing
assert x > 384         # check the player's state at this point in the tape
assert !grounded
```

A count defaults to 1, and `#` starts a comment. `wait`, `none` and `idle` all
mean "no input". Every other key is an action from the table below, which is
generated from `ACTIONS` in `src/systems/input.rs` — the single place an action
is defined, and what `tapes_readme_lists_every_action` checks this against.

<!-- actions:begin -->
| Tape name | Keyboard | Trigger |
| --- | --- | --- |
| `left` | Left, A | level |
| `right` | Right, D | level |
| `down` | Down, S | level |
| `up` | Up, W | level |
| `jump` | Up, W, Space | edge |
| `attack` | J, X | edge |
| `cast` | K | edge |
| `interact` | E | edge |
| `inventory` | I | edge |
| `confirm` | Enter | edge |
| `cancel` | Backspace | edge |
<!-- actions:end -->

Combined keys do double duty: `down+jump` while running is a slide, `down`
alone on a one-way platform is a drop-through, `attack` during a swing chains
the next link of the combo, and `down+attack` *in the air* is the plunge — on
the ground the same combination is still the ordinary combo, and `attack`
alone in the air is still the air attack.

`cast` spends mana and starts the spell named by the player's stat block
(`spell:` in `assets/data/stats.ron`, resolved against `assets/data/spells.ron`).
It is refused, loudly, if a swing or another cast is running, if the cooldown
has not lapsed, or if the pool is short — see `cast_failed` below.

`inventory` opens and closes the inventory screen, and `confirm` (Enter),
`cancel` (Backspace) and the four directions drive it once it is open. See
[Modes](#modes) — the screen is simulation, so every one of those keys is a
real key going through the real action table, and a tape can walk the whole UI.

`interact` (E) talks to whatever is in reach, and the same `confirm`, `cancel`,
`up` and `down` drive the conversation. There is deliberately no `choose 2`
directive: a tape selects a reply by pressing the keys a player presses, so what
the tape exercises is the input path the game ships. See
[Talking to people](#talking-to-people).

**Edge-triggered actions fire once per press.** `jump 10` is one jump held for
ten ticks — exactly what holding the key does — not ten jumps. Releasing and
pressing again is what produces a second. `attack 1` is the usual way to write
a swing, since holding the key does nothing after the first tick. Level-
triggered ones are the opposite: `right 40` is 40 ticks of running.

## Randomness

The simulation has one seeded generator (`Sim::rng`) and nothing else — no
`thread_rng`, no clock — because a tape that rolled different numbers each run
would assert nothing. `seed <n>` fixes it for a tape; leave it out and the run
uses the default every trace here was recorded at. A tape that sets a seed gets
it written onto the first line of its trace, so a recording says what it takes
to reproduce it.

Assertions read `assert <flag>`, `assert !<flag>`, or `assert <field> <op>
<value>` with `< <= > >= == !=`. Available names:

- numeric: `x`, `y`, `vx`, `vy`, `tick`, `air_jumps`, `frame`, `hp`, `hp_max`,
  `iframes`, `hitstun`, `mana`, `mana_max`, `cast_cooldown`, `projectiles`,
  `pickups`, `inventory_count`, `selection`, `dialogue_choices`,
  `dialogue_selection`
- boolean: `grounded`, `facing_right`, `wall_sliding`, `double_jumping`,
  `crouching`, `dead`, `on_one_way_only`, `attacking`, `casting`, `plunging`
- text: `clip`, `mode`, `pane`, `prompt`, `dialogue_node` — compared with `==`
  or `!=` only

`projectiles` and `pickups` are counts of every bolt in the air and every item
on the floor anywhere in the world, not a probe each: a line per entity per
tick makes a trace unreadable long before it makes it informative, and what a
tape wants to say is "the bolt existed", "it hit", "the kill dropped
something". `mana` is `0` for anything with no pool at all, which is the same
answer an empty pool gives — and correctly so, since both mean "cannot cast".

`hp_max` is the *derived* maximum: base plus whatever is equipped. It is the
field to watch when something is put on, because current health is clamped when
the maximum moves and never topped up — equipment is not a heal.

## Modes

`mode` is what the simulation is doing: `playing`, `inventory`, or `dialogue`.
A modal mode pauses the world and is still simulation, because using a potion
changes health. So the inventory screen's whole state lives in `Sim`, and a
tape can drive it:

```
inventory 1                  # open it — the world stops on this very tick
assert mode == inventory
assert pane == bag           # `bag` or `gear`; left/right switch
down 1                       # one press is one step, even if the key is held
assert selection == 1
confirm 1                    # use a consumable, or equip anything else
expect minor_potion.item_used == 1
inventory 1                  # or `cancel 1`
assert mode == playing
```

While modal, **nothing** about the world moves — not positions, not timers, not
even the animation frame, because `animation::advance` is a world system and no
world system runs. The tick counter does keep going, exactly as it does through
hitstop, so a trace stays aligned with wall-clock time and the pause reads as
the run of identical frames it is. The consequence a player feels is that a
detour through the bag costs the run nothing at all;
`tapes/inventory_frozen.tape` measures that against the same run with the
detour deleted.

Pause (`P`) is deliberately *not* a mode. It stops the sim being stepped at
all, and nothing about it is simulation.

## Talking to people

Dialogue is the other modal state, and it is simulation for a stronger reason
than the inventory is: a reply takes an item out of the bag, puts one in, and
sets a quest flag. So the graph, the current node, which replies are on offer
and where the highlight sits all live in `Sim`, and a tape drives the whole
conversation through the real action table:

```
right 160
assert prompt == talk         # `none` when nothing is in reach
interact 1                    # the world stops on this very tick
assert mode == dialogue
assert dialogue_node == greet
assert dialogue_choices == 3  # what you can actually say, see below
confirm 1                     # read the next line of the speech
wait 1
down 1                        # one press is one step, even if the key is held
confirm 1                     # take the reply under the highlight
assert item.minor_potion.count == 1
cancel 1                      # back out from anywhere
assert mode == playing
```

**A reply whose condition fails is hidden, not greyed out.** There is no row you
can highlight and be refused, so `dialogue_choices` is a count of what the
player can *do* and `dialogue_selection` indexes into that. The observable this
buys is the one that matters: the count goes up the moment you are carrying the
thing a branch is gated on, which is what `tapes/dialogue_branch.tape` asserts.
`src/systems/dialogue.rs` has the full argument.

`prompt` is the word the interactable offers — `talk` — or the literal `none`
when nothing is in reach. A word rather than an empty string because an
assertion is whitespace-separated tokens, so `assert prompt ==` with nothing
after it would not parse and "there is nobody there" would be unassertable.

`dialogue_choices` is the current node's filtered count from the moment the node
is entered, including while there is still speech to page through — the
filtering is a fact about the world, not about how far into a speech you have
read. Only whether the overlay *draws* the replies waits for the last line.

## Asserting on items

Items are addressed by id rather than by index, because a stack leaves the bag
the moment it empties and nothing about its position is stable:

```
assert item.minor_potion.count == 2
assert item.knight_helm.equipped
assert inventory_count == 1        # occupied slots — a count of *kinds*
```

- numeric: `count` — how many are in the bag, `0` for something worn and no
  longer carried
- boolean: `equipped`

An item nobody is carrying reads as `count == 0` rather than failing to
resolve, so a tape can assert both sides of a pickup. The price is that a
mistyped id also reads as zero: `tests/data.rs` is what catches a typo in
*content*, and a tape should assert a count going **up** rather than merely
being what it already was.

## Asserting on NPCs

A bare name is the player, and always will be — every tape written before there
were NPCs still means what it said. NPCs are addressed as `<kind>.<index>`,
where the index is spawn order (the map grid scanned left to right, top to
bottom, then the explicit entity list):

```
assert knight.0.x <= 458
assert knight.0.dir == -1
assert !knight.0.facing_right
assert player.grounded        # the long way round, if you want the symmetry
```

- numeric: `x`, `y`, `vx`, `vy`, `dir`, `frame`, `hp`, `hitstun`
- boolean: `grounded`, `facing_right`, `dead`
- text: `clip`, `kind`

Spawn order is stable across a run even as components are added and removed —
`Sim::npcs` sorts by entity id rather than trusting query order, and
`npc_order_survives_components_being_added_and_removed` is what holds that
down. It is *not* stable across map edits: move a `K` and `knight.0` may be a
different knight.

## Checking animations

`assert clip == die` and `assert knight.0.clip == death` say which animation is
playing. That covers animation *selection* — a swing painted over by the run
cycle looks exactly like an attack that never happened, and this is what tells
them apart.

It does not cover animation *mapping*: which sheet cells a clip resolves to.
Traces record clip names and frame indices, not cells, so a clip set pointing
`wall_slide` at the wrong art produces byte-identical traces before and after
the fix. Nothing automated catches that, because "is this the right animation?"
is a question about pixels. What there is instead is a way to look cheaply:

```bash
cargo run --bin sheet -- player          # one PNG per clip, named after it
cargo run --bin sheet -- knight --clip run
cargo run --bin sheet -- player --grid   # the raw sheet with a cell grid,
                                         # for deriving a new pack's layout
```

## Events

`assert` samples state; `expect` checks things that *happened*. Most of what a
game does is transient — a hit that lands and resolves inside one tick leaves
nothing in any state field, so no `assert` can see it. Combat, dialogue, and
quests are almost entirely this shape.

```
expect landed            # fired at least once by this point
expect no died           # has not fired at all
expect wall_jumped == 1  # fired exactly once
```

Events: `jumped`, `double_jumped`, `wall_jumped`, `landed`, `dropped_through`,
`slid`, `died`, `respawned`, `attacked`, `damaged`, `spell_cast`,
`cast_failed`, `mode_changed`, `picked_up`, `inventory_full`, `item_used`,
`equipped`, `unequipped`, `interact_prompted`, `interacted`, `dialogue_opened`,
`choice_taken`, `dialogue_closed`.

Three things to know:

**Narrow with `<name>.<event>`.** Sixteen events carry a discriminating name.
`damaged` and `died` carry the victim, so `expect knight.damaged == 3` counts
hits on the knight and `expect player.damaged == 4` counts hits on you.
`attacked` carries the attack id, so `expect player_slash3.attacked >= 3` says
the finisher actually saw use, and `expect no player_plunge.attacked` says a
tape never plunged. `spell_cast` and `cast_failed` carry the spell id, so
`expect shock.spell_cast == 1`. The six item events carry the item, so
`expect knight_helm.equipped == 1` and `expect minor_potion.item_used == 2`;
`mode_changed` carries the mode *entered*, so `expect inventory.mode_changed`
counts openings rather than openings and closings. The four dialogue events
carry the graph — `expect elder_intro.dialogue_opened == 2` — except
`choice_taken`, which carries the *node*, because "how many replies were taken
at this node" is the question worth asking of a conversation and the graph is
already countable through `dialogue_opened`. A bare `expect damaged`
counts every hit on anything, which is rarely what you mean now that both sides
bleed — and a bare `expect no died` counts the knight's death as well as yours,
so what you almost always want is `expect no player.died`.

**`inventory_full` is the pickup's `cast_failed`.** An item you have no room
for stays on the floor and leaves nothing in any state field, so without the
event "the pickup is broken" and "your bag is full" are the same absence. It
covers both directions of the same problem — a pickup refused, and a worn item
that could not be taken off — and fires once per approach rather than once per
tick of standing on the thing.

**`cast_failed` is not decoration.** A cast that does not happen leaves nothing
in any state field, so without this event "the key did nothing" and "you were
out of mana" are the same absence in a trace. The event records which of the
four it was — `busy`, `cooling`, `no_mana`, `unknown` — and it fires on the
tick the key was pressed. `spell_cast` likewise fires when a cast *starts*, not
when the bolt appears: that is the tick the decision was made and the tick the
mana left the pool. The bolt is `cast_ticks` later, and shows up as
`projectiles` going to 1.

**Counts are cumulative** from the start of the tape, not per-tick — you rarely
know which tick a landing happened on, but you always know it should have
happened by now. The flip side is that "nothing new fired in this section" is
written as "still the same count as before" (`expect jumped == 1`), not
`expect no jumped`.

**`landed` fires on tick 1** as the player settles onto the ground under their
spawn point, so a tape that jumps once and comes back down has landed twice.
The flip side of the same fact catches people out: on tick 1 the player has
*not yet collided with anything*, so `grounded` is still false and
`down+attack` on that tick is a plunge rather than a combo. A tape that means
to start on the floor opens with `wait 1`.

## Fixture maps

Movement tapes run against `testbed*.ron`, not against real levels. Real levels
get redesigned; a tape that encodes castle geometry would break every time the
level changed, for no good reason. The fixtures are frozen — add to them, but
do not move what is already there.

| Map | For |
| --- | --- |
| `testbed.ron` | running, jumping, gap clearing, spike death |
| `testbed_platform.ron` | one-way platforms and drop-through |
| `testbed_chimney.ron` | wall slide, wall jump, wall-contact grace |
| `testbed_knight.ron` | patrolling: a knight on a ledge, with the player on a shelf below and out of its sight |
| `testbed_arena.ron` | fighting: a small walled room with a knight and nowhere to run |
| `testbed_fire.ron` | fire: two alternating fires on a flat corridor, one to cross while it is out and one to stand in until it lights |
| `testbed_mover.ron` | moving platforms: two ledges with a spiked pit between them and one platform ferrying across |
| `testbed_swing.ron` | swinging hazards: one room with a spiked ball on a chain across the middle of it |
| `testbed_village.ron` | interaction and dialogue: a long empty room with a step down into a bay, and Runa the Herbalist in it |

Items have no fixture map of their own, and deliberately so: the only source of
items in the game is a corpse, so `loot.tape` and `inventory_use.tape` both run
on `testbed_arena.ron` and start by killing the knight there. They kill it
differently on purpose — `loot.tape` at range, so the drops land where the
knight fell and walking over them is actually tested, and `inventory_use.tape`
in melee, so the knockback puts the drops underfoot and the fight costs enough
health that the potion has something to heal.

`testbed_knight.ron` keeps the player two tiles below the knight's ledge on
purpose. Once the knight became hostile, a player standing on the same ledge
got chased, hit and knocked into the pit — correct behaviour, and useless for
testing a patrol route.

`testbed_mover.ron`'s pit is spiked on purpose too: if the ride fails in any
way the player lands in it, so `expect no died` is the whole crossing in one
line. The platform's path is level with both ledges, so the crossing is walked
onto and off rather than jumped between — which is what lets the tape assert
`expect no jumped` and `expect landed == 1` and thereby claim that contact
never flickered off, not even for a tick, while the ground was moving.

`testbed_swing.ron` is the one fixture whose tape is genuinely *timed*. The
ball is only at body height near the bottom of its arc, so the room has a
window rather than a safe route: `swing_dodge.tape` starts its run on the tick
that leaves the widest clearance (66px), and starting twenty-two ticks later or
sixty-five earlier is a death. That number came out of a sweep, not out of
guessing — see "Balance by probing" in the dev skill.

`testbed_village.ron`'s bay is the one piece of its geometry that is load-
bearing. A villager has `Patrol` and no `Hostile`, so it paces, and a patroller
only turns at a wall or at the edge of what it is standing on — which makes its
route as wide as the room it is in. Runa crossing four hundred pixels while the
player walks over would make every assertion in both dialogue tapes depend on
when exactly she turned round. The step down at the far end pens her into a
thirty-pixel beat, the player runs off the ledge into the bay and is stopped by
the far wall, and neither tape has to time anything: run right until the wall
stops you, and she is there.

`castle_spawn.tape` is the exception: it runs on the real map but asserts only
that the player spawns on solid ground and is not standing in a hazard, so
redesigning the castle cannot break it.

## Tapes that run on real levels

Three do, and each has a reason to.

| Tape | Map | What it is for |
| --- | --- | --- |
| `castle_spawn.tape` | `castle.ron` | the spawn point is solid ground and not a hazard, and nothing else |
| `village_smoke.tape` | `village.ron` | the hub is walkable end to end, the elder is in reach, and nothing in it can hurt you |
| `dungeon_run.tape` | `dungeon.ron` | a full traversal: both fires, both knights, the pit, the ride, the gallery, the loot, the way out |

The rule the fixtures exist for still holds — a *mechanic* gets a testbed,
because a real level will be redesigned — so these three assert what a level is
for rather than what the physics does. `village_smoke` says the street has no
gap in it and Runa is where the map says she is; `dungeon_run` says the level
is completable and that its hazards are survivable *with correct play*, which
is a claim only a tape can make. Both will need editing if their map is
redesigned, and that is the intended cost of shipping a level with a test.

`dungeon_run.tape` is timed in three places and outcome-asserted everywhere
else. The fire crossing waits for a gap, the ride is boarded as the platform
docks, and the drop off the gallery is taken while the knight below is walking
away — none of which can be written as anything but a tick count, because they
are all "go now". Everything about the fights is an outcome: which knight is
dead, whose health moved, what ended up in the bag.

It also spells `expect no died` as **`expect no player.died`**, which the N-3
ticket's own wording would have got wrong: a bare `died` counts a knight's
death too, and the tape kills two of them.

`village.ron`'s doorstep and `dungeon.ron`'s rubble steps are the same trick
`testbed_village.ron`'s bay is, generalized. A 32px step is a wall to anything
that walks and nothing at all to anything that jumps, so it pens an NPC into a
readable beat while costing the player one keypress. Runa is penned in a
two-tile notch so that "walk east until the street drops out from under you"
puts the player somewhere definite with her in reach; both dungeon knights are
penned so that a fight stays in the room it was designed for.
