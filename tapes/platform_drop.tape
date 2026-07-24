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
expect dropped_through == 1
# a drop-through, not a jump. Counts are cumulative, so "nothing new fired" is
# spelled as "still the one jump from getting up here".
expect jumped == 1

wait 40
assert grounded
assert !on_one_way_only
assert y == 382
# three landings: settling onto the spawn floor, the platform, the floor again
expect landed == 3
