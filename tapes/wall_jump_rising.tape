map maps/testbed_chimney.ron

# Wall jump on the way UP, and just after letting go of the wall.
#
# The wall jump used to require an active wall slide, which meant airborne
# *and* falling *and* still holding into the wall. Both presses below happen
# outside that state, and both used to do nothing (or waste the double jump).
# Companion to wall_jump.tape, which covers the slide path.

wait 5
assert grounded
assert y == 414

# jump and steer into the right wall; contact happens while still rising
right+jump 18
assert !grounded
assert vy < 0
assert !wall_sliding

# release for one tick so the next press is a fresh edge, then kick off a
# wall the player is rising against, not sliding down
right 1
assert !wall_sliding
right+jump 1
assert vx < 0
assert vy < 0
# vx/vy only say "went up and left" -- which a double jump plus leftward drift
# also produces. This is the assertion the tape was really trying to make.
expect wall_jumped == 1
expect no double_jumped
