//! Standing next to someone, and talking to them.
//!
//! **Why every line of this is `Sim` and none of it is a scene.** Dialogue is
//! the most convincing UI in the game and it is not UI at all. A choice takes
//! an item out of the bag, puts one in, heals, and sets a quest flag — every
//! one of which decides whether the next fight is winnable and what the next
//! conversation offers. Put the current node and the selection index in
//! `scenes/` and the largest system in the game becomes untestable in one
//! commit: no tape can press a key that only a `ggez::Context` can see, so
//! "does the gated branch appear once you have the amulet?" would be a question
//! only a human with a keyboard could answer. So the graph, the node, the
//! filtered choices, the selection and every consequence of `confirm` live
//! here, driven by the same [`Action`]s a player presses, and
//! [`crate::scenes::dialogue`] draws the result and decides nothing.
//!
//! This is [`crate::systems::inventory`]'s arrangement, deliberately unchanged:
//! M4 established it on the simpler of the two modal features so that this one
//! would be a copy rather than a design.
//!
//! ## Failing choices are hidden, not greyed out
//!
//! A choice whose `condition` does not hold is **absent** from the list. It is
//! not shown dimmed, and there is no row you can select and be refused.
//!
//! Three reasons, in the order they matter:
//!
//! 1. *It is what a quest is for.* A greyed-out "I have your amulet." announces
//!    that an amulet exists, that this person wants it, and that you have not
//!    found it — before the player has been told any of that. Discovery is the
//!    content; a disabled row spoils it on the first line of the first
//!    conversation.
//! 2. *It makes the state observable.* Because the filtered list is the only
//!    list, `dialogue_choices` is a count of what a player can actually do and
//!    `dialogue_selection` indexes into it. If refused choices were listed, a
//!    tape asserting "the branch is not available yet" would have to assert
//!    "the branch is listed but does nothing", which needs a probe for the
//!    refusal as well — and the assertion that matters, *the count changed when
//!    I picked the item up*, would not exist at all.
//! 3. *It is the reversible choice.* The full choice list is still in the
//!    graph, so a scene that wanted to draw the locked ones dimmed could ask
//!    for them later without any of this changing. Going the other way — from
//!    "everything is listed" back to "only what is legal" — would move the
//!    meaning of every selection index already recorded in a trace.
//!
//! `a_choice_whose_condition_fails_is_absent_rather_than_inert` is the test.
//!
//! ## Villagers, and who may kill them
//!
//! A villager is on [`Team::Player`] and does carry [`Health`]. The
//! consequence is that **the player cannot kill the quest giver**: friendly
//! fire is already impossible in [`crate::systems::combat::resolve`], which
//! skips a target on the attacker's own team, so this needs no special case
//! anywhere and there is no check that could later be forgotten. Omitting
//! `Health` instead would have worked too, and worse — `npc::think`,
//! `animation::select_patrol_clip` and `Sim::npc_probes` all query for it, so a
//! villager without one would silently stop patrolling, stop animating, and
//! vanish from every trace.
//!
//! A *knight* can still kill a villager, and that is deliberate rather than
//! overlooked: it costs nothing today, it is what makes "protect the elder"
//! expressible later, and it is the reason [`nearest_interactable`] skips the
//! dead. A corpse is never despawned — it keeps its spawn index and its
//! `Interactable` — so without that check you could hold a conversation with a
//! body.

use std::collections::HashMap;
use std::sync::Arc;

use ggez::glam::Vec2;
use hecs::World;

use crate::assets::{
    DialogueChoice, DialogueCondition, DialogueEffect, DialogueGraph, DialogueNode, ItemTable,
};
use crate::ecs::components::{Health, InteractTarget, Interactable, Inventory, Position, Size};
use crate::physics::Aabb;
use crate::sim::event::GameEvent;
use crate::systems::input::{Action, ActionSet, PlayerInput};
use crate::systems::inventory;

/// How far from an interactable's box the player's may be and still be offered
/// the prompt, in pixels, on every side.
///
/// A number in code rather than in RON, for the reason
/// [`crate::ecs::spawn::PICKUP_SIZE`] is one: this is the shape of an
/// interaction, not balance. No tuning pass ever wants blacksmiths to be
/// harder to stand near than herbalists. Roughly a body width, which is close
/// enough to read as "next to" and far enough that walking up to someone does
/// not require pixel alignment.
pub const INTERACT_RANGE: f32 = 24.0;

/// What the `prompt` probe field says when there is nothing in reach.
///
/// A word rather than an empty string, because a tape assertion is
/// whitespace-separated tokens: `assert prompt ==` with nothing after it does
/// not parse, so "nothing to talk to" would be unassertable. It is also what
/// keeps a trace greppable — `"prompt":"none"` is a state, `"prompt":""` looks
/// like a bug.
pub const NO_PROMPT: &str = "none";

/// The interactable the player would act on if they pressed the key right now.
///
/// Recomputed once per tick in the resolve phase and cached on
/// [`crate::sim::Sim`], rather than worked out at the moment of the press, so
/// that the prompt on screen and the thing the key actually does are the same
/// answer to the same question. Two computations would eventually disagree by
/// one tick, and the symptom is a player pressing the button they can see and
/// talking to somebody else.
#[derive(Clone, Debug)]
pub struct Prompt {
    /// Who is being offered. Kept so the HUD can draw the prompt over them.
    pub entity: hecs::Entity,
    pub prompt: String,
    pub target: InteractTarget,
}

