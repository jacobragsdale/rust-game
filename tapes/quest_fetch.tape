map maps/testbed_quest.ron

# **M6's exit criterion, end to end and headless:** accept a quest, complete
# it, and get paid for it — with no window, no GPU and nobody at a keyboard.
# The save/load half of the criterion is `tests/save.rs`, which replays this
# kind of run through a file and diffs the traces.
#
# One tape rather than three, because the criterion is the chain. Each leg is
# only interesting given the one before it:
#
#   1. kill the knight            the only source of items in the game is a
#                                 corpse, so "something to fetch" is a fight
#   2. walk left, take the errand `quest.helm.stage` 0 -> 1, and the reply that
#                                 hands the helm over does not exist yet
#   3. walk right, take the helm  off the floor where the knight fell
#   4. walk back, hand it over    stage 1 -> 2, the helm leaves the bag and
#                                 Runa's band arrives in it
#   5. ask again                  the reply is gone: five options become four
#
# **What only a flag can say.** Legs 2 and 4 are the same node, entered with the
# same bag, and they offer different replies — because between them a flag
# moved. Delete the `SetFlag` and step 4's option never appears; delete the
# `FlagEq` half of its condition and step 2's "I have the helm you asked for."
# is offered to somebody who has never been asked for anything. `quest.helm.stage`
# is asserted at all three of its values, which is Q-1's whole point: a quest
# stage is a number a tape can read, not an inference from what is in the bag.
#
#   room     x 32..768, upper floor surface y=224
#   bay      x 32..96, floor y=256 — a step down at the left end
#   player   spawns x=294 facing right, 5 mana
#   knight   spawns x=453 with 6 hp; four bolts kill it and the knockback
#            carries the body to x=688, where its drops land
#   villager Runa, pacing x 38..68 in the bay and unable to leave it
#   graph    elder_intro, in assets/data/dialogue/elder.ron

assert quest.helm.stage == 0
assert mode == playing
assert prompt == none
assert inventory_count == 0
assert item.knight_helm.count == 0
assert item.iron_band.count == 0
expect no dialogue_opened
expect no picked_up

# --- 1. the thing to fetch has to be killed for -------------------------
#
# `spell_kill.tape`'s opening exactly: the player never moves, and the 159px
# between the two of them is `testbed_arena.ron`'s spacing to the pixel. Four
# bolts land; the fifth cast is the spare that makes this about the errand
# rather than about the mana curve.
cast 1
wait 55
cast 1
wait 55
cast 1
wait 55
cast 1
wait 55
cast 1
wait 80

assert knight.0.dead
assert pickups == 2
assert inventory_count == 0
expect knight.died == 1
expect no player.damaged

# --- 2. take the errand -------------------------------------------------
#
# Run left until the wall stops you and she is there — the bay pens her into a
# 30px beat inside the interact reach, so nothing here is timed.
left 130
assert prompt == talk
assert x == 32
assert grounded

interact 1
assert mode == dialogue
assert dialogue_node == greet

# Three replies, and the one that matters is *absent*: the helm is lying on the
# floor two rooms away, and Runa has not mentioned it. There is no row here to
# highlight and be refused — see `src/systems/dialogue.rs`.
assert dialogue_choices == 3

confirm 1
wait 1
confirm 1
wait 1
assert dialogue_node == who
assert dialogue_choices == 2

# "Which of them came down here?" — the second reply, and the only place in the
# game the errand is offered.
confirm 1
wait 1
down 1
wait 1
assert dialogue_selection == 1
confirm 1
wait 1
assert dialogue_node == errand
assert quest.helm.stage == 0

# Taking it is what sets the stage. Asking about it did not.
confirm 1
wait 1
confirm 1
wait 1
assert quest.helm.stage == 1
assert dialogue_node == greet
expect who.choice_taken == 1
expect errand.choice_taken == 1

# Back at the greeting with the errand accepted and empty hands: still three
# replies. Half a condition is not a condition.
assert dialogue_choices == 3
cancel 1
assert mode == playing
assert quest.helm.stage == 1

# --- 3. fetch it --------------------------------------------------------
#
# Out of the bay and up the room to where the body fell. The right wall stops
# the run, so the distance is a bound rather than a measurement.
right+jump 1
right 220
assert grounded
assert y == 190
assert inventory_count == 2
assert item.knight_helm.count == 1
assert pickups == 0
expect knight_helm.picked_up == 1
expect minor_potion.picked_up == 1

# --- 4. hand it over ----------------------------------------------------
left 260
assert prompt == talk
assert x == 32

interact 1
assert mode == dialogue
assert dialogue_node == greet

# Five replies now, where there were three. The draught branch came back with
# the knight's potion; the fourth is the one the errand unlocked, and it needed
# both halves of its condition to be true at once.
assert dialogue_choices == 5

confirm 1
wait 1
down 1
wait 1
down 1
wait 1
down 1
wait 1
assert dialogue_selection == 3
confirm 1
wait 1

# **The state change, and the reward.** The helm goes back through the same
# `Inventory` a pickup lands in, the band arrives the same way, and the stage
# moves to 2 in the same breath.
assert dialogue_node == helm
assert quest.helm.stage == 2
assert item.knight_helm.count == 0
assert item.iron_band.count == 1
assert !item.iron_band.equipped
expect iron_band.picked_up == 1
expect greet.choice_taken == 2

confirm 1
wait 1
confirm 1
wait 1
assert mode == playing
assert dialogue_node == none
expect elder_intro.dialogue_closed == 2

# --- 5. and it stays done -----------------------------------------------
#
# The same node a third time. The errand's reply is gone — not because the bag
# is empty (the potion is still in it, and its own branch is still offered) but
# because the flag says the errand is finished.
interact 1
assert mode == dialogue
assert dialogue_node == greet
assert dialogue_choices == 4
assert quest.helm.stage == 2
cancel 1

# The world starts again where it stopped, and the errand stays done: the stage
# is on the simulation, not on the conversation that moved it — the same thing
# `tests/save.rs` says about the file it survives.
wait 40
assert mode == playing
assert item.iron_band.count == 1
assert item.minor_potion.count == 1
assert quest.helm.stage == 2
assert quest.draught.stage == 0
assert hp == 5
expect no player.died
expect elder_intro.dialogue_opened == 3
