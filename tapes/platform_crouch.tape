map maps/testbed_platform.ron

# The other half of the drop-through rule: on solid ground, down crouches
# rather than dropping. Without this, making down drop through platforms
# would quietly delete crouching everywhere.
#
# Which means this tape has to fail if `Avatar::crouching` stops being set.
# `grounded`, `!dead` and `y == 382` are all true of a player standing there
# doing nothing, so on their own they prove nothing about the feature this
# file is named after; `assert crouching` and `assert clip == crouch` are what
# actually hold it down.

wait 5
assert grounded
assert !on_one_way_only
assert !crouching
assert clip == idle

down 20
assert grounded
assert !dead
assert y == 382
# Crouching itself, and the animation that is the only thing on screen saying
# so — the collider does not shrink, so the clip is the whole tell.
assert crouching
assert clip == crouch
# Down on solid ground is neither of the other two things down can mean. The
# platform rule needs a platform underfoot and the slide needs a direction
# held; this floor is stone and the tape has pressed no direction at all.
expect no dropped_through
expect no slid

down 5
assert crouching

# Releasing it stands back up, or crouching would be a state the player enters
# and never leaves.
wait 5
assert !crouching
assert clip == idle

# ...and down+jump on solid ground is still an ordinary jump — the line this
# tape used to claim while pressing only `down`. A slide needs `dir != 0`, so
# with neither left nor right held this has to come out as the jump: off the
# floor, rising, and up through the plank overhead rather than down through
# anything.
down+jump 1
wait 6
assert !grounded
assert vy < 0
assert clip == jump
expect jumped == 1
expect no slid
expect no dropped_through
