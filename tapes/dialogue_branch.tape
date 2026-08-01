map maps/testbed_village.ron

# **M5's exit criterion, end to end and headless:** walk up to an NPC, hold a
# conversation, take a branch gated on an inventory item, and watch the state
# change — with no window, no GPU and nobody at a keyboard.
#
# It is one tape rather than three because the criterion is the chain. The same
# node is visited twice, and the whole point is that it offers *different
# replies the second time*: the branch gated on carrying a draught does not
# exist at all until the player is carrying one. That is the observable, and
# `dialogue_choices` going from three to four is how a tape sees it. Choices
# whose conditions fail are hidden, not greyed out — see `systems/dialogue.rs`
# for the argument; the consequence here is that this count is a count of what
# the player can actually do.
#
# It is also the tape that proves dialogue is *simulation*. Every key in it —
# `interact`, `down`, `confirm` — goes through the same action table a player's
# keyboard does, and every consequence is a probe field or an event. Had the
# node, the selection and the effects gone into `scenes/`, none of this file
# could exist and the largest system in the game would be verifiable only by a
# human with a keyboard.
#
#   graph   elder_intro, in assets/data/dialogue/elder.ron
#   greet   two lines, then: who / gift / (gated) thanks / goodbye
#   gift    Runa hands over a draught      -> GiveItem minor_potion, stage 1
#   thanks  handing it back                -> TakeItem minor_potion,
#                                             GiveItem elder_charm, stage 2

assert mode == playing
assert prompt == none
assert dialogue_node == none
assert dialogue_choices == 0
assert inventory_count == 0
assert item.minor_potion.count == 0
assert item.elder_charm.count == 0
assert quest.draught.stage == 0
expect no dialogue_opened

right 160
assert prompt == talk

# --- first conversation: no draught, so no fourth reply -------------------
#
# The world stops on the tick the key is pressed, exactly as it does for the
# inventory: the tick a modal state is entered is itself a frozen one.
interact 1
assert mode == dialogue
assert dialogue_node == greet
assert dialogue_selection == 0
expect elder_intro.interacted == 1
expect elder_intro.dialogue_opened == 1

# Three replies, countable from the moment the node is entered — the filtering
# is a fact about the world, not about how far into a speech you have read.
# Only whether they are *drawn* waits for the last line.
assert dialogue_choices == 3

# Two lines of greeting. `confirm` reads on; the replies become selectable once
# the last one has been.
confirm 1
wait 1
assert dialogue_node == greet
assert dialogue_choices == 3

# Three replies, and the fourth is *absent* rather than listed and refused:
# "I brought your draught back." is gated on carrying one, and the bag is
# empty. Ask for one instead — the second reply.
down 1
wait 1
assert dialogue_selection == 1
confirm 1
assert dialogue_node == gift
expect greet.choice_taken == 1

# One line, one reply, and taking it hands the draught over. The reward goes
# through the same `Inventory::add` a pickup off the floor does, so it is
# bookkept as the `picked_up` it is — a quest reward invisible to the trace
# would be a quest reward invisible to every tape ever written.
wait 1
confirm 1
assert mode == playing
assert item.minor_potion.count == 1
assert inventory_count == 1
# The errand's first stage, set by the same reply that handed the draught over.
# Unset until now, and 0 is what an unset flag reads as — which is why the
# assertion at the top of this tape resolves at all.
assert quest.draught.stage == 1
expect minor_potion.picked_up == 1
expect gift.choice_taken == 1
expect elder_intro.dialogue_closed == 1
expect choice_taken == 2

# --- second conversation: the gated branch is now there -------------------
#
# The same node, the same graph, a different set of replies — because
# conditions are re-read every time a node is entered rather than once when the
# conversation opens.
wait 1
assert prompt == talk
interact 1
assert mode == dialogue
assert dialogue_node == greet
expect elder_intro.dialogue_opened == 2

confirm 1
wait 1
assert dialogue_choices == 4

# Down twice onto the branch that did not exist a moment ago.
down 1
wait 1
down 1
wait 1
assert dialogue_selection == 2
confirm 1

# **The state change.** The draught is gone, the charm is in its place, and
# both went through the inventory rather than around it.
assert dialogue_node == thanks
assert item.minor_potion.count == 0
assert item.elder_charm.count == 1
assert inventory_count == 1
assert quest.draught.stage == 2
expect elder_charm.picked_up == 1
expect greet.choice_taken == 2

# Read her out and close.
wait 1
confirm 1
wait 1
confirm 1
assert mode == playing
assert dialogue_node == none
assert dialogue_choices == 0
expect thanks.choice_taken == 1
expect choice_taken == 4
expect elder_intro.dialogue_closed == 2

# The world starts again where it stopped, and what was handed over stays
# handed over — it is on the entity, not on the conversation that moved it.
left 40
assert mode == playing
assert item.elder_charm.count == 1
assert item.minor_potion.count == 0
assert quest.draught.stage == 2
assert !item.elder_charm.equipped
assert hp == 5
expect no died
