//! The owner's batch: the records of one segment write in a payload buffer of
//! fixed capacity, their per-kind entries, their one UTC capture day and their
//! first and last capture instant; and the one constructor of stored text.

use std::fmt;
use std::io;
use std::time::Duration;

use halvo_control::model::UtcDay;
use zeroize::Zeroize;

use super::port::{EntryKind, KindEntry, SegmentId, SegmentWrite, ENTRIES_MAX};

pub use stored::{StoredText, TextFilter, TextFilters};

/// The most events of one batch.
pub const BATCH_MAX_EVENTS: usize = 512;

/// The longest that a batch stays open after its first event, on the
/// monotonic clock.
pub const BATCH_MAX_AGE: Duration = Duration::from_secs(5);

const NANOS_PER_DAY: i64 = 86_400 * 1_000_000_000;

/// The UTC day of an instant in signed unix nanoseconds.
pub fn utc_day(at: i64) -> UtcDay {
    let days = at.div_euclid(NANOS_PER_DAY);
    UtcDay::from_days_since_epoch(days).unwrap_or(if days < 0 { UtcDay::MIN } else { UtcDay::MAX })
}

/// A byte buffer whose capacity is fixed when it is made. It has no growing
/// push, [`PayloadBuffer::wipe`] zeroes what it held, and so does its drop.
pub struct PayloadBuffer {
    bytes: Box<[u8]>,
    len: usize,
}

impl PayloadBuffer {
    pub fn with_capacity(capacity: usize) -> PayloadBuffer {
        PayloadBuffer {
            bytes: vec![0; capacity].into_boxed_slice(),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.bytes.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends `line` and a line end when both fit, and says whether they did.
    pub(super) fn push_line(&mut self, line: &[u8]) -> bool {
        let end = self.len + line.len() + 1;
        if end > self.bytes.len() {
            return false;
        }
        self.bytes[self.len..end - 1].copy_from_slice(line);
        self.bytes[end - 1] = b'\n';
        self.len = end;
        true
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Zeroes the bytes that the buffer held and empties it.
    pub fn wipe(&mut self) {
        self.bytes[..self.len].zeroize();
        self.len = 0;
    }

    #[cfg(test)]
    fn block(&self) -> (*const u8, usize) {
        (self.bytes.as_ptr(), self.bytes.len())
    }
}

impl Drop for PayloadBuffer {
    fn drop(&mut self) {
        self.bytes.zeroize();
        #[cfg(test)]
        seen_at_free("payload", self.bytes.as_ptr(), self.bytes.len());
    }
}

#[cfg(test)]
thread_local! {
    static WIPES: std::cell::RefCell<Option<Vec<(&'static str, bool)>>> =
        const { std::cell::RefCell::new(None) };
}

/// In a test that watches with [`watch_wipes`], records whether the `len`
/// bytes at `block` read zero after the drop code has run and before the block
/// is given back.
#[cfg(test)]
pub(super) fn seen_at_free(what: &'static str, block: *const u8, len: usize) {
    WIPES.with(|wipes| {
        if let Some(log) = wipes.borrow_mut().as_mut() {
            // SAFETY: the caller's block is still allocated, and its first
            // `len` bytes were written, by the wipe or before it.
            let bytes = unsafe { std::slice::from_raw_parts(block, len) };
            log.push((what, bytes.iter().all(|byte| *byte == 0)));
        }
    });
}

/// Runs `run` and returns, for each block that a drop gave back on this
/// thread meanwhile, what it held and whether it read zero.
#[cfg(test)]
pub(super) fn watch_wipes(run: impl FnOnce()) -> Vec<(&'static str, bool)> {
    WIPES.with(|wipes| *wipes.borrow_mut() = Some(Vec::new()));
    run();
    WIPES.with(|wipes| wipes.borrow_mut().take().unwrap_or_default())
}

impl fmt::Debug for PayloadBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PayloadBuffer(..)")
    }
}

/// Whether a record can join the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    Fits,
    /// The batch must be written first: it is full, the record would not fit,
    /// or the record is of another UTC day.
    FlushFirst,
    /// The record is larger than the empty buffer.
    TooLarge,
}

/// The records of one segment write.
pub struct Batch {
    payload: PayloadBuffer,
    entries: Vec<KindEntry>,
    events: usize,
    day: Option<UtcDay>,
    first_capture: i64,
    last_capture: i64,
    opened_at: Option<Duration>,
    segment_id: Option<SegmentId>,
}

impl Batch {
    /// An empty batch whose payload buffer holds `capacity` bytes, allocated
    /// here once.
    pub fn with_capacity(capacity: usize) -> Batch {
        Batch {
            payload: PayloadBuffer::with_capacity(capacity),
            entries: Vec::with_capacity(ENTRIES_MAX + 1),
            events: 0,
            day: None,
            first_capture: 0,
            last_capture: 0,
            opened_at: None,
            segment_id: None,
        }
    }

