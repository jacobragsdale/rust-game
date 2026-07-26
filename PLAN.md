# Grand Plan: 2D Side-Scroller Adventure RPG in Rust

Merging the solid Rust/ggez foundation of this repo with the game design, assets, and
feature set of [SuperGame](https://github.com/jacobragsdale/SuperGame) (LÖVE2D/Lua),
and building it into a scalable adventure/RPG where new content — equipment, NPCs,
dialogue, weapons, potions — can be added without touching engine code.

> **This is the design document.** For what to build next and in what order, see
> [ROADMAP.md](ROADMAP.md) — it supersedes the phased roadmap in section 4 below,
> which predates Phases 0–3a shipping and is stale in the ways ROADMAP.md lists.
> The designs here (data schemas, architecture rules, story) are still current.

---

## 1. Where each codebase stands

### rust-game (this repo) — the engine seed

**Keep:**
- Working game loop with ggez 0.9, window/config bootstrapping from `config.toml`
- Swept AABB collision resolution with correct side detection (`Player::resolve_collisions`
  uses previous-position tests for top/bottom/left/right — this logic is sound and ports
  directly into a generalized physics system)
- Camera with clamped follow
- SQLite persistence via rusqlite (`score.rs`) — grows into the save system
- Platform-rider velocity handling (player inherits platform speed while grounded)

**Fix (these are the scaling walls):**
- **`Vec<Box<dyn Entity>>` + `downcast_ref` everywhere.** `game.rs` already has to
  downcast to `Player`/`Platform`/`SpikedBall` in six different places (collisions,
  cleanup, speed updates, colors, death checks). Every new entity type multiplies this.
  This is the #1 refactor target — replace with an ECS (components + systems).
- **Inconsistent physics units.** Platforms move in px/second scaled by delta time;
  the player moves in px/frame (`position += velocity` with per-frame gravity).
  Player physics is frame-rate dependent. Fix with a fixed simulation timestep.
- **`Game` is a god object** (~650 lines): world gen, scoring, color cycling, collision
  orchestration, camera, and UI drawing all live in one struct. Split into systems/scenes.
- **Meshes rebuilt every frame** (`Mesh::new_rectangle` per entity per draw). Fine for
  boxes, fatal for a sprite game. Replace with a sprite batch + texture atlas renderer.
- **No scene concept.** "Not started / playing / dead" is tracked with bools and ifs.
  An RPG needs a proper state stack (menu, level, pause, dialogue, inventory, dead).
- Minor: config is loaded twice (in `main.rs` and again in `Game::new`); `.unwrap()`s in
  config loading; `player_index == 0` assumption; `scores.db` should be gitignored.

### SuperGame (Lua/LÖVE) — the game design seed

**What it has that we're porting (the feature checklist):**
- **Gamestate stack** (hump.gamestate): main menu, level, pause overlay, death overlay
- **Tiled maps** (`.tmx` + `.tsx` tilesets, 32px castle/dungeon tiles) with map-driven
  collision (sti + bump)
- **bump.lua-style collision**: grid broad-phase, movement filters (`slide`, `cross`,
  `bounce`, `touch`) — a very good model to port
- **Rich movement set**: run, jump, double jump, wall jump, wall slide, crouch, slide,
  cliff hang + climb-up
- **Combat**: 3-hit ground melee combo, 3 air attacks (including down-slam), attack
  hitboxes via world queries, damage + knockback, i-frames (hurt cooldown)
- **Magic**: mana pool, cast timers/cooldowns, `ShockSpell` projectile with cost/damage
- **Weapon states**: sword drawn/sheathed with draw/sheath animations
- **Enemy AI**: `Knight` NPC — chase player, stop in range, attack, health bar, death
  animation, despawn; NPC base class with health/knockback/physics
- **Sprite animation**: player sheet with 20 animation states; knight with 7 sheets
  (idle/run/attack/death/jump/roll/shield)
- **HUD**: health + mana bars; **spawn queue** pattern for entities created mid-update
- **Story premise** (from the `main.lua` comment — worth keeping!): a medieval fighter
  is abducted by aliens after a solar flare destroyed their technology; they recruit
  pre-electricity warriors from across the galaxy. Magic exists; the aliens don't have
  it. Spells double as the upgrade/progression system.

**Lessons NOT to repeat (why the Lua version got hard to grow):**
- ~30 boolean+timer field pairs on the player (`canX`/`isX` + `xTimer`/`xTimerMax`).
  → Replace with a proper player state machine + a generic `Cooldown` component.
- Animation selection is a 100-line if/else chain with magic frame-table indices
  (`curFrame[14]` = "attack 6"). → Animations become named, data-driven clips.
- Knight animation frame counters are file-local globals — two knights on screen would
  share animation state. → Per-entity `AnimationState` component.
- Hard-coded damage, mana costs, frame rects sprinkled through code. → All content
  stats live in data files.

---

## 2. Architecture decision

**Recommendation: keep ggez as the platform layer, add the `hecs` ECS, and make all
content data-driven.**

Why not stay with `Box<dyn Entity>`: downcasting already dominates `game.rs` with 3
entity types; an RPG has dozens (player, enemies, NPCs, projectiles, pickups, chests,
doors, triggers, particles...). Composition over inheritance is the industry-standard
answer, and in Rust that means ECS.

Why `hecs` over full Bevy: this repo already has a working loop, input, rendering, and
you're clearly enjoying building the engine systems yourself (hand-rolled collision,
camera, timing). `hecs` is a small, boring, stable library — just `World`,
`spawn((CompA, CompB))`, and `world.query::<(&mut Pos, &Vel)>()`. ggez keeps doing
window/input/draw. You keep full control of the loop.

