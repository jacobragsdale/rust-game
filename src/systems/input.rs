//! Translates raw keyboard state into gameplay intent. Gameplay systems only
//! ever see `PlayerInput`, never the keyboard. Arrows and WASD both work.
//!
//! Everything here is derived from one table. [`ACTIONS`] gives each
//! [`Action`] the name tapes spell it with, the keys that trigger it, and
//! whether it is read as a state (`left` is true for as long as the key is
//! down) or as a one-shot (`jump` fires once per press). The keyboard reader,
//! the press latch, the tape parser and the bitsets all read that table, so
//! adding an action is one enum variant and one row — there is no second list
//! to forget.

use ggez::winit::event::VirtualKeyCode as Key;
use ggez::Context;

/// Everything the player can ask the game to do.
///
/// The discriminant is the bit each action occupies in an [`ActionSet`], so
/// the order of the variants is the order of [`ACTIONS`] (checked by
/// `the_table_is_indexed_by_variant`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Action {
    Left,
    Right,
    Down,
    Up,
    Jump,
    Attack,
    Cast,
    Interact,
    Inventory,
    Confirm,
    Cancel,
}

/// How an action is read: while the key is down, or once per press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// True for every tick the key is held. Movement.
    Level,
    /// True for exactly one tick per press, however the frames fall. Anything
    /// that spends something: a jump, a swing, a cast.
    Edge,
}

/// One action's name, keys and trigger.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub action: Action,
    /// How a tape spells this action.
    pub name: &'static str,
    /// Default keyboard bindings. Every one of them triggers the action.
    pub keys: &'static [Key],
    pub trigger: Trigger,
}

impl Binding {
    pub const fn is_edge(&self) -> bool {
        matches!(self.trigger, Trigger::Edge)
    }
}

/// The action table: the single place an action is defined.
///
/// `Up`/`W` deliberately drive both `Jump` and `Up`. Jumping has always been
/// on the up key and stays there; `Up` exists as an action of its own so that
/// menus, ladders and dialogue choices have a direction to read that is not
/// "the jump key". M5's `interact` is on `E` for the same reason in reverse —
/// it wants a key with no direction attached.
pub const ACTIONS: [Binding; 11] = [
    Binding {
        action: Action::Left,
        name: "left",
        keys: &[Key::Left, Key::A],
        trigger: Trigger::Level,
    },
    Binding {
        action: Action::Right,
        name: "right",
        keys: &[Key::Right, Key::D],
        trigger: Trigger::Level,
    },
    Binding {
        action: Action::Down,
        name: "down",
        keys: &[Key::Down, Key::S],
        trigger: Trigger::Level,
    },
    Binding {
        action: Action::Up,
        name: "up",
        keys: &[Key::Up, Key::W],
        trigger: Trigger::Level,
    },
    Binding {
        action: Action::Jump,
        name: "jump",
        keys: &[Key::Up, Key::W, Key::Space],
        trigger: Trigger::Edge,
    },
    Binding {
        // `J` for WASD hands, `X` for arrow hands.
        action: Action::Attack,
        name: "attack",
        keys: &[Key::J, Key::X],
        trigger: Trigger::Edge,
    },
    Binding {
        action: Action::Cast,
        name: "cast",
        keys: &[Key::K],
        trigger: Trigger::Edge,
    },
    Binding {
        action: Action::Interact,
        name: "interact",
        keys: &[Key::E],
        trigger: Trigger::Edge,
    },
    Binding {
        action: Action::Inventory,
        name: "inventory",
        keys: &[Key::I],
        trigger: Trigger::Edge,
    },
    Binding {
        action: Action::Confirm,
        name: "confirm",
        keys: &[Key::Return],
        trigger: Trigger::Edge,
    },
    Binding {
        // Not `Escape`: the app quits on that before any scene sees it.
        action: Action::Cancel,
        name: "cancel",
        keys: &[Key::Back],
        trigger: Trigger::Edge,
    },
];

/// Every edge-triggered action. A press can only ever be one of these.
pub const EDGE_ACTIONS: ActionSet = ActionSet(edge_mask());

