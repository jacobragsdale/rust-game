map maps/testbed_fire.ron

# Fire is a hazard that comes and goes, so both halves of that have to be
# checked: crossing one that is out has to be survivable, and standing in one
# that lights has to kill. A fire that was simply always lethal passes half of
# this tape, and a fire whose collider never reached the geometry passes the
# other half.
#
# Fire A (the grid `F`, default cycle) is lit for ticks 0-59 of every 120.
# Fire B (authored, phase 60) is its exact opposite. The two alternate, which
# is what `phase` is for.

wait 5
assert !dead
expect no died

# --- across fire A while it is out ---
# The run reaches A's box around tick 89 and is clear of it by 100, inside the
# gap that opens at 60 and closes at 120. Walking through fire is only safe
# because the collider is not there.
right 100
assert x > 378
assert !dead
expect no died

# --- on into the dead end, which is fire B's cell ---
# Running right parks the player inside B against the wall. B is out when they
# arrive, and standing in an unlit fire is standing on an empty tile.
right 170
assert x > 900
assert !dead
assert grounded
expect no died

# --- and then it lights, with the player still standing in it ---
wait 40
assert dead
expect died == 1

# after the death freeze: back at the spawn point, which no fire reaches
wait 40
assert !dead
assert x == 70
assert y == 318
expect respawned
expect died == 1
