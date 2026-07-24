map maps/testbed.ron

# A held jump leaves the ground, rises, and lands again.

wait 5
assert grounded

jump 20
assert !grounded
assert vy < 0

wait 90
assert grounded
assert vy == 0
