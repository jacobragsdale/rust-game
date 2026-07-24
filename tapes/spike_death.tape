map maps/testbed.ron

# Running into the pit without jumping falls onto the spikes and kills.
# The counterpart to gap_jump.tape: the hazard has to actually be lethal.

wait 5
assert !dead

right 90
assert dead

# after the death freeze, back at the spawn point
wait 40
assert !dead
assert x == 70
assert y == 350
