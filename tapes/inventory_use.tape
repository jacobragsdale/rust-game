map maps/testbed_arena.ron

# M4's exit criterion, end to end and headless: kill the knight, loot drops,
# pick it up, open the inventory, drink a potion, equip a helm, and watch the
# stats move — with no window, no GPU and nobody at a keyboard.
#
# It is one tape rather than five because the criterion is the chain, and a
# chain tested a link at a time is exactly what leaves the joins untested. If
# any part of the milestone were deleted this stops passing.
#
# It is also the tape that proves the inventory screen is *simulation*. Every
# key here — `inventory`, `down`, `confirm` — goes through the same action
# table a player's keyboard does, and every consequence is a probe field. Had
# the selection index and the use/equip logic gone into `scenes/`, none of the
# second half of this file could exist.
#
# The fight is `knight_kill.tape`'s, unchanged: six passes of step in, chain
# all three swings, back out. Melee on purpose — the knockback drops the
# knight onto the player, so the drops land underfoot and the run costs the
# player four of their five health, which is what gives the potion something
# to heal. (`loot.tape` kills at range instead, so that the walking-over is
# tested somewhere too.)
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70 with 5 hp and no bag
#   knight  spawns x=229 with 6 hp; drops a helm and a potion, always

assert hp == 5
assert hp_max == 5
assert mode == playing
assert inventory_count == 0
expect no picked_up

right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28
right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28
right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28
right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28
right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28
right 16
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
left 26
wait 28

# Dead knight, drops collected, and a player who badly wants that potion.
assert knight.0.dead
expect knight.died == 1
expect picked_up == 2
assert inventory_count == 2
assert item.knight_helm.count == 1
assert item.minor_potion.count == 1
assert hp == 1

# --- the screen ------------------------------------------------------------
#
# One press opens it. The world stops on that same tick, which is why the
# detour costs the run nothing (`inventory_frozen.tape` is where that is
# measured).
inventory 1
assert mode == inventory
assert pane == bag
assert selection == 0
expect inventory.mode_changed == 1

# The bag is in pickup order: the helm landed first. Move down to the potion.
down 1
assert selection == 1
wait 1

# Drink it. Two points, and the stack is gone with it.
confirm 1
expect minor_potion.item_used == 1
assert hp == 3
assert item.minor_potion.count == 0
assert inventory_count == 1
# The row went away underneath the selection, which must not leave it hanging
# past the end of a one-row list.
assert selection == 0
wait 1

# Now the helm, which is what "stats change" means: the maximum moves and the
# current value is clamped rather than topped up.
confirm 1
expect knight_helm.equipped == 1
assert hp_max == 7
assert hp == 3
assert item.knight_helm.equipped
assert item.knight_helm.count == 0
assert inventory_count == 0
wait 1

# The other pane lists every slot, filled or not, and the selection survives
# the move into it.
right 1
assert pane == gear
assert selection == 0
wait 1

# Take it off again: the maximum goes back to exactly the base, with no drift
# left behind — there is nothing to undo, because nothing was ever added.
confirm 1
expect knight_helm.unequipped == 1
assert hp_max == 5
assert hp == 3
assert !item.knight_helm.equipped
assert item.knight_helm.count == 1
wait 1

# And back on, because the milestone's exit criterion is the helm being worn.
left 1
wait 1
confirm 1
assert hp_max == 7
assert item.knight_helm.equipped

# Close it. The world starts again where it stopped.
inventory 1
assert mode == playing
expect playing.mode_changed == 1
expect no player.died

# The equipment survives leaving the screen — it is on the entity, not on the
# screen that was used to put it there.
right 20
assert hp_max == 7
assert item.knight_helm.equipped
assert mode == playing
