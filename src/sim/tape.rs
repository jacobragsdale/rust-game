//! Input tapes: scripted keypress sequences that drive the sim deterministically.
//!
//! A tape replaces a human at the keyboard. It exists so that "does the wall
//! jump work?" is a question answerable in 50 ms without a window, a GPU, or
//! OS-level keystroke injection — and so that the answer keeps being checked
//! forever once the feature ships.
//!
//! ```text
//! # tapes/wall_jump.tape
//! map maps/testbed_chimney.ron
//! right 40          # run at the chimney
//! right+jump 1      # jump
//! right 12
//! jump 1            # wall jump off the right wall
//! left 20
//! assert x < 900
//! assert !grounded
//! ```
//!
//! Each line is `<keys> [count]`, where `<keys>` is `+`-separated from the
//! names in [`crate::systems::input::ACTIONS`] (`left right down up jump
//! attack cast ...`), or `wait`/`none` for no input. `count` defaults to 1.
//! `#` starts a comment. Three directives are also recognized: `map <path>`
//! names the map to run against, `seed <n>` fixes the simulation's random
//! seed, and `assert ...` checks the player's state at the tick where it
//! appears.
//!
//! The subtle part is [`Tape::inputs`]: `jump_pressed` is edge-triggered while
//! `jump_held` is level-triggered, exactly as
//! [`crate::systems::input::InputLatch`] delivers them from the keyboard. The
//! tape reproduces that by diffing consecutive ticks, so `jump 10` is one jump
//! held for ten ticks — what actually happens when you hold the key — rather
//! than ten jumps. Get this wrong and tapes would perform inputs no human
//! could. (One press reaching two ticks is exactly the bug the latch exists to
//! prevent, so a tape must never express it either.) Every edge-triggered
//! action is diffed the same way, so a new one needs no work here.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{bail, Context as _};

use crate::assets::{Assets, DialogueTable, ItemTable, SpellTable, StatTable};
use crate::sim::event::{EventCounts, GameEvent, Subject, MODES};
use crate::sim::rng;
use crate::sim::trace::{Frame, ProbePath};
use crate::systems::input::{Action, PlayerInput};

/// Which actions are down on a single tick. The same bitset the keyboard
/// reader fills, so a tape cannot express an input a player could not.
pub use crate::systems::input::ActionSet as Keys;

/// Runaway-tape guard: 100k ticks is ~28 minutes of game time.
const MAX_TICKS: usize = 100_000;

