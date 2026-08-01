map maps/testbed_mover.ron

# The M7 exit criterion: ride a moving platform across a gap and arrive.
#
# The pit is spiked, so `expect no died` is the whole crossing in one line —
# a platform that failed to carry the player would drop them into it. And the
# player never jumps and never leaves the ground: `expect no jumped` plus
# `expect landed == 1` (the one on tick 1, settling onto the spawn ledge) say
# the crossing was a ride and not a leap, and that contact never flickered off
# for even a single tick while the ground was moving.

wait 5
assert grounded
assert x == 70
assert y == 222

# Walk to the edge of the left ledge and wait there. The platform is out over
# the pit and coming back; it docks against the left gangway on tick 60.
right 20
wait 30
assert grounded
assert y == 222
assert x < 160

# Aboard: over the plank gangway (x 160..192) and onto the platform, which
# docks flush with the plank's far end. `x > 192` is past the gangway
# entirely — on the ferry itself, not on the map.
right 26
assert grounded
assert y == 222
assert x > 192

# Ride, with no input at all: everything moving the player is under their feet.
# The pit runs x 160..608 and is spiked, so being grounded at y == 222 out here
# at all is the ride working. The platform's far dock is x 480 and this ends
# fifteen ticks short of it, hence 460.
wait 204
assert grounded
assert y == 222
assert x > 460
# Carried by position, not by velocity — a velocity-based carry would show up
# right here as the platform's speed sitting in the player's own vx, and would
# have thrown them off the far end when it reversed.
assert vx == 0
expect no died
expect no jumped

# Step off onto the far ledge.
right 60
assert grounded
assert y == 222
assert x > 620
expect no died
expect no jumped
expect landed == 1
