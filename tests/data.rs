//! Cross-references every content file against every other one.
//!
//! Content is a graph of strings: a map names an entity kind, a kind names a
//! clip set, a clip names a sheet, an attack names a clip and chains to
//! another attack. Nothing in the type system connects the two ends of any of
//! those edges, so a one-character typo produces a knight that fails to spawn,
//! a sword that swings invisibly, or a combo that dead-ends — all of which
//! read as engine bugs and none of which any other test sees.
//!
//! Every check here follows one edge of that graph, and every failure names
//! the file it read, the id it could not resolve, and what would have made it
//! resolve.
//!
//! ## Content that does not exist yet
//!
//! This file is deliberately written ahead of most of the content it checks.
//! Checks for data a later ticket introduces are gated on that data's path
//! existing: they skip cleanly (printing why) while the directory is absent,
//! and start biting the day it lands, without anyone remembering to come
//! back. Each names its ticket. If a schema lands in a shape these checks
//! cannot read, the parse failure says so and the ticket that changed it owns
//! extending this file — which is the intended way for it to grow.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::de::{DeserializeOwned, IgnoredAny};
use serde::Deserialize;

use supergame::assets::{Assets, AttackTable, ItemKind, ItemTable, StatTable};
use supergame::ecs::spawn::KINDS;
use supergame::level::LevelData;

/// The clip set the player draws from. `Sim::load` loads exactly this name
/// alongside one set per entity kind, and the player is not in
/// [`KINDS`] — nothing places it, so nothing may name it in a map.
const PLAYER_SET: &str = "player";

// ---------------------------------------------------------------------------
// Finding content
// ---------------------------------------------------------------------------

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn assets_root() -> PathBuf {
    manifest().join("assets")
}

/// A path in the form a failure message should print it: repo-relative, so it
/// can be pasted into an editor and is identical on every machine.
fn rel(path: &Path) -> String {
    path.strip_prefix(manifest())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// `assets/<path>`, or `None` when that content does not exist yet.
///
/// Every check for a content type a later ticket introduces goes through
/// this. Gating on the path rather than on a hard-coded "not implemented yet"
/// flag is what makes those checks self-arming.
fn optional_content(path: &str) -> Option<PathBuf> {
    let full = assets_root().join(path);
    full.exists().then_some(full)
}

/// Say why a check did nothing. `cargo test -- --nocapture` shows these, and
/// a skip that is never explained is indistinguishable from a check that
/// silently stopped working.
fn skipping(check: &str, path: &str, ticket: &str) {
    eprintln!("data.rs: skipping {check} — assets/{path} does not exist yet; {ticket} creates it");
}

/// Every `.ron` file directly in a directory, sorted so failures are stable.
fn ron_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", rel(dir)))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    paths.sort();
    paths
}

fn map_paths() -> Vec<PathBuf> {
    ron_files(&assets_root().join("maps"))
}

/// Every shipped clip set, as (name, path). The name is what code and maps
/// use; the path is what a failure message has to print.
fn clip_sets() -> Vec<(String, PathBuf)> {
    ron_files(&assets_root().join("data/animations"))
        .into_iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().into_owned();
            Some((name, p))
        })
        .collect()
}

/// Parse a content file the way the game does.
///
/// `implicit_some` matters: content is written `sheet: "x"`, not
/// `sheet: Some("x")`, and a test that parsed it strictly would reject files
/// the game loads happily. See `assets.rs::load_ron`.
fn load_ron<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {}", rel(path)))?;
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", rel(path)))
}