/// Tolerance for `==` / `!=` on floats.
const EPSILON: f32 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Op {
    fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "<" => Op::Lt,
            "<=" => Op::Le,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "==" | "=" => Op::Eq,
            "!=" => Op::Ne,
            _ => return None,
        })
    }

    fn apply(self, lhs: f32, rhs: f32) -> bool {
        match self {
            Op::Lt => lhs < rhs,
            Op::Le => lhs <= rhs,
            Op::Gt => lhs > rhs,
            Op::Ge => lhs >= rhs,
            Op::Eq => (lhs - rhs).abs() <= EPSILON,
            Op::Ne => (lhs - rhs).abs() > EPSILON,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Check {
    Flag {
        name: String,
        expected: bool,
    },
    Compare {
        field: String,
        op: Op,
        value: f32,
    },
    /// A string field against a literal. Only equality makes sense, which is
    /// enough for what these are: `assert clip == die`, `assert knight.0.kind
    /// == knight`.
    CompareText {
        field: String,
        op: Op,
        value: String,
    },
    /// How many times an event has fired since the tape started, optionally
    /// narrowed to who it happened to.
    Event {
        subject: Option<String>,
        name: String,
        op: Op,
        count: u32,
    },
}

/// An assertion bound to the tick at which it appears in the tape.
#[derive(Clone, Debug)]
pub struct Assertion {
    /// Number of ticks that must have been stepped before checking.
    pub tick: usize,
    /// Source line number, for error messages.
    pub line: usize,
    pub check: Check,
}

impl Assertion {
    pub fn evaluate(&self, frame: &Frame, events: &EventCounts) -> Result<(), String> {
        match &self.check {
            Check::Flag { name, expected } => {
                let actual = frame
                    .flag(name)
                    .ok_or_else(|| format!("`{name}` does not resolve in this frame"))?;
                if actual == *expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {}{name}, but it was {actual}",
                        if *expected { "" } else { "!" }
                    ))
                }
            }
            Check::Compare { field, op, value } => {
                let actual = frame
                    .field(field)
                    .ok_or_else(|| format!("`{field}` does not resolve in this frame"))?;
                if op.apply(actual, *value) {
                    Ok(())
                } else {
                    let mut message = format!(
                        "expected {field} {} {value}, but {field} was {actual}",
                        op_str(*op)
                    );
                    // A quest flag nobody has set reads as 0 rather than
                    // failing, so a mistyped flag name looks exactly like a
                    // quest that has not started. Say which flags exist, which
                    // tells the two apart at a glance.
                    if frame.flag_value_is_absent(field) {
                        message.push_str(&format!(
                            " (nothing has set `{field}`; {})",
                            frame.flag_list()
                        ));
                    }
                    Err(message)
                }
            }
            Check::CompareText { field, op, value } => {
                let actual = frame
                    .text(field)
                    .ok_or_else(|| format!("`{field}` does not resolve in this frame"))?;
                let equal = actual == value;
                if (*op == Op::Eq) == equal {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {field} {} {value}, but {field} was {actual}",
                        op_str(*op)
                    ))
                }
            }
            Check::Event {
                subject,
                name,
                op,
                count,
            } => {
                let actual = match subject {
                    Some(who) => events.count_for(who, name),
                    None => events.count(name),
                };
                if op.apply(actual as f32, *count as f32) {
                    Ok(())
                } else {
                    let label = match subject {
                        Some(who) => format!("{who}.{name}"),
                        None => name.clone(),
                    };
                    Err(format!(
                        "expected `{label}` {} {count} times by now, but it fired {actual}",
                        op_str(*op)
                    ))
                }
            }
        }
    }

    /// How this assertion reads in the tape, for error messages.
    pub fn describe(&self) -> String {
        match &self.check {
            Check::Flag { name, expected } => {
                format!("assert {}{name}", if *expected { "" } else { "!" })
            }
            Check::Compare { field, op, value } => {
                format!("assert {field} {} {value}", op_str(*op))
            }
            Check::CompareText { field, op, value } => {
                format!("assert {field} {} {value}", op_str(*op))
            }
            Check::Event {
                subject,
                name,
                op,
                count,
            } => {
                let label = match subject {
                    Some(who) => format!("{who}.{name}"),
                    None => name.clone(),
                };
                format!("expect {label} {} {count}", op_str(*op))
            }
        }
    }
}