*The alternative:* migrate to **Bevy** and get ECS + asset server + animation + tilemap
plugins (`bevy_ecs_tilemap`/`bevy_ecs_ldtk`) + UI for free. It's the right call if you'd
rather build *the game* than *the engine*. It is, however, a full rewrite, a bigger API
surface to learn, and less "yours." Decide at the end of Phase 1 — the component/system
design below transfers to Bevy almost 1:1, so nothing in Phases 0–1 is wasted either way.

New dependencies (all small, well-maintained):

| Crate | Purpose |
|---|---|
| `hecs` | ECS world |
| `tiled` | Load the `.tmx`/`.tsx` maps from SuperGame |
| `serde` + `ron` | Data-driven content definitions (items, NPCs, animations, dialogue) |
| `anyhow`/`thiserror` | Stop `.unwrap()`ing config/asset errors |

### Target module layout

```
src/
  main.rs              // bootstrap only: config, window, scene stack, run
  app.rs               // owns SceneStack + shared Resources, impl EventHandler
  scenes/              // state stack (push/pop like hump.gamestate)
    mod.rs             //   trait Scene { update, draw, handle_input, transparent? }
    main_menu.rs
    level.rs           //   gameplay scene: owns hecs::World + runs systems
    pause.rs           //   transparent overlay
    dialogue.rs        //   transparent overlay, drives dialogue graphs
    inventory.rs       //   transparent overlay
    game_over.rs
  ecs/
    components.rs      // Position, Velocity, Collider, Sprite, AnimationState,
                       // Health, Mana, Facing, PlayerTag, Enemy, Npc, Interactable,
                       // Inventory, Equipment, MeleeAttack, Projectile, Lifetime,
                       // Cooldowns, KnockBack, Loot, ...
    events.rs          // Event queue: Damage, Death, Pickup, Interact, SpawnRequest
  systems/             // free functions: fn run(world, resources, dt)
    input.rs           // intent only: sets PlayerInput resource from keyboard
    player_控制 ⇒ player_state.rs  // state machine: Idle/Run/Jump/WallSlide/Attack/...
    physics.rs         // fixed-timestep integration + collision (port of bump model)
    combat.rs          // hitboxes, damage events, i-frames, knockback
    ai.rs              // behavior enum per enemy: Patrol / Chase / AttackRange
    animation.rs       // advances AnimationState from clip data
    camera.rs
    render.rs          // sprite batching, y-sorted draw, HUD
    spawn.rs           // drains SpawnRequest events (the Lua objectQueue, done right)
  physics/
    world.rs           // spatial hash broad-phase; move(entity, goal, filter)
                       //   filters: Slide | Cross | Touch (port bump.lua semantics)
    sweep.rs           // the existing swept-AABB code, generalized
  assets/
    mod.rs             // AssetCache: textures, fonts, sounds, data files
    sprite_sheet.rs    // atlas + named clips loaded from RON
    map.rs             // Tiled loader -> collision colliders + spawn points + tiles
  data/                // serde structs for the RON content schemas
    item.rs, npc.rs, dialogue.rs, level.rs, spell.rs
  save/
    mod.rs             // rusqlite: player progress, inventory, flags, settings
assets/                // (new top-level dir, copied from SuperGame + new)
  graphics/...         // playerSpriteSheet.png, knight/*.png, tile_castle.png, ...
  maps/castle.tmx
  data/
    animations/player.ron, knight.ron
    items/weapons.ron, potions.ron, armor.ron
    npcs/knight.ron, villager.ron
    dialogue/intro.ron
```

