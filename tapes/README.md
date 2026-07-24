# Input tapes

A tape is a scripted keypress sequence played through the real simulation with
no window and no GPU. Every tape here is a regression test: `cargo test` replays
all of them, and a failure names the tick, the line, and the actual value.

Adding coverage for a movement feature means writing a tape, not writing Rust.

## Running one

```bash
cargo run --bin sim -- --tape tapes/wall_jump.tape
```

Add `--trace out.jsonl` to record every tick. The sim is deterministic, so
`diff` over two traces of the same tape reports the exact tick at which a code
change altered behavior:

```bash
git stash && cargo run --bin sim -- --tape tapes/jump.tape --trace before.jsonl
git stash pop && cargo run --bin sim -- --tape tapes/jump.tape --trace after.jsonl
diff before.jsonl after.jsonl | head
```

`--geometry` prints a map's collision rectangles, which is how you find the
coordinates to write assertions against.

## Format

```
map maps/testbed.ron   # which map to run on — required

right 40               # hold right for 40 ticks (60 ticks = 1 second)
right+jump 1           # combine keys with '+'
wait 30                # hold nothing
assert x > 384         # check the player's state at this point in the tape
assert !grounded
```

Keys are `left`, `right`, `down`, `jump`, and `wait`/`none`. The count defaults
to 1. `#` starts a comment.

**Jump is edge-triggered.** `jump 10` is one jump held for ten ticks — exactly
what holding the key does — not ten jumps. Releasing and pressing again is what
produces a second jump.

Assertions read `assert <flag>`, `assert !<flag>`, or `assert <field> <op>
<value>` with `< <= > >= == !=`. Available names:

- numeric: `x`, `y`, `vx`, `vy`, `tick`, `air_jumps`, `frame`
- boolean: `grounded`, `facing_right`, `wall_sliding`, `double_jumping`,
  `crouching`, `dead`, `on_one_way_only`

## Fixture maps

Movement tapes run against `testbed*.ron`, not against real levels. Real levels
get redesigned; a tape that encodes castle geometry would break every time the
level changed, for no good reason. The fixtures are frozen — add to them, but
do not move what is already there.

| Map | For |
| --- | --- |
| `testbed.ron` | running, jumping, gap clearing, spike death |
| `testbed_platform.ron` | one-way platforms and drop-through |
| `testbed_chimney.ron` | wall slide and wall jump |

`castle_spawn.tape` is the exception: it runs on the real map but asserts only
that the player spawns on solid ground and is not standing in a hazard, so
redesigning the castle cannot break it.
