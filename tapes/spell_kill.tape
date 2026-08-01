map maps/testbed_arena.ron

# Kill the arena knight with the shock spell and nothing else.
#
# The sword is never drawn, so every point of damage here came out of the mana
# pool: six hit points at two a bolt is three connections, and the pool only
# pays for two of them — the third has to be waited for. That is the whole
# shape of the spell, and it is why this tape is mostly waiting.
#
# Outcome assertions only, as with `knight_kill.tape`. Which tick a bolt lands
# on falls out of the knight's chase speed, the bolt's speed, knockback,
# i-frames, the cooldown and hitstop all at once; `traces/spell_kill.jsonl`
# holds the tick-by-tick detail so a timing change still shows up, just not as
# an unreadable failure in this file.
#
#   room    x 32..288, floor top y=128
#   player  spawns x=70 facing right, 5 mana
#   knight  spawns x=229 with 6 hp
#   shock   2 damage, 2 mana, 45-tick cooldown; regen one point per 50 ticks

assert mana == 5
assert knight.0.hp == 6
assert !knight.0.dead
expect no spell_cast
expect no damaged

# The player never moves: facing right from spawn is already down the room.
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

# A dead knight, and it cost nothing but the pool.
assert knight.0.dead
assert knight.0.hp <= 0
expect knight.died == 1
expect no player.damaged
assert hp == 5
assert !dead

# The sword stayed sheathed — nothing here is a melee kill in disguise.
expect no player_slash1.attacked
expect no player_air.attacked
expect no player_plunge.attacked
expect shock.spell_cast >= 3
expect knight.damaged == 3

# The bolts are gone, and the corpse stays where it fell: a projectile may be
# despawned because nothing addresses one, and an NPC may not because
# `knight.0` has to keep meaning the same knight.
wait 120
assert projectiles == 0
assert knight.0.dead
assert knight.0.grounded
assert knight.0.clip == death
