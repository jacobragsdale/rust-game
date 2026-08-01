map maps/testbed_arena.ron

# The plunge, and the two attacks it must not have swallowed.
#
# `attack` now means three different things depending on where you are and
# whether down is held, and the whole risk of adding the third is that it eats
# one of the other two. So this tape performs all three: the ground combo with
# down held, the air attack without it, and only then the plunge.
#
# The hit is asserted as an outcome — that it connected, and for how much —
# rather than on the tick it lands, because that tick falls out of the knight's
# chase speed, the drop speed, i-frames and hitstop together. The *clip
# sequence* is not an outcome and is pinned exactly: which of the three plunge
# animations is on screen is decided by the player's own state machine and by
# nothing else, and getting it wrong is the failure this feature is most likely
# to ship with.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70; knight spawns x=229 and closes at 69 px/s
#   plunge  10-tick hover, 1000 px/s drop, 20-tick impact, 2 damage
#
# The opening `wait 1` is not decoration: the player spawns resting on the
# floor but has not collided with it yet, so on tick 1 the body still believes
# it is airborne — and down+attack there would be a plunge, not a combo.
wait 1
assert grounded
assert !plunging
expect no attacked

# --- down + attack on the ground is still the ground combo ---
down+attack 1
expect player_slash1.attacked == 1
assert !plunging
assert clip == attack1
wait 38

# --- attack alone in the air is still the air attack ---
jump 1
wait 8
attack 1
expect player_air.attacked == 1
assert !plunging
assert clip == air_attack
wait 50
assert grounded
assert !plunging
expect no player_plunge.attacked

# --- down + attack in the air is the plunge: hang, then drop ---
right 30
right+jump 1
right 6
down+attack 1
assert plunging
assert clip == plunge_ready
assert !grounded
assert vy == 0
expect no player_plunge.attacked

# The hover lasts exactly as long as `plunge_hover_ticks` says, and the hitbox
# does not exist during it — the drop is the attack.
down 9
assert clip == plunge_ready
assert vy == 0
expect no player_plunge.attacked

# Then the drop: faster than terminal velocity, straight down, hitbox live.
down 1
assert clip == plunge_loop
assert vy > 900
assert vx == 0
expect player_plunge.attacked == 1
expect knight.damaged >= 1

# It lands on the knight, and the impact is what is on screen when it does.
down 6
assert grounded
assert clip == plunge_impact
assert knight.0.hp <= 4
expect no player.damaged

# --- the impact roots the player: no steering out of the recovery ---
left 19
assert clip == plunge_impact
assert vx == 0
assert plunging

# ...and control comes back on the tick after it, not before.
left 1
assert !plunging
assert vx < 0
assert clip == run

# Nothing here turned into one of the other two attacks along the way.
left 20
expect player_slash1.attacked == 1
expect player_air.attacked == 1
expect player_plunge.attacked == 1
expect no player_slash2.attacked
