map maps/circle.ron

# The Proving Circle's secret. The claim: from the entrance, the rungs climb to
# the gallery over the first pit, a warning is scratched at its end, the wall
# there is false, and the thing behind it — the Nameless, a champion the Choir
# hollowed, with your own kit and reactions no person has — wakes and comes for
# you. Beating it is not claimed: it is the hardest fight in the game, and a
# fresh start with five health and bare hands is not meant to.
#
# Nothing on the way wakes Kesh, below in the first pit: the gallery is too
# far above her.

wait 1
left 10
jump 14
wait 16
jump 14
wait 16
jump 14
wait 16
right+jump 16
right 10
wait 6
assert grounded
assert y == 190
assert !kesh.0.dead

# East along the gallery to its end, and the warning.
right 190
assert mode == dialogue
expect nameless_warning.dialogue_opened == 1
confirm 1
wait 2
confirm 1
wait 2
assert mode == playing
assert world.circle.warning == 1

# The wall at the end of the gallery is false: straight through it, into the
# cell. Four tiles in, the Nameless wakes — and meets you in the middle.
right 70
assert x > 960
right 16
assert nameless.0.x == 1222
wait 30
expect shock.spell_cast >= 1
expect player.damaged >= 1
assert nameless.0.x < 1222
assert !nameless.0.dead
