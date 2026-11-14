//! The owner's counters for the life of the process, the handles that sources
//! fill, and the snapshot that the owner publishes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use crate::event::Modality;
use crate::floor::exclude::DropReason;

const KINDS: usize = 8;

fn kind_index(kind: Modality) -> usize {
    match kind {
        Modality::ScreenFrame => 0,
        Modality::Keystroke => 1,
        Modality::Pointer => 2,
        Modality::ForegroundApp => 3,
        Modality::WindowTitle => 4,
        Modality::SystemAudio => 5,
        Modality::Microphone => 6,
        Modality::Camera => 7,
    }
}

/// Why the owner discarded a received event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardReason {
    /// It was received while the owner discards, between a pause's barrier
    /// and the next resume.
    Barrier,
    /// The exclusion decision dropped it.
    Exclusion(DropReason),
    /// It came from an emit of the old form, which has no stored record.
    OldPath,
}

/// The counters that sources fill: each handle is shared with its source.
#[derive(Debug, Clone, Default)]
pub struct SourceTallies {
    tap_disabled_timeout: Arc<AtomicU64>,
    tap_disabled_user_input: Arc<AtomicU64>,
    secure_input_keystrokes: Arc<AtomicU64>,
}

impl SourceTallies {
    pub fn tap_disabled_timeout(&self) -> Arc<AtomicU64> {
        self.tap_disabled_timeout.clone()
    }

    pub fn tap_disabled_user_input(&self) -> Arc<AtomicU64> {
        self.tap_disabled_user_input.clone()
    }

    pub fn secure_input_keystrokes(&self) -> Arc<AtomicU64> {
        self.secure_input_keystrokes.clone()
    }
}

/// What the owner counted, as one value read at one moment by the owner's
/// thread, so a relation of two of its counts holds as the owner saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    received: [u64; KINDS],
    pub events_persisted: u64,
    pub discarded_by_barrier: u64,
    pub discarded_by_exclusion: u64,
    pub discarded_old_path: u64,
    pub events_lost: u64,
    pub events_pending: u64,
    pub segments_written: u64,
    pub tap_disabled_timeout: u64,
    pub tap_disabled_user_input: u64,
    pub secure_input_keystrokes: u64,
    pub disk_bytes: Option<u64>,
    pub last_flush_at: Option<SystemTime>,
    pub last_expiry_at: Option<SystemTime>,
    pub last_eviction_at: Option<SystemTime>,
}

impl Snapshot {
    /// The events of `kind` that the owner received.
    pub fn received(&self, kind: Modality) -> u64 {
        self.received[kind_index(kind)]
    }

    /// The sum of the counts of every kind.
    pub fn events_received(&self) -> u64 {
        self.received.iter().sum()
    }

    pub fn events_discarded(&self) -> u64 {
        self.discarded_by_barrier + self.discarded_by_exclusion + self.discarded_old_path
    }

    /// Whether every received event is persisted, discarded, lost or pending.
    pub fn is_conserved(&self) -> bool {
        self.events_received()
            == self.events_persisted
                + self.events_discarded()
                + self.events_lost
                + self.events_pending
    }
}

/// The owner's own counts; only the owner's thread changes them.
#[derive(Debug, Default)]
pub struct Counters {
    received: [u64; KINDS],
    persisted: u64,
    by_barrier: u64,
    by_exclusion: u64,
    old_path: u64,
    lost: u64,
    segments: u64,
    disk_bytes: Option<u64>,
    last_flush_at: Option<SystemTime>,
    last_expiry_at: Option<SystemTime>,
    last_eviction_at: Option<SystemTime>,
    tallies: SourceTallies,
}

impl Counters {
    pub fn tallies(&self) -> &SourceTallies {
        &self.tallies
    }

    pub fn received(&mut self, kind: Modality) {
        self.received[kind_index(kind)] += 1;
    }

    pub fn discarded(&mut self, reason: DiscardReason) {
        match reason {
            DiscardReason::Barrier => self.by_barrier += 1,
            DiscardReason::Exclusion(_) => self.by_exclusion += 1,
            DiscardReason::OldPath => self.old_path += 1,
        }
    }

    pub fn lost(&mut self, events: u64) {
        self.lost += events;
    }

    /// A segment of `events` events was acknowledged at `at`, `disk_bytes`
    /// more on disk.
    pub fn persisted(&mut self, events: u64, disk_bytes: u64, at: SystemTime) {
        self.persisted += events;
        self.segments += 1;
        self.last_flush_at = Some(at);
        self.disk_bytes = self.disk_bytes.map(|total| total + disk_bytes);
    }

    pub fn disk_bytes(&mut self, total: u64) {
        self.disk_bytes = Some(total);
    }

    pub fn expired(&mut self, at: SystemTime) {
        self.last_expiry_at = Some(at);
    }

    pub fn evicted(&mut self, at: SystemTime) {
        self.last_eviction_at = Some(at);
    }

    /// The snapshot with `pending` events held in the batch and the spill.
    pub fn snapshot(&self, pending: u64) -> Snapshot {
        let read = |count: &Arc<AtomicU64>| count.load(Ordering::Relaxed);
        Snapshot {
            received: self.received,
            events_persisted: self.persisted,
            discarded_by_barrier: self.by_barrier,
            discarded_by_exclusion: self.by_exclusion,
            discarded_old_path: self.old_path,
            events_lost: self.lost,
            events_pending: pending,
            segments_written: self.segments,
            tap_disabled_timeout: read(&self.tallies.tap_disabled_timeout),
            tap_disabled_user_input: read(&self.tallies.tap_disabled_user_input),
            secure_input_keystrokes: read(&self.tallies.secure_input_keystrokes),
            disk_bytes: self.disk_bytes,
            last_flush_at: self.last_flush_at,
            last_expiry_at: self.last_expiry_at,
            last_eviction_at: self.last_eviction_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counters_and_the_snapshot_stay_under_4_kib() {
        let bound = 4096;
        assert!(std::mem::size_of::<Snapshot>() < bound);
        assert!(std::mem::size_of::<Counters>() < bound);
        assert!(std::mem::size_of::<Snapshot>() + std::mem::size_of::<Counters>() < bound);
    }

    #[test]
    fn every_kind_has_its_own_count_and_the_sum_is_received() {
        let mut counters = Counters::default();
        for (n, kind) in Modality::ALL.into_iter().enumerate() {
            for _ in 0..=n {
                counters.received(kind);
            }
        }
        let snapshot = counters.snapshot(0);
        for (n, kind) in Modality::ALL.into_iter().enumerate() {
            assert_eq!(snapshot.received(kind), n as u64 + 1, "{kind:?}");
        }
        assert_eq!(snapshot.events_received(), 36);
    }

    #[test]
    fn the_source_handles_reach_the_snapshot() {
        let counters = Counters::default();
        counters
            .tallies()
            .secure_input_keystrokes()
            .fetch_add(2, Ordering::Relaxed);
        counters
            .tallies()
            .tap_disabled_timeout()
            .fetch_add(3, Ordering::Relaxed);
        counters
            .tallies()
            .tap_disabled_user_input()
            .fetch_add(4, Ordering::Relaxed);
        let snapshot = counters.snapshot(0);
        assert_eq!(snapshot.secure_input_keystrokes, 2);
        assert_eq!(snapshot.tap_disabled_timeout, 3);
        assert_eq!(snapshot.tap_disabled_user_input, 4);
    }
}
