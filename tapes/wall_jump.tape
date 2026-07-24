map maps/testbed_chimney.ron

# Climb the 96px shaft by alternating wall jumps. Pressing into a wall while
# falling starts a slide that caps fall speed; jumping out of a slide kicks
# up and away from the wall.

wait 5
assert grounded
assert y == 414

# full jump, drifting right into the wall; the slide begins on the way down
right+jump 40
assert !grounded
assert wall_sliding
assert vy <= 70

# release so the next press is a fresh edge, then kick off the right wall
right 3
right+jump 1
assert !wall_sliding
assert vx < 0
assert vy < 0

# cross the shaft and slide down the left wall
left 40
assert wall_sliding

left 3
left+jump 1
assert vx > 0
assert vy < 0

# the climb ended higher than the floor it started from
right 20
assert y < 414