fn op_str(op: Op) -> &'static str {
    match op {
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Eq => "==",
        Op::Ne => "!=",
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tape {
    /// Map this tape is written against, from a `map <path>` directive.
    /// Tapes carry their own map so that a runner can replay any tape
    /// without being told which fixture it belongs to.
    pub map: Option<String>,
    /// Random seed, from a `seed <n>` directive. Carried for the same reason
    /// the map is: a tape plus its map must be the whole of what a golden
    /// trace depends on, or the trace stops being reproducible from the files.
    pub seed: Option<u64>,
    /// Keys held, one entry per tick.
    pub keys: Vec<Keys>,
    pub asserts: Vec<Assertion>,
}

impl Tape {
    pub fn load(path: &Path) -> anyhow::Result<Tape> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read tape {}", path.display()))?;
        Tape::parse(&text).with_context(|| format!("invalid tape {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Tape> {
        let mut tape = Tape::default();

        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let content = raw.split('#').next().unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }

            let mut tokens = content.split_whitespace();
            let head = tokens.next().expect("non-empty line has a first token");

            if head == "map" {
                let path = tokens
                    .next()
                    .with_context(|| format!("line {line}: `map` needs a path"))?;
                if tokens.next().is_some() {
                    bail!("line {line}: `map` takes exactly one path");
                }
                if tape.map.is_some() {
                    bail!("line {line}: `map` is already set");
                }
                tape.map = Some(path.to_string());
                continue;
            }

            if head == "seed" {
                let raw = tokens
                    .next()
                    .with_context(|| format!("line {line}: `seed` needs a number"))?;
                let seed: u64 = raw
                    .parse()
                    .with_context(|| format!("line {line}: `{raw}` is not a seed"))?;
                if tokens.next().is_some() {
                    bail!("line {line}: `seed` takes exactly one number");
                }
                if tape.seed.is_some() {
                    bail!("line {line}: `seed` is already set");
                }
                tape.seed = Some(seed);
                continue;
            }

            if head == "assert" || head == "expect" {
                let rest: Vec<&str> = tokens.collect();
                let check = if head == "expect" {
                    parse_expect(&rest)
                } else {
                    parse_check(&rest)
                }
                .with_context(|| format!("line {line}: `{content}`"))?;
                tape.asserts.push(Assertion {
                    tick: tape.keys.len(),
                    line,
                    check,
                });
                continue;
            }

            let keys = parse_keys(head).with_context(|| format!("line {line}: `{content}`"))?;
            let count = match tokens.next() {
                None => 1usize,
                Some(n) => n
                    .parse::<usize>()
                    .with_context(|| format!("line {line}: `{n}` is not a tick count"))?,
            };
            if count == 0 {
                bail!("line {line}: tick count must be at least 1");
            }
            if let Some(extra) = tokens.next() {
                bail!("line {line}: unexpected trailing token `{extra}`");
            }
            if tape.keys.len() + count > MAX_TICKS {
                bail!("line {line}: tape exceeds the {MAX_TICKS} tick limit");
            }

            tape.keys.resize(tape.keys.len() + count, keys);
        }

        Ok(tape)
    }

    pub fn ticks(&self) -> usize {
        self.keys.len()
    }

    /// The seed this tape runs at, defaulting to the constant every sim
    /// starts from.
    pub fn seed(&self) -> u64 {
        self.seed.unwrap_or(rng::DEFAULT_SEED)
    }

    /// Compile held-key sets into per-tick `PlayerInput`, deriving the
    /// edge-triggered presses by diffing against the previous tick — the same
    /// way `input::read` derives them from the keyboard latch.
    ///
    /// Holding a key must be one press, not one per tick, for every one-shot
    /// action there is: `PlayerInput::new` keeps only the edge-triggered bits
    /// of the diff, so nothing here needs to know which actions those are.
    pub fn inputs(&self) -> Vec<PlayerInput> {
        let mut prev = Keys::default();
        self.keys
            .iter()
            .map(|&keys| {
                // `PlayerInput::new` applies the rest of what the keyboard
                // reader applies, including left and right cancelling.
                let input = PlayerInput::new(keys, keys.newly_set(prev));
                prev = keys;
                input
            })
            .collect()
    }

    /// Assertions that should be checked once `tick` ticks have been stepped.
    pub fn asserts_at(&self, tick: usize) -> impl Iterator<Item = &Assertion> {
        self.asserts.iter().filter(move |a| a.tick == tick)
    }
}

/// Resolve a `+`-separated token against the action table. Adding an action
/// to that table is all it takes to be able to write it in a tape.
fn parse_keys(token: &str) -> anyhow::Result<Keys> {
    let mut keys = Keys::default();
    for part in token.split('+') {
        match part {
            "wait" | "none" | "idle" => {}
            name => match Action::from_name(name) {
                Some(action) => keys.insert(action),
                None => bail!(
                    "unknown key `{name}` (expected one of {}, or wait)",
                    Action::name_list()
                ),
            },
        }
    }
    Ok(keys)
}

