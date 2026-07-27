map maps/testbed_platform.ron

# The player's melee kit, away from anything that fights back.
#
# Nothing here needs an enemy: this is about the moveset existing and the right
# animations playing. knight_kill.tape is where it has to work under pressure.
#
# On testbed_platform rather than testbed because a slide covers 300 px/s and
# testbed has a spike pit at the end of the runway.

wait 1
assert grounded
expect no attacked

# --- the three-hit ground combo ---
#
# Pressing during a swing buffers the next link, which starts when the current
# animation ends. Mashing gets the whole chain, which is what a player will
# try first.
wait 10
attack 1
wait 10
attack 1
wait 12
attack 1

# Each link runs its full animation before the next starts, so the whole
# chain takes far longer than the three presses did: slash1 at ~12, slash2 at
# ~32, slash3 at ~56.
wait 30
expect player_slash1.attacked == 1
expect player_slash2.attacked == 1
expect player_slash3.attacked == 1
assert attacking

# the finisher has the longest recovery, so commitment outlasts its animation
wait 40
assert !attacking

# --- slide: down + jump while running ---
#
# Distinct from drop-through (down alone on a platform) and from crouching
# (down while standing still).
right 30
assert !crouching
right+down+jump 1
expect slid == 1
assert vx > 200

# it bleeds back to running speed rather than stopping dead
wait 25
expect slid == 1

# --- air attack ---
#
# A different, shorter attack, and it does not chain: pressing again in the
# air must not start a ground combo up there.
wait 20
right+jump 1
wait 12
assert !grounded
attack 1
wait 4
expect player_air.attacked == 1
attack 1
attack 1
wait 30
expect player_air.attacked == 1
expect player_slash2.attacked == 1

# ...and nothing hostile happened to anyone on this map
expect no damaged
expect no died
assert hp == 5
