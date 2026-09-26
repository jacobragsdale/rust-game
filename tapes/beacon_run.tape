map maps/beacon.ron

# The Beacon, bottom to top. The claim: the tower is climbable from a fresh
# start — ledge to ledge, a shield fought and a shaft wall-jumped — its secret
# is where the map says, and the light at the top takes you to the Ark, which
# speaks as you land.
#
# The fights were recorded with `sim --fight <npc>` and say so; everything
# else is written, every landing asserted.

# At the foot of the tower, by the arch back down to the wood.
wait 1
assert grounded
assert x == 166
assert y == 1054

# --- ledge by ledge ------------------------------------------------------------
# Onto the step, up to ledge 1 over the thrall's head — two tiles above it and
# far outside the 36px it can see — then back left across the plank to ledge 2.
right 55
right+jump 12
right 14
right+jump 16
right 16
assert grounded
assert y == 926
wait 12
left+jump 20
left 16
wait 10
assert grounded
assert y == 862
left 24
left+jump 16
left 16
wait 6
assert grounded
assert y == 798
# --- bat.0: recorded with `sim --fight bat.0` ---
left+jump 1
left 1
left+cast 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
# --- end of the recording ---
assert bat.0.dead

# The hauberk at the back of ledge 2.
wait 10
left 40
assert prompt == open
interact 1
assert item.thrall_mail.count == 1

# Across the second plank to ledge 3, where a thrall stands behind its shield.
right 48
right+jump 16
right 10
wait 8
assert grounded
assert y == 734
right 20
right+jump 16
right 10
wait 8
assert grounded
assert y == 670
# --- shieldbearer.0: recorded with `sim --fight shieldbearer.0` ---
right 24
right+jump 1
right 15
left 2
right 3
left 1
attack 1
right 15
left 1
left+jump 1
left 1
left+jump 1
left 16
right 7
right+jump 1
right 6
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
right 7
left 11
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+jump 1
left 4
left+jump 1
left 9
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
right 7
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left+jump 1
left 6
left+jump 1
left 15
right 2
left 3
right 1
attack 1
left 4
# --- end of the recording ---
assert shieldbearer.0.dead
expect no player.damaged

# --- the shaft -------------------------------------------------------------------
# In under the inner wall, and up between it and the tower's own: jump into the
# right wall, then kick off each wall in turn — the kick is pressed toward the
# wall you are on, and throws you off it — two tiles a pair of kicks, four
# pairs to clear the top, and a step left onto ledge 4.
wait 10
right 80
wait 6
right+jump 16
right 3
right+jump 1
left 12
left+jump 1
right 12
right+jump 1
left 12
left+jump 1
right 12
right+jump 1
left 12
left+jump 1
right 12
right+jump 1
left 12
left+jump 1
right 12
right+jump 1
left 20
wait 10
assert grounded
assert y == 350
expect wall_jumped >= 8
# --- knight.1: recorded with `sim --fight knight.1` ---
right 10
left+cast 1
left 7
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 17
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
right 18
left+cast 1
left 7
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
# --- end of the recording ---
assert knight.1.dead
expect no player.died

# --- the top ---------------------------------------------------------------------
# Past the checkpoint to the ledge's west end, up the two stacked planks, and
# — level with the upper one — into the wall on the left, which is the
# watchman's cubby.
wait 10
left 100
left+jump 16
left 6
wait 8
assert grounded
assert y == 286
expect checkpoint == 1
jump 14
wait 16
assert grounded
assert y == 222
left 40
assert prompt == open
interact 1
assert item.elixir.count == 2
assert world.beacon.secret_cubby == 1

# Back out onto the upper plank and up onto the top: the light speaks as you
# come over the edge (a trigger, told once), and the door in it is the way up.
right 36
right+jump 16
right 10
wait 8
assert mode == dialogue
expect beacon_top.dialogue_opened == 1
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 20
assert mode == playing
assert grounded
assert y == 158
assert world.beacon.light == 1
right 60
assert prompt == enter
interact 1
expect ark_hold.traveled == 1
assert map == ark_hold
# ...and the Ark speaks as you land, on the first tick you are there.
wait 1
assert mode == dialogue
expect ark_arrival.dialogue_opened == 1
expect no player.died