/// Folded out of the table by a `const fn` rather than written as a literal,
/// so the mask cannot drift away from the rows it summarizes.
const fn edge_mask() -> u16 {
    let mut bits = 0u16;
    let mut i = 0;
    while i < ACTIONS.len() {
        if ACTIONS[i].is_edge() {
            bits |= ACTIONS[i].action.bit();
        }
        i += 1;
    }
    bits
}

impl Action {
    /// This action's bit in an [`ActionSet`].
    pub const fn bit(self) -> u16 {
        1u16 << (self as u16)
    }

    /// This action's row in [`ACTIONS`]. By value because the table is a
    /// `const`; the fields it hands back are all `'static`.
    pub const fn binding(self) -> Binding {
        ACTIONS[self as usize]
    }

    /// How a tape spells this action.
    pub const fn name(self) -> &'static str {
        self.binding().name
    }

    pub const fn is_edge(self) -> bool {
        self.binding().is_edge()
    }

    /// Resolve a tape token, or `None` if nothing is spelled that way.
    pub fn from_name(name: &str) -> Option<Action> {
        ACTIONS
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.action)
    }

    pub fn all() -> impl Iterator<Item = Action> {
        ACTIONS.iter().map(|binding| binding.action)
    }

    /// Every action's name, in table order.
    pub fn names() -> Vec<&'static str> {
        ACTIONS.iter().map(|binding| binding.name).collect()
    }

    /// The names as an error message lists them.
    pub fn name_list() -> String {
        Action::names().join(", ")
    }

    /// The table as markdown, which is how `tapes/README.md` documents it.
    /// `tapes_readme_lists_every_action` keeps the file and this in step.
    pub fn table_markdown() -> String {
        let mut out = String::from("| Tape name | Keyboard | Trigger |\n| --- | --- | --- |\n");
        for binding in &ACTIONS {
            let keys: Vec<String> = binding.keys.iter().map(|&key| key_label(key)).collect();
            let trigger = if binding.is_edge() { "edge" } else { "level" };
            out.push_str(&format!(
                "| `{}` | {} | {trigger} |\n",
                binding.name,
                keys.join(", ")
            ));
        }
        out
    }
}

/// What a key is called in the documentation. `VirtualKeyCode`'s own names are
/// right for all but the two nobody would recognize.
fn key_label(key: Key) -> String {
    match key {
        Key::Return => "Enter".to_string(),
        Key::Back => "Backspace".to_string(),
        other => format!("{other:?}"),
    }
}

/// A set of actions, one bit each.
///
/// This is what a tape line compiles to and what `PlayerInput` is made of, so
/// a new action costs no struct field anywhere.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionSet(u16);

impl ActionSet {
    pub const EMPTY: ActionSet = ActionSet(0);

    pub const fn of(actions: &[Action]) -> ActionSet {
        let mut bits = 0u16;
        let mut i = 0;
        while i < actions.len() {
            bits |= actions[i].bit();
            i += 1;
        }
        ActionSet(bits)
    }

    pub const fn contains(self, action: Action) -> bool {
        self.0 & action.bit() != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, action: Action) {
        self.0 |= action.bit();
    }

    pub fn remove(&mut self, action: Action) {
        self.0 &= !action.bit();
    }

    pub fn set(&mut self, action: Action, on: bool) {
        if on {
            self.insert(action);
        } else {
            self.remove(action);
        }
    }

    /// Actions in this set, in table order.
    pub fn iter(self) -> impl Iterator<Item = Action> {
        Action::all().filter(move |&action| self.contains(action))
    }

    /// Actions newly in `self` that were not in `previous` — the presses.
    pub const fn newly_set(self, previous: ActionSet) -> ActionSet {
        ActionSet(self.0 & !previous.0)
    }

    /// The actions in both sets.
    pub const fn intersect(self, other: ActionSet) -> ActionSet {
        ActionSet(self.0 & other.0)
    }

    /// Left and right cancel out: holding both is standing still, whether the
    /// input came from a keyboard or from a tape line reading `left+right`.
    pub const fn without_opposing_directions(self) -> ActionSet {
        const BOTH: u16 = Action::Left.bit() | Action::Right.bit();
        if self.0 & BOTH == BOTH {
            ActionSet(self.0 & !BOTH)
        } else {
            self
        }
    }
}

