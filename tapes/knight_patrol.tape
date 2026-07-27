map maps/testbed_knight.ron

# The knight patrols its ledge and stays on it, with the player standing still
# on the shelf below. Its route is decided entirely by geometry: a wall at the
# right end, a drop at the left.
#
# The player is two tiles below the ledge, outside the knight's sight box, so
# this stays a test of patrolling rather than of aggression — see
# knight_kill.tape and knight_fights.tape for the fight.
#
#   ledge   row 10, cols 2-15   world x 64..512, top y=320
#   wall    rows 8-9, col 15    world x 480, the right-hand stop
#   knight  spawns x=261, walking right
#   player  spawns x=38 on the floor below, at y=382

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

# It never noticed the player, never swung, and never fell off. The player is
# in plain view horizontally — it is the two-tile drop that keeps them out of
# the sight box.
assert grounded
assert x == 38
assert hp == 5
expect no attacked
expect no damaged
expect no died
