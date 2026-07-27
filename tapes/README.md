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

right 40               # hold right for 40 ticks (60 ticks = 1 second)
right+jump 1           # combine keys with '+'
wait 30                # hold nothing
assert x > 384         # check the player's state at this point in the tape
assert !grounded
```

Keys are `left`, `right`, `down`, `jump`, `attack`, and `wait`/`none`. The
count defaults to 1. `#` starts a comment.

**Jump and attack are edge-triggered.** `jump 10` is one jump held for ten
ticks — exactly what holding the key does — not ten jumps. Releasing and
pressing again is what produces a second. `attack 1` is the usual way to write
a swing, since holding the key does nothing after the first tick.

Assertions read `assert <flag>`, `assert !<flag>`, or `assert <field> <op>
<value>` with `< <= > >= == !=`. Available names:

- numeric: `x`, `y`, `vx`, `vy`, `tick`, `air_jumps`, `frame`, `hp`,
  `iframes`, `hitstun`
- boolean: `grounded`, `facing_right`, `wall_sliding`, `double_jumping`,
  `crouching`, `dead`, `on_one_way_only`, `attacking`

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

Spawn order is stable across a run even as components are added and removed —
`Sim::npcs` sorts by entity id rather than trusting query order, and
`npc_order_survives_components_being_added_and_removed` is what holds that
down. It is *not* stable across map edits: move a `K` and `knight.0` may be a
different knight.

Clips are not assertable yet. `assert knight.0.clip == run` would need string
comparison in the tape language, which does not exist; the trace records the
clip name, so a golden trace catches an animation change even though no
assertion can name one.

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
`died`, `respawned`, `attacked`, `damaged`.

Three things to know:

**Narrow to a victim with `<subject>.<event>`.** `damaged` and `died` record
who they happened to, so `expect knight.damaged == 3` counts hits on the knight
and `expect player.damaged == 4` counts hits on you. A bare `expect damaged`
counts every hit on anything, which is rarely what you mean now that both sides
bleed. Only `damaged` and `died` can be narrowed; the rest have no subject.

**Counts are cumulative** from the start of the tape, not per-tick — you rarely
know which tick a landing happened on, but you always know it should have
happened by now. The flip side is that "nothing new fired in this section" is
written as "still the same count as before" (`expect jumped == 1`), not
`expect no jumped`.

**`landed` fires on tick 1** as the player settles onto the ground under their
spawn point, so a tape that jumps once and comes back down has landed twice.

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

`testbed_knight.ron` keeps the player two tiles below the knight's ledge on
purpose. Once the knight became hostile, a player standing on the same ledge
got chased, hit and knocked into the pit — correct behaviour, and useless for
testing a patrol route.

`castle_spawn.tape` is the exception: it runs on the real map but asserts only
that the player spawns on solid ground and is not standing in a hazard, so
redesigning the castle cannot break it.
