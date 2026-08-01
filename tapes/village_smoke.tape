map maps/village.ron

# N-2's smoke test: the hub is safe, the elder is there, and the street runs
# from the spawn point to the gate east without a gap in it.
#
# Everything here is an assertion about the *map*, not about a mechanic — the
# mechanics all have testbeds of their own. What this catches is the class of
# bug a hub can have: a spawn inside geometry, a building that closes the
# street at head height, a villager who has strolled off somewhere by the time
# you arrive, or a gate you cannot actually walk through.
#
#   street       surface y=320, the full width; player stands at y=286
#   doorstep     cols 11-12, a notch out of the street — floor y=352
#   Runa         paces x 358..388 in the notch and cannot leave it
#   counter      x 672..768, surface y=288 — one 32px step
#   gatehouse    x 1056..1152 above y=256; the passage under it is 64px
#   east edge    the player is clamped at x=1260

wait 1
assert grounded
assert x == 70
assert y == 286
assert !dead
assert prompt == none
expect no interact_prompted

# Nothing in this map can hurt anyone: no hazards, no `Hostile`, and the one
# NPC is on the player's team. Re-asserted at the end, which is the real claim
# — a hub is only a hub if the whole walk through it was harmless.
expect no damaged

# East until the street drops out from under you, which is the doorstep. The
# far face of the notch stops the run at x=396, and from there she is in reach
# wherever she happens to be in her beat — that is what the notch is for.
right 110
assert grounded
assert x == 396
assert y == 318
assert prompt == talk
expect elder_intro.interact_prompted == 1

# She is `Patrol` without `Hostile`, so she paces and nothing else. A hundred
# and twenty ticks of standing next to her is a hundred and twenty ticks of
# nobody swinging at anybody.
wait 120
assert prompt == talk
assert hp == 5
expect no damaged
expect no attacked

# Out of the doorstep — one jump, the notch is a single tile deep — and on
# east to the shop counter, which stops the run again at its face.
right+jump 1
right 90
assert grounded
assert y == 286
assert x == 652
assert prompt == none

# Over the counter. The shop is a shop-shaped building and nothing else: there
# is no mechanic behind it to exercise, so all a tape can say about it is that
# it is climbable and does not block the street.
right+jump 1
right 40
assert grounded
assert x > 700

# Under the gatehouse and out onto the east road, which is the way to the
# dungeon. Clamped by the map's own edge, not by a wall.
right 200
assert grounded
assert x > 1200
assert !dead
expect no died
expect no damaged
