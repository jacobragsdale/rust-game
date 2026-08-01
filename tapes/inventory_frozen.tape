map maps/testbed.ron

# The modal contract, measured: opening the inventory stops the world dead,
# the tick counter keeps going, and a detour through the screen costs the run
# exactly nothing.
#
# The numbers below are not guesses. They come from the same tape with the
# detour deleted — `wait 1 / right 22 / right 22` — which ends at x = 205.0
# with vx = 200 on the `run` clip at frame 2. That this tape reaches the same
# pixel after ninety ticks with the screen up is the whole assertion, and it is
# the reason the mode dispatch does not run a single world system while modal:
# not "the world moves slowly", not "the world moves and we hide it", but
# nothing at all.
#
# `steps_advance_the_tick_counter_in_every_mode` and
# `a_detour_through_the_inventory_costs_the_world_nothing` make the same two
# claims in Rust, where the comparison can be exhaustive over every probe field
# rather than over the handful written out here. This tape is what makes them
# true of the shipped input path as well — real keys, through the real action
# table, into the real `Sim::step`.
#
#   spawn  world (70, 350), feet on the floor at y=384
#   pit    spikes from x=256, which is why the run stops well short of it

assert mode == playing
assert inventory_count == 0
expect no mode_changed

wait 1
right 22

assert mode == playing
assert x == 131.66669
assert vx == 200

# One press, and the world is already stopped: the tick it happens on is
# itself a frozen tick, which is what makes the arithmetic below come out even.
inventory 1
assert mode == inventory
assert x == 131.66669
assert vx == 200
assert clip == run
expect inventory.mode_changed == 1

# Ninety ticks of nothing happening. Position, velocity, the animation frame
# and the physics all exactly where they were — the sprite does not even
# advance, because `animation::advance` is a world system and no world systems
# run here.
wait 90
assert mode == inventory
assert x == 131.66669
assert vx == 200
assert grounded
assert clip == run
assert frame == 4

# The tick counter does keep going, exactly as it does through hitstop, so a
# trace stays aligned with wall-clock time and the pause is visible as the run
# of identical frames it is.
assert tick == 114

# Out again, and the run continues from the velocity it still had.
inventory 1
assert mode == playing
expect playing.mode_changed == 1
expect mode_changed == 2

right 22

# The same pixel, the same speed, the same frame of the same clip as the run
# that never opened the screen at all.
assert x == 204.99991
assert vx == 200
assert clip == run
assert frame == 2
assert grounded
expect no died
