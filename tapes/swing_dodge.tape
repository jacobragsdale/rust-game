map maps/testbed_swing.ron

# A swinging hazard has to be both dodgeable and lethal, so this tape does
# both. A ball that was simply always deadly passes the second half; a ball
# whose collider never reached the geometry passes the first.
#
# The map's chain hangs from (400, 176), 160px long, swinging 75 degrees either
# side of straight down every 300 ticks. That puts the ball at head height only
# near the bottom of its arc — x 303..497, during ticks 53..97 and 203..247 of
# every cycle — and a hundred pixels above the player at either end of it.

wait 5
assert grounded
assert x == 70
assert y == 318
assert !dead
expect no died

# --- watch it sweep past ---
# The ball crosses the middle of the room leftward during ticks 53..97. The
# spawn is at x 70, well left of the arc's left-hand end at x 245, so waiting
# here is safe however long you wait — the point of standing still is that the
# run afterwards starts on a known tick of the cycle.
wait 90
assert x == 70
assert !dead
expect no died

# --- the run, under the ball while it is out at the left of its arc ---
# Timed rather than lucky. Sweeping the tick this run starts on: before 31 the
# ball catches the player on its way back and after 116 it catches them on the
# way out, and this tape starts on 95 because that is where the closest
# approach over the whole crossing is widest — 66px, against 2px at 116. The
# ball passes overhead around tick 157, high enough at the end of its arc to
# run under with the player's head clear by ninety pixels.
right 180
assert x == 652
assert grounded
assert !dead
expect no died
# Never left the ground: the only landing so far is the one on tick 1, settling
# onto the floor under the spawn point. A hazard that had briefly become solid
# would show up right here as an extra landing.
expect no jumped
expect landed == 1

# --- and then stand in the arc and wait to be hit ---
# Walking back into the middle of the room and stopping there needs no timing
# at all: the ball comes round every 300 ticks and the bottom of its swing is
# exactly where the player is standing.
left 84
assert x > 310
assert x < 470
assert !dead
expect no died

wait 20
assert dead
expect died == 1

# after the death freeze: back at the spawn point, which no part of the arc
# reaches — so one death and one respawn, not a loop
wait 40
assert !dead
assert grounded
assert x == 70
assert y == 318
expect respawned
expect died == 1
expect landed == 2