/// The nearest interactable within reach of the player, if any.
///
/// **Nearest, not first found, and stable when two are equidistant.** Two NPCs
/// standing together is the one case where "whichever hecs iterated first"
/// produces a prompt that flickers between them as the player shifts a pixel,
/// and a flickering prompt is a bug that only ever shows up on the one map that
/// has a pair. Distance is between the boxes' centres and ties break by entity
/// id, so the answer is a function of the world rather than of archetype order.
///
/// The dead are skipped: a corpse is never despawned, so it keeps whatever it
/// was carrying, `Interactable` included.
pub fn nearest_interactable(world: &World) -> Option<Prompt> {
    let (reach, centre) = player_reach(world)?;

    let mut best: Option<((f32, u32), Prompt)> = None;
    for (entity, (interactable, pos, size, health)) in world
        .query::<(&Interactable, &Position, &Size, Option<&Health>)>()
        .iter()
    {
        if health.is_some_and(|h| h.dead()) {
            continue;
        }
        let box_ = Aabb::new(pos.0.x, pos.0.y, size.0.x, size.0.y);
        if !reach.overlaps(&box_) {
            continue;
        }

        let key = (
            (centre_of(&box_) - centre).length_squared(),
            // The tie-break. `f32` is only `PartialOrd`, so the pair is
            // compared by hand rather than by `min_by_key`.
            entity.id(),
        );
        let better = match &best {
            None => true,
            Some((current, _)) => key.0 < current.0 || (key.0 == current.0 && key.1 < current.1),
        };
        if better {
            best = Some((
                key,
                Prompt {
                    entity,
                    prompt: interactable.prompt.clone(),
                    target: interactable.target.clone(),
                },
            ));
        }
    }
    best.map(|(_, prompt)| prompt)
}

/// The player's collider grown by [`INTERACT_RANGE`] on every side, and the
/// centre of the collider itself — which is what distances are measured from,
/// so that growing the reach never moves the ranking.
///
/// Keyed off the bag rather than off `Avatar` so that whoever is carrying the
/// inventory is whoever gets the prompt — the same entity a `GiveItem` would
/// hand something to. Two different answers to "who is the player" is exactly
/// how a reward ends up in somebody else's pocket.
fn player_reach(world: &World) -> Option<(Aabb, Vec2)> {
    let holder = inventory::holder(world)?;
    let pos = world.get::<&Position>(holder).ok()?.0;
    let size = world.get::<&Size>(holder).ok()?.0;
    let box_ = Aabb::new(pos.x, pos.y, size.x, size.y);
    let reach = Aabb::new(
        pos.x - INTERACT_RANGE,
        pos.y - INTERACT_RANGE,
        size.x + INTERACT_RANGE * 2.0,
        size.y + INTERACT_RANGE * 2.0,
    );
    Some((reach, centre_of(&box_)))
}

fn centre_of(box_: &Aabb) -> Vec2 {
    Vec2::new(box_.x + box_.w / 2.0, box_.y + box_.h / 2.0)
}

/// A conversation in progress: where it is in a graph, and what may be said.
///
/// Kept in an `Option` on [`crate::sim::Sim`] rather than as an always-present
/// struct the way the inventory's `Screen` is, because "no conversation" is a
/// real state with no sensible default node to sit on — and because closing one
/// and opening another must not inherit a selection from the last person you
/// spoke to.
#[derive(Clone, Debug)]
pub struct Conversation {
    graph: Arc<DialogueGraph>,
    node: String,
    /// Which of the current node's lines is being read. Choices are offered
    /// once the last one has been.
    line: usize,
    /// Indices into the current node's *authored* choice list, filtered down to
    /// the ones whose conditions hold. Authored indices rather than clones, so
    /// [`GameEvent::ChoiceTaken`] can name the choice as the content file
    /// numbers it — a filtered index would mean something different depending
    /// on what the player happened to be carrying.
    choices: Vec<usize>,
    selection: usize,
}

impl Conversation {
    pub fn graph_id(&self) -> &str {
        &self.graph.id
    }

    pub fn node_id(&self) -> &str {
        &self.node
    }

    pub fn node(&self) -> &DialogueNode {
        self.graph
            .node(&self.node)
            .expect("the current node was resolved when it was entered")
    }

    pub fn speaker(&self) -> &str {
        &self.node().speaker
    }

