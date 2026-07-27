map maps/testbed_arena.ron

# The other direction: stand still and be killed for it.
#
# The player does nothing at all, so everything here is the knight's doing —
# it notices, closes, and swings on its cooldown until there is nothing left
# to swing at. Then the death freeze and respawn that have existed since the
# spike pit take over, and the fight starts again from full health.
#
#   player  spawns x=70, 5 hp
#   knight  spawns x=229 with 6 hp, swings once every 75 ticks

assert hp == 5
assert !dead
expect no damaged

# it has to walk over first
wait 200
assert hp == 5
expect no player.damaged

# five swings, five hits, one dead player
wait 390
assert hp <= 0
assert dead
assert clip == die
expect player.damaged == 5
expect player.died == 1

# the death freeze, then back at spawn with full health
wait 60
assert !dead
assert hp == 5
assert x == 70
expect respawned == 1

# ...and the knight, which never took a scratch, starts again
expect no knight.damaged
assert knight.0.hp == 6