    pub fn events(&self) -> usize {
        self.events
    }

    pub fn is_empty(&self) -> bool {
        self.events == 0
    }

    /// The bytes of the records with their line ends.
    pub fn record_bytes(&self) -> usize {
        self.payload.len()
    }

    /// The first and the last capture instant of the records, in signed unix
    /// nanoseconds; both 0 for an empty batch.
    pub fn captures(&self) -> (i64, i64) {
        (self.first_capture, self.last_capture)
    }

    pub fn admits(&self, day: UtcDay, record_len: usize) -> Admit {
        if record_len + 1 > self.payload.capacity() {
            return Admit::TooLarge;
        }
        let other_day = self.day.is_some_and(|open| open != day);
        if self.events >= BATCH_MAX_EVENTS
            || other_day
            || self.payload.len() + record_len + 1 > self.payload.capacity()
        {
            return Admit::FlushFirst;
        }
        Admit::Fits
    }

    /// Adds one record of `kind` captured at `at`; `now` is the monotonic
    /// reading. The caller has had [`Admit::Fits`] for it.
    pub(super) fn push(&mut self, kind: EntryKind, at: i64, record: &[u8], now: Duration) {
        let pushed = self.payload.push_line(record);
        debug_assert!(pushed, "a record was pushed that the batch did not admit");
        if !pushed {
            return;
        }
        let bytes = record.len() as u64 + 1;
        match self.entries.iter_mut().find(|entry| entry.kind == kind) {
            Some(entry) => {
                entry.events += 1;
                entry.bytes += bytes;
            }
            None => self.entries.push(KindEntry {
                kind,
                events: 1,
                bytes,
            }),
        }
        if self.events == 0 {
            self.day = Some(utc_day(at));
            self.first_capture = at;
            self.last_capture = at;
            self.opened_at = Some(now);
        } else {
            self.first_capture = self.first_capture.min(at);
            self.last_capture = self.last_capture.max(at);
        }
        self.events += 1;
    }

    /// The monotonic reading at which the batch reaches [`BATCH_MAX_AGE`].
    pub fn deadline(&self) -> Option<Duration> {
        self.opened_at.map(|opened| opened + BATCH_MAX_AGE)
    }

    /// Whether the batch holds [`BATCH_MAX_EVENTS`] events or has reached its
    /// age at `now`.
    pub fn is_due(&self, now: Duration) -> bool {
        self.events >= BATCH_MAX_EVENTS || self.deadline().is_some_and(|due| now >= due)
    }

    /// The segment id of the batch: minted at the first call and kept until
    /// the batch is cleared, so every retry carries the same id.
    pub fn segment_id(&mut self) -> io::Result<SegmentId> {
        match self.segment_id {
            Some(id) => Ok(id),
            None => {
                let id = SegmentId::mint()?;
                self.segment_id = Some(id);
                Ok(id)
            }
        }
    }

    /// The write of the batch under `segment_id`, or `None` for an empty batch.
    pub fn write<'a>(&'a self, segment_id: &'a SegmentId) -> Option<SegmentWrite<'a>> {
        Some(SegmentWrite::new(
            segment_id,
            self.day?,
            self.first_capture,
            self.last_capture,
            &self.entries,
            self.payload.as_bytes(),
        ))
    }

    #[cfg(test)]
    pub(crate) fn payload_block(&self) -> (*const u8, usize) {
        self.payload.block()
    }

    #[cfg(test)]
    pub(crate) fn payload_whole(&self) -> Vec<u8> {
        self.payload.bytes.to_vec()
    }

    /// Wipes the payload buffer and empties the batch for reuse.
    pub fn clear(&mut self) {
        self.payload.wipe();
        self.entries.clear();
        self.events = 0;
        self.day = None;
        self.opened_at = None;
        self.segment_id = None;
    }
}

mod stored {
    use std::fmt;

