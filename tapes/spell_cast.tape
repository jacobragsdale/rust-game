map maps/testbed_arena.ron

# The cast itself: what it costs, what it commits you to, when the bolt
# actually appears, and how long it is before you may do it again.
#
# Everything here is the player and their own bolt, so — unlike a tape with a
# knight in it — pinning ticks is legitimate: none of these numbers falls out
# of chase speed, knockback or i-frames. To keep it that way the player turns
# and casts into the *left* wall, away from the knight at the far end. A bolt
# that connected would add four ticks of hitstop to every count below, which is
# exactly the coupling this file exists to avoid.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70, 5 mana
#   shock   cost 2, cast_ticks 12, recovery 6, cooldown 45
#   regen   20 thousandths a tick: one point every 50 ticks

assert mana == 5
assert mana_max == 5
assert cast_cooldown == 0
assert !casting
assert projectiles == 0
expect no spell_cast

# One tick of left is enough to turn around and not enough to travel: the bolt
# leaves from the player's leading edge, so walking into the wall first would
# spawn it inside the wall. Facing is decided *after* the cast within a tick,
# so the turn has to be its own press rather than `left+cast`.
left 1
assert !facing_right
wait 9

# --- a cast is paid for up front, and commits the caster ---
cast 1
assert casting
assert mana == 3
assert cast_cooldown == 45
assert clip == cast
assert projectiles == 0
expect shock.spell_cast == 1

# --- pressing again mid-cast is refused, and says which refusal it was ---
wait 1
cast 1
expect shock.cast_failed == 1
assert mana == 3
assert projectiles == 0

# --- the bolt appears on the release tick, and not one tick before ---
wait 9
assert projectiles == 0
wait 1
assert projectiles == 1
assert casting

# --- the cast ends after its recovery, and the clip goes with it ---
wait 6
assert !casting
assert clip != cast

# --- the cooldown blocks the next one, whatever the mana says ---
cast 1
assert cast_cooldown > 0
assert mana == 3
expect shock.cast_failed == 2
expect shock.spell_cast == 1

# --- and expires on schedule: still blocking at 44 ticks, gone at 45 ---
wait 24
assert cast_cooldown == 2
wait 1
assert cast_cooldown == 1
cast 1
expect shock.spell_cast == 2
assert mana == 1
assert cast_cooldown == 45

# --- an empty pool is a different refusal, and spends nothing ---
wait 45
assert cast_cooldown == 0
assert mana == 1
cast 1
expect shock.cast_failed == 3
assert mana == 1
expect shock.spell_cast == 2
assert !casting

# --- regeneration is exact: one point every 50 ticks ---
wait 4
assert mana == 2
wait 50
assert mana == 3

# --- and it stops at the maximum rather than banking anything ---
#
# Loose on purpose from here: the knight arrives somewhere in this stretch and
# every hit it lands freezes the world for four ticks, so the tick the pool
# tops up on is no longer the player's own business. That it *does* top up,
# and never goes past five, is.
wait 250
assert mana == 5
assert mana_max == 5
