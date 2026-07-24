map maps/testbed_platform.ron

# The other half of the drop-through rule: on solid ground, down crouches
# rather than dropping. Without this, making down drop through platforms
# would quietly delete crouching everywhere.

wait 5
assert grounded
assert !on_one_way_only

down 20
assert grounded
assert !dead
assert y == 382

# and down+jump on solid ground is still an ordinary jump
down 5
wait 5
