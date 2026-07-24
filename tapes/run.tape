map maps/testbed.ron

# Ground movement: accelerate to top speed, turn around, decelerate to rest.

wait 5
assert grounded
assert vx == 0

right 40
assert grounded
assert facing_right
assert vx == 200

left 60
assert !facing_right
assert vx == -200

wait 20
assert grounded
assert vx == 0