fn parse_check(tokens: &[&str]) -> anyhow::Result<Check> {
    match tokens {
        [flag] => {
            let (name, expected) = match flag.strip_prefix('!') {
                Some(rest) => (rest, false),
                None => (*flag, true),
            };
            let path =
                ProbePath::parse(name).with_context(|| format!("`{name}` is not a valid path"))?;
            if path.is_flag() {
                Ok(Check::Flag {
                    name: name.to_string(),
                    expected,
                })
            } else {
                bail!(
                    "`{name}` is not a boolean field (known: {})",
                    path.known_names()
                )
            }
        }
        [field, op, value] => {
            let op =
                Op::parse(op).with_context(|| format!("`{op}` is not a comparison operator"))?;
            let path = ProbePath::parse(field)
                .with_context(|| format!("`{field}` is not a valid path"))?;

            // The field decides how the literal is read. Numeric fields parse
            // it as a number; text fields take it verbatim, so a clip called
            // `attack2` is not mistaken for arithmetic.
            if path.is_field() {
                let value: f32 = value
                    .parse()
                    .with_context(|| format!("`{value}` is not a number"))?;
                Ok(Check::Compare {
                    field: field.to_string(),
                    op,
                    value,
                })
            } else if path.is_text() {
                if !matches!(op, Op::Eq | Op::Ne) {
                    bail!(
                        "`{field}` is text, so only `==` and `!=` apply, not `{}`",
                        op_str(op)
                    );
                }
                Ok(Check::CompareText {
                    field: field.to_string(),
                    op,
                    value: value.to_string(),
                })
            } else {
                bail!(
                    "`{field}` is not a comparable field (known: {})",
                    path.known_names()
                )
            }
        }
        _ => bail!("expected `assert <flag>`, `assert !<flag>`, or `assert <field> <op> <value>`"),
    }
}

/// `expect <event>`, `expect no <event>`, or `expect <event> <op> <count>`,
/// where `<event>` may be qualified as `<subject>.<event>`.
///
/// Counts are cumulative over every tick so far, which is how a tape actually
/// reads: after writing `right+jump 25` you want to say the player landed
/// *somewhere* in there, not work out which tick it was. `expect landed` is
/// therefore `>= 1`, and `expect no died` is `== 0`.
///
/// The event **and its subject** are both resolved here, against
/// [`GameEvent::names`] and against the shipped content tables respectively.
/// The subject half is the one that is not obvious: see the comment on it
/// below for why a typo there is otherwise permanent and silent.
fn parse_expect(tokens: &[&str]) -> anyhow::Result<Check> {
    let (name, op, count) = match tokens {
        ["no", name] => (*name, Op::Eq, 0),
        [name] => (*name, Op::Ge, 1),
        [name, op, count] => {
            let op =
                Op::parse(op).with_context(|| format!("`{op}` is not a comparison operator"))?;
            let count: u32 = count
                .parse()
                .with_context(|| format!("`{count}` is not an event count"))?;
            (*name, op, count)
        }
        _ => bail!(
            "expected `expect <event>`, `expect no <event>`, or `expect <event> <op> <count>`"
        ),
    };

    // A quest flag is state, so it belongs to `assert` — and it is the mistake
    // this line exists to answer, because a quest is almost entirely events
    // and a stage is the one part of it that is not. Caught before the split
    // below, which would otherwise report `rescue.stage` as an unknown event.
    if matches!(ProbePath::parse(name), Some(ProbePath::Flag(_))) {
        bail!(
            "`{name}` is a quest flag, which is state rather than something that \
             happened — write `assert {name} {} {count}`",
            op_str(op)
        );
    }

    // `knight.damaged` narrows to one victim; a bare `damaged` counts every
    // one. Only events that record a subject can be narrowed.
    let (subject, name) = match name.split_once('.') {
        Some((who, event)) => (Some(who.to_string()), event),
        None => (None, name),
    };

    if !GameEvent::names().contains(&name) {
        bail!(
            "`{name}` is not an event (known events: {})",
            GameEvent::known_names()
        );
    }
    // The subject is checked here rather than at run time because *here* is
    // where a mistake is still cheap. `EventCounts::count_for` answers "how
    // many times did this happen to nobody?" with 0, which is indistinguishable
    // from the truth: `expect no shcok.spell_cast` passes while shock is cast
    // four times, and `expect no <attack>.attacked` — the line
    // `tapes/spell_kill.tape` uses to prove the kill came from the spell and
    // not the sword — evaporates on one transposed letter, silently and
    // permanently. Every subject is an id out of a content table, so the set
    // of things it could have been is loaded and finite.
    match (&subject, GameEvent::subject_kind(name)) {
        (None, _) => {}
        (Some(_), None) => bail!(
            "`{name}` does not record who it happened to, so it cannot be \
             narrowed to one (events that can: {})",
            GameEvent::subject_names()
        ),
        (Some(who), Some(kind)) => {
            let known = known_subjects(kind);
            if !known.iter().any(|k| k == who) {
                bail!(
                    "`{who}` is not {} — which is what `{name}` records (known: {})",
                    kind.label(),
                    known.join(", ")
                );
            }
        }
    }
    Ok(Check::Event {
        subject,
        name: name.to_string(),
        op,
        count,
    })
}

