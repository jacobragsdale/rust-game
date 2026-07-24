map maps/testbed_platform.ron

# One-way platforms: rise through from below, land on top, then drop back
# down by pressing down alone -- no jump needed.

wait 5
assert grounded
assert y == 382

# jump up through the platform and land on it
jump 20
wait 40
assert grounded
assert on_one_way_only
assert y == 318

# down alone drops through
down 2
assert !grounded

wait 40
assert grounded
assert !on_one_way_only
assert y == 382
