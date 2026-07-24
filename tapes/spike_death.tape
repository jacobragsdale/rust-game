map maps/testbed.ron

# Running into the pit without jumping falls onto the spikes and kills.
# The counterpart to gap_jump.tape: the hazard has to actually be lethal.

wait 5
assert !dead
expect no died

right 90
assert dead
# not just "dead" -- dead *of the spikes*. Walking off the map kills too, and
# the `dead` flag alone cannot tell those apart.
expect died

# after the death freeze, back at the spawn point
wait 40
assert !dead
assert x == 70
assert y == 350
expect respawned
# one death, one respawn: the respawn point must not land back in the hazard
expect died == 1
