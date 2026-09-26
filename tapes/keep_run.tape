map maps/keep.ron

# The Barracks: where a new game starts. The claim: the prologue opens on the
# first tick by itself — a trigger on the spawn — the footlocker holds the one
# thing the garrison left behind, and the door goes into the castle at its
# `keep` spawn. None of it happens twice: the trigger's flag survives a save
# and a load along with the rest.

# --- the prologue -----------------------------------------------------------------
wait 1
assert mode == dialogue
expect prologue.dialogue_opened == 1
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
assert mode == playing
assert world.keep.wake == 1

# --- the footlocker ---------------------------------------------------------------
wait 20
right 20
assert prompt == open
interact 1
assert item.minor_potion.count == 1
assert world.keep.footlocker == 1

# --- out --------------------------------------------------------------------------
wait 5
right 100
assert prompt == enter
interact 1
expect castle.traveled == 1
assert map == castle
reload
assert map == castle
assert item.minor_potion.count == 1
assert world.keep.wake == 1
wait 30
assert mode == playing
expect prologue.dialogue_opened == 1