### The rules that keep it scalable

1. **Components are plain data. Systems are functions. No downcasting, ever.**
   A new entity type is a new *combination of existing components*, not a new struct
   implementing a trait.
2. **Content lives in data files, code only interprets.** Adding a potion = adding a
   RON entry with `{ name, sprite, effects: [Heal(50)] }`. Adding an enemy = RON stats
   + animation clip file + picking an AI behavior. No new systems unless there's a
   genuinely new *mechanic*.
3. **Events decouple systems.** Combat emits `Damage { target, amount, knockback }`;
   the health system applies it; the UI system shows floating text; the audio system
   plays a grunt. None of them know about each other.
4. **Fixed simulation timestep** (60 Hz accumulator inside `update`), render as fast as
   ggez wants. Kills the frame-rate-dependent physics bug permanently and makes
   gameplay deterministic/testable.
5. **Generic `Cooldowns` component** (`HashMap<Action, Timer>` or small fixed struct)
   instead of the Lua `canX/timerX` explosion.
6. **Logic stays `Context`-free.** Physics, combat, inventory, dialogue traversal take
   `&mut World` + plain data — so they get real unit tests (`cargo test`, no window).

---

## 3. Data-driven feature designs (the "easily add X" part)

**Items / weapons / potions / equipment** — one `ItemDef` schema in RON:
```ron
ItemDef(
  id: "iron_sword", name: "Iron Sword", sprite: "items/iron_sword",
  kind: Weapon( damage: 15, combo: ["slash1","slash2","heavy"], speed: 1.0 ),
)
ItemDef(
  id: "minor_health_potion", name: "Minor Health Potion", sprite: "items/potion_red",
  kind: Consumable( effects: [Heal(50)] ),
)
ItemDef(
  id: "knight_helm", name: "Knight Helm", sprite: "items/helm",
  kind: Equipment( slot: Head, modifiers: [MaxHealth(20)] ),
)
```
`Inventory` = `Vec<ItemStack>` component. `Equipment` = `HashMap<Slot, ItemId>`.
Stats are always computed as `base + sum(equipment modifiers)` — never mutated in place.

**NPCs / enemies** — `NpcDef` RON: stats, sprite/clip file, AI behavior
(`Hostile(Chase)`, `Friendly(dialogue: "blacksmith_intro")`), loot table. Maps place
them via Tiled object layers (`npc: "knight"` at x,y) — level design happens in Tiled,
not code.

**Dialogue** — a graph in RON: nodes with speaker + lines, choices with optional
`condition` (quest flag, item owned) and `effects` (set flag, give item, open shop).
The `Dialogue` scene is a transparent overlay that walks the graph. This structure is
exactly what quests hang off of later (quest = flags + dialogue conditions).

**Spells** — `SpellDef`: mana cost, cooldown, cast time, effect
(`Projectile{ speed, damage, sprite }`, later `Aoe`, `Buff`). The shock spell is the
first entry. Fits the story: spells are the upgrade currency.

**Animation** — `ClipSet` RON per sheet: `{ "run": (row/rects, fps, looping), ... }`.
The animation system just plays whatever clip the state machine names. Porting the
player = transcribing the 20 quad tables from `playerData.lua` into one RON file.

---

## 4. Phased roadmap

Each phase ends with a game that runs. Don't start a phase until the previous one's
exit criteria are met.