    use zeroize::Zeroize;

    use crate::floor::site::SITE_MAX_BYTES;
    use crate::floor::text::{TextField, TextFilterPoint, TEXT_MAX_BYTES};

    /// A text as it is stored: an app id, a title text or a site domain after
    /// the registered filters. [`StoredText::filtered`] is its only
    /// constructor. Its `Debug` prints a placeholder and its drop wipes it.
    pub struct StoredText(String);

    impl StoredText {
        /// `raw` passed through each filter of `filters` in their order, for
        /// `field`; with no filter registered it is `raw` unchanged.
        pub fn filtered(field: TextField, raw: &str, filters: &TextFilters) -> StoredText {
            let mut text = raw.to_owned();
            for filter in &filters.filters {
                let next = filter.apply(field, &text);
                #[cfg(test)]
                let (block, len) = (text.as_ptr(), text.len());
                text.zeroize();
                #[cfg(test)]
                super::seen_at_free("between", block, len);
                text = next;
            }
            StoredText(text)
        }
    }

    impl AsRef<str> for StoredText {
        fn as_ref(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Debug for StoredText {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("StoredText(..)")
        }
    }

    impl Drop for StoredText {
        fn drop(&mut self) {
            #[cfg(test)]
            let (block, len) = (self.0.as_ptr(), self.0.len());
            self.0.zeroize();
            #[cfg(test)]
            super::seen_at_free("stored", block, len);
        }
    }

    /// One filter of stored text: it maps a text of a field to the text that
    /// is stored.
    pub trait TextFilter: Send + Sync {
        fn apply(&self, field: TextField, text: &str) -> String;
    }

    impl<F: Fn(TextField, &str) -> String + Send + Sync> TextFilter for F {
        fn apply(&self, field: TextField, text: &str) -> String {
            self(field, text)
        }
    }

    /// The ordered set of the registered filters.
    pub struct TextFilters {
        filters: Vec<Box<dyn TextFilter>>,
    }

    impl TextFilters {
        /// A set with no filter.
        pub fn empty() -> TextFilters {
            TextFilters {
                filters: Vec::new(),
            }
        }

        /// The filters of R1: the byte cap and nothing else. A later run
        /// registers its filter ahead of the cap.
        pub fn r1() -> TextFilters {
            let mut filters = TextFilters::empty();
            #[cfg(feature = "r2")]
            filters.register(crate::daemon::r2::text_filter());
            filters.register(byte_cap);
            filters
        }

        /// Adds `filter` after the filters registered before it.
        pub fn register(&mut self, filter: impl TextFilter + 'static) {
            self.filters.push(Box::new(filter));
        }
    }

    impl TextFilterPoint for TextFilters {
        type Stored = StoredText;

        fn filter(&self, field: TextField, text: &str) -> StoredText {
            StoredText::filtered(field, text, self)
        }
    }

    /// The most bytes that a stored text of `field` holds.
    pub const fn cap_of(field: TextField) -> usize {
        match field {
            TextField::BundleId | TextField::ExecutablePath | TextField::Title => TEXT_MAX_BYTES,
            TextField::Site => SITE_MAX_BYTES,
        }
    }

