map maps/castle.ron

# Smoke test for the real map rather than a fixture: the player spawns on
# solid ground and is not standing in a hazard. Deliberately asserts nothing
# about castle geometry beyond that, so redesigning the level cannot break it.

wait 30
assert grounded
assert !dead