### Phase 0 — Housekeeping (half a day)
- Rename crate `rust-playground` → game name; add `scores.db` + `.idea/` to `.gitignore`
- `cargo fmt` + `clippy` clean; commit the currently-dirty working tree first
- Copy SuperGame `graphics/` + `maps/` into `assets/` (**check asset licenses** — the
  player sheet looks like the "Adventurer" pack and the knight sheets are an itch.io
  pack; both are typically free for non-commercial but verify before distributing)

### Phase 1 — ECS foundation refactor (the big one; no new features)
- Add `hecs`; convert Player/Platform/SpikedBall into components + systems
- Fixed timestep; unify all motion to px/sec
- Port bump-style physics world: spatial hash + `move_with_filter` (Slide/Cross/Touch),
  reusing the existing swept-AABB math
- Scene stack: MainMenu / Level / Pause / GameOver (replaces the bool flags)
- Event queue + spawn-request system
- **Exit criteria:** the current endless-runner plays identically (keep it forever as a
  minigame/"endless mode" — it's the architecture's canary), `game.rs` contains zero
  `downcast`, physics behaves the same at 30 and 144 fps, collision/inventory-free
  systems have unit tests.

### Phase 2 — Sprites, animation, Tiled levels
- AssetCache; sprite batch renderer (one draw batch per atlas, camera transform applied
  once — not per-entity mesh building)
- Animation system + RON clip sets; render the player with the Adventurer sheet
- Load `castle_level.tmx` with the `tiled` crate: tile layers → sprite batches,
  collision layer → static colliders, object layer → spawn points
- **Exit criteria:** walk/jump around the castle map as the animated knight-adventurer,
  camera clamped to map bounds.

### Phase 3 — Movement & combat (port SuperGame's feel)
- Player state machine: idle/run/jump/fall/double-jump/wall-slide/wall-jump/crouch/
  slide/cliff-hang (port mechanics one at a time; each is a state + a couple of
  world queries)
- Health/Mana components, i-frames, knockback; HUD bars
- Melee: attack states drive timed hitbox queries; 3-hit ground combo + air attacks
- Shock spell projectile via `SpellDef`
- Knight enemy: Patrol/Chase/Attack AI, health bar, death anim, despawn; spikes hazard
- **Exit criteria:** the SuperGame castle level plays in Rust — fight the knight, cast
  the spell, die, respawn.

### Phase 4 — RPG layer
- `ItemDef` pipeline: inventory component + inventory scene (grid UI), pickups/loot
  drops, consumables (potions), equipment slots with stat modifiers, weapon defs
  driving melee damage/combos
- Save system v2 (rusqlite): player progress, inventory, equipped items, flags
- **Exit criteria:** kill knight → loot drops → pick up → equip/drink → stats change →
  save → quit → load.

### Phase 5 — NPCs, dialogue, quests
- `Interactable` component + "press E" prompt; friendly NPC archetype
- Dialogue graph data format + dialogue overlay scene (portrait, lines, choices)
- Quest flags in save DB; dialogue conditions/effects read/write them; simple fetch
  quest as proof
- Optional: shop node type (buy/sell against inventory + gold)
- **Exit criteria:** talk to an NPC, accept a quest, complete it, get a reward, state
  survives save/load.

### Phase 6 — Content & polish (ongoing)
- Build out the story from the SuperGame premise (medieval world → alien recruitment →
  spell progression); levels in Tiled: village hub, castle, dungeon, "advanced world"
- Audio (music + SFX), particles, screen shake, transitions, main menu art
- Balancing pass driven entirely by editing RON files — if a balance change requires
  editing Rust, something leaked out of the data layer

---

## 5. Risks & notes

- **Biggest risk is Phase 1 scope creep.** Resist adding features during the refactor;
  the endless runner working on ECS *is* the deliverable.
- **ggez maintenance** is slow but the crate is stable; if it ever becomes a problem the
  renderer is isolated in `systems/render.rs` + `assets/`, and macroquad/Bevy are exits.
- **Asset licensing**: verify the itch.io packs (Adventurer sprite, knight pack, castle
  tiles) before any public distribution.
- `scores.db` and `config.toml` load from the CWD; switch to paths relative to the
  executable/`assets/` mount early (ggez's resource path handles this).