    /// The line being read right now, or `""` for a node with nothing to say.
    pub fn line(&self) -> &str {
        self.node()
            .lines
            .get(self.line)
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// Which line of the node is showing, and how many there are — for an
    /// overlay that wants to draw a "more" marker.
    pub fn line_index(&self) -> (usize, usize) {
        (self.line, self.node().lines.len())
    }

    /// Whether the choices are being offered yet. False while there is still
    /// speech to page through.
    pub fn choosing(&self) -> bool {
        self.line + 1 >= self.node().lines.len()
    }

    /// The choices the player may actually take, in authored order.
    pub fn choices(&self) -> Vec<&DialogueChoice> {
        self.choices
            .iter()
            .map(|&index| &self.node().choices[index])
            .collect()
    }

    pub fn choice_count(&self) -> usize {
        self.choices.len()
    }

    pub fn selection(&self) -> usize {
        self.selection
    }

    /// Move to `node`, re-reading which of its choices are available. Called on
    /// open and on every branch taken, so a condition is always evaluated
    /// against the state the player is in *now* — which is what makes walking
    /// back to a node after picking something up show a new option.
    fn enter(&mut self, node: &str, world: &World, flags: &HashMap<String, i64>) {
        self.node = node.to_string();
        self.line = 0;
        self.selection = 0;
        self.choices = available(self.node(), world, flags);
    }
}

/// Open `graph` at its start node, or `None` if it has none — which load-time
/// validation rules out for shipped content and a hand-built graph in a test
/// may not.
pub fn open(
    world: &World,
    flags: &HashMap<String, i64>,
    graph: Arc<DialogueGraph>,
) -> Option<Conversation> {
    let start = graph.start.clone();
    graph.node(&start)?;
    let mut talk = Conversation {
        graph,
        node: String::new(),
        line: 0,
        choices: Vec::new(),
        selection: 0,
    };
    talk.enter(&start, world, flags);
    Some(talk)
}

/// Advance a conversation by one tick. Returns whether it should close.
///
/// `newly_held` is this tick's directions minus last tick's, taken once by
/// [`crate::sim::Sim::step`] and handed to whichever mode is running. Exactly
/// the mechanism the inventory screen reads its up and down with, and
/// deliberately not a second one: a held key is not a repeat, and there should
/// be one place that decides so.
///
/// The three actions, and why each is the one it is:
///
/// - `confirm` reads the next line, and once the speech is done, takes the
///   selected choice. One button for "go on" and "yes" because there is never
///   a moment when both are offered.
/// - `up`/`down` move between choices.
/// - `cancel` backs out of the whole conversation, from anywhere. Always
///   available, including mid-speech, because a dialogue you cannot leave is
///   how a modal state becomes a soft lock.
///
/// `interact` deliberately does *not* advance. It is the key that opened the
/// conversation, and on the tick it is pressed the world is still `Playing`;
/// honouring it here would make one press both open the conversation and read
/// its first line.
#[allow(clippy::too_many_arguments)]
pub fn step_conversation(
    world: &mut World,
    items: &ItemTable,
    flags: &mut HashMap<String, i64>,
    talk: &mut Conversation,
    input: PlayerInput,
    newly_held: ActionSet,
    events: &mut Vec<GameEvent>,
) -> bool {
    if input.pressed(Action::Cancel) {
        return true;
    }

    if newly_held.contains(Action::Up) {
        talk.selection = talk.selection.saturating_sub(1);
    }
    if newly_held.contains(Action::Down) {
        talk.selection += 1;
    }
    talk.selection = talk.selection.min(talk.choices.len().saturating_sub(1));

    if !input.pressed(Action::Confirm) {
        return false;
    }

    // Still speaking: read on.
    if !talk.choosing() {
        talk.line += 1;
        return false;
    }

    // Nothing left to say and nothing to say back: this node is an end.
    let Some(&index) = talk.choices.get(talk.selection) else {
        return true;
    };

    // Cloned out of the graph, because applying the effects borrows the world
    // mutably and the choice is read out of an `Arc` the conversation holds.
    let choice = talk.node().choices[index].clone();
    events.push(GameEvent::ChoiceTaken {
        node: talk.node.clone(),
        index,
    });
    for effect in &choice.effects {
        apply(world, items, flags, effect, events);
    }

    match &choice.next {
        Some(next) => {
            talk.enter(next, world, flags);
            false
        }
        None => true,
    }
}

/// Which of a node's choices may be offered, as authored indices.
fn available(node: &DialogueNode, world: &World, flags: &HashMap<String, i64>) -> Vec<usize> {
    node.choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| match &choice.condition {
            None => true,
            Some(condition) => holds(condition, world, flags),
        })
        .map(|(index, _)| index)
        .collect()
}

/// Does a condition hold right now?
///
/// An unset flag reads as 0 rather than failing: quests check their stage
/// before they have one, and `FlagEq("quest.x.stage", 0)` is the natural way to
/// write "not started".
fn holds(condition: &DialogueCondition, world: &World, flags: &HashMap<String, i64>) -> bool {
    match condition {
        DialogueCondition::HasItem(item) => carried(world, item) > 0,
        DialogueCondition::FlagEq(name, value) => flag(flags, name) == *value,
        DialogueCondition::FlagAtLeast(name, value) => flag(flags, name) >= *value,
    }
}

/// Read a quest flag, defaulting to 0.
pub fn flag(flags: &HashMap<String, i64>, name: &str) -> i64 {
    flags.get(name).copied().unwrap_or(0)
}

/// How many of `item` the player is carrying.
fn carried(world: &World, item: &str) -> u32 {
    inventory::holder(world)
        .and_then(|holder| {
            world
                .get::<&Inventory>(holder)
                .ok()
                .map(|bag| bag.count(item))
        })
        .unwrap_or(0)
}

