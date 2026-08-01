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

- numeric: `x`, `y`, `vx`, `vy`, `tick`, `air_jumps`, `frame`, `hp`,
  `iframes`, `hitstun`, `mana`, `mana_max`, `cast_cooldown`, `projectiles`
- boolean: `grounded`, `facing_right`, `wall_sliding`, `double_jumping`,
  `crouching`, `dead`, `on_one_way_only`, `attacking`, `casting`, `plunging`
- text: `clip` — compared with `==` or `!=` only

`projectiles` is a count of every bolt in the air anywhere in the world, not a
probe per bolt: a line per projectile per tick makes a trace unreadable long
before it makes it informative, and what a tape wants to say is "the bolt
existed" and "it hit". `mana` is `0` for anything with no pool at all, which is
the same answer an empty pool gives — and correctly so, since both mean "cannot
cast".

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
`slid`, `died`, `respawned`, `attacked`, `damaged`, `spell_cast`, `cast_failed`.

Three things to know:

**Narrow with `<name>.<event>`.** Five events carry a discriminating name.
`damaged` and `died` carry the victim, so `expect knight.damaged == 3` counts
hits on the knight and `expect player.damaged == 4` counts hits on you.
`attacked` carries the attack id, so `expect player_slash3.attacked >= 3` says
the finisher actually saw use, and `expect no player_plunge.attacked` says a
tape never plunged. `spell_cast` and `cast_failed` carry the spell id, so
`expect shock.spell_cast == 1`. A bare `expect damaged` counts every hit on
anything, which is rarely what you mean now that both sides bleed.

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

`castle_spawn.tape` is the exception: it runs on the real map but asserts only
that the player spawns on solid ground and is not standing in a hazard, so
redesigning the castle cannot break it.
