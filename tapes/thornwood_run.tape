map maps/thornwood.ron

# The Thornwood, end to end. The claim is that Act II's first map can be
# finished from a fresh start — five health, no gear — and that everything in
# it works: a pack of wolves, the ravine crossed log to log under an archer,
# the hermit's errand, the grove hidden inside the hill, the camp and its
# armoury, a weapon worn with a style of its own, Gorran Redhand behind his
# shield, the palisade that opens on his death, and the arch up to the Beacon.
#
# Every fight was recorded with `sim --fight <npc>` — the brain a rival
# champion fights with, put at the player's controls — and says so where it
# starts and ends. When balance moves, cut the tape at the fight's first line
# and record it again. Everything else is written, and asserted as outcomes.

# --- the west road ---------------------------------------------------------------
# Arriving by the Ashford gate, with two wolves penned just ahead.
wait 1
assert grounded
assert x == 134
assert y == 286
# --- wolf.0: recorded with `sim --fight wolf.0` ---
right 54
right+jump 1
right 62
right+cast 1
right 9
right+jump 1
right 1
right+jump 1
right 37
right+cast 1
right 8
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 13
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
# --- end of the recording ---
wait 30
assert wolf.0.dead
assert wolf.1.dead
expect no player.died

# --- the ravine ------------------------------------------------------------
# Up onto the rock ledge, then log to log: three running jumps of three tiles
# each, and a shorter one onto the far bank. Every landing is asserted: a
# missed log is the thorns.
right 40
right+jump 14
right 16
right 8
right+jump 24
right 18
assert grounded
assert y == 286
assert x > 864
right 4
right+jump 24
right 20
assert grounded
assert x > 1056
right 8
right+jump 24
right 22
assert grounded
assert x > 1248
right 8
right+jump 24
right 22
assert grounded
assert x > 1408
expect no player.died
# --- archer.0: recorded with `sim --fight archer.0` ---
left 1
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
left+cast 1
left 15
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
# --- end of the recording ---
# --- brigand.0: recorded with `sim --fight brigand.0` ---
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 20
right+cast 1
right 10
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 27
left 24
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 6
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 17
attack 1
wait 1
attack 1
wait 1
attack 1
# --- end of the recording ---
assert archer.0.dead
assert brigand.0.dead
expect no player.died

# --- the hermit ----------------------------------------------------------------
# Up the step into his clearing — the checkpoint there sees you back to full —
# and on until he is in reach.
wait 20
right 40
right+jump 12
right 60
assert prompt == talk
expect checkpoint == 1
assert hp == 5
# He has four things to say to a stranger: who he is, what the lights are, the
# lesson, and goodbye. The lesson is the third, and it has a price.
interact 1
assert mode == dialogue
assert dialogue_node == greet
assert dialogue_choices == 4
wait 2
down 1
wait 1
down 1
wait 1
confirm 1
assert dialogue_node == teach
assert quest.hermit.stage == 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
assert mode == playing
expect hermit.dialogue_opened == 1

# --- the hidden grove ----------------------------------------------------------
# Up the step, and straight on into the hill's face: at this height it is false
# wall all through, and the grove is inside. The sign first, then the chest.
right 30
right+jump 12
right 70
assert x > 2176
assert grounded
assert prompt == read
right 16
assert prompt == open
interact 1
expect chest_opened == 1
assert item.wildheart_charm.count == 1
assert world.thornwood.secret_grove == 1

# --- over the hill -----------------------------------------------------------
# Back out through the hill's face onto the step, and a jump and a double jump
# onto the plateau four tiles above it.
left 60
jump 10
wait 2
right+jump 14
right 16
assert grounded
assert y == 94
expect double_jumped >= 1
right 110
assert grounded
assert y == 286
assert prompt == read
# --- brigand.1: recorded with `sim --fight brigand.1` ---
right+cast 1
right 8
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 22
left 25
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 7
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 11
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
right+cast 1
right 1
right+cast 1
right+jump 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 2
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 15
left 4
right 17
left+cast 1
left 7
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
# --- end of the recording ---
# --- archer.1: recorded with `sim --fight archer.1` ---
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 86
right+cast 1
right 6
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 62
right+jump 1
right 28
right+cast 1
right 8
right+jump 1
right 1
right+jump 1
right 1
# --- end of the recording ---
# --- wolf.2: recorded with `sim --fight wolf.2` ---
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 19
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
right 4
right+cast 1
right 1
right+cast 1
right 7
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 19
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
right 6
right+cast 1
right 1
right+cast 1
right 7
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 19
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
right+cast 1
right 1
right+cast 1
right 7
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 12
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
right 1
# --- end of the recording ---
assert archer.1.dead
assert wolf.2.dead
expect no player.died

# --- the armoury ---------------------------------------------------------------
# Back to the lookout, up onto its log, and the chest the brigands kept there.
wait 20
left 60
left+jump 12
left 4
wait 20
assert grounded
assert y == 222
assert prompt == open
interact 1
assert item.hunting_spear.count == 1

# The spear on, the potion down, and the grove's charm worn — the first time
# anything here has been a weapon with a style of its own.
inventory 1
wait 1
down 1
wait 1
down 1
wait 1
down 1
wait 1
down 1
wait 1
confirm 1
assert item.hunting_spear.equipped
wait 1
confirm 1
expect minor_potion.item_used == 1
assert hp == 4
# The empty stack is gone and the highlight has fallen back onto the pelt,
# two rows below the charm.
wait 1
up 1
wait 1
up 1
wait 1
confirm 1
assert item.wildheart_charm.equipped
inventory 1
assert mode == playing

# --- Gorran Redhand ------------------------------------------------------------
right 60
# --- gorran.0: recorded with `sim --fight gorran.0` ---
right 9
right+jump 1
right 23
left 1
right 4
left 1
attack 1
right 4
left 10
left+jump 1
left 1
left+cast 1
left 17
right 4
right+jump 1
left 4
right 4
left 6
right 3
left 8
right 19
right+jump 1
right 1
right+jump 1
right 3
left 3
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
left 7
right 9
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+jump 1
right 1
right+jump 1
right 22
left 1
attack 1
right 12
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
left 11
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
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+jump 1
left 1
left+jump 1
left 34
right 1
attack 1
left 5
right+jump 1
right 5
right+jump 1
right 1
right+jump 1
right 1
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
left 7
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
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right+jump 1
right 1
right+jump 1
right 22
left 1
attack 1
right 12
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
left 11
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
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+jump 1
left 1
left+jump 1
left 33
right 1
attack 1
left 5
right+jump 1
right 5
right+jump 1
right 1
right+jump 1
right 1
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
left 7
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+jump 1
right 1
right+jump 1
right 22
left 1
attack 1
right 12
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
left 11
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
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 1
left+jump 1
left 1
left+jump 1
left 34
right 1
attack 1
left 5
right+jump 1
right 5
right+jump 1
right 1
right+jump 1
right 1
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
left 17
right 20
left+cast 1
left 9
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
right 3
left 5
right 1
attack 1
left 6
# --- end of the recording ---
assert gorran.0.dead
assert quest.thornwood.gorran == 1
expect no player.died
expect gorran.blocked >= 1

# --- the ridge -----------------------------------------------------------------
# Through the palisade, which opened on his death, and three steps up to the
# arch. It offers the way on rather than taking it: a door, not an exit.
wait 30
right 120
assert x > 3552
right+jump 12
right 20
right+jump 12
right 20
right+jump 12
right 24
assert grounded
assert prompt == enter
interact 1
expect beacon.traveled == 1
assert map == beacon
expect no player.died
