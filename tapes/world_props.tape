map maps/testbed_world.ron

# The level's furniture, met in the order testbed_world.ron lays it out: a
# sign, a key on the floor, a checkpoint, the spikes it is there for, a chest,
# a lever and the gate it opens, and a locked door the key fits.
#
# Every one of these is state the *world* remembers, in a flag, rather than on
# an entity — so the tape also reloads the run halfway and checks that nothing
# forgot anything. The door at the end goes through to the annex; getting back
# is `world_travel.tape`'s.
#
#   spawn x=70 · sign 168 · key 266 · checkpoint 362 · spikes 448..512
#   chest 646 · lever 746 · gate 832..864 · door 928..992

wait 1

# --- a sign is a conversation with nothing to say back ------------------------
right 24
assert prompt == read
interact 1
assert mode == dialogue
assert dialogue_node == read
cancel 1
assert mode == playing
expect sign_proving_hall.dialogue_opened == 1
expect sign_proving_hall.interacted == 1

# --- the key is lying on the floor --------------------------------------------
right 36
assert item.iron_key.count == 1
expect iron_key.picked_up == 1
assert world.testbed_world.item_8_9 == 1

# --- the checkpoint lights as you pass, and is where you come back to ---------
right 30
expect checkpoint == 1
wait 10
expect checkpoint == 1

# Into the spikes on purpose: death, and back at the brazier rather than the
# door the run started at.
right 30
expect player.died == 1
wait 60
expect respawned == 1
assert x > 355
assert x < 380
assert !dead

# --- over the spikes, and the chest ----------------------------------------
#
# A run-up and a held jump: a tapped one is a short hop, and lands in them.
right 18
right+jump 18
right 20
wait 10
assert !dead
assert grounded
assert x > 512
expect player.died == 1
right 26
assert prompt == open
interact 1
expect chest_opened == 1
expect coin.picked_up == 1
expect mana_potion.picked_up == 1
assert item.coin.count == 5
assert item.mana_potion.count == 1
assert world.testbed_world.chest_20_9 == 1
wait 1
assert prompt == none

# --- the gate is shut; the lever opens it ------------------------------------
right 60
assert x < 832
assert quest.testbed.gate == 0
left 22
assert prompt == pull
interact 1
expect lever.interacted == 1
expect lever_pulled == 1
assert quest.testbed.gate == 1

# --- a save and a load in the middle of it all -------------------------------
#
# The map is rebuilt from its file and the flags are put back on it: the chest
# is still open, the key is still taken, the gate is still up.
reload
assert quest.testbed.gate == 1
assert world.testbed_world.chest_20_9 == 1
assert item.iron_key.count == 1
assert item.coin.count == 5
assert pickups == 0

# --- through the gate to the locked door, which the key opens ----------------
right 60
assert x > 864
assert prompt == unlock
interact 1
expect iron_key.unlocked == 1
expect testbed_world_b.traveled == 1
assert map == testbed_world_b
assert item.iron_key.count == 0
assert world.testbed_world.cellar == 1
expect no locked
expect no travel_failed
