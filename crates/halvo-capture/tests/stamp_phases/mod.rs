//! Which phase of x8 a listener's stamp belongs to.
//!
//! The boundaries come from the precise wall clock. A stamp is the wall clock
//! less the age of its event by two readings of the tick counter, so it lies
//! within one tick period of its key, before it or after it. Each phase is
//! therefore widened by one period on both sides.

/// The tick period that stands where the system gives none: the default
/// interval between clock interrupts is 15.625 ms, rounded up to whole ms.
pub const TICK_PERIOD_FALLBACK_MS: u128 = 16;

/// The interval between clock interrupts, which GetSystemTimeAdjustment gives
/// in 100 ns units, rounded up to whole ms; none for an interval of 0.
pub fn period_ms(increment_100ns: u32) -> Option<u128> {
    (increment_100ns > 0).then(|| u128::from(increment_100ns).div_ceil(10_000))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The phases lie two periods apart or closer, so no stamp tells them apart.
    Indistinct,
    BeforeThePhases,
    /// From elevated_from less the period to elevated_to plus the period.
    Elevated,
    /// After the elevated phase, up to and not including medium_from less the period.
    BetweenThePhases,
    /// From medium_from less the period to medium_to plus the period.
    Medium,
    AfterThePhases,
}

impl Phase {
    /// A key of the elevated phase that reached the medium-integrity listener.
    pub fn fails(self) -> bool {
        matches!(self, Phase::Elevated | Phase::BetweenThePhases)
    }
}

pub fn phase_of(
    stamp: u128,
    elevated_from: u128,
    elevated_to: u128,
    medium_from: u128,
    medium_to: u128,
    period: u128,
) -> Phase {
    if medium_from.saturating_sub(elevated_to) <= 2 * period {
        return Phase::Indistinct;
    }
    if stamp < elevated_from.saturating_sub(period) {
        Phase::BeforeThePhases
    } else if stamp <= elevated_to + period {
        Phase::Elevated
    } else if stamp < medium_from - period {
        Phase::BetweenThePhases
    } else if stamp <= medium_to + period {
        Phase::Medium
    } else {
        Phase::AfterThePhases
    }
}
