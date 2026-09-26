map maps/ark_hold.ron

# The Holding: the Ark's first deck. The claim: the Vael speak as you land, the
# Archivist gives the key the story needs, the lower hold — where the Choir's
# husks and eyes have got in — can be fought through, its secret is where the
# map says, and the star-sealed hatch opens onto the Proving Circle.
#
# The fights were recorded with `sim --fight <npc>` and say so.

# --- arrival -----------------------------------------------------------------------
wait 1
assert mode == dialogue
expect ark_arrival.dialogue_opened == 1
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
assert mode == playing
assert grounded

# --- the upper deck ----------------------------------------------------------------
# East past the pods of sleepers to Ysolde, taken like you, who knows how the
# Circle works.
wait 20
right 200
assert prompt == talk
interact 1
expect ysolde.dialogue_opened == 1
assert dialogue_choices == 4
wait 2
down 1
wait 1
down 1
wait 1
confirm 1
assert dialogue_node == circle
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
assert dialogue_node == greet
wait 2
down 1
wait 1
down 1
wait 1
down 1
wait 1
confirm 1
assert mode == playing

# The Archivist: why the Vael took anyone, and — the reply the story turns on —
# the Star Key, for saying you will fight the thing in the Heart.
right 70
assert prompt == talk
interact 1
expect archivist.dialogue_opened == 1
assert dialogue_choices == 5
wait 2
confirm 1
assert dialogue_node == why
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
assert dialogue_node == greet
wait 2
down 1
wait 1
down 1
wait 1
down 1
wait 1
confirm 1
assert dialogue_node == key
assert item.star_key.count == 1
assert quest.ark.stage == 1
wait 2
confirm 1
wait 2
confirm 1
assert mode == playing

# --- the lower hold ----------------------------------------------------------------
# Down through the grate, and down each rung under it, to the floor.
right 40
down 60
assert grounded
assert y == 414
expect dropped_through >= 3
# --- husk.1: recorded with `sim --fight husk.1` ---
right 17
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
right 32
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
right 26
attack 1
wait 1
left 8
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
right 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
# --- end of the recording ---
# --- husk.0: recorded with `sim --fight husk.0` ---
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
left 39
left+cast 1
left 8
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 31
attack 1
wait 1
attack 1
right 7
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
left 1
left+jump 1
left 1
# --- end of the recording ---
# --- husk.2: recorded with `sim --fight husk.2` ---
right 8
left 14
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 1
right+jump 1
right 8
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 3
right+jump 1
right 1
right+jump 1
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
right+cast 1
right 2
right+jump 1
right 1
right+jump 1
right 1
attack 1
wait 1
attack 1
wait 1
left 2
# --- end of the recording ---
# --- drone.0: recorded with `sim --fight drone.0` ---
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
left 1
left+jump 23
left 89
left+jump 11
left+jump+cast 1
left+jump 11
left 6
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
left+jump 4
jump+attack 1
jump 1
jump+attack 1
left+jump 9
down+jump+attack 1
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
left 1
# --- end of the recording ---
# --- drone.1: recorded with `sim --fight drone.1` ---
right 11
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
right 136
right+jump 11
right+jump+cast 1
right+jump 11
right 7
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
right+jump 4
jump+attack 1
jump 1
jump+attack 1
jump 1
right+jump 10
down+jump+attack 1
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
assert husk.0.dead
assert husk.1.dead
assert husk.2.dead
assert drone.0.dead
assert drone.1.dead
expect no player.died

# The hold's chest, by the grate.
wait 20
left 30
assert prompt == open
interact 1
assert item.greater_potion.count == 1

# --- the crawlspace --------------------------------------------------------------
# East to the reactor housing, and straight into its face: at floor height it
# is false wall, and the crawlspace inside holds the circlet.
right 130
assert x > 1664
assert prompt == open
interact 1
assert item.lantern_circlet.count == 1
assert world.ark_hold.secret_crawlspace == 1
# Back out, and over the housing: a jump and a double jump.
left 36
jump 12
wait 2
right+jump 14
right 20
assert grounded
assert y == 318
# --- sentinel.0: recorded with `sim --fight sentinel.0` ---
right 34
right+jump 9
down+jump+attack 1
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
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
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
right 6
right+jump 16
left+jump 7
left 9
right 12
right+jump 4
left+jump 2
down+jump+attack 1
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
right 12
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right+jump 1
right 1
right+jump 11
down+jump+attack 1
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
down 1
right 5
left 3
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
right+jump 1
left 15
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
right+jump 18
down+jump+attack 1
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
down 1
down+attack 1
down 1
# --- end of the recording ---
assert sentinel.0.dead

# --- the star-sealed hatch -------------------------------------------------------
# The Archivist's key opens it, and is kept: the Ark's inner doors all answer
# to it.
wait 20
assert prompt == unlock
interact 1
expect star_key.unlocked == 1
expect circle.traveled == 1
assert map == circle
assert item.star_key.count == 1
expect no player.died
