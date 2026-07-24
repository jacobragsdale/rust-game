map maps/testbed.ron

# One mid-air jump is available, refills on landing, and a third press in
# the same airtime does nothing.

wait 5
assert grounded
assert air_jumps == 1

jump 10
wait 10
assert !grounded
assert air_jumps == 1

jump 10
assert double_jumping
assert air_jumps == 0
expect jumped == 1
expect double_jumped == 1

# a third press mid-air must not launch again
wait 10
jump 10
assert air_jumps == 0
# the flag-based check above can only say the counter did not move; this says
# no launch of any kind fired, which is the actual claim
expect double_jumped == 1
expect jumped == 1

wait 100
assert grounded
assert air_jumps == 1
expect landed
