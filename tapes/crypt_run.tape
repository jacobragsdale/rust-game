# The crypt, end to end: the enemies that are only data — bats that fly and
# bite, a mage that throws embers, the Warden that does both — the level's
# furniture, a save in the middle of it, the long stair up into the village,
# and a shop that is nothing but a dialogue file.
#
# Written by probing: every timing below was found by running the sim and
# looking, not by arithmetic. The Warden fight in the middle was *recorded* —
# see the note there — and is the part to re-record when balance changes.
map maps/crypt.ron

# --- the stair down: the sign and the first brazier -------------------------
wait 1
assert map == crypt
assert grounded
assert bat.0.hp == 2
assert warden.0.hp == 12
right 88
expect checkpoint == 1
expect sign_crypt.interact_prompted == 1

# --- the cavern --------------------------------------------------------------
# Bat 0 hangs over the cavern floor until the player is in sight, then comes
# straight in. Two cuts kill it; the second is a combo link, not a new swing.
wait 54
attack 1
wait 10
attack 1
wait 40
assert bat.0.dead
expect bat.died == 1
expect no player.damaged

# Up the planks: a jump to the low one, then a jump and a double jump to the
# high one, where the chest is. Bat 2 hangs over it and comes to meet you.
right+jump 16
right 20
assert on_one_way_only
right 6
right+jump 12
right 3
right+jump 14
right 20
expect double_jumped == 1
assert on_one_way_only
wait 45
attack 1
wait 10
attack 1
wait 40
assert bat.2.dead
interact 1
expect chest_opened == 1
assert world.crypt.cavern_chest == 1
assert item.coin.count == 6
assert item.minor_potion.count == 1
wait 2

# Bat 1 waits past the spike pit and sees you from the end of the plank.
right 26
wait 64
attack 1
wait 10
attack 1
wait 40
assert bat.1.dead
expect bat.died == 3
expect no player.damaged

# --- the ledge -----------------------------------------------------------------
# Off the plank, a jump and a double jump onto the ledge behind the mage,
# which is walking away and has not seen you. Two shocks kill it (4 hp, 2 a
# bolt); it turns on the first and gets one ember off, which lands.
right 30
wait 20
right+jump 12
right 4
wait 8
right+jump 12
right 20
wait 10
cast 1
wait 56
cast 1
wait 40
assert mage.0.dead
expect shock.spell_cast == 2
expect ember.spell_cast >= 1
expect player.damaged == 1

# The ember knocked you back down onto the step. Up again, over the mage's
# coins, and to the lever.
wait 32
right+jump 12
right 44
expect coin.picked_up == 2
right 14
wait 6
assert prompt == pull
interact 1
expect lever_pulled == 1
assert quest.crypt.gate == 1

# A save and a load in the middle of the dungeon: the lever and the chest stay
# done, because they are flags. The bats and the mage are back — a save holds
# no entities, and only a kind with a death flag stays dead — but the way
# forward is ahead of you, not behind.
reload
assert quest.crypt.gate == 1
assert world.crypt.cavern_chest == 1
assert item.coin.count == 9
assert !bat.0.dead
right 30
wait 40
expect checkpoint == 1

# --- the Warden --------------------------------------------------------------
# Recorded, not written: the lines from here to `end of the recording` are
# what `cargo run --bin sim -- --tape <this file cut here> --fight warden.0`
# printed. When a balance change breaks the fight, cut the tape at this line,
# record it again, and paste the new lines in.
right 106
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
wait 1
attack 1
right 18
left 14
wait 12
right 15
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
right 10
left 14
right 25
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
wait 1
attack 1
left 18
wait 8
right 16
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
left 10
# end of the recording
assert warden.0.dead
assert quest.crypt.warden == 1
assert !dead
expect no player.died

# The Warden's death is its gate's flag: the way to its chest is open.
wait 30
right 22
wait 4
assert prompt == open
interact 1
assert item.warden_seal.count == 1
assert world.crypt.warden_chest == 1
wait 2

# --- the long stair up, and the pedlar -------------------------------------
right 18
wait 4
assert prompt == enter
interact 1
expect village.traveled == 1
assert map == village
wait 30
right 40
wait 4
assert prompt == trade
interact 1
assert mode == dialogue
assert dialogue_node == greet
# Twenty-nine coins and the seal: both prices, the seal and the rumour.
assert dialogue_choices == 5
wait 2
down 1
wait 1
down 1
wait 1
confirm 1
assert dialogue_node == seal
assert item.warden_seal.count == 0
assert item.coin.count == 49
assert quest.warden.stage == 2
wait 2
confirm 1
wait 2
confirm 1
assert dialogue_node == greet
wait 2
confirm 1
assert dialogue_node == sold
assert item.coin.count == 46
assert item.minor_potion.count == 4
wait 2
down 1
wait 1
confirm 1
assert mode == playing
expect pedlar.dialogue_opened == 1
expect choice_taken == 4
wait 5
