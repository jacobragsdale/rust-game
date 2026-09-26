map maps/circle.ron

# The Proving Circle: three duels against champions with the player's own kit
# — the same controller, the same combo engine, the same jump — flown by a
# brain instead of a keyboard. The claim: each pit's gate opens on its
# champion's fall, each champion leaves its style behind, the Arbiter rewards
# the Circle's champion, and the hatch at the far end goes down into the Engine
# Deep.
#
# The Nameless, the Circle's secret, has a tape of its own:
# `circle_secret.tape`.
#
# The duels were recorded with `sim --fight <champion>`: the player flown by
# the same brain the champions are, against them. They say so where they start
# and end; re-record them when balance moves.

wait 1
assert grounded
# --- kesh.0: recorded with `sim --fight kesh.0` ---
right 115
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
right 24
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
right 1
right+jump 1
right 1
right+jump 1
right 1
right+cast 1
right 12
left 1
right 7
left 1
attack 1
wait 1
attack 1
wait 1
left 11
right 1
attack 1
wait 1
left 12
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
right+cast 1
right 2
attack 1
wait 1
left 15
right 11
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
right 1
right+jump 1
right 1
right+jump 1
right 1
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
left 14
attack 1
wait 1
attack 1
wait 1
left 7
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
right+jump 16
jump+attack 1
left+jump 4
# --- end of the recording ---
assert kesh.0.dead
assert quest.circle.kesh == 1
expect no player.died
expect saber_1.attacked >= 1

# Through Kesh's gate, which opened on her fall, and whole again at the
# brazier inside it.
wait 20
right 140
assert x > 928
assert hp == 5
expect checkpoint == 2

# Kesh dropped her saber on the way through, and it went into the bag: her
# style — three quick cuts, less of you committed to each — is the one to meet
# a slow, heavy blow with.
assert item.glass_saber.count == 1
inventory 1
wait 1
confirm 1
assert item.glass_saber.equipped
inventory 1
assert mode == playing
wait 2
# --- brakka.0: recorded with `sim --fight brakka.0` ---
right 35
right+cast 1
right 38
wait 3
left 17
right 8
right+jump 23
right 3
left 14
attack 1
right 2
left 9
right 9
left 1
attack 1
right 39
left 13
left+cast 1
left 1
left+cast 1
left 1
left+cast 1
left 59
wait 5
right 18
left 8
left+jump 23
left 3
right 14
attack 1
left 4
right 5
left 11
right 1
attack 1
left 38
right 14
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 62
wait 3
left 19
right 8
right+jump 23
right 3
left 14
attack 1
right 2
left 9
right 9
left 1
attack 1
right 3
# --- end of the recording ---
assert brakka.0.dead
assert quest.circle.brakka == 1
expect no player.died
expect maul_1.attacked >= 1

# Through Brakka's gate to the last pit, whole again at its brazier.
wait 20
right 120
assert x > 1568
expect checkpoint == 3
# --- morwen.0: recorded with `sim --fight morwen.0` ---
right 17
right+cast 1
right 18
right+jump 18
jump+attack 1
jump 1
jump+attack 1
jump 1
jump+attack 1
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
right 1
right+jump 1
right 1
right+jump 1
right 1
right+cast 1
right 13
left 6
right 2
left 4
attack 1
wait 1
attack 1
wait 1
right 4
left 4
right 8
left 4
right+cast 1
right 33
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
right 1
right+jump 15
down+jump+attack 1
down 1
right 2
attack 1
left 11
left+jump 1
left 1
left+jump 1
left 1
left+jump 1
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
right 2
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 1
right+cast 1
right 15
right+jump 1
right 1
right+jump 25
jump+attack 1
left+jump 1
left 8
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
right 11
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
right 1
right+jump 1
jump+attack 1
jump 1
jump+attack 1
jump 1
right+jump 18
left 7
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
down+attack 1
down 1
right 3
# --- end of the recording ---
assert morwen.0.dead
assert quest.circle.morwen == 1
expect no player.died
expect hex.spell_cast >= 1

# --- the Arbiter -----------------------------------------------------------------
# Through the last gate — over Morwen's grimoire, which she dropped where she
# fell — and whole again at the exit hall's brazier. The Arbiter has a reply
# now that was not there before: all three are down.
wait 20
right 100
assert item.tome_seeking.count == 1
assert prompt == talk
interact 1
expect arbiter.dialogue_opened == 1
assert dialogue_choices == 3
wait 2
down 1
wait 1
confirm 1
assert dialogue_node == champion
assert item.circle_blade.count == 1
assert quest.circle.stage == 1
wait 2
confirm 1
wait 2
confirm 1
wait 2
confirm 1
assert mode == playing

# Every champion left its style on the sand, and the tape has walked over all
# three. The Circle's blade in hand, and Morwen's grimoire where the spark sits.
# The bag holds the maul, the grimoire and the blade, in the order they came.
assert item.glass_saber.equipped
assert item.brakka_maul.count == 1
inventory 1
wait 1
down 1
wait 1
down 1
wait 1
confirm 1
assert item.circle_blade.equipped
assert item.glass_saber.count == 1
wait 1
up 1
wait 1
confirm 1
assert item.tome_seeking.equipped
inventory 1
assert mode == playing


# --- down ------------------------------------------------------------------------
# The hatch at the end of the hall, down into the Engine Deep.
right 40
assert prompt == enter
interact 1
expect engine_deep.traveled == 1
assert map == engine_deep
expect no player.died
