map maps/testbed_world_b.ron

# Going between maps, both ways, by both kinds of way through: the annex's
# west edge and the hall's east edge are exits you walk into, and the two
# cellar doors are doors you press `interact` at.
#
# What crosses is the player — health, mana, bag — and nothing else: each map
# is rebuilt from its file and its flags on arrival. An exit fires on the way
# *in*, so arriving somewhere does not bounce you straight back out.
#
#   annex  x 0..640 · exit x 0..32 · Spawn "west" x 70 · door x 320..384
#   hall   Spawn "east" x 1158 · exit x 1248..1280 · cellar door x 928..992
#          (locked: the gate at 832..864 keeps the west of the hall out of reach)

wait 1
assert map == testbed_world_b
assert x == 70

# --- west, out of the annex and into the hall --------------------------------
left 30
expect testbed_world.traveled == 1
assert map == testbed_world
# Arrived at 1158 part-way through the press, and still walking west.
assert x > 1100
assert x < 1180

# Standing still after arriving: nothing sends you back.
wait 30
assert map == testbed_world
expect traveled == 1

# --- east, out of the hall and back into the annex -------------------------
right 40
expect testbed_world_b.traveled == 1
assert map == testbed_world_b
assert x < 100

# --- the annex's door, back to the hall's cellar -----------------------------
right 76
assert prompt == enter
interact 1
expect door.interacted == 1
expect testbed_world.traveled == 2
assert map == testbed_world
assert x > 928
assert x < 992

# The cellar door is locked from this side too, and there is no key here.
wait 1
assert prompt == unlock
interact 1
expect iron_key.locked == 1
assert map == testbed_world

# A save taken on the far side of a door comes back on the far side of it.
reload
assert map == testbed_world
assert x > 928
assert x < 992
expect no travel_failed
expect no player.died