/// Every name the shipped content offers for a subject of this kind, sorted.
///
/// Read once per process, the way [`StatTable::shipped`] and its counterparts
/// are, and for the same reason: a headless test run parses every tape in the
/// repo, and content that does not load is a broken build rather than a
/// condition to degrade around. Parsing a tape therefore depends on
/// `assets/data/`, which is the price of the error naming the line — and the
/// alternative, checking as the tape runs, reports a typo at tick 400 of a run
/// that had to be set up first.
fn known_subjects(kind: Subject) -> &'static [String] {
    struct Tables {
        kinds: Vec<String>,
        attacks: Vec<String>,
        spells: Vec<String>,
        items: Vec<String>,
        modes: Vec<String>,
        graphs: Vec<String>,
        nodes: Vec<String>,
    }

    static TABLES: OnceLock<Tables> = OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        let owned = |ids: Vec<&str>| ids.into_iter().map(str::to_string).collect();
        let attacks = Assets::new()
            .attacks()
            .expect("assets/data/attacks.ron should load");
        let mut attack_ids: Vec<String> = attacks.0.keys().cloned().collect();
        attack_ids.sort();
        // Node ids are per graph, so the set a tape can name is their union —
        // `expect greet.choice_taken` does not say which conversation, and
        // does not need to: the count is keyed on the node name alone.
        let dialogue = DialogueTable::shipped();
        let mut nodes: Vec<String> = dialogue
            .ids()
            .iter()
            .filter_map(|id| dialogue.get(id))
            .flat_map(|graph| graph.node_ids().into_iter().map(str::to_string))
            .collect();
        nodes.sort();
        nodes.dedup();

        Tables {
            kinds: owned(StatTable::shipped().kinds()),
            attacks: attack_ids,
            spells: owned(SpellTable::shipped().ids()),
            items: owned(ItemTable::shipped().ids()),
            modes: MODES.iter().map(|m| m.name().to_string()).collect(),
            graphs: owned(dialogue.ids()),
            nodes,
        }
    });

    match kind {
        Subject::Kind => &tables.kinds,
        Subject::Attack => &tables.attacks,
        Subject::Spell => &tables.spells,
        Subject::Item => &tables.items,
        Subject::Mode => &tables.modes,
        Subject::Graph => &tables.graphs,
        Subject::Node => &tables.nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_counts_into_per_tick_keys() {
        let tape = Tape::parse("right 3\nwait 2\n").unwrap();
        assert_eq!(tape.ticks(), 5);
        assert!(tape.keys[0].contains(Action::Right));
        assert!(tape.keys[2].contains(Action::Right));
        assert_eq!(tape.keys[3], Keys::default());
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let tape = Tape::parse("# header\n\nright 2   # trailing\n\n").unwrap();
        assert_eq!(tape.ticks(), 2);
    }

    #[test]
    fn parses_key_combinations() {
        let tape = Tape::parse("right+jump 1\ndown+left 1").unwrap();
        assert!(tape.keys[0].contains(Action::Right) && tape.keys[0].contains(Action::Jump));
        assert!(tape.keys[1].contains(Action::Down) && tape.keys[1].contains(Action::Left));
    }

    /// The point of the action table: a new action is spelled in tapes with no
    /// parser change at all. `cast` is bound to nothing in gameplay yet.
    #[test]
    fn every_action_in_the_table_is_a_tape_key() {
        for action in Action::all() {
            let source = format!("{} 1", action.name());
            let tape = Tape::parse(&source).unwrap_or_else(|e| panic!("`{source}`: {e:#}"));
            assert!(
                tape.keys[0].contains(action),
                "`{}` did not resolve to its action",
                action.name()
            );
        }

        let tape = Tape::parse("cast 1").expect("`cast 1` parses");
        assert!(tape.inputs()[0].pressed(Action::Cast));
    }

    /// An unknown key names the whole table, so the fix is in the message.
    #[test]
    fn an_unknown_key_lists_the_actions() {
        let err = format!("{:#}", Tape::parse("sideways 3").unwrap_err());
        assert!(err.contains("left, right, down, up, jump"), "{err}");
    }

    /// The whole reason tapes are trustworthy: holding jump for N ticks must
    /// be one jump, not N. Otherwise a tape could triple-jump where a player
    /// could not.
    #[test]
    fn jump_is_edge_triggered_but_stays_held() {
        let tape = Tape::parse("jump 5").unwrap();
        let inputs = tape.inputs();
        assert!(inputs[0].jump_pressed(), "press fires on the first tick");
        assert!(
            inputs[1..].iter().all(|i| !i.jump_pressed()),
            "and never again while held"
        );
        assert!(
            inputs.iter().all(|i| i.jump_held()),
            "but the key stays held throughout"
        );
    }

    #[test]
    fn releasing_and_repressing_jump_fires_again() {
        let tape = Tape::parse("jump 2\nwait 2\njump 2").unwrap();
        let inputs = tape.inputs();
        let presses: Vec<usize> = inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.jump_pressed())
            .map(|(t, _)| t)
            .collect();
        assert_eq!(presses, vec![0, 4]);
    }

    /// ...and that is true of every one-shot action, not just the two that
    /// existed when the rule was written.
    #[test]
    fn every_edge_triggered_action_fires_once_per_press() {
        for action in Action::all().filter(|a| a.is_edge()) {
            let name = action.name();
            let tape = Tape::parse(&format!("{name} 3\nwait 2\n{name} 3")).unwrap();
            let presses: Vec<usize> = tape
                .inputs()
                .iter()
                .enumerate()
                .filter(|(_, i)| i.pressed(action))
                .map(|(t, _)| t)
                .collect();
            assert_eq!(presses, vec![0, 5], "`{name}` should fire twice, not six");
        }
    }

    /// Level-triggered actions have no press at all — holding is the whole of
    /// what `left` means.
    #[test]
    fn level_triggered_actions_never_report_a_press() {
        let tape = Tape::parse("left 3\ndown 3\nup 3").unwrap();
        for input in tape.inputs() {
            for action in Action::all().filter(|a| !a.is_edge()) {
                assert!(!input.pressed(action), "`{}` is not a press", action.name());
            }
        }
    }

    #[test]
    fn opposing_directions_cancel_like_the_keyboard() {
        let tape = Tape::parse("left+right 1").unwrap();
        let input = tape.inputs()[0];
        assert!(!input.left() && !input.right());
    }

    #[test]
    fn seed_directive_is_captured_and_defaults_to_the_constant() {
        let tape = Tape::parse("right 2").unwrap();
        assert_eq!(tape.seed, None);
        assert_eq!(tape.seed(), rng::DEFAULT_SEED);

        let tape = Tape::parse("seed 12345\nright 2").unwrap();
        assert_eq!(tape.seed(), 12345);
        assert_eq!(tape.ticks(), 2, "the directive costs no ticks");
    }

    #[test]
    fn asserts_bind_to_the_tick_they_appear_at() {
        let tape = Tape::parse("assert grounded\nright 10\nassert x > 5\nright 5").unwrap();
        assert_eq!(tape.asserts.len(), 2);
        assert_eq!(tape.asserts[0].tick, 0);
        assert_eq!(tape.asserts[1].tick, 10);
    }

    #[test]
    fn parses_negated_flags_and_all_operators() {
        let tape = Tape::parse("assert !grounded\nassert vy <= 0\nassert air_jumps == 1").unwrap();
        assert_eq!(tape.asserts.len(), 3);
        assert!(matches!(
            tape.asserts[0].check,
            Check::Flag {
                expected: false,
                ..
            }
        ));
    }

    #[test]
    fn map_directive_is_captured() {
        let tape = Tape::parse("map maps/testbed.ron\nright 2").unwrap();
        assert_eq!(tape.map.as_deref(), Some("maps/testbed.ron"));
        assert_eq!(tape.ticks(), 2, "the directive costs no ticks");
    }

    /// Clip names are what make animation *selection* testable: that the
    /// death animation plays when something dies, and that a swing is not
    /// painted over by the run cycle.
    #[test]
    fn text_fields_compare_by_equality() {
        let tape =
            Tape::parse("assert clip == idle\nassert clip != run\nassert knight.0.clip == death")
                .unwrap();
        assert_eq!(tape.asserts.len(), 3);
        assert!(matches!(tape.asserts[0].check, Check::CompareText { .. }));
    }

    #[test]
    fn ordering_operators_are_rejected_on_text() {
        let err = Tape::parse("assert clip > run").unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("only `==` and `!=`"), "{text}");
    }

    /// A clip named `attack2` must not be read as a number.
    #[test]
    fn a_numeric_looking_clip_name_stays_text() {
        let tape = Tape::parse("assert clip == attack2").unwrap();
        match &tape.asserts[0].check {
            Check::CompareText { value, .. } => assert_eq!(value, "attack2"),
            other => panic!("expected a text comparison, got {other:?}"),
        }
    }

    /// Q-1's path syntax, from the tape parser's side: a flag is a numeric
    /// field, its literal is read as a number, and it is not a boolean.
    #[test]
    fn quest_flags_parse_as_numeric_comparisons() {
        let tape =
            Tape::parse("assert quest.helm.stage == 2\nassert quest.helm.stage < 3").unwrap();
        assert_eq!(tape.asserts.len(), 2);
        match &tape.asserts[0].check {
            Check::Compare { field, value, .. } => {
                assert_eq!(field, "quest.helm.stage");
                assert_eq!(*value, 2.0);
            }
            other => panic!("expected a numeric comparison, got {other:?}"),
        }

        let err = format!("{:#}", Tape::parse("assert quest.helm.stage").unwrap_err());
        assert!(err.contains("not a boolean"), "{err}");
    }

    /// A stage is state, and state is `assert`. The mistake is worth catching
    /// by name because a quest is otherwise almost entirely events.
    #[test]
    fn a_flag_written_as_an_expect_is_redirected_to_assert() {
        let err = format!(
            "{:#}",
            Tape::parse("expect quest.rescue.stage == 2").unwrap_err()
        );
        assert!(err.contains("quest flag"), "{err}");
        assert!(
            err.contains("assert quest.rescue.stage == 2"),
            "the message should contain the line that would have worked: {err}"
        );
    }

    /// One subject of every shape the events use, all of them out of shipped
    /// content. If a content id is renamed and no tape is updated, this is
    /// where it shows up.
    #[test]
    fn every_shape_of_subject_resolves() {
        for line in [
            "expect knight.damaged == 1",
            "expect no player.died",
            "expect player_slash1.attacked >= 1",
            "expect shock.spell_cast == 1",
            "expect shock.cast_failed == 1",
            "expect minor_potion.picked_up == 1",
            "expect knight_helm.equipped == 1",
            "expect inventory.mode_changed == 1",
            "expect elder_intro.dialogue_opened == 1",
            "expect elder_intro.interact_prompted == 1",
            "expect greet.choice_taken == 1",
        ] {
            Tape::parse(line).unwrap_or_else(|e| panic!("`{line}`: {e:#}"));
        }
    }

    /// **The bug this check exists for.** `EventCounts::count_for` answers
    /// "how many times did this happen to nobody?" with 0, so a transposed
    /// letter turns `expect no <attack>.attacked` — the line `spell_kill.tape`
    /// proves the kill came from the spell with — into a line that can never
    /// fail. Every one of these used to pass.
    #[test]
    fn a_mistyped_subject_is_rejected_and_the_message_lists_the_real_ones() {
        for (line, expected) in [
            ("expect no shcok.spell_cast", "shock"),
            ("expect no knigt.damaged", "knight"),
            ("expect no player_plunj.attacked", "player_plunge"),
            ("expect no minor_potio.picked_up", "minor_potion"),
            ("expect bag.mode_changed", "inventory"),
        ] {
            let err = format!("{:#}", Tape::parse(line).unwrap_err());
            assert!(err.contains("is not"), "`{line}` should be rejected: {err}");
            assert!(
                err.contains(expected),
                "`{line}` should name `{expected}` as something that resolves: {err}"
            );
        }
    }

    /// A subject out of the *wrong* table is the same mistake wearing a real
    /// id: `greet` is a node, `elder_intro` is a graph, and swapping them
    /// counts nothing forever.
    #[test]
    fn a_subject_from_the_wrong_table_is_rejected() {
        let err = format!(
            "{:#}",
            Tape::parse("expect greet.dialogue_opened").unwrap_err()
        );
        assert!(err.contains("dialogue graph id"), "{err}");
        let err = format!(
            "{:#}",
            Tape::parse("expect no elder_intro.choice_taken").unwrap_err()
        );
        assert!(err.contains("dialogue node id"), "{err}");
    }

    /// Every event that can be narrowed has somewhere for its subjects to come
    /// from, and that somewhere is not empty — an empty table would reject
    /// every tape that used the event, which is worse than the bug.
    #[test]
    fn every_narrowable_event_has_a_non_empty_namespace() {
        for name in GameEvent::names() {
            let Some(kind) = GameEvent::subject_kind(name) else {
                continue;
            };
            assert!(
                !known_subjects(kind).is_empty(),
                "`{name}` narrows on {} and nothing defines any",
                kind.label()
            );
        }
    }

    /// The other half of "an unset flag reads as 0": nothing can fail to
    /// resolve, so a failure has to say what is actually set — otherwise a
    /// mistyped flag name and a quest that has not started are the same
    /// message.
    #[test]
    fn a_failure_against_an_unset_flag_lists_the_flags_that_are_set() {
        use crate::sim::trace::Frame;
        use std::collections::BTreeMap;

        let tape = Tape::parse("assert quest.helm.stage == 2").unwrap();
        let frame = Frame {
            probe: crate::sim::Sim::fixture(&["..P...", "######"]).probe(),
            npcs: Vec::new(),
            items: Vec::new(),
            flags: BTreeMap::from([("quest.draught.stage".to_string(), 1)]),
            events: Vec::new(),
            seed: None,
        };
        let err = tape.asserts[0]
            .evaluate(&frame, &EventCounts::new())
            .unwrap_err();
        assert!(err.contains("nothing has set `quest.helm.stage`"), "{err}");
        assert!(err.contains("quest.draught.stage = 1"), "{err}");

        // A flag that *is* set gets the ordinary message, with no lecture.
        let tape = Tape::parse("assert quest.draught.stage == 2").unwrap();
        let err = tape.asserts[0]
            .evaluate(&frame, &EventCounts::new())
            .unwrap_err();
        assert!(!err.contains("nothing has set"), "{err}");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Tape::parse("map").is_err(), "map without a path");
        assert!(
            Tape::parse("map a.ron\nmap b.ron").is_err(),
            "duplicate map"
        );
        assert!(Tape::parse("sideways 3").is_err(), "unknown key");
        assert!(Tape::parse("seed").is_err(), "seed without a number");
        assert!(Tape::parse("seed x").is_err(), "seed that is not a number");
        assert!(
            Tape::parse("seed 1 2").is_err(),
            "seed with a trailing token"
        );
        assert!(Tape::parse("seed 1\nseed 2").is_err(), "duplicate seed");
        assert!(Tape::parse("right zero").is_err(), "bad count");
        assert!(Tape::parse("right 0").is_err(), "zero count");
        assert!(Tape::parse("right 3 extra").is_err(), "trailing token");
        assert!(Tape::parse("assert nonsense").is_err(), "unknown flag");
        assert!(Tape::parse("assert x ~ 3").is_err(), "bad operator");
        assert!(Tape::parse("assert grounded > 3").is_err(), "flag as field");
        assert!(Tape::parse("assert x > yes").is_err(), "bad value");
        assert!(
            Tape::parse("assert clip < run").is_err(),
            "ordering on text"
        );
        assert!(
            Tape::parse("assert nonsense == x").is_err(),
            "unknown text field"
        );
        assert!(Tape::parse("expect nonsense").is_err(), "unknown event");
        assert!(
            Tape::parse("expect knight.jumped").is_err(),
            "an event that records nobody cannot be narrowed"
        );
        assert!(
            Tape::parse("expect nobody.damaged").is_err(),
            "unknown subject"
        );
    }
}
