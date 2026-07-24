map maps/castle.ron

# Regression for the reported bug: the one-way platform above the first spike
# pit (x 256..384, top y=288) could not be landed on, because a solid block
# sat one tile above it and the player's head hit that block before their feet
# cleared the platform. The player is 34px tall; one empty row is 32px.
#
# This asserts on real castle geometry, so redesigning that area will break it
# on purpose -- the platform being landable is the point of that area.

wait 5
right 42

# jump across the pit, releasing right at the apex so the landing is not
# carried off the platform's far edge by momentum
right+jump 25
assert y < 254

wait 20
assert grounded
assert on_one_way_only
assert y == 254
assert !dead

# a stable perch, not a one-tick graze
wait 20
assert grounded
assert y == 254

# and down alone drops off it, into the pit the platform spans
down 2
assert !grounded
