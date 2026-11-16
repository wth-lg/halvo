#![cfg(target_os = "windows")]
//! The cases of x8's phase function. They need no desktop and make no call of
//! the system; the windows job runs them in its test step.

mod stamp_phases;

use stamp_phases::{period_ms, phase_of, Phase, TICK_PERIOD_FALLBACK_MS};

const ELEVATED_FROM: u128 = 10_000;
const ELEVATED_TO: u128 = 10_400;
const MEDIUM_FROM: u128 = 11_000;
const MEDIUM_TO: u128 = 11_400;
const P: u128 = 16;

fn phase(stamp: u128) -> Phase {
    phase_of(stamp, ELEVATED_FROM, ELEVATED_TO, MEDIUM_FROM, MEDIUM_TO, P)
}

#[test]
fn a_stamp_one_period_before_medium_from_is_of_the_medium_phase() {
    assert_eq!(phase(MEDIUM_FROM - P), Phase::Medium);
}

#[test]
fn a_stamp_one_ms_earlier_is_counted_as_a_key_that_crossed() {
    assert_eq!(phase(MEDIUM_FROM - P - 1), Phase::BetweenThePhases);
    assert!(phase(MEDIUM_FROM - P - 1).fails());
}

#[test]
fn a_stamp_two_periods_before_medium_from_is_counted_as_a_key_that_crossed() {
    assert!(phase(MEDIUM_FROM - 2 * P).fails());
}

#[test]
fn a_stamp_one_period_before_elevated_from_is_counted() {
    assert_eq!(phase(ELEVATED_FROM - P), Phase::Elevated);
    assert!(phase(ELEVATED_FROM - P).fails());
}

#[test]
fn a_stamp_one_ms_earlier_lies_before_the_phases() {
    assert_eq!(phase(ELEVATED_FROM - P - 1), Phase::BeforeThePhases);
    assert!(!phase(ELEVATED_FROM - P - 1).fails());
}

#[test]
fn a_stamp_one_period_after_medium_to_is_of_the_medium_phase() {
    assert_eq!(phase(MEDIUM_TO + P), Phase::Medium);
}

#[test]
fn a_stamp_one_ms_later_lies_after_the_phases() {
    assert_eq!(phase(MEDIUM_TO + P + 1), Phase::AfterThePhases);
    assert!(!phase(MEDIUM_TO + P + 1).fails());
}

#[test]
fn phases_two_periods_apart_or_closer_cannot_be_told_apart() {
    for elevated_to in [MEDIUM_FROM - 2 * P, MEDIUM_FROM - 1, MEDIUM_FROM] {
        for stamp in [ELEVATED_FROM, MEDIUM_FROM - P - 1, MEDIUM_FROM] {
            let found = phase_of(stamp, ELEVATED_FROM, elevated_to, MEDIUM_FROM, MEDIUM_TO, P);
            assert_eq!(found, Phase::Indistinct, "elevated_to={elevated_to}");
            assert!(!found.fails());
        }
    }
    let apart = phase_of(
        MEDIUM_FROM - P - 1,
        ELEVATED_FROM,
        MEDIUM_FROM - 2 * P - 1,
        MEDIUM_FROM,
        MEDIUM_TO,
        P,
    );
    assert!(apart.fails(), "{apart:?}");
}

#[test]
fn the_period_is_the_interrupt_interval_rounded_up_to_whole_ms() {
    assert_eq!(period_ms(156_250), Some(16));
    assert_eq!(period_ms(10_000), Some(1));
    assert_eq!(period_ms(10_001), Some(2));
    assert_eq!(period_ms(0), None);
    assert_eq!(period_ms(156_250), Some(TICK_PERIOD_FALLBACK_MS));
}