    fn byte_cap(field: TextField, text: &str) -> String {
        let max = cap_of(field);
        if text.len() <= max {
            return text.to_owned();
        }
        if field.keeps_tail() {
            let mut start = text.len() - max;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text[start..].to_owned()
        } else {
            let mut end = max;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::daemon::model_store::{
        foreground, foreground_now, frame, key, title_of, Harness, AFTER_MIDNIGHT, AT_NOON, NOON,
    };
    use crate::daemon::owner::Command;
    use crate::daemon::port::PAYLOAD_MAX_BYTES;
    use crate::event::Modality;
    use crate::floor::record::DecodedPayload;
    use crate::floor::text::{TextField, TextFilterPoint};

    const FIELDS: [TextField; 4] = [
        TextField::BundleId,
        TextField::ExecutablePath,
        TextField::Title,
        TextField::Site,
    ];

    #[test]
    fn no_registered_filter_returns_the_input_bytes() {
        let none = TextFilters::empty();
        let long = "é".repeat(2000);
        for field in FIELDS {
            for raw in [
                "",
                "com.example.editor",
                "C:\\Program Files\\x.exe",
                long.as_str(),
            ] {
                let stored = StoredText::filtered(field, raw, &none);
                assert_eq!(stored.as_ref().as_bytes(), raw.as_bytes(), "{field:?}");
            }
        }
    }

    #[test]
    fn the_cap_filter_cuts_at_a_character_boundary_and_keeps_the_tail_of_a_windows_app_id() {
        let r1 = TextFilters::r1();
        let caps = [
            (TextField::BundleId, 1024),
            (TextField::ExecutablePath, 1024),
            (TextField::Title, 1024),
            (TextField::Site, 253),
        ];
        for (field, cap) in caps {
            let raw = if field.keeps_tail() {
                format!("{}é{}", "b".repeat(cap), "a".repeat(cap - 1))
            } else {
                format!("{}é{}", "a".repeat(cap - 1), "b".repeat(cap))
            };
            let stored = StoredText::filtered(field, &raw, &r1);
            let text = stored.as_ref();
            assert!(text.len() <= cap, "{field:?} kept {} bytes", text.len());
            assert!(text.len() >= cap - 1, "{field:?} cut too much");
            assert!(raw.contains(text));
            if field.keeps_tail() {
                assert!(raw.ends_with(text), "{field:?} did not keep its tail");
            } else {
                assert!(raw.starts_with(text), "{field:?} did not keep its head");
            }
            let short = "x".repeat(cap);
            assert_eq!(StoredText::filtered(field, &short, &r1).as_ref(), short);
        }
        let path = format!("C:\\{}\\Programs\\Editor\\editor.exe", "deep\\".repeat(400));
        let stored = StoredText::filtered(TextField::ExecutablePath, &path, &r1);
        assert!(stored.as_ref().ends_with("\\Editor\\editor.exe"));
        assert!(stored.as_ref().len() <= 1024);
        assert_eq!(
            r1.filter(TextField::Site, &"s".repeat(300)).as_ref().len(),
            253
        );
    }

    trait AmbiguousIfImplemented<Marker> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfImplemented<()> for T {}

    #[test]
    fn a_text_field_is_written_from_stored_text_only() {
        fn stored_of<P: TextFilterPoint<Stored = StoredText>>(_point: &P) {}
        stored_of(&TextFilters::r1());

        struct IsFromString;
        impl<T: From<String>> AmbiguousIfImplemented<IsFromString> for T {}
        struct IsFromStr;
        impl<T: for<'a> From<&'a str>> AmbiguousIfImplemented<IsFromStr> for T {}
        struct IsParse;
        impl<T: std::str::FromStr> AmbiguousIfImplemented<IsParse> for T {}
        struct IsDefault;
        impl<T: Default> AmbiguousIfImplemented<IsDefault> for T {}
        struct IsDeserialize;
        impl<T: serde::de::DeserializeOwned> AmbiguousIfImplemented<IsDeserialize> for T {}
        let _ = <StoredText as AmbiguousIfImplemented<_>>::check;

        let written: <TextFilters as TextFilterPoint>::Stored =
            StoredText::filtered(TextField::Title, "a title", &TextFilters::r1());
        assert_eq!(format!("{written:?}"), "StoredText(..)");
    }

    #[test]
    fn the_payload_buffer_never_grows() {
        let mut buffer = PayloadBuffer::with_capacity(PAYLOAD_MAX_BYTES);
        let block = buffer.block();
        assert_eq!(block.1, 1_048_576);
        let line = [b'x'; 4095];
        let pushed = (0..300).filter(|_| buffer.push_line(&line)).count();
        assert_eq!(pushed, 256);
        assert_eq!(buffer.len(), 1_048_576);
        assert!(!buffer.push_line(b""), "a full buffer took a line end");
        assert_eq!(buffer.block(), block, "the buffer moved or grew");
        buffer.wipe();
        assert!(buffer.is_empty());
        assert_eq!(buffer.block(), block);
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let owner_block = harness.owner.payload_block();
        assert_eq!(owner_block.1, 1_048_576);
        for _ in 0..1200 {
            harness.emit(title_of(1024, 1024));
            harness.pass();
        }
        assert!(harness.store.batches().len() >= 2);
        assert_eq!(
            harness.owner.payload_block(),
            owner_block,
            "the owner's buffer moved or grew"
        );
    }

    #[test]
    fn the_payload_block_is_wiped_before_it_is_given_back() {
        let seen = watch_wipes(|| {
            let mut buffer = PayloadBuffer::with_capacity(64);
            assert!(buffer.push_line(b"a title of a person"));
            drop(buffer);
        });
        assert_eq!(seen, [("payload", true)]);
    }

    #[test]
    fn the_text_between_two_filters_is_wiped_before_it_is_given_back() {
        let mut filters = TextFilters::empty();
        filters.register(|_field: TextField, text: &str| format!("{text} one"));
        filters.register(|_field: TextField, text: &str| format!("{text} two"));
        let mut kept = None;
        let seen = watch_wipes(|| {
            kept = Some(StoredText::filtered(TextField::Title, "a title", &filters));
        });
        assert_eq!(seen, [("between", true), ("between", true)]);
        assert_eq!(
            kept.as_ref().map(|text| text.as_ref()),
            Some("a title one two")
        );
    }

    #[test]
    fn a_stored_text_is_wiped_before_it_is_given_back() {
        let stored = StoredText::filtered(TextField::Title, "a title", &TextFilters::empty());
        let seen = watch_wipes(|| drop(stored));
        assert_eq!(seen, [("stored", true)]);
    }

    #[test]
    fn the_payload_block_stays_in_place_and_reads_zero_after_each_flush() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let block = harness.owner.payload_block();
        for round in 0..3 {
            for _ in 0..100 {
                harness.emit(title_of(1024, 1024));
                harness.pass();
            }
            harness.ask(Command::Settle);
            assert_eq!(
                harness.owner.payload_block(),
                block,
                "round {round}: the buffer moved"
            );
            assert!(
                harness.owner.payload_bytes().iter().all(|&b| b == 0),
                "round {round}: the buffer held record bytes after its flush"
            );
        }
        assert_eq!(harness.store.records().len(), 300);
    }

    #[test]
    fn a_batch_of_title_records_at_their_caps_flushes_at_the_byte_budget() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        for _ in 0..511 {
            harness.emit(title_of(1024, 1024));
            harness.pass();
        }
        let batches = harness.store.batches();
        assert!(
            !batches.is_empty(),
            "no flush before the 512-event threshold"
        );
        let first = &batches[0];
        let events: u64 = first.entries.iter().map(|entry| entry.events).sum();
        assert!(events < 512, "the first batch held {events} events");
        assert!(first.payload.len() <= PAYLOAD_MAX_BYTES);
        assert!(
            first.payload.len() + 2100 > PAYLOAD_MAX_BYTES,
            "flushed early"
        );
    }

