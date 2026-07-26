map maps/testbed_knight.ron

# The knight patrols its ledge and stays on it, with the player standing still
# at the far left doing nothing. Its route is decided entirely by geometry:
# a wall at the right end, a drop at the left.
#
#   ledge    row 11, cols 2-15   world x 64..512, top y=352
#   wall     rows 9-10, col 15   world x 480, the right-hand stop
#   knight   spawns col 8        world x 261, walking right
#
# NPCs are addressed as <kind>.<index> in spawn order, so `knight.0` is the
# only knight on this map.

# at rest on tick 0, before anything has moved
assert knight.0.x == 261
assert knight.0.dir == 1

wait 1
assert knight.0.grounded

# ...walking right, into the wall at x=480. The knight is 22 wide, so it can
# get no further than 458.
wait 240
assert knight.0.grounded
assert knight.0.x <= 458
assert knight.0.dir == -1
assert !knight.0.facing_right

# ...and back left, to the drop at x=64.
wait 420
assert knight.0.grounded
assert knight.0.x >= 63
assert knight.0.dir == 1

# Never fell off, never stopped walking, and never touched the player: the
# knight has no idea the player exists yet.
assert grounded
assert x == 70
expect no died
