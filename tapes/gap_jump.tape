map maps/testbed.ron

# Clear the 128px spike pit at x 256..384 with a running jump, and survive.
# The jump fires at tick 58, just as the player's right edge reaches the
# ledge — a full-speed jump carries ~148px, so an early launch falls short.

wait 5
right 53
assert grounded
assert vx == 200

right+jump 25
assert !grounded
assert x > 300

right+jump 20
right 25
assert grounded
assert !dead
assert x > 384