fn report(problems: &[String]) {
    assert!(
        problems.is_empty(),
        "{} unresolved content reference(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Maps -> entity kinds -> clip sets
// ---------------------------------------------------------------------------

/// A map naming a kind `spawn::entity` does not know fails at load, which is
/// to say the first time someone opens that map — possibly long after the
/// typo, and never in CI if no tape visits it.
///
/// Doors are included on purpose: `ascii.rs` turns `Door { to }` into the
/// kind `door:<to>`, and nothing spawns those yet, so a map that places one
/// is broken today. When doors become real, either they join `KINDS` or this
/// check learns to route them elsewhere.
#[test]
fn every_entity_kind_a_map_places_is_a_kind_that_spawns() {
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for path in map_paths() {
        let level =
            LevelData::load(&path, &mut assets).unwrap_or_else(|e| panic!("{}: {e:#}", rel(&path)));

        for placement in &level.entities {
            if !KINDS.contains(&placement.kind.as_str()) {
                problems.push(format!(
                    "{}: places entity kind `{}`, which is not in `spawn::KINDS` \
                     (known kinds: {}). Add an arm to `spawn::entity` or fix the map.",
                    rel(&path),
                    placement.kind,
                    KINDS.join(", "),
                ));
            }
        }
    }

    report(&problems);
}

/// `Sim::load` loads `assets/data/animations/<kind>.ron` for every kind a map
/// places, so a kind advertised without art cannot be spawned at all.
#[test]
fn every_kind_has_a_clip_set_that_loads() {
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for name in animated_kinds() {
        let path = assets_root().join(format!("data/animations/{name}.ron"));
        if let Err(e) = assets.clip_set(name) {
            problems.push(format!(
                "`{name}` needs a clip set at {}, which does not load: {e:#}",
                rel(&path),
            ));
        }
    }

    report(&problems);
}

/// Everything that needs art or numbers of its own: the entity kinds a map
/// may place, plus the player, which no map places but `Sim::load` always
/// spawns.
fn animated_kinds() -> Vec<&'static str> {
    std::iter::once(PLAYER_SET)
        .chain(KINDS.iter().copied())
        .collect()
}

/// H-3 moves `Avatar::RUN_SPEED` and the `KNIGHT_*` consts into
/// `assets/data/stats.ron`, one block per kind including the player. Until
/// then there is no table to check against.
#[test]
fn every_kind_has_a_stat_block() {
    /// Only the ids matter here; whatever fields a `StatBlock` grows are H-3's
    /// business, and a test that listed them would need editing every time one
    /// was tuned.
    #[derive(Deserialize)]
    #[serde(rename = "Stats")]
    struct StatTable(HashMap<String, IgnoredAny>);

    let Some(path) = optional_content("data/stats.ron") else {
        skipping("stat block check", "data/stats.ron", "ticket H-3");
        return;
    };

    let table: StatTable = load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"));
    let mut problems: Vec<String> = Vec::new();

    for kind in animated_kinds() {
        if !table.0.contains_key(kind) {
            problems.push(format!(
                "{}: no stat block for `{kind}`, which `spawn` will ask for",
                rel(&path),
            ));
        }
    }

    report(&problems);
}

// ---------------------------------------------------------------------------
// Clips -> sheets
// ---------------------------------------------------------------------------

/// A frame outside its sheet draws garbage or nothing, and a sheet that is
/// not on disk stops the game at load.
///
/// `tests/assets.rs` checks the same bound from the art side, where it sits
/// alongside the tile/collision checks; it is repeated here because this file
/// is the one place a content author is told to look when an id does not
/// resolve, and a cross-reference file with a hole in it is worse than a
/// duplicated assertion.
#[test]
fn every_clip_frame_lands_on_a_sheet_that_exists() {
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();
    // Pixel size per sheet: the player's twelve clips share one atlas, and
    // decoding a PNG per clip is the slow way to learn the same number. The
    // grid is derived per clip rather than cached, because two clips may read
    // the same sheet at different cell sizes.
    let mut sheets: HashMap<String, (u32, u32)> = HashMap::new();

    for (set_name, set_path) in clip_sets() {
        let set = match assets.clip_set(&set_name) {
            Ok(set) => set,
            Err(e) => {
                problems.push(format!("{}: does not load: {e:#}", rel(&set_path)));
                continue;
            }
        };

        // Sorted, so two runs blame the same clip first.
        let clips: BTreeMap<&String, _> = set.clips.iter().collect();
        for (clip_name, clip) in clips {
            let sheet = set.sheet_of(clip);
            let (fw, fh) = set.frame_size_of(clip);
            let sheet_path = assets_root().join(format!("graphics/{sheet}.png"));

            let (sheet_w, sheet_h) = match sheets.get(sheet) {
                Some(size) => *size,
                None => {
                    if !sheet_path.exists() {
                        problems.push(format!(
                            "{}: clip `{clip_name}` names sheet `{sheet}`, but {} does not exist",
                            rel(&set_path),
                            rel(&sheet_path),
                        ));
                        continue;
                    }
                    match assets.decode_image(sheet, None) {
                        Ok(image) => {
                            let size = image.dimensions();
                            sheets.insert(sheet.to_string(), size);
                            size
                        }
                        Err(e) => {
                            problems.push(format!(
                                "{}: clip `{clip_name}` names sheet `{sheet}`, \
                                 which does not decode: {e:#}",
                                rel(&set_path),
                            ));
                            continue;
                        }
                    }
                }
            };

            let cols = (sheet_w as f32 / fw) as u32;
            let rows = (sheet_h as f32 / fh) as u32;
            for &(col, row) in &clip.frames {
                if col >= cols || row >= rows {
                    problems.push(format!(
                        "{}: clip `{clip_name}` uses frame ({col}, {row}), outside the \
                         {cols}x{rows} grid of {} at {fw}x{fh} per cell",
                        rel(&set_path),
                        rel(&sheet_path),
                    ));
                }
            }
        }
    }

    report(&problems);
}

/// Every image a data file names, whatever the schema around it.
///
/// Clip sheets are already covered structurally above; this catches the same
/// mistake in files no Rust type has been written for yet — C-1's
/// `spells.ron` names a projectile sheet inside an effect enum, and this sees
/// it the day it lands with no change here.
#[test]
fn every_image_a_data_file_names_exists() {
    let mut problems: Vec<String> = Vec::new();

    for path in data_files() {
        let text = strip_comments(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", rel(&path))),
        );

        for field in ["sheet", "image"] {
            for name in quoted_after(&text, &format!("{field}:")) {
                let image = assets_root().join(format!("graphics/{name}.png"));
                if !image.exists() {
                    problems.push(format!(
                        "{}: {field} `{name}`, but {} does not exist",
                        rel(&path),
                        rel(&image),
                    ));
                }
            }
        }
    }

    report(&problems);
}

/// Every `.ron` file under `assets/data`, at any depth. Item, dialogue and
/// spell files land in subdirectories, and the scanning checks are supposed
/// to pick those up without being told about them.
fn data_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "ron") {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(&assets_root().join("data"), &mut out);
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Attacks -> clips, and attacks -> attacks
// ---------------------------------------------------------------------------

/// The attack ids the game names, and the clip set whoever uses them animates
/// from. Everything else in the table is reached from here by `chain`.
///
/// Read out of `assets/data/stats.ron` rather than hand-maintained here: since
/// H-3, a kind's opening attack (and the player's air attack) is a field on
/// its stat block, so a new attacker arrives in this list the moment it has
/// numbers. [`every_attack_in_the_table_is_reachable_by_something`] is what
/// catches an attack nothing can reach from these entry points.
fn attack_entry_points() -> Vec<(String, String)> {
    let table = stat_table();
    let mut entries: Vec<(String, String)> = Vec::new();

    for kind in animated_kinds() {
        let block = table
            .get(kind)
            .unwrap_or_else(|e| panic!("{}: {e:#}", rel(&assets_root().join("data/stats.ron"))));
        entries.push((kind.to_string(), block.attack.clone()));
        if let Some(avatar) = &block.avatar {
            entries.push((kind.to_string(), avatar.air_attack.clone()));
            entries.push((kind.to_string(), avatar.plunge_attack.clone()));
        }
    }

    // A weapon's `combo` is a second way into the attack table, and it is the
    // one a content author reaches for when adding a sword that swings
    // differently. Whoever can equip a weapon can perform its combo, and today
    // that is the player — when an NPC can wear things, this is where that is
    // said. Without it a typo in a weapon's combo would be a sword whose first
    // swing does nothing, with every other check in this file passing.
    let items = item_table();
    for id in items.ids() {
        let Some(ItemKind::Weapon { combo, .. }) = items.get(id).map(|def| &def.kind) else {
            continue;
        };
        for attack in combo {
            entries.push((PLAYER_SET.to_string(), attack.clone()));
        }
    }

    entries
}

/// The shipped item table, read the way the game reads it.
fn item_table() -> std::sync::Arc<ItemTable> {
    Assets::new()
        .items()
        .unwrap_or_else(|e| panic!("{}: {e:#}", rel(&assets_root().join("data/items"))))
}

fn stat_table() -> StatTable {
    let path = assets_root().join("data/stats.ron");
    load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"))
}

/// Clip set -> every attack an entity drawing from it can perform, following
/// `chain` to its end. Ids the table does not define are kept in the result
/// rather than dropped, so a broken chain is reported as the dangling id it
/// is rather than silently becoming a shorter combo.
fn reachable_attacks(table: &AttackTable) -> BTreeMap<String, BTreeSet<String>> {
    let mut reachable: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (set, entry) in attack_entry_points() {
        let mut queue = VecDeque::from([entry]);
        while let Some(id) = queue.pop_front() {
            if !reachable.entry(set.clone()).or_default().insert(id.clone()) {
                continue;
            }
            if let Some(next) = table.get(&id).and_then(|def| def.chain.clone()) {
                queue.push_back(next);
            }
        }
    }

    reachable
}

fn attack_table() -> AttackTable {
    let path = assets_root().join("data/attacks.ron");
    load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"))
}

/// `Avatar::ATTACK` and friends are the seam between code and the attack
/// table: a rename in the RON file compiles perfectly and produces a player
/// whose attack button does nothing.
#[test]
fn every_attack_id_named_in_code_exists_in_the_table() {
    let table = attack_table();
    let path = assets_root().join("data/attacks.ron");
    let mut problems: Vec<String> = Vec::new();

    for (set, id) in attack_entry_points() {
        if table.get(&id).is_none() {
            problems.push(format!(
                "{}: `{set}` performs attack `{id}`, which the table does not define \
                 (defined: {})",
                rel(&path),
                sorted_ids(&table).join(", "),
            ));
        }
    }

    report(&problems);
}

/// The combo is three entries and a pointer each; a dangling pointer turns
/// the finisher into a swing that ends the chain early, which reads as a
/// timing bug.
#[test]
fn every_attack_chain_target_exists() {
    let table = attack_table();
    let path = assets_root().join("data/attacks.ron");
    let mut problems: Vec<String> = Vec::new();

    for id in sorted_ids(&table) {
        let Some(chain) = table.get(&id).and_then(|def| def.chain.clone()) else {
            continue;
        };
        if table.get(&chain).is_none() {
            problems.push(format!(
                "{}: attack `{id}` chains into `{chain}`, which the table does not define \
                 (defined: {})",
                rel(&path),
                sorted_ids(&table).join(", "),
            ));
        }
    }

    report(&problems);
}

/// An attack plays its clip on the attacker, out of that attacker's own clip
/// set — so the same attack shared by two entities has to exist in both.
/// A missing clip leaves the attacker committed for the full duration with
/// the sprite frozen on whatever it was doing.
#[test]
fn every_attack_clip_exists_in_the_clip_set_of_every_entity_that_uses_it() {
    let table = attack_table();
    let attacks_path = assets_root().join("data/attacks.ron");
    let mut assets = Assets::new();
    let mut problems: Vec<String> = Vec::new();

    for (set_name, ids) in reachable_attacks(&table) {
        let set_path = assets_root().join(format!("data/animations/{set_name}.ron"));
        let set = match assets.clip_set(&set_name) {
            Ok(set) => set,
            Err(e) => {
                problems.push(format!("{}: does not load: {e:#}", rel(&set_path)));
                continue;
            }
        };

        for id in ids {
            let Some(def) = table.get(&id) else {
                continue; // reported by the two checks above
            };
            if set.clip(&def.clip).is_none() {
                let mut known: Vec<&str> = set.clips.keys().map(String::as_str).collect();
                known.sort_unstable();
                problems.push(format!(
                    "{}: attack `{id}` plays clip `{}` on `{set_name}`, but {} defines no \
                     such clip (it has: {})",
                    rel(&attacks_path),
                    def.clip,
                    rel(&set_path),
                    known.join(", "),
                ));
            }
        }
    }

    report(&problems);
}

/// An attack nobody can reach is either dead content or, far more likely, an
/// attacker this file has not been told about — in which case the clip check
/// above is quietly not covering it. Failing here is how that gets noticed.
#[test]
fn every_attack_in_the_table_is_reachable_by_something() {
    let table = attack_table();
    let path = assets_root().join("data/attacks.ron");
    let reachable: BTreeSet<String> = reachable_attacks(&table).into_values().flatten().collect();

    let problems: Vec<String> = sorted_ids(&table)
        .into_iter()
        .filter(|id| !reachable.contains(id))
        .map(|id| {
            format!(
                "{}: attack `{id}` is defined but no entity can reach it — add its \
                 attacker to `attack_entry_points` in tests/data.rs, or delete the attack",
                rel(&path),
            )
        })
        .collect();

    report(&problems);
}

fn sorted_ids(table: &AttackTable) -> Vec<String> {
    let mut ids: Vec<String> = table.0.keys().cloned().collect();
    ids.sort();
    ids
}

// ---------------------------------------------------------------------------
// Content that later tickets introduce
// ---------------------------------------------------------------------------

/// C-1 writes `assets/data/spells.ron`, whose `clip` is played on the caster.
/// Only the player casts for now; when something else does, give this the
/// same clip-set treatment the attack check has.
///
/// A projectile's art is a clip on the *caster's* set too, rather than a raw
/// sheet — same reason an attack names a clip and not a PNG — so both ends of
/// a spell are checked here, and both fail the same way if the id is wrong: a
/// spell that animates nothing, or a bolt that flies invisibly.
#[test]
fn every_spell_clip_exists_on_its_caster() {
    /// Deliberately minimal: serde ignores the fields it is not told about,
    /// so cost, cooldown and the rest of the effect can change shape without
    /// touching this.
    #[derive(Deserialize)]
    #[serde(rename = "Spells")]
    struct SpellTable(HashMap<String, SpellShape>);

    #[derive(Deserialize)]
    #[serde(rename = "SpellDef")]
    struct SpellShape {
        clip: String,
        effect: EffectShape,
    }

    #[derive(Deserialize)]
    enum EffectShape {
        Projectile { clip: String },
    }

    impl SpellShape {
        /// Every clip this spell asks its caster's set for, and what each one
        /// is for, so a failure says which end is broken.
        fn clips(&self) -> Vec<(&'static str, &str)> {
            let EffectShape::Projectile { clip } = &self.effect;
            vec![("casts", self.clip.as_str()), ("throws", clip.as_str())]
        }
    }

    let Some(path) = optional_content("data/spells.ron") else {
        skipping("spell checks", "data/spells.ron", "ticket C-1");
        return;
    };

    let table: SpellTable = load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"));
    let set_path = assets_root().join(format!("data/animations/{PLAYER_SET}.ron"));
    let set = Assets::new()
        .clip_set(PLAYER_SET)
        .unwrap_or_else(|e| panic!("{}: {e:#}", rel(&set_path)));

    let mut spells: Vec<(&String, &SpellShape)> = table.0.iter().collect();
    spells.sort_by_key(|(id, _)| *id);

    let problems: Vec<String> = spells
        .into_iter()
        .flat_map(|(id, spell)| spell.clips().into_iter().map(move |named| (id, named)))
        .filter(|(_, (_, clip))| set.clip(clip).is_none())
        .map(|(id, (verb, clip))| {
            format!(
                "{}: spell `{id}` {verb} clip `{clip}`, but {} defines no such clip",
                rel(&path),
                rel(&set_path),
            )
        })
        .collect();

    report(&problems);
}

/// The other direction: a kind whose stat block names a spell the table does
/// not define presses the key and nothing happens, forever. The same seam
/// [`every_attack_id_named_in_code_exists_in_the_table`] covers for swings.
#[test]
fn every_spell_a_kind_names_exists_in_the_table() {
    #[derive(Deserialize)]
    #[serde(rename = "Spells")]
    struct SpellTable(HashMap<String, IgnoredAny>);

    let Some(path) = optional_content("data/spells.ron") else {
        skipping("spell id checks", "data/spells.ron", "ticket C-1");
        return;
    };

    let table: SpellTable = load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"));
    let stats = stat_table();
    let mut known: Vec<&str> = table.0.keys().map(String::as_str).collect();
    known.sort_unstable();

    let problems: Vec<String> = animated_kinds()
        .into_iter()
        .filter_map(|kind| {
            let block = stats.get(kind).ok()?;
            let spell = block.spell.clone()?;
            (!table.0.contains_key(&spell)).then(|| {
                format!(
                    "{}: `{kind}` casts spell `{spell}`, which the table does not define \
                     (defined: {})",
                    rel(&path),
                    known.join(", "),
                )
            })
        })
        .collect();

    report(&problems);
}

/// I-2 writes `assets/data/items/*.ron`. Ids are the currency of every later
/// system — loot, dialogue effects, quest rewards — so two items answering to
/// the same id is a bug that only shows up as the wrong thing being given.
///
/// Item *sprites* are not checked: I-2 explicitly allows a `sprite` naming
/// art that does not exist yet, drawn as a coloured quad, so that content
/// does not have to be rewritten when the art arrives.
#[test]
fn every_item_id_is_defined_once() {
    let Some(dir) = optional_content("data/items") else {
        skipping("item id checks", "data/items", "ticket I-2");
        return;
    };

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();

    for (path, item) in item_defs(&dir) {
        if let Some(first) = seen.insert(item.id.clone(), rel(&path)) {
            problems.push(format!(
                "{}: item id `{}` is already defined in {first} — ids are how every \
                 other file names an item, so they have to be unique",
                rel(&path),
                item.id,
            ));
        }
    }

    report(&problems);
}

/// Every item id any other content file names has to resolve, or a quest
/// hands out nothing and a dialogue choice is invisible forever.
///
/// The references are found textually rather than by deserializing the
/// effect and condition enums, on purpose: D-2 ships `HasItem`/`GiveItem`/
/// `TakeItem` and Q-2 is expected to add more, and a closed enum mirrored in
/// a test would reject the whole file the first time a variant was added —
/// turning a cross-reference check into a maintenance tax. Q-2's "every quest
/// reward is a real item id" is this check: rewards are `GiveItem` effects in
/// a dialogue graph, not a separate file.
#[test]
fn every_item_id_referenced_by_content_exists() {
    // A set, not a list: a graph that gates a choice on an item and then takes
    // it names that item three times, and three copies of one message is
    // noise rather than information.
    let mut references: BTreeSet<(PathBuf, String)> = BTreeSet::new();
    for path in data_files().into_iter().chain(map_paths()) {
        let text = strip_comments(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", rel(&path))),
        );
        // Constructor arguments (dialogue effects and conditions), plus the
        // `item:` field a loot table names its drops with. Both are found
        // textually rather than by deserializing, for the reason above: the
        // enums either side of this grow, and a closed mirror of them in a
        // test would reject the whole file the first time a variant landed.
        for ctor in ["HasItem(", "GiveItem(", "TakeItem(", "item:"] {
            for id in quoted_after(&text, ctor) {
                references.insert((path.clone(), id));
            }
        }
    }

    let dir = optional_content("data/items");
    if dir.is_none() && references.is_empty() {
        skipping("item reference checks", "data/items", "ticket I-2");
        return;
    }

    let known: BTreeSet<String> = dir
        .iter()
        .flat_map(|dir| item_defs(dir))
        .map(|(_, item)| item.id)
        .collect();

    let problems: Vec<String> = references
        .into_iter()
        .filter(|(_, id)| !known.contains(id))
        .map(|(path, id)| {
            format!(
                "{}: names item `{id}`, which no file in assets/data/items defines \
                 (defined: {})",
                rel(&path),
                if known.is_empty() {
                    "nothing".to_string()
                } else {
                    known.iter().cloned().collect::<Vec<_>>().join(", ")
                },
            )
        })
        .collect();

    report(&problems);
}

/// The other direction from [`every_item_id_referenced_by_content_exists`], and
/// stronger where it applies: a loot table is a *typed* reference, so it can be
/// checked through the same `StatTable` and `ItemTable` the game loads rather
/// than by scanning text. A knight whose table names an item nobody defines
/// drops nothing, forever, with no error anywhere.
#[test]
fn every_item_a_loot_table_names_exists() {
    let Some(items_dir) = optional_content("data/items") else {
        skipping("loot table checks", "data/items", "ticket I-2");
        return;
    };
    let _ = items_dir;

    let stats = stat_table();
    let items = item_table();
    let stats_path = assets_root().join("data/stats.ron");
    let known = items.ids().join(", ");
    let mut problems: Vec<String> = Vec::new();

    for kind in animated_kinds() {
        let Ok(block) = stats.get(kind) else {
            continue; // reported by `every_kind_has_a_stat_block`
        };
        for drop in &block.loot {
            if items.get(&drop.item).is_none() {
                problems.push(format!(
                    "{}: `{kind}` drops item `{}`, which no file in assets/data/items \
                     defines (defined: {known})",
                    rel(&stats_path),
                    drop.item,
                ));
            }
            if !(0.0..=1.0).contains(&drop.chance) {
                problems.push(format!(
                    "{}: `{kind}` drops `{}` with chance {}, which is not a probability",
                    rel(&stats_path),
                    drop.item,
                    drop.chance,
                ));
            }
        }
    }

    report(&problems);
}

/// A weapon's `combo` is a list of attack ids, so it is a reference like any
/// other. The clip and chain checks above already cover the ids themselves
/// through [`attack_entry_points`]; this is the direct message, naming the
/// weapon rather than "the player".
#[test]
fn every_attack_a_weapon_names_exists() {
    let Some(dir) = optional_content("data/items") else {
        skipping("weapon combo checks", "data/items", "ticket I-2");
        return;
    };

    let items = item_table();
    let attacks = attack_table();
    let mut problems: Vec<String> = Vec::new();

    for id in items.ids() {
        let Some(ItemKind::Weapon { combo, .. }) = items.get(id).map(|def| &def.kind) else {
            continue;
        };
        if combo.is_empty() {
            problems.push(format!(
                "{}: weapon `{id}` has an empty combo, so equipping it would leave the \
                 player with no opening attack at all",
                rel(&dir),
            ));
        }
        for attack in combo {
            if attacks.get(attack).is_none() {
                problems.push(format!(
                    "{}: weapon `{id}` swings attack `{attack}`, which {} does not define \
                     (defined: {})",
                    rel(&dir),
                    rel(&assets_root().join("data/attacks.ron")),
                    sorted_ids(&attacks).join(", "),
                ));
            }
        }
    }

    report(&problems);
}

#[derive(Deserialize)]
#[serde(rename = "ItemDef")]
struct ItemShape {
    id: String,
}

/// Every item definition under `dir`. A file may hold one `ItemDef` or a list
/// of them; I-2's schema shows both shapes, so accept either rather than
/// forcing content into a layout this test happens to prefer.
fn item_defs(dir: &Path) -> Vec<(PathBuf, ItemShape)> {
    let mut defs = Vec::new();
    for path in ron_files(dir) {
        if let Ok(item) = load_ron::<ItemShape>(&path) {
            defs.push((path, item));
            continue;
        }
        match load_ron::<Vec<ItemShape>>(&path) {
            Ok(items) => defs.extend(items.into_iter().map(|item| (path.clone(), item))),
            Err(e) => panic!(
                "{}: is neither an `ItemDef(...)` nor a list of them: {e:#}",
                rel(&path)
            ),
        }
    }
    defs
}

/// D-2 writes `assets/data/dialogue/*.ron`. Three ways a graph can be wrong
/// without looking wrong: `start` names a node that is not there, a choice
/// points at a node that is not there, and a node nobody points at — the last
/// being writing that ships and is never read, which no runtime error will
/// ever tell you about.
#[test]
fn every_dialogue_graph_is_connected() {
    #[derive(Deserialize)]
    #[serde(rename = "Dialogue")]
    struct Graph {
        id: String,
        start: String,
        nodes: HashMap<String, Node>,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Node")]
    struct Node {
        #[serde(default)]
        choices: Vec<Choice>,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Choice")]
    struct Choice {
        /// `None` ends the conversation.
        #[serde(default)]
        next: Option<String>,
    }

    let Some(dir) = optional_content("data/dialogue") else {
        skipping("dialogue graph checks", "data/dialogue", "ticket D-2");
        return;
    };

    let mut problems: Vec<String> = Vec::new();

    for path in ron_files(&dir) {
        let graph: Graph = load_ron(&path).unwrap_or_else(|e| panic!("{e:#}"));
        let id = &graph.id;

        let mut node_ids: Vec<&String> = graph.nodes.keys().collect();
        node_ids.sort();
        let known = node_ids
            .iter()
            .map(|n| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        if !graph.nodes.contains_key(&graph.start) {
            problems.push(format!(
                "{}: graph `{id}` starts at node `{}`, which it does not define \
                 (nodes: {known})",
                rel(&path),
                graph.start,
            ));
            continue;
        }

        // Walk from `start`, reporting dangling targets and collecting what
        // a player can actually reach.
        let mut reached: BTreeSet<&str> = BTreeSet::new();
        let mut queue = VecDeque::from([graph.start.as_str()]);
        while let Some(node_id) = queue.pop_front() {
            if !reached.insert(node_id) {
                continue;
            }
            let Some(node) = graph.nodes.get(node_id) else {
                continue;
            };
            for choice in &node.choices {
                let Some(next) = &choice.next else {
                    continue;
                };
                match graph.nodes.get_key_value(next) {
                    Some((next, _)) => queue.push_back(next.as_str()),
                    None => problems.push(format!(
                        "{}: graph `{id}`, node `{node_id}`: a choice leads to `{next}`, \
                         which the graph does not define (nodes: {known})",
                        rel(&path),
                    )),
                }
            }
        }

        for node_id in node_ids {
            if !reached.contains(node_id.as_str()) {
                problems.push(format!(
                    "{}: graph `{id}` defines node `{node_id}`, which nothing reaches \
                     from `{}` — written and never read",
                    rel(&path),
                    graph.start,
                ));
            }
        }
    }

    report(&problems);
}

// ---------------------------------------------------------------------------
// Reading references out of files no Rust type exists for yet
// ---------------------------------------------------------------------------

/// Drop `//` line comments, leaving string literals alone.
///
/// Content files here carry as much prose as data, and a commented-out
/// `sheet: "old_art"` is not a reference to anything.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut cut = line.len();
        let bytes = line.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if escaped {
                escaped = false;
            } else if in_string && b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = !in_string;
            } else if !in_string && b == b'/' && bytes.get(i + 1) == Some(&b'/') {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// Every string literal that immediately follows `needle`, e.g. the `"x"` in
/// `sheet: "x"` for the needle `sheet:`, or in `GiveItem("x", 1)` for the
/// needle `GiveItem(`.
///
/// Anything else between the needle and the quote means this was not the
/// construct we were looking for, and is skipped rather than guessed at.
fn quoted_after(text: &str, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;

    while let Some(at) = rest.find(needle) {
        let after = rest[at + needle.len()..].trim_start();
        if let Some(body) = after.strip_prefix('"') {
            if let Some(end) = body.find('"') {
                found.push(body[..end].to_string());
            }
        }
        rest = &rest[at + needle.len()..];
    }

    found
}

/// The scanners are the one piece of this file with logic of its own, so they
/// get tests of their own.
mod scanning {
    use super::{quoted_after, strip_comments};

    #[test]
    fn a_commented_out_reference_is_not_a_reference() {
        let text = strip_comments("// sheet: \"old\"\nsheet: \"new\" // was \"old\"\n");
        assert_eq!(quoted_after(&text, "sheet:"), vec!["new".to_string()]);
    }

    #[test]
    fn a_url_inside_a_string_is_not_a_comment() {
        let text = strip_comments("note: \"see http://x/y\"\nsheet: \"art\"\n");
        assert_eq!(quoted_after(&text, "sheet:"), vec!["art".to_string()]);
    }

    #[test]
    fn constructor_arguments_are_found_and_other_fields_are_not() {
        let text = "effects: [GiveItem(\"amulet\", 1), SetFlag(\"quest.x\", 2)]";
        assert_eq!(quoted_after(text, "GiveItem("), vec!["amulet".to_string()]);
        assert!(quoted_after(text, "TakeItem(").is_empty());
    }

    #[test]
    fn a_field_holding_something_other_than_a_string_is_skipped() {
        assert!(quoted_after("sheet: None", "sheet:").is_empty());
    }
}
