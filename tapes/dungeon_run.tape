map maps/dungeon.ron

# N-3, end to end: spawn to the stair down, with every hazard in the level
# crossed the way it is meant to be crossed.
#
# The ticket asks for `expect no died`. That is written `expect no player.died`
# here and deliberately: a bare `died` counts the knights' deaths too, so the
# literal line would contradict the very next one. See tapes/README.md.
#
# Nothing in this tape asserts a tick. It asserts where the player ends up
# after each section, that the two knights are dead, that the player is not,
# and that the second knight was opened on for free — which is the one thing
# the level is trying to teach. traces/dungeon_run.jsonl holds the detail.
#
#   fire A/B     x 326..346 and x 422..442, alternating on the house cycle
#   rubble       x 576..608, 928..960 (knight 0's pen) and 1664..1696
#   knight 0     spawns x=741, penned in an open room on the player's floor
#   pit          x 1088..1472, spiked; the platform is the only way over
#   gangways     one-way plank, x 1088..1120 and 1440..1472, surface y=320 —
#                the clearance the ferry has to leave at each end, walked over
#   ferry        x 1120..1216 at the near end, 1344..1440 at the far one,
#                surface y=320, 179 ticks a leg
#   landing      x 1472..2080, surface y=320
#   stair        one-way, x 1536..1664, surface y=256
#   gallery      one-way, x 1664..2144, surface y=192 — 128px over the hall
#   knight 1     spawns x=1829, paces x 1702..2057
#   stairwell    x 2080..2144, floor y=352 — the way down, and the end

wait 1
assert grounded
assert x == 70
assert y == 286
assert hp == 5
assert knight.0.hp == 6
assert knight.1.hp == 6
expect no damaged

# --- the fire corridor -------------------------------------------------------
#
# Fire A is out from tick 60, so the run past it clears it around tick 88. It
# stops on the tile between the two, which is inside neither box: A relights at
# 120 and B is lit until 119, so there is no single dash that crosses both.
# Running straight through dies at B — see the map's notes.

right 95
assert x > 346
assert x < 402
assert grounded
assert !dead
expect no died

# ...and go again the moment B goes out.
wait 25
right 40
assert x > 442
assert !dead
expect no died

right 40
assert grounded
assert x == 556
expect no died

# --- knight 0, head-on -------------------------------------------------------
#
# Over the rubble and into its room, on its floor and inside its sight box.
# Three passes of the full combo with a step back between them; the exact
# ticks of each hit fall out of knockback, i-frames and its cooldown at once,
# so what is asserted is that it died and the player did not.

right+jump 1
right 40
assert grounded
assert x > 608
wait 60

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

assert knight.0.dead
assert knight.0.hp <= 0
expect knight.died >= 1
assert !dead
expect no player.died

# --- the pit -----------------------------------------------------------------
#
# East until the second rubble step stops the run, then over it and up to the
# lip, stopping short of it. The platform is out over the pit and coming back.

right 60
assert grounded
assert x == 908

right+jump 1
right 45
wait 20
assert grounded
assert y == 286
assert x > 1000
assert x < 1068

# Over the plank gangway (x 1088..1120) and aboard as the ferry docks against
# its far end, then ride with no input at all: everything moving the player is
# under their feet, which is what `vx == 0` out over the pit says. `x > 1120`
# is past the gangway entirely — on the ferry, not on the map.
right 36
assert grounded
assert y == 286
assert x > 1120

# The outbound leg ends at tick 979; this stops a little short of it, so the
# walk off starts while the ferry's leading edge is still closing on the far
# gangway rather than receding from it — step off late and the widening gap is
# a fall into the spikes, which is the ride being a ride.
wait 124
assert grounded
assert y == 286
assert vx == 0
assert x > 1300
expect no player.died

# Off across the far gangway (x 1440..1472) onto the landing.
right 54
assert grounded
assert y == 286
assert x > 1472
expect no player.died

# --- up to the gallery -------------------------------------------------------
#
# Two hops: onto the one-way stair, and from its far end onto the plank
# gallery over the last hall.

right+jump 8
right 20
assert grounded
assert y == 222

right+jump 20
right 20
assert grounded
assert y == 158

# --- over knight 1, unnoticed ------------------------------------------------
#
# The gallery is 128px above the hall floor and the sight box is 36px deep, so
# the knight cannot see the player however close they are or which way it is
# facing. `knight.1.vx == -55` is the proof: 55 is its patrol speed, and a
# knight that had noticed anything would be moving at the chase multiplier
# instead. The player walks the length of its patrol — straight over its head
# on the way — and stops above the far end of the hall.

right 50
assert y == 158
assert knight.1.vx == -55
assert knight.1.hp == 6
expect no player.damaged

right 30
wait 20
assert grounded
assert y == 158
assert x > 2000
assert knight.1.vx == -55
expect no player.damaged

# --- down behind it ----------------------------------------------------------
#
# Straight down through the plank, landing at its back while it walks away.
# The swings that follow land before it has any idea, which is the whole
# difference between this fight and the last one.

down 1
wait 30
assert grounded
assert y == 286
expect dropped_through == 1

left 40
attack 1
wait 24
left 22
attack 1
wait 24
left 22
attack 1
wait 24
left 22

assert knight.1.hp <= 4
assert !dead
assert hp == 5
expect no player.damaged

# It reaches the end of its pen, turns, and now it is knight 0 again — so
# finish it the way knight 0 was finished, giving ground between passes.
right 40
wait 40

left 10
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
right 20
wait 28
left 10
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
right 20
wait 28
left 10
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
right 20
wait 28
left 10
attack 1
wait 10
attack 1
wait 12
attack 1
wait 24
right 20
wait 28

assert knight.1.dead
assert knight.1.hp <= 0
expect knight.died == 2
assert !dead
expect no player.died

# --- what it was carrying ----------------------------------------------------
#
# There is no `Item(...)` entity form, so the thing at the end of the dungeon
# is the thing the guard had on it. Its two guaranteed drops are on the floor
# where it fell; the helm is under the player's feet already, and the potion is
# swept up on the way out.

left 12
wait 20
assert item.knight_helm.count == 1
assert knight.1.dead
expect knight_helm.picked_up == 1

# --- the way down ------------------------------------------------------------
#
# East along the hall and one step down into the stairwell, which is where the
# level ends. The step down is also what pens the knight: a patroller turns at
# the edge of what it is standing on, so nothing follows you in here.

right 170
assert grounded
assert x == 2124
assert y == 318
assert !dead
assert item.minor_potion.count == 1
expect minor_potion.picked_up == 1
expect no player.died
expect knight.died == 2
