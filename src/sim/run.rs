//! Driving a [`Sim`] with a [`Tape`], collecting a [`Trace`] and checking the
//! tape's assertions as it goes.
//!
//! This lives in the library rather than in the `sim` binary so the binary,
//! the integration tests, and anything else all exercise the identical run
//! loop — including the off-by-one that matters: an assertion written after
//! `right 10` is checked once ten ticks have been *stepped*.

use crate::sim::event::EventCounts;
use crate::sim::tape::Tape;
use crate::sim::trace::Trace;
use crate::sim::Sim;
use crate::systems::input::PlayerInput;

/// One assertion that did not hold.
#[derive(Clone, Debug)]
pub struct Failure {
    /// Line number in the tape source.
    pub line: usize,
    /// Ticks stepped when the check ran.
    pub tick: usize,
    /// The assertion as written.
    pub assertion: String,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {} (tick {}): {} — {}",
            self.line, self.tick, self.assertion, self.message
        )
    }
}

pub struct RunOutcome {
    pub trace: Trace,
    pub failures: Vec<Failure>,
}

impl RunOutcome {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Play a tape into a sim. The trace holds the state before the first tick
/// plus one entry per tick, so it has `tape.ticks() + 1` entries.
pub fn run_tape(sim: &mut Sim, tape: &Tape) -> RunOutcome {
    let mut trace = Trace::new();
    let mut failures = Vec::new();
    // Cumulative, so `expect` reads as "by this point in the tape".
    let mut counts = EventCounts::new();

    let record = |sim: &Sim,
                  tick: usize,
                  counts: &EventCounts,
                  trace: &mut Trace,
                  failures: &mut Vec<Failure>| {
        let probe = sim.probe();
        for assertion in tape.asserts_at(tick) {
            if let Err(message) = assertion.evaluate(&probe, counts) {
                failures.push(Failure {
                    line: assertion.line,
                    tick,
                    assertion: assertion.describe(),
                    message,
                });
            }
        }
        trace.push(probe, sim.events());
    };

    // Assertions written before any input describe the starting state, when
    // nothing has happened yet.
    record(sim, 0, &counts, &mut trace, &mut failures);

    for (index, input) in tape.inputs().iter().enumerate() {
        sim.step(*input);
        counts.record(sim.events());
        record(sim, index + 1, &counts, &mut trace, &mut failures);
    }

    RunOutcome { trace, failures }
}

/// Step the sim with no input, for probing a map without authoring a tape.
pub fn run_idle(sim: &mut Sim, ticks: usize) -> Trace {
    let mut trace = Trace::new();
    trace.push(sim.probe(), &[]);
    for _ in 0..ticks {
        sim.step(PlayerInput::default());
        trace.push(sim.probe(), sim.events());
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;
    use crate::sim::GameEvent;

    fn castle() -> Sim {
        Sim::load(&mut Assets::new(), "maps/castle.ron").expect("castle map loads")
    }

    #[test]
    fn trace_has_one_entry_per_tick_plus_the_initial_state() {
        let tape = Tape::parse("right 10").unwrap();
        let outcome = run_tape(&mut castle(), &tape);
        assert_eq!(outcome.trace.len(), 11);
        assert_eq!(outcome.trace.frames()[0].probe.tick, 0);
        assert_eq!(outcome.trace.last().unwrap().probe.tick, 10);
    }

    /// Events land on the tick they happened, not smeared across the trace.
    #[test]
    fn the_trace_records_events_on_the_tick_they_fired() {
        let tape = Tape::parse("wait 10\njump 5").unwrap();
        let outcome = run_tape(&mut castle(), &tape);

        let ticks_with_a_jump: Vec<u64> = outcome
            .trace
            .frames()
            .iter()
            .filter(|f| f.events.contains(&GameEvent::Jumped))
            .map(|f| f.probe.tick)
            .collect();
        assert_eq!(ticks_with_a_jump, vec![11], "one jump, on the press tick");
    }

    /// `expect` counts cumulatively, so a tape can assert something happened
    /// at some point earlier without knowing which tick.
    #[test]
    fn expect_checks_events_accumulated_so_far() {
        let tape = Tape::parse(
            "wait 10\n\
             expect no jumped\n\
             jump 5\n\
             wait 60\n\
             expect jumped\n\
             expect landed >= 1\n\
             expect no died",
        )
        .unwrap();
        let outcome = run_tape(&mut castle(), &tape);
        assert!(outcome.passed(), "unexpected failures: {:?}", outcome.failures);
    }

    #[test]
    fn a_violated_expect_reports_the_actual_count() {
        let tape = Tape::parse("wait 10\nexpect died").unwrap();
        let outcome = run_tape(&mut castle(), &tape);
        assert_eq!(outcome.failures.len(), 1);
        assert!(
            outcome.failures[0].message.contains("it fired 0"),
            "{}",
            outcome.failures[0].message
        );
    }

    #[test]
    fn satisfied_assertions_pass() {
        let tape = Tape::parse("wait 10\nassert grounded\nassert vx == 0").unwrap();
        let outcome = run_tape(&mut castle(), &tape);
        assert!(outcome.passed(), "unexpected failures: {:?}", outcome.failures);
    }

    #[test]
    fn violated_assertions_report_tick_line_and_actual_value() {
        let tape = Tape::parse("wait 10\nassert !grounded\nassert x > 99999").unwrap();
        let outcome = run_tape(&mut castle(), &tape);
        assert_eq!(outcome.failures.len(), 2);
        assert_eq!(outcome.failures[0].tick, 10);
        assert_eq!(outcome.failures[0].line, 2);
        assert!(outcome.failures[1].message.contains("x was"));
    }
}
