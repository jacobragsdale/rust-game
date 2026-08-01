map maps/testbed_arena.ron

# Kill the arena knight from across the room, then go and pick up what it
# dropped. Two features in one run, because they are two halves of one thing:
# a loot table that rolls and a body that has to be walked over.
#
# The kill is by spell on purpose. A melee kill knocks the knight back onto the
# player, so its drops land underfoot and "walk over the item" is never tested
# at all — the tape would pass with the pickup check deleted. Killing at range
# leaves the drops where the knight fell, a hundred and forty pixels away, so
# the walk is the thing being asserted.
#
# The drops are deterministic because the rolls are: the same seed, the same
# table, the same order, every time. `traces/loot.jsonl` is that promise
# written down — if a roll ever moves, the drop moves with it and the baseline
# says so on the exact tick.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70 facing right, 5 mana
#   knight  spawns x=229 with 6 hp, dies around x=212
#   drops   knight_helm and minor_potion, both chance 1.0; the iron_sword is
#           the coin flip and does not land at the default seed

assert pickups == 0
assert inventory_count == 0
assert item.knight_helm.count == 0
assert item.minor_potion.count == 0
expect no picked_up

# Standing still and casting: exactly `spell_kill.tape`'s opening, because the
# fight is not what this tape is about.
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
expect knight.died == 1

# The table rolled and the floor has things on it — but nothing is in the bag,
# because nobody has walked anywhere yet.
assert pickups == 2
assert inventory_count == 0
expect no picked_up

# Now go and get them. The knight fell to the right of the spawn point.
right 90

expect picked_up == 2
assert pickups == 0
assert inventory_count == 2
assert item.knight_helm.count == 1
assert item.minor_potion.count == 1
assert !item.knight_helm.equipped

# Picking something up does not put it on.
assert hp_max == 5

# And a corpse drops once, not once per tick it spends dead.
wait 120
assert pickups == 0
expect picked_up == 2
