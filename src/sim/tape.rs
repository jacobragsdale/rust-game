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
//! Each line is `<keys> [count]`, where `<keys>` is `+`-separated from
//! `left right down jump`, or `wait`/`none` for no input. `count` defaults
//! to 1. `#` starts a comment. Two directives are also recognized:
//! `map <path>` names the map to run against, and `assert ...` checks the
//! player's state at the tick where it appears.
//!
//! The subtle part is [`Tape::inputs`]: `PlayerInput::jump_pressed` is
//! edge-triggered while `jump_held` is level-triggered, exactly as
//! `input::read` derives them from the keyboard. The tape reproduces that by
//! diffing consecutive ticks, so `jump 10` is one jump held for ten ticks —
//! what actually happens when you hold the key — rather than ten jumps. Get
//! this wrong and tapes would perform inputs no human could.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context as _};

use crate::sim::Probe;
use crate::systems::input::PlayerInput;

/// Runaway-tape guard: 100k ticks is ~28 minutes of game time.
const MAX_TICKS: usize = 100_000;

/// Tolerance for `==` / `!=` on floats.
const EPSILON: f32 = 1e-3;

/// Which keys are down on a single tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub left: bool,
    pub right: bool,
    pub down: bool,
    pub jump: bool,
}

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
    Flag { name: String, expected: bool },
    Compare { field: String, op: Op, value: f32 },
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
    pub fn evaluate(&self, probe: &Probe) -> Result<(), String> {
        match &self.check {
            Check::Flag { name, expected } => {
                let actual = probe
                    .flag(name)
                    .ok_or_else(|| format!("unknown flag `{name}`"))?;
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
                let actual = probe
                    .field(field)
                    .ok_or_else(|| format!("unknown field `{field}`"))?;
                if op.apply(actual, *value) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {field} {} {value}, but {field} was {actual}",
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

            if head == "assert" {
                let rest: Vec<&str> = tokens.collect();
                let check = parse_check(&rest)
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

    /// Compile held-key sets into per-tick `PlayerInput`, deriving the
    /// edge-triggered `jump_pressed` by diffing against the previous tick —
    /// the same way `input::read` derives it from the keyboard.
    pub fn inputs(&self) -> Vec<PlayerInput> {
        let mut prev = Keys::default();
        self.keys
            .iter()
            .map(|&keys| {
                let input = PlayerInput {
                    // left and right cancel, matching `input::read`
                    left: keys.left && !keys.right,
                    right: keys.right && !keys.left,
                    down: keys.down,
                    jump_pressed: keys.jump && !prev.jump,
                    jump_held: keys.jump,
                };
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

fn parse_keys(token: &str) -> anyhow::Result<Keys> {
    let mut keys = Keys::default();
    for part in token.split('+') {
        match part {
            "wait" | "none" | "idle" => {}
            "left" => keys.left = true,
            "right" => keys.right = true,
            "down" => keys.down = true,
            "jump" => keys.jump = true,
            other => bail!(
                "unknown key `{other}` (expected left, right, down, jump, or wait)"
            ),
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
            if Probe::flag_names().contains(&name) {
                Ok(Check::Flag {
                    name: name.to_string(),
                    expected,
                })
            } else {
                bail!(
                    "`{name}` is not a boolean field (known fields: {})",
                    Probe::known_names()
                )
            }
        }
        [field, op, value] => {
            let op = Op::parse(op)
                .with_context(|| format!("`{op}` is not a comparison operator"))?;
            let value: f32 = value
                .parse()
                .with_context(|| format!("`{value}` is not a number"))?;
            if Probe::field_names().contains(field) {
                Ok(Check::Compare {
                    field: field.to_string(),
                    op,
                    value,
                })
            } else {
                bail!(
                    "`{field}` is not a numeric field (known fields: {})",
                    Probe::known_names()
                )
            }
        }
        _ => bail!("expected `assert <flag>`, `assert !<flag>`, or `assert <field> <op> <value>`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_counts_into_per_tick_keys() {
        let tape = Tape::parse("right 3\nwait 2\n").unwrap();
        assert_eq!(tape.ticks(), 5);
        assert!(tape.keys[0].right);
        assert!(tape.keys[2].right);
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
        assert!(tape.keys[0].right && tape.keys[0].jump);
        assert!(tape.keys[1].down && tape.keys[1].left);
    }

    /// The whole reason tapes are trustworthy: holding jump for N ticks must
    /// be one jump, not N. Otherwise a tape could triple-jump where a player
    /// could not.
    #[test]
    fn jump_is_edge_triggered_but_stays_held() {
        let tape = Tape::parse("jump 5").unwrap();
        let inputs = tape.inputs();
        assert!(inputs[0].jump_pressed, "press fires on the first tick");
        assert!(
            inputs[1..].iter().all(|i| !i.jump_pressed),
            "and never again while held"
        );
        assert!(
            inputs.iter().all(|i| i.jump_held),
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
            .filter(|(_, i)| i.jump_pressed)
            .map(|(t, _)| t)
            .collect();
        assert_eq!(presses, vec![0, 4]);
    }

    #[test]
    fn opposing_directions_cancel_like_the_keyboard() {
        let tape = Tape::parse("left+right 1").unwrap();
        let input = tape.inputs()[0];
        assert!(!input.left && !input.right);
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
        let tape =
            Tape::parse("assert !grounded\nassert vy <= 0\nassert air_jumps == 1").unwrap();
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

    #[test]
    fn rejects_bad_input() {
        assert!(Tape::parse("map").is_err(), "map without a path");
        assert!(Tape::parse("map a.ron\nmap b.ron").is_err(), "duplicate map");
        assert!(Tape::parse("sideways 3").is_err(), "unknown key");
        assert!(Tape::parse("right zero").is_err(), "bad count");
        assert!(Tape::parse("right 0").is_err(), "zero count");
        assert!(Tape::parse("right 3 extra").is_err(), "trailing token");
        assert!(Tape::parse("assert nonsense").is_err(), "unknown flag");
        assert!(Tape::parse("assert x ~ 3").is_err(), "bad operator");
        assert!(Tape::parse("assert grounded > 3").is_err(), "flag as field");
        assert!(Tape::parse("assert x > yes").is_err(), "bad value");
    }
}