    #[test]
    fn each_kind_with_a_constructor_keeps_its_record_shape_through_a_flush() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let events = [foreground_now(), key(), frame(), title_of(5, 7)];
        let ids: Vec<_> = events.iter().map(|event| event.id().uuid()).collect();
        for event in events {
            harness.emit(event);
        }
        harness.ask(Command::Settle);
        let records = harness.store.records();
        assert_eq!(records.iter().map(|r| r.id()).collect::<Vec<_>>(), ids);
        let kinds: Vec<_> = records.iter().map(|r| r.kind()).collect();
        assert_eq!(
            kinds,
            [
                "foreground_app",
                "keystroke",
                "screen_frame",
                "window_title"
            ]
        );
        assert!(matches!(
            records[0].payload(),
            DecodedPayload::ForegroundApp
        ));
        assert!(matches!(records[1].payload(), DecodedPayload::Keystroke(_)));
        assert!(matches!(records[2].payload(), DecodedPayload::ScreenFrame));
        match records[3].payload() {
            DecodedPayload::WindowTitle { text, .. } => assert_eq!(text.as_str(), "ttttttt"),
            _ => panic!("the fourth record is no title"),
        }
        assert_eq!(records[3].app(), Some("aaaaa"));
        let seqs: Vec<_> = records.iter().map(|r| r.seq()).collect();
        assert!(
            seqs.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "{seqs:?}"
        );
        let batches = harness.store.batches();
        let total: u64 = batches
            .iter()
            .flat_map(|batch| &batch.entries)
            .map(|entry| entry.events)
            .sum();
        assert_eq!(total, 4);
        for batch in &batches {
            let bytes: u64 = batch.entries.iter().map(|entry| entry.bytes).sum();
            assert_eq!(bytes, batch.payload.len() as u64);
        }
        assert!(batches
            .iter()
            .flat_map(|batch| &batch.entries)
            .any(|entry| entry.kind == EntryKind::of(Modality::Keystroke)));
    }

