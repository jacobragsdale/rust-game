map maps/testbed_arena.ron

# Win the fight, using the whole combo.
#
# Six passes of: step in, chain all three swings, back out while the knight's
# cooldown and the i-frames run down. The knight has six health and a full
# combo does four, so this needs the better part of two clean chains landed.
#
# The tape asserts the *outcome*, not per-hit ticks. Exact timing falls out of
# knockback distance, chase speed, i-frames, the knight's cooldown and hitstop
# all at once; pinning it here would encode five tunables. traces/knight_kill
# holds the tick-by-tick detail, so a timing change still shows up — just not
# as a mystery failure in this file.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70, 5 hp
#   knight  spawns x=229, 6 hp

assert hp == 5
assert knight.0.hp == 6
expect no attacked
expect no damaged

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

# Dead knight, and it cost most of the player's health.
assert knight.0.dead
assert knight.0.hp <= 0
expect knight.died == 1
expect no respawned
assert !dead

# The corpse stays where it fell. Despawning would renumber every knight after
# it and silently repoint every later `knight.<n>` assertion.
wait 60
assert knight.0.dead
assert knight.0.grounded
assert knight.0.vx == 0
assert knight.0.clip == death

# All three links of the combo saw use, not just the opener.
expect player_slash1.attacked >= 3
expect player_slash2.attacked >= 3
expect player_slash3.attacked >= 3
