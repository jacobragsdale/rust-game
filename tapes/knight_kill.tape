map maps/testbed_arena.ron

# Win the fight. The knight fights back now, so this is an exchange rather
# than target practice: close, swing, back out of reach while i-frames and the
# knight's cooldown run down, repeat.
#
# The tape asserts the *outcome*, not per-hit ticks. Exact timing falls out of
# knockback distance, chase speed, the i-frame window and the knight's cooldown
# all at once; pinning it here would encode four tunables. traces/knight_kill
# holds the tick-by-tick detail, so a timing change still shows up — just not
# as a mystery failure in this file.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70, 5 hp
#   knight  spawns x=229, 3 hp

assert hp == 5
assert knight.0.hp == 3
expect no attacked
expect no damaged

# ten passes: step in, swing, retreat
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26

# two hits in, and the knight has started answering
assert knight.0.hp == 1
expect knight.damaged == 2

right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26
right 14
attack 1
wait 22
left 22
wait 26

# Three hits, one dead knight — and it cost four of the player's five.
# `knight.damaged` rather than `damaged`: both sides bleed now, so the
# unqualified count is not an assertion about anyone in particular.
assert knight.0.dead
assert knight.0.hp <= 0
expect knight.damaged == 3
expect knight.died == 1
expect player.damaged == 4
expect no respawned

# Survived, barely.
assert hp == 1
assert !dead

# The corpse stays where it fell. Despawning would renumber every knight after
# it and silently repoint every later `knight.<n>` assertion.
wait 60
assert knight.0.dead
assert knight.0.grounded
assert knight.0.vx == 0

# And a dead knight stops fighting: no further damage after it went down.
expect player.damaged == 4