/// Names rather than a number, so a failing tape assertion reads.
impl std::fmt::Debug for ActionSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.iter().map(Action::name).collect();
        write!(f, "ActionSet({})", names.join("+"))
    }
}

/// One tick's intent: what is held, and what was pressed this tick.
///
/// Presses only ever contain edge-triggered actions; holding is the whole
/// story for the rest. A press without the corresponding hold is a real state
/// — a key tapped and released between two ticks — and is not filtered out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerInput {
    held: ActionSet,
    pressed: ActionSet,
}

impl PlayerInput {
    /// The rules that are true of any source of input: opposing directions
    /// cancel, and only edge-triggered actions can be "pressed".
    pub const fn new(held: ActionSet, pressed: ActionSet) -> PlayerInput {
        PlayerInput {
            held: held.without_opposing_directions(),
            pressed: pressed.intersect(EDGE_ACTIONS),
        }
    }

    /// Hold these actions, and fire the edge-triggered ones. The usual way to
    /// write an input in a test: `from_actions(&[Action::Right, Action::Jump])`
    /// is running right and pressing jump this tick.
    pub const fn from_actions(actions: &[Action]) -> PlayerInput {
        let held = ActionSet::of(actions);
        PlayerInput::new(held, held)
    }

    /// Hold these actions without pressing anything — the tick after a jump,
    /// with the key still down.
    pub const fn holding(actions: &[Action]) -> PlayerInput {
        PlayerInput::new(ActionSet::of(actions), ActionSet::EMPTY)
    }

    pub const fn held(self, action: Action) -> bool {
        self.held.contains(action)
    }

    pub const fn pressed(self, action: Action) -> bool {
        self.pressed.contains(action)
    }

    pub fn set_held(&mut self, action: Action, on: bool) {
        self.held.set(action, on);
    }

    /// Only edge-triggered actions can be pressed; asking for anything else is
    /// a mistake in the caller.
    pub fn set_pressed(&mut self, action: Action, on: bool) {
        debug_assert!(action.is_edge(), "{} is not edge-triggered", action.name());
        self.pressed.set(action, on);
    }

    // --- the actions gameplay reads today ---------------------------------

    pub const fn left(self) -> bool {
        self.held(Action::Left)
    }

    pub const fn right(self) -> bool {
        self.held(Action::Right)
    }

    pub const fn down(self) -> bool {
        self.held(Action::Down)
    }

    pub const fn up(self) -> bool {
        self.held(Action::Up)
    }

    /// Edge-triggered jump: one press, one jump (buffered by the avatar).
    pub const fn jump_pressed(self) -> bool {
        self.pressed(Action::Jump)
    }

    /// Jump key currently held: releasing early cuts the jump short.
    pub const fn jump_held(self) -> bool {
        self.held(Action::Jump)
    }

    /// Edge-triggered attack: one press, one swing. Holding does not flail.
    pub const fn attack_pressed(self) -> bool {
        self.pressed(Action::Attack)
    }

    /// -1 left, +1 right, 0 for neither or both.
    pub fn dir(self) -> f32 {
        f32::from(self.right()) - f32::from(self.left())
    }
}

/// Holds one-shot presses from the moment the OS reports them until a
/// simulation tick consumes them.
///
/// This exists because presses and ticks are not on the same clock. The sim
/// runs on a fixed 60 Hz accumulator, but ggez clears its "just pressed" edge
/// once per rendered *frame*. Polling that edge from inside the tick loop
/// therefore loses presses: whenever the window renders faster than 60 Hz —
/// vsync on a 120 Hz display, say — a good share of frames run no tick at
/// all, and any press that landed on one of those frames is gone before the
/// simulation ever sees it. That is the "I hit jump and nothing happened",
/// and because which frames get a tick drifts, it feels random.
///
/// A frame that runs *two* ticks has the mirror-image bug: the same edge is
/// read twice, so one tap spends the ground jump and the double jump back to
/// back and leaves the player airborne with nothing left.
///
/// Latching at the event and clearing on consumption makes it exactly one
/// action per press, whatever the frames and ticks are doing.
///
/// Every edge-triggered action is latched, not just jump: casting a spell or
/// confirming a dialogue choice would otherwise be dropped on exactly the
/// frames a jump is.
#[derive(Debug, Default)]
pub struct InputLatch {
    pressed: ActionSet,
}