/// Apply one effect.
///
/// Every arm delegates to the system that owns the state it touches. Nothing
/// here reaches into an `Inventory` or a `Health` directly, which is the whole
/// of why a quest reward shows up in a trace as the `picked_up` it is and why a
/// full bag refuses a reward the same way it refuses a potion off the floor.
fn apply(
    world: &mut World,
    items: &ItemTable,
    flags: &mut HashMap<String, i64>,
    effect: &DialogueEffect,
    events: &mut Vec<GameEvent>,
) {
    // Everything but `SetFlag` is about the player, and the player is whoever
    // carries the bag — the same answer `collect_pickups` uses.
    let holder = inventory::holder(world);

    match effect {
        DialogueEffect::SetFlag(name, value) => {
            flags.insert(name.clone(), *value);
        }
        DialogueEffect::GiveItem(item, count) => {
            // Unknown ids are dropped rather than handed over: `tests/data.rs`
            // is where a typo in content is caught, and the simulation's job is
            // to keep running with a bag that only ever holds real items.
            if let (Some(holder), true) = (holder, items.get(item).is_some()) {
                inventory::give_item(world, holder, item, *count, events);
            }
        }
        DialogueEffect::TakeItem(item, count) => {
            if let Some(holder) = holder {
                inventory::take_item(world, holder, item, *count);
            }
        }
        DialogueEffect::Heal(amount) => {
            if let Some(holder) = holder {
                inventory::heal(world, holder, *amount);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Assets, DialogueTable};
    use crate::ecs::components::{Attacking, Equipment, Team};
    use crate::sim::{GameEvent, Mode, Sim};
    use ggez::glam::Vec2;

    const INTERACT: PlayerInput = PlayerInput::from_actions(&[Action::Interact]);
    const CONFIRM: PlayerInput = PlayerInput::from_actions(&[Action::Confirm]);
    const CANCEL: PlayerInput = PlayerInput::from_actions(&[Action::Cancel]);
    const DOWN: PlayerInput = PlayerInput::from_actions(&[Action::Down]);
    const UP: PlayerInput = PlayerInput::from_actions(&[Action::Up]);

    fn shipped() -> Arc<DialogueTable> {
        DialogueTable::shipped()
    }

    /// A settled player on flat ground.
    fn grounded_sim() -> Sim {
        let mut sim = Sim::fixture(&[
            "....................",
            "..P.................",
            "####################",
        ]);
        for _ in 0..30 {
            sim.step(PlayerInput::default());
            if sim.probe().grounded {
                break;
            }
        }
        assert!(sim.probe().grounded);
        sim
    }

    /// Something to talk to, `dx` pixels to the right of the player's box, with
    /// the given graph.
    fn place(sim: &mut Sim, dx: f32, graph: &str, prompt: &str) -> hecs::Entity {
        let probe = sim.probe();
        let size = Vec2::new(22.0, 30.0);
        let at = Vec2::new(probe.x + 20.0 + dx, probe.y);
        sim.world.spawn((
            Interactable {
                prompt: prompt.to_string(),
                target: InteractTarget::Dialogue(graph.to_string()),
            },
            Position(at),
            Size(size),
        ))
    }

    fn bag_count(sim: &Sim, item: &str) -> u32 {
        inventory::holder(&sim.world)
            .and_then(|h| sim.world.get::<&Inventory>(h).ok().map(|b| b.count(item)))
            .unwrap_or(0)
    }

    fn give(sim: &mut Sim, item: &str, count: u32) {
        let holder = inventory::holder(&sim.world).expect("the fixture player carries a bag");
        assert!(sim
            .world
            .get::<&mut Inventory>(holder)
            .unwrap()
            .add(item, count));
    }

    // --- the prompt -------------------------------------------------------

    #[test]
    fn walking_near_something_offers_it_and_walking_away_clears_it() {
        let mut sim = grounded_sim();
        place(&mut sim, 200.0, "elder_intro", "talk");

        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().prompt, NO_PROMPT, "far away, nothing offered");
        assert!(sim.prompt().is_none());

        for _ in 0..90 {
            sim.step(PlayerInput::holding(&[Action::Right]));
            if sim.probe().prompt != NO_PROMPT {
                break;
            }
        }
        assert_eq!(sim.probe().prompt, "talk");
        assert!(sim.events().iter().any(
            |e| matches!(e, GameEvent::InteractPrompted { target } if target == "elder_intro")
        ));

        for _ in 0..120 {
            sim.step(PlayerInput::holding(&[Action::Left]));
            if sim.probe().prompt == NO_PROMPT {
                break;
            }
        }
        assert_eq!(sim.probe().prompt, NO_PROMPT, "walked away again");
    }

    /// Once per approach, not once per tick of standing there — otherwise a
    /// trace of a conversation is a wall of identical events.
    #[test]
    fn the_prompt_is_announced_once_however_long_you_stand_there() {
        let mut sim = grounded_sim();
        place(&mut sim, 10.0, "elder_intro", "talk");

        let mut announced = 0;
        for _ in 0..120 {
            sim.step(PlayerInput::default());
            announced += sim
                .events()
                .iter()
                .filter(|e| matches!(e, GameEvent::InteractPrompted { .. }))
                .count();
        }
        assert_eq!(sim.probe().prompt, "talk");
        assert_eq!(announced, 1);
    }

    /// D-1's acceptance criterion. Two interactables the same distance apart
    /// must produce the same answer from either side, and never flicker.
    #[test]
    fn the_nearer_of_two_interactables_wins_from_both_sides() {
        let mut sim = grounded_sim();
        // Close on the left, far on the right, then the mirror image.
        place(&mut sim, -60.0, "near_left", "left");
        place(&mut sim, 4.0, "near_right", "right");
        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().prompt, "right");

        let mut sim = grounded_sim();
        place(&mut sim, 4.0, "near_right", "right");
        place(&mut sim, -60.0, "near_left", "left");
        sim.step(PlayerInput::default());
        assert_eq!(
            sim.probe().prompt,
            "right",
            "the answer must not depend on which was spawned first"
        );
    }

    /// A tie is a real state — two NPCs standing symmetrically — and it has to
    /// resolve the same way every tick, or the prompt flickers as the player
    /// breathes.
    #[test]
    fn equidistant_interactables_break_their_tie_deterministically() {
        let mut sim = grounded_sim();
        let first = place(&mut sim, -32.0, "a", "a");
        let second = place(&mut sim, 10.0, "b", "b");
        // Move the second so both centres are exactly the same distance away.
        let player = sim.probe();
        let centre = player.x + 10.0;
        sim.world.get::<&mut Position>(first).unwrap().0.x = centre - 40.0 - 11.0;
        sim.world.get::<&mut Position>(second).unwrap().0.x = centre + 40.0 - 11.0;

        for _ in 0..30 {
            sim.step(PlayerInput::default());
            assert_eq!(sim.probe().prompt, "a", "the lower entity id wins, always");
        }
    }

    /// A corpse is never despawned, so it keeps its `Interactable`. Talking to
    /// one would be a bug you only find by killing a quest giver.
    #[test]
    fn the_dead_are_not_offered() {
        let mut sim = grounded_sim();
        let elder = place(&mut sim, 10.0, "elder_intro", "talk");
        sim.world
            .insert_one(elder, Health::new(3, 10))
            .expect("just spawned");
        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().prompt, "talk");

        sim.world.get::<&mut Health>(elder).unwrap().current = 0;
        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().prompt, NO_PROMPT);
    }

    #[test]
    fn pressing_interact_with_nothing_in_reach_does_nothing_at_all() {
        let mut sim = grounded_sim();
        place(&mut sim, 400.0, "elder_intro", "talk");
        sim.step(INTERACT);
        assert_eq!(sim.mode(), Mode::Playing);
        assert!(
            sim.events().is_empty(),
            "an empty press should be silent: {:?}",
            sim.events()
        );
    }

    // --- opening and closing ---------------------------------------------

    /// A sim with the shipped elder standing next to the player.
    fn talking_sim() -> Sim {
        let mut sim = grounded_sim();
        place(&mut sim, 10.0, "elder_intro", "talk");
        sim.step(PlayerInput::default());
        assert_eq!(sim.probe().prompt, "talk");
        sim
    }

    #[test]
    fn interacting_opens_the_graph_at_its_start_node_and_freezes_the_world() {
        let mut sim = talking_sim();
        let before = sim.probe().x;

        sim.step(INTERACT);
        assert_eq!(sim.mode(), Mode::Dialogue);
        assert_eq!(sim.probe().mode, "dialogue");
        assert_eq!(sim.probe().dialogue_node, "greet");
        assert_eq!(sim.conversation().unwrap().graph_id(), "elder_intro");
        assert!(sim.events().contains(&GameEvent::Interacted {
            target: "elder_intro".to_string()
        }));
        assert!(sim.events().contains(&GameEvent::DialogueOpened {
            graph: "elder_intro".to_string()
        }));

        for _ in 0..60 {
            sim.step(PlayerInput::holding(&[Action::Right]));
        }
        assert_eq!(sim.probe().x, before, "the world is frozen while talking");
    }

    #[test]
    fn cancel_closes_a_conversation_from_anywhere_and_says_so() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        sim.step(CANCEL);
        assert_eq!(sim.mode(), Mode::Playing);
        assert_eq!(sim.probe().dialogue_node, NO_PROMPT);
        assert!(sim.conversation().is_none());
        assert!(sim.events().contains(&GameEvent::DialogueClosed {
            graph: "elder_intro".to_string()
        }));
    }

    /// Speech is paged: `confirm` reads on, and the choices are only offered
    /// once the last line has been.
    #[test]
    fn confirm_reads_the_lines_before_it_offers_the_choices() {
        let mut sim = talking_sim();
        sim.step(INTERACT);

        let (_, lines) = sim.conversation().unwrap().line_index();
        assert!(lines > 1, "the greeting should be worth paging through");
        for read in 0..lines - 1 {
            assert!(!sim.conversation().unwrap().choosing(), "line {read}");
            sim.step(CONFIRM);
            sim.step(PlayerInput::default());
        }
        assert!(sim.conversation().unwrap().choosing());
        assert_eq!(sim.probe().dialogue_node, "greet", "still the same node");
    }

    /// Read to the end of the speech, so the choices are on offer.
    fn read_out(sim: &mut Sim) {
        while !sim.conversation().expect("in a conversation").choosing() {
            sim.step(CONFIRM);
            sim.step(PlayerInput::default());
        }
    }

    /// Move the highlight onto the choice whose text contains `text`, and
    /// return its *authored* index.
    ///
    /// By text rather than by a hard-coded row, so reordering the content file
    /// cannot silently retarget a test onto a different branch — which is the
    /// mistake a graph is most likely to invite.
    fn select(sim: &mut Sim, text: &str) -> usize {
        let talk = sim.conversation().expect("in a conversation");
        let row = talk
            .choices()
            .iter()
            .position(|choice| choice.text.contains(text))
            .unwrap_or_else(|| {
                panic!(
                    "no choice mentioning `{text}` is on offer: {:?}",
                    talk.choices().iter().map(|c| &c.text).collect::<Vec<_>>()
                )
            });
        let authored = talk.choices[row];

        while sim.probe().dialogue_selection > row {
            sim.step(UP);
            sim.step(PlayerInput::default());
        }
        while sim.probe().dialogue_selection < row {
            sim.step(DOWN);
            sim.step(PlayerInput::default());
        }
        authored
    }

    // --- choices ----------------------------------------------------------

    #[test]
    fn a_choice_whose_condition_fails_is_absent_rather_than_inert() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        read_out(&mut sim);

        let without = sim.probe().dialogue_choices;
        let offered: Vec<String> = sim
            .conversation()
            .unwrap()
            .choices()
            .iter()
            .map(|c| c.text.clone())
            .collect();
        assert!(
            !offered.iter().any(|text| text.contains("draught back")),
            "the gated branch should not be listed at all: {offered:?}"
        );

        // The same node, with the gating item in the bag.
        sim.step(CANCEL);
        give(&mut sim, "minor_potion", 1);
        sim.step(PlayerInput::default());
        sim.step(INTERACT);
        read_out(&mut sim);

        assert_eq!(
            sim.probe().dialogue_choices,
            without + 1,
            "carrying the item should add exactly one option"
        );
        assert!(sim
            .conversation()
            .unwrap()
            .choices()
            .iter()
            .any(|c| c.text.contains("draught back")));
    }

    /// A held direction is one step, not one per tick — the same rule the
    /// inventory screen is under, read from the same diff.
    #[test]
    fn holding_a_direction_moves_the_selection_once() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        read_out(&mut sim);
        assert!(sim.probe().dialogue_choices >= 2);

        for _ in 0..20 {
            sim.step(PlayerInput::holding(&[Action::Down]));
        }
        assert_eq!(sim.probe().dialogue_selection, 1, "one step for one press");

        sim.step(PlayerInput::default());
        sim.step(DOWN);
        assert_eq!(sim.probe().dialogue_selection, 2);
    }

    #[test]
    fn the_selection_cannot_leave_the_choices() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        read_out(&mut sim);
        let choices = sim.probe().dialogue_choices;

        for _ in 0..10 {
            sim.step(DOWN);
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.probe().dialogue_selection, choices - 1);
        for _ in 0..10 {
            sim.step(UP);
            sim.step(PlayerInput::default());
        }
        assert_eq!(sim.probe().dialogue_selection, 0);
    }

    /// The whole of D-2's acceptance criteria in one run: walk to an end node,
    /// have an effect give an item through the inventory path, and close.
    #[test]
    fn an_effect_that_gives_an_item_goes_through_the_inventory_and_says_so() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        read_out(&mut sim);
        assert_eq!(bag_count(&sim, "minor_potion"), 0);

        let index = select(&mut sim, "anything for the road");
        sim.step(CONFIRM);
        assert_eq!(sim.probe().dialogue_node, "gift");
        assert!(
            sim.events().iter().any(|e| matches!(
                e,
                GameEvent::ChoiceTaken { node, index: taken } if node == "greet" && *taken == index
            )),
            "the event names the node and the authored index: {:?}",
            sim.events()
        );

        sim.step(PlayerInput::default());
        read_out(&mut sim);
        sim.step(CONFIRM);

        assert_eq!(bag_count(&sim, "minor_potion"), 1, "it is in the bag");
        assert_eq!(
            sim.probe().inventory_count,
            1,
            "and the bag knows it is there"
        );
        assert!(
            sim.events().contains(&GameEvent::PickedUp {
                item: "minor_potion".to_string(),
                count: 1
            }),
            "a reward is bookkept exactly as a pickup is: {:?}",
            sim.events()
        );
        assert_eq!(sim.mode(), Mode::Playing, "a `next: None` choice closes it");
    }

    /// The other half: a branch gated on an item, taken, takes it away and
    /// hands something back — the exit criterion, in Rust.
    #[test]
    fn a_gated_branch_trades_one_item_for_another_and_sets_its_flag() {
        let mut sim = talking_sim();
        give(&mut sim, "minor_potion", 1);
        sim.step(PlayerInput::default());
        sim.step(INTERACT);
        read_out(&mut sim);

        select(&mut sim, "draught back");
        sim.step(CONFIRM);

        assert_eq!(bag_count(&sim, "minor_potion"), 0, "handed over");
        assert_eq!(bag_count(&sim, "elder_charm"), 1, "and paid for");
        assert_eq!(sim.flag("quest.draught.stage"), 2);
        assert_eq!(sim.probe().dialogue_node, "thanks");

        sim.step(PlayerInput::default());
        read_out(&mut sim);
        sim.step(CONFIRM);
        assert_eq!(sim.mode(), Mode::Playing);
    }

    /// Conditions are re-read on every entry, so a node revisited after the
    /// world changed offers what is true now rather than what was true then.
    #[test]
    fn revisiting_a_node_re_evaluates_its_conditions() {
        let mut sim = talking_sim();
        sim.step(INTERACT);
        read_out(&mut sim);
        let before = sim.probe().dialogue_choices;

        // "Who are you?" leads to a node whose only choice comes back to
        // `greet`. Give the gating item while standing in it.
        select(&mut sim, "Who are you");
        sim.step(CONFIRM);
        assert_eq!(sim.probe().dialogue_node, "who");

        give(&mut sim, "minor_potion", 1);
        sim.step(PlayerInput::default());
        read_out(&mut sim);
        sim.step(CONFIRM);

        assert_eq!(sim.probe().dialogue_node, "greet", "back where we started");
        assert_eq!(
            sim.probe().dialogue_choices,
            before + 1,
            "and the branch that was hidden a moment ago is on offer"
        );
    }

    // --- flags and effects, unit ------------------------------------------

    #[test]
    fn an_unset_flag_reads_as_zero() {
        let flags = HashMap::new();
        assert_eq!(flag(&flags, "quest.nothing"), 0);
        assert!(holds(
            &DialogueCondition::FlagEq("quest.nothing".to_string(), 0),
            &World::new(),
            &flags
        ));
        assert!(!holds(
            &DialogueCondition::FlagAtLeast("quest.nothing".to_string(), 1),
            &World::new(),
            &flags
        ));
    }

    #[test]
    fn heal_and_take_go_through_the_same_paths_the_rest_of_the_game_uses() {
        let mut sim = grounded_sim();
        let holder = inventory::holder(&sim.world).unwrap();
        sim.world.get::<&mut Health>(holder).unwrap().current = 1;
        give(&mut sim, "minor_potion", 2);

        let items = sim.items.clone();
        let mut flags = HashMap::new();
        let mut events = Vec::new();
        apply(
            &mut sim.world,
            &items,
            &mut flags,
            &DialogueEffect::Heal(99),
            &mut events,
        );
        assert_eq!(
            sim.world.get::<&Health>(holder).unwrap().current,
            sim.world.get::<&Health>(holder).unwrap().max,
            "clamped to the maximum, not overhealed"
        );

        apply(
            &mut sim.world,
            &items,
            &mut flags,
            &DialogueEffect::TakeItem("minor_potion".to_string(), 1),
            &mut events,
        );
        assert_eq!(bag_count(&sim, "minor_potion"), 1);

        apply(
            &mut sim.world,
            &items,
            &mut flags,
            &DialogueEffect::TakeItem("minor_potion".to_string(), 9),
            &mut events,
        );
        assert_eq!(
            bag_count(&sim, "minor_potion"),
            1,
            "taking more than is there changes nothing"
        );
    }

    /// A full bag refuses a reward the way it refuses a pickup, and says so,
    /// rather than silently swallowing it.
    #[test]
    fn a_full_bag_refuses_a_reward_loudly() {
        let mut sim = grounded_sim();
        let holder = inventory::holder(&sim.world).unwrap();
        {
            let mut bag = sim.world.get::<&mut Inventory>(holder).unwrap();
            for index in 0..bag.capacity {
                assert!(bag.add(&format!("filler_{index}"), 1));
            }
        }

        let items = sim.items.clone();
        let mut events = Vec::new();
        apply(
            &mut sim.world,
            &items,
            &mut HashMap::new(),
            &DialogueEffect::GiveItem("minor_potion".to_string(), 1),
            &mut events,
        );
        assert_eq!(bag_count(&sim, "minor_potion"), 0);
        assert_eq!(
            events,
            vec![GameEvent::InventoryFull {
                item: "minor_potion".to_string()
            }]
        );
    }

    /// Content is cross-referenced in `tests/data.rs`; the simulation's job is
    /// to keep running when it is not.
    #[test]
    fn an_effect_naming_an_item_nothing_defines_hands_over_nothing() {
        let mut sim = grounded_sim();
        let items = sim.items.clone();
        let mut events = Vec::new();
        apply(
            &mut sim.world,
            &items,
            &mut HashMap::new(),
            &DialogueEffect::GiveItem("no_such_item".to_string(), 1),
            &mut events,
        );
        assert_eq!(sim.probe().inventory_count, 0);
        assert!(events.is_empty());
    }

    // --- content ----------------------------------------------------------

    /// Every shipped graph opens, and every node it can reach resolves. The
    /// corpus check is `tests/data.rs`; this one is here so a broken graph
    /// fails without leaving the crate.
    #[test]
    fn every_shipped_graph_opens_at_a_node_that_exists() {
        let table = shipped();
        assert!(!table.ids().is_empty(), "no dialogue ships at all");
        let world = World::new();
        for id in table.ids() {
            let graph = table.get(id).expect("just listed").clone();
            assert!(
                open(&world, &HashMap::new(), graph).is_some(),
                "`{id}` does not open"
            );
        }
    }

    /// The load-time contract, checked against a graph written to be wrong.
    #[test]
    fn a_dangling_next_is_a_load_time_error_naming_the_graph_and_the_node() {
        let dir = std::env::temp_dir().join(format!("supergame-dialogue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/dialogue")).unwrap();
        std::fs::write(
            dir.join("data/dialogue/broken.ron"),
            r#"Dialogue(
                id: "broken",
                start: "greet",
                nodes: {
                    "greet": Node(
                        speaker: "Nobody",
                        lines: ["..."],
                        choices: [Choice(text: "onward", next: "nowhere")],
                    ),
                },
            )"#,
        )
        .unwrap();

        let mut assets = Assets::rooted(&dir);
        let err = assets.dialogue().expect_err("a dangling target must fail");
        let text = format!("{err:#}");
        assert!(text.contains("broken"), "{text}");
        assert!(text.contains("greet"), "should name the node: {text}");
        assert!(text.contains("nowhere"), "should name the target: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other edge: a `start` that names nothing at all.
    #[test]
    fn a_start_node_that_does_not_exist_is_a_load_time_error() {
        let dir = std::env::temp_dir().join(format!("supergame-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/dialogue")).unwrap();
        std::fs::write(
            dir.join("data/dialogue/broken.ron"),
            r#"Dialogue(
                id: "broken",
                start: "missing",
                nodes: { "greet": Node(speaker: "Nobody", lines: ["..."]) },
            )"#,
        )
        .unwrap();

        let err = Assets::rooted(&dir)
            .dialogue()
            .expect_err("a missing start must fail");
        let text = format!("{err:#}");
        assert!(text.contains("missing"), "{text}");
        assert!(text.contains("greet"), "should list what is there: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D-4's test: `Patrol` without `Hostile` is a blacksmith who paces but
    /// will not stab you, and it is what the split was designed for.
    #[test]
    fn a_villager_paces_while_a_knight_chases() {
        let mut sim = Sim::fixture(&[
            "........................",
            "..P.....................",
            "########################",
        ]);
        let stats = crate::assets::StatTable::shipped();
        let clips = Arc::new(crate::sim::fixture_clips());
        for (kind, cell) in [("knight", 14u32), ("villager", 20u32)] {
            crate::ecs::spawn::entity(
                &mut sim.world,
                &crate::level::EntitySpawn {
                    kind: kind.to_string(),
                    pos: Vec2::new(cell as f32 * 32.0, 32.0),
                },
                32.0,
                clips.clone(),
                stats.get(kind).unwrap(),
            )
            .unwrap();
        }

        let start: Vec<f32> = sim.npc_positions().iter().map(|p| p.x).collect();
        for _ in 0..240 {
            sim.step(PlayerInput::holding(&[Action::Right]));
        }
        let now: Vec<f32> = sim.npc_positions().iter().map(|p| p.x).collect();
        let kinds: Vec<String> = sim.npc_probes().into_iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            vec!["knight".to_string(), "villager".to_string()],
            "a villager takes its place in spawn order like any other NPC"
        );

        let player = sim.probe().x;
        assert!(
            (now[0] - player).abs() < (start[0] - player).abs(),
            "the knight should have closed: {start:?} -> {now:?}"
        );
        assert!(
            (now[1] - start[1]).abs() > 1.0,
            "the villager should still be pacing: {start:?} -> {now:?}"
        );
    }

    /// The mortality decision, written down as a test so it stays a decision:
    /// the player's sword cannot touch a villager, because a villager is on the
    /// player's team and `combat::resolve` refuses friendly fire.
    #[test]
    fn the_player_cannot_kill_a_villager() {
        let mut sim = Sim::fixture(&["............", "..P.........", "############"]);
        let clips = Arc::new(crate::sim::fixture_clips());
        let stats = crate::assets::StatTable::shipped();
        let villager = crate::ecs::spawn::entity(
            &mut sim.world,
            &crate::level::EntitySpawn {
                kind: "villager".to_string(),
                pos: Vec2::new(3.0 * 32.0, 32.0),
            },
            32.0,
            clips,
            stats.get("villager").unwrap(),
        )
        .unwrap();
        assert_eq!(*sim.world.get::<&Team>(villager).unwrap(), Team::Player);
        let full = sim.world.get::<&Health>(villager).unwrap().current;

        for tick in 0..300 {
            let input = if tick % 20 == 0 {
                PlayerInput::from_actions(&[Action::Attack])
            } else {
                PlayerInput::holding(&[Action::Right])
            };
            sim.step(input);
        }
        assert_eq!(
            sim.world.get::<&Health>(villager).unwrap().current,
            full,
            "the quest giver survived a sustained beating"
        );
        assert!(sim
            .events()
            .iter()
            .all(|e| !matches!(e, GameEvent::Damaged { who, .. } if who == "villager")));

        // ...and it carries no bag, so it can never be mistaken for the player
        // by anything that looks for whoever is holding one.
        assert!(sim.world.get::<&Inventory>(villager).is_err());
        assert!(sim.world.get::<&Equipment>(villager).is_err());
    }

    /// The other half of the same decision, and the reason it is a decision
    /// rather than an accident: a *knight* can. Nothing about being friendly
    /// makes a villager invulnerable — it is on the player's team, and the only
    /// thing that follows from that is that the player's own sword passes
    /// through it. This is what "protect the elder" would be built on, and it
    /// is why `nearest_interactable` skips the dead.
    #[test]
    fn a_knight_can_kill_a_villager() {
        let mut sim = Sim::fixture(&["............", "..P.........", "############"]);
        let clips = Arc::new(crate::sim::fixture_clips());
        let stats = crate::assets::StatTable::shipped();
        let mut spawn_at = |kind: &str, cell: f32| {
            crate::ecs::spawn::entity(
                &mut sim.world,
                &crate::level::EntitySpawn {
                    kind: kind.to_string(),
                    pos: Vec2::new(cell * 32.0, 32.0),
                },
                32.0,
                clips.clone(),
                stats.get(kind).unwrap(),
            )
            .unwrap()
        };
        let knight = spawn_at("knight", 7.0);
        let villager = spawn_at("villager", 8.0);

        // The knight never *chooses* to swing at a neighbour — it hunts the
        // avatar — so the swing is started by hand. What is under test is who
        // the blow lands on once it is thrown.
        // Generous, because a hit throws her clear and she has to stroll back
        // into reach before the next one — this is a test of *who can be hit*,
        // not of how long it takes.
        let full = sim.world.get::<&Health>(villager).unwrap().current;
        for _ in 0..4000 {
            if sim.world.get::<&Health>(villager).unwrap().dead() {
                break;
            }
            if !sim.world.get::<&Attacking>(knight).unwrap().busy() {
                let attack = stats.get("knight").unwrap().attack.clone();
                sim.world
                    .get::<&mut Attacking>(knight)
                    .unwrap()
                    .start(&attack);
            }
            sim.step(PlayerInput::default());
        }

        let health = *sim.world.get::<&Health>(villager).unwrap();
        assert!(health.current < full, "the knight's sword went through her");
        assert!(health.dead(), "and eventually killed her");

        // ...and a corpse is not someone you can hold a conversation with,
        // even though it is never despawned and still carries what it did.
        assert!(sim.world.get::<&Interactable>(villager).is_ok());
        assert!(
            nearest_interactable(&sim.world).is_none(),
            "a body should not offer a prompt"
        );
    }
}
