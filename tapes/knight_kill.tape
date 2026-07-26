map maps/testbed_arena.ron

# Kill the knight. Three hits at one damage each, spaced out by i-frames.
#
# The player sweeps the little room, swinging as they go, rather than trying to
# hit on named ticks. Exact hit timing depends on where knockback throws the
# knight and which way it was patrolling when it landed, and pinning that in a
# tape would encode the knockback impulse, the walk speed and the i-frame
# window all at once — so the tape asserts the *outcome* and the golden trace
# holds the tick-by-tick detail. If the timing shifts, traces/knight_kill.jsonl
# says so; if the fight stops being winnable, this does.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70, 5 hp
#   knight  spawns x=229, 3 hp

assert hp == 5
assert knight.0.hp == 3
assert !knight.0.dead
expect no attacked
expect no damaged

# eight sweeps of the room, swinging at each end
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26

# by here the first blow has landed, and the knight is not dead of it
assert knight.0.hp == 2
expect damaged == 1

left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26
right 18
attack 1
wait 26
left 18
attack 1
wait 26

# three hits, one kill
assert knight.0.dead
assert knight.0.hp <= 0
expect damaged == 3
expect died == 1

# The corpse stays where it fell. Despawning would renumber every knight after
# it and silently repoint every later `knight.<n>` assertion.
wait 60
assert knight.0.dead
assert knight.0.grounded
assert knight.0.vx == 0

# The player never took a scratch: this knight cannot fight back until M3b.
assert hp == 5
assert iframes == 0
