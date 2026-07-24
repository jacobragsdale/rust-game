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

# a third press mid-air must not launch again
wait 10
jump 10
assert air_jumps == 0

wait 100
assert grounded
assert air_jumps == 1
