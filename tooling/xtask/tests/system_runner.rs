#![cfg(all(feature = "dev", unix))]
//! The real runner: a command that a signal of an interrupt ended sets the
//! interrupt, and one that failed by itself does not. It runs `sh` only.

use std::process::Command;
use xtask::{Interrupt, Runner, SystemRunner};

#[test]
fn a_child_ended_by_an_interrupt_signal_sets_the_interrupt() {
    for signal in ["INT", "TERM", "HUP"] {
        let interrupt = Interrupt::default();
        let mut runner = SystemRunner {
            interrupt: interrupt.clone(),
        };
        let failure = runner
            .run(Command::new("sh").args(["-c", &format!("kill -{signal} $$")]))
            .unwrap_err();
        assert!(failure.starts_with("sh exited with signal"), "{failure}");
        assert!(interrupt.is_set(), "SIG{signal} is an interrupt");
    }
}

#[test]
fn a_child_that_fails_by_itself_is_no_interrupt() {
    let interrupt = Interrupt::default();
    let mut runner = SystemRunner {
        interrupt: interrupt.clone(),
    };
    runner
        .run(Command::new("sh").args(["-c", "exit 3"]))
        .unwrap_err();
    runner
        .output(Command::new("sh").args(["-c", "kill -KILL $$"]))
        .unwrap_err();
    assert!(!interrupt.is_set());
}