impl InputLatch {
    /// Record a key-down event. Keys bound to nothing are ignored, and the
    /// caller is expected to have already filtered out OS key-repeat — holding
    /// a key must not re-arm its latch.
    pub fn key_down(&mut self, key: Key) {
        for binding in &ACTIONS {
            if binding.is_edge() && binding.keys.contains(&key) {
                self.pressed.insert(binding.action);
            }
        }
    }

    /// Drop every pending press. Called on scene changes so a press meant for
    /// a menu does not turn into a jump the instant gameplay resumes.
    pub fn clear(&mut self) {
        self.pressed = ActionSet::EMPTY;
    }

    /// Take every pending press, arming the latch for the next one.
    fn take(&mut self) -> ActionSet {
        std::mem::take(&mut self.pressed)
    }
}

pub fn read(ctx: &Context, latch: &mut InputLatch) -> PlayerInput {
    let kb = &ctx.keyboard;
    let mut held = ActionSet::EMPTY;
    for binding in &ACTIONS {
        if binding.keys.iter().any(|&key| kb.is_key_pressed(key)) {
            held.insert(binding.action);
        }
    }
    PlayerInput::new(held, latch.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(latch: &mut InputLatch, action: Action) -> bool {
        latch.take().contains(action)
    }

    // --- the table --------------------------------------------------------

    /// `Action::binding` indexes straight into the table, so the table has to
    /// be in variant order and complete.
    #[test]
    fn the_table_is_indexed_by_variant() {
        for (index, binding) in ACTIONS.iter().enumerate() {
            assert_eq!(
                binding.action as usize, index,
                "`{}` is out of order in ACTIONS",
                binding.name
            );
            assert_eq!(binding.action.name(), binding.name);
        }
    }

    #[test]
    fn no_two_actions_share_a_name() {
        let mut names = Action::names();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate action name in ACTIONS");
    }

    /// A key bound to two *edge* actions would fire two one-shots from one
    /// press, which is the bug the latch exists to prevent. A key shared by an
    /// edge action and a level one is fine and deliberate: `Up`/`W` jump and
    /// also mean "up".
    #[test]
    fn no_key_drives_two_one_shot_actions() {
        for binding in ACTIONS.iter().filter(|b| b.is_edge()) {
            for other in ACTIONS.iter().filter(|b| b.is_edge()) {
                if binding.action == other.action {
                    continue;
                }
                for key in binding.keys {
                    assert!(
                        !other.keys.contains(key),
                        "{key:?} drives both `{}` and `{}`",
                        binding.name,
                        other.name
                    );
                }
            }
        }
    }

    /// The one deliberate overlap, written down so removing it is a decision.
    #[test]
    fn the_up_key_is_both_jump_and_up() {
        for key in [Key::Up, Key::W] {
            assert!(Action::Jump.binding().keys.contains(&key));
            assert!(Action::Up.binding().keys.contains(&key));
        }
        assert!(!Action::Up.is_edge(), "`up` is a direction, not a one-shot");
    }

    #[test]
    fn every_action_round_trips_name_to_bitset_to_input() {
        for action in Action::all() {
            let resolved = Action::from_name(action.name()).expect("name resolves");
            assert_eq!(resolved, action);

            let set = ActionSet::of(&[action]);
            assert!(set.contains(action));
            assert_eq!(set.iter().collect::<Vec<_>>(), vec![action]);

            let input = PlayerInput::from_actions(&[action]);
            assert!(input.held(action));
            assert_eq!(
                input.pressed(action),
                action.is_edge(),
                "`{}` press bit should follow its trigger",
                action.name()
            );
        }
        assert_eq!(Action::from_name("sideways"), None);
    }

    #[test]
    fn presses_can_only_hold_edge_triggered_actions() {
        // A source that claims `left` was "pressed" is filtered, not trusted.
        let input = PlayerInput::new(
            ActionSet::of(&[Action::Left]),
            ActionSet::of(&[Action::Left, Action::Jump]),
        );
        assert!(!input.pressed(Action::Left));
        assert!(input.jump_pressed());
    }

    /// However the input was built. A test that could hold left and right at
    /// once would be driving the avatar with something no keyboard produces.
    #[test]
    fn opposing_directions_cancel() {
        let both = [Action::Left, Action::Right, Action::Down];
        for input in [
            PlayerInput::new(ActionSet::of(&both), ActionSet::EMPTY),
            PlayerInput::from_actions(&both),
            PlayerInput::holding(&both),
        ] {
            assert!(!input.left() && !input.right(), "{input:?}");
            assert!(input.down(), "only the pair cancels");
            assert_eq!(input.dir(), 0.0);
        }
    }

    // --- the latch --------------------------------------------------------

    #[test]
    fn a_press_survives_until_a_tick_consumes_it() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Space);
        // ...however many frames render before the next fixed tick.
        assert!(
            take(&mut latch, Action::Jump),
            "the press is still there when a tick runs"
        );
        assert!(!take(&mut latch, Action::Jump), "and only fires once");
    }

    #[test]
    fn every_jump_key_latches_and_other_keys_do_not() {
        for &key in Action::Jump.binding().keys {
            let mut latch = InputLatch::default();
            latch.key_down(key);
            assert!(take(&mut latch, Action::Jump), "{key:?} should jump");
        }
        let mut latch = InputLatch::default();
        latch.key_down(Key::Left);
        latch.key_down(Key::P);
        assert!(!take(&mut latch, Action::Jump));
    }

    /// Every edge-triggered action gets the same treatment, which is the point
    /// of generalizing the latch: `cast` must not be dropped on the frames a
    /// jump would have been.
    #[test]
    fn every_edge_triggered_action_latches() {
        for binding in ACTIONS.iter().filter(|b| b.is_edge()) {
            let mut latch = InputLatch::default();
            latch.key_down(binding.keys[0]);
            assert!(
                take(&mut latch, binding.action),
                "`{}` should latch on {:?}",
                binding.name,
                binding.keys[0]
            );
        }
    }

    /// Level-triggered actions are read from the keyboard every tick, so
    /// latching them would leave a stale "still held" behind.
    #[test]
    fn level_triggered_actions_are_not_latched() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Left);
        latch.key_down(Key::S);
        assert!(latch.take().is_empty(), "movement is polled, not latched");
    }

    /// Two presses inside one tick are still one jump. The player cannot
    /// physically double-tap in 16 ms, but key repeat and a stalled frame
    /// both look like this, and both used to burn the double jump.
    #[test]
    fn repeated_presses_before_a_tick_collapse_to_one_jump() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Up);
        latch.key_down(Key::Up);
        assert!(take(&mut latch, Action::Jump));
        assert!(!take(&mut latch, Action::Jump));
    }

    #[test]
    fn clearing_drops_a_pending_press() {
        let mut latch = InputLatch::default();
        latch.key_down(Key::Up);
        latch.clear();
        assert!(
            !take(&mut latch, Action::Jump),
            "a press meant for a menu is not a jump"
        );
    }

    // --- documentation ----------------------------------------------------

    /// The tape language's key list is this table, so the two cannot be
    /// allowed to drift. On failure, paste the printed block into the README.
    #[test]
    fn tapes_readme_lists_every_action() {
        const BEGIN: &str = "<!-- actions:begin -->";
        const END: &str = "<!-- actions:end -->";

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tapes/README.md");
        let text = std::fs::read_to_string(&path).expect("tapes/README.md is readable");

        let start = text
            .find(BEGIN)
            .expect("README has an actions:begin marker")
            + BEGIN.len();
        let end = text.find(END).expect("README has an actions:end marker");
        let documented = text[start..end].trim();
        let generated = Action::table_markdown();

        assert_eq!(
            documented,
            generated.trim(),
            "tapes/README.md is out of date; replace the block between the \
             markers with:\n\n{generated}"
        );
    }
}