    #[test]
    fn a_batch_crossing_utc_midnight_is_two_writes() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        harness.emit(foreground(&AFTER_MIDNIGHT, Duration::from_secs(3)));
        harness.emit(foreground(&AFTER_MIDNIGHT, Duration::from_secs(2)));
        harness.emit(foreground(&AFTER_MIDNIGHT, Duration::ZERO));
        harness.ask(Command::Settle);
        let batches = harness.store.batches();
        assert_eq!(batches.len(), 2, "one write spanned two days");
        let day = |nanos: i64| utc_day(nanos).days_since_epoch();
        assert_eq!(
            batches[0].day.days_since_epoch(),
            (NOON / 86_400 - 1) as i64
        );
        assert_eq!(batches[1].day.days_since_epoch(), (NOON / 86_400) as i64);
        for batch in &batches {
            assert_eq!(day(batch.first_capture), batch.day.days_since_epoch());
            assert_eq!(day(batch.last_capture), batch.day.days_since_epoch());
        }
        let counts: Vec<u64> = batches.iter().map(|b| b.entries[0].events).collect();
        assert_eq!(counts, [2, 1]);
    }

    #[test]
    fn first_capture_is_the_minimum_when_emitters_interleave() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let ages = [1, 3, 0, 2];
        for age in ages {
            harness.emit(foreground(&AT_NOON, Duration::from_secs(age)));
        }
        harness.ask(Command::Settle);
        let batches = harness.store.batches();
        assert_eq!(batches.len(), 1);
        let noon = i64::try_from(Duration::from_secs(NOON).as_nanos()).unwrap();
        let second = 1_000_000_000;
        assert_eq!(batches[0].first_capture, noon - 3 * second);
        assert_eq!(batches[0].last_capture, noon);
        let ats: Vec<_> = batches[0].records().iter().map(|r| r.at()).collect();
        assert_eq!(
            ats[0],
            noon - second,
            "the records are not in arrival order"
        );
    }

    #[test]
    fn a_300_ms_cadence_flushes_within_the_age_bound() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let tick = Duration::from_millis(50);
        let mut first_flush = None;
        for step in 0u32..=220 {
            if step % 6 == 0 {
                harness.emit(foreground(&AT_NOON, Duration::ZERO));
            }
            harness.pass();
            if first_flush.is_none() && !harness.store.batches().is_empty() {
                first_flush = Some(tick * step);
            }
            harness.clocks.advance(tick);
        }
        let first = first_flush.expect("no flush in 11 s");
        assert!(
            first <= BATCH_MAX_AGE,
            "the first flush came {first:?} after the first event"
        );
        assert!(
            harness.store.batches().len() >= 2,
            "fewer than 2 batches in 11 s"
        );
    }

    #[test]
    fn the_age_and_count_rules_of_a_batch() {
        let mut batch = Batch::with_capacity(64);
        let day = utc_day(0);
        assert_eq!(batch.admits(day, 64), Admit::TooLarge);
        assert_eq!(batch.admits(day, 63), Admit::Fits);
        batch.push(
            EntryKind::of(Modality::Keystroke),
            5,
            b"0123456789",
            Duration::from_secs(1),
        );
        assert!(!batch.is_due(Duration::from_secs(5)));
        assert!(batch.is_due(Duration::from_secs(6)));
        assert_eq!(batch.admits(utc_day(NANOS_PER_DAY), 1), Admit::FlushFirst);
        assert_eq!(batch.admits(day, 53), Admit::FlushFirst);
        assert_eq!(batch.admits(day, 52), Admit::Fits);
        let id = batch.segment_id().unwrap();
        assert_eq!(
            batch.segment_id().unwrap(),
            id,
            "a second call minted a new id"
        );
        batch.clear();
        assert!(batch.write(&id).is_none());
        assert_ne!(batch.segment_id().unwrap(), id);
        assert_eq!(utc_day(-1).days_since_epoch(), -1);
        assert_eq!(utc_day(0).days_since_epoch(), 0);
    }
}
