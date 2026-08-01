map maps/testbed_village.ron

# D-1, end to end: the prompt appears when you are near enough to talk, clears
# when you walk away, and is announced once per approach rather than once per
# tick of standing there.
#
# It also pins the half of the feature that is an *absence*: pressing the key
# with nothing in reach does nothing and emits nothing. Without an assertion on
# that, "interact is broken" and "there was nobody there" are the same empty
# stretch of trace, and only one of them is a bug.
#
#   upper floor  surface y=224, x 0..480
#   bay          floor y=256, x 480..544 — a step down at the far end
#   player       spawns x=70; runs off the ledge and is stopped by the wall at
#                x=524, feet in the bay
#   villager     paces x 485..516 and cannot leave the bay
#   reach        24px around the player's box, on every side

assert prompt == none
assert mode == playing
expect no interact_prompted

# Nothing in reach. The press is swallowed whole: no event, no mode change, and
# — the part that matters — not a tick of the world skipped either.
interact 1
assert mode == playing
assert prompt == none
expect no interacted
expect no dialogue_opened

# Walk up to her. The offer is announced on the tick it becomes true, which is
# while the player is still on the ledge above the bay — vertical reach is the
# same 24px, and a step is less than that.
right 160
assert prompt == talk
assert mode == playing
assert grounded
expect elder_intro.interact_prompted == 1

# Standing there is not a second offer. Sixty identical events a second is not
# a trace, which is why this fires on the change and not on the state — and
# why she has a bay to pace rather than the whole room, so "standing there"
# stays true for as long as the tape wants it to.
wait 120
assert prompt == talk
assert x == 524
expect elder_intro.interact_prompted == 1

# Jump out of the bay and walk away, and it clears. Nothing is emitted for the
# clearing: `prompt` reading `none` again in the trace is the record of it.
left+jump 1
left 60
assert prompt == none
assert grounded
expect elder_intro.interact_prompted == 1

# ...and the same press is dead again the moment she is out of reach.
interact 1
assert mode == playing
expect no interacted

# Coming back offers it afresh, because the offer changed and changed back.
right 90
assert prompt == talk
expect elder_intro.interact_prompted == 2
expect no dialogue_opened
expect no died
