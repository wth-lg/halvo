//! The episode step: one call of it reads the records of one analysis batch and returns the episodes in a seed
//! category. It masks the `text` of every `window_title` record first, whatever `cut` says, and every later part
//! of the step (the order, the run marks, the cut, the site, the stage and the result) reads the masked title
//! alone. Then it orders the records by their instant, drops what lies between run marks, cuts episodes by `GAP`
//! and `FLICKER`, and asks the stage for each episode's category.

use std::collections::BTreeSet;
use std::time::Duration;

use halvo_redact::mask;
use halvo_schema::{
    AddressFieldLayer, AppId, Category, Domain, Episode, FloorLayer, InputClass, Instant,
    KeyCounts, ObservedSite, ObservedStep, Ordinal, Part, PointerCounts, SiteDomain, SummaryId,
    Tagged, Text,
};
use halvo_seam::message::{Entry, Expiry};
use halvo_seam::record::{Edge, KeyClass, Motion, Payload, Pointer};

/// An episode of a key stays open across records of other keys until more than this has passed since its own last
/// record.
pub const GAP: Duration = Duration::from_secs(300);

/// A stretch of records in other keys between two records of one key is merged into that key's episode when the
/// record that returns to the key lies less than this after the first record of the stretch.
pub const FLICKER: Duration = Duration::from_secs(20);

/// The resolution of the instant of a keystroke or a click: such a record moves before a record placed before it
/// only when its instant plus this is at or below that record's instant.
pub const RESOLUTION: Duration = Duration::from_millis(1_000);

/// The farthest a record moves back: it moves before no record whose instant lies more than this after its own.
pub const LOOK_BACK: Duration = Duration::from_millis(31_000);

const fn nanos(d: Duration) -> i64 {
    d.as_nanos() as i64
}

const SECOND: i64 = 1_000_000_000;
const HOUR: i64 = 3_600 * SECOND;
const DAY: i64 = 24 * HOUR;
const FIVE_MINUTES: i64 = 300 * SECOND;

/// One call of the episode step: the records of one analysis batch in the order of place, the expiry entry of each
/// segment that they name, the offset of local time from UTC at the batch, in seconds (0 in a replay), and the window
/// that a batch carries over from before its first record, or none.
pub struct Call<'a> {
    pub entries: &'a [Entry],
    pub expiry: &'a [Expiry],
    pub utc_offset_s: i32,
    pub carry: Option<Carry<'a>>,
}

/// The window in front at a batch's first record, which the batch carries over: the last title record before the
/// batch, which the worker read back in front of its committed position, and the count of the batch's first records,
/// in the order of place, that it reaches. The title record names the window alone: it is no record of the call, so
/// it makes no step and adds no occasion, no input and no instant of its own to an episode. A record of the call
/// whose place is below `until` and in front of the call's first title record belongs to that window; one at or
/// after `until` is dropped as when no window is carried. A carry whose record is no `window_title` record carries
/// nothing.
pub struct Carry<'a> {
    pub title: &'a Entry,
    pub until: usize,
}

/// What one call of the episode step returns: the episodes in a seed category, in the order of their first record;
/// whether an agent's run was open at the end of the call; and whether that run's `start` was the call's first
/// record in the step's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub episodes: Vec<Episode>,
    pub run_open: bool,
    pub start_first: bool,
}

/// A stage names the category of one episode from its observed steps, whose titles are masked.
pub trait Stage {
    fn classify(&self, steps: &[ObservedStep]) -> Category;
}

/// What a record holds once its title is masked. No variant holds the title as captured.
enum What {
    Title { title: Text, site: Option<Domain> },
    Key(Option<KeyClass>),
    Pointer(Pointer),
    Mark(Edge),
    Other,
}

struct Rec {
    place: usize,
    seg: u64,
    at: i64,
    app: Option<AppId>,
    what: What,
}

impl Rec {
    fn moves(&self) -> bool {
        matches!(self.what, What::Key(_) | What::Pointer(Pointer::Click(_)))
    }
}

/// The step's first operation: every record of the call, each title replaced by `mask` of its whole text.
fn masked(entries: &[Entry]) -> Vec<Rec> {
    entries
        .iter()
        .enumerate()
        .map(|(place, entry)| masked_one(place, entry))
        .collect()
}

/// One record at the place `place` of the call, its title replaced by `mask` of its whole text.
fn masked_one(place: usize, entry: &Entry) -> Rec {
    let record = &entry.record;
    let what = match record.payload() {
        Payload::WindowTitle(title) => What::Title {
            title: Text::cut(&mask(title.text.as_str())),
            site: title.site.clone(),
        },
        Payload::Keystroke(class) => What::Key(*class),
        Payload::Pointer(pointer) => What::Pointer(*pointer),
        Payload::RunMark(edge) => What::Mark(*edge),
        Payload::ForegroundApp | Payload::ScreenFrame => What::Other,
    };
    Rec {
        place,
        seg: entry.pos.seg,
        at: record.at().0,
        app: record.app().cloned(),
        what,
    }
}

/// The window that a title record opens: the app plus the site where the record holds one, else the app plus the
/// masked title.
fn window_of(app: Option<AppId>, title: Text, site: Option<Domain>) -> Window {
    let place = match &site {
        Some(domain) => Place::Site(domain.clone()),
        None => Place::Title(title.clone()),
    };
    Window {
        key: Key { app, place },
        title,
        site,
    }
}

/// The window of the carry and the count of the call's first records that it reaches, none where the carry's record
/// is no title record. The title is masked as every title of the call is.
fn carried(carry: Option<&Carry>) -> Option<(Window, usize)> {
    let carry = carry?;
    let rec = masked_one(0, carry.title);
    match rec.what {
        What::Title { title, site } => Some((window_of(rec.app, title, site), carry.until)),
        _ => None,
    }
}

/// Orders the records of a call by their instant: from the order of place, a keystroke or a click moves before a
/// record placed before it only when its instant plus `RESOLUTION` is at or below that record's instant and that
/// record lies at most `LOOK_BACK` after it; every other pair keeps its place.
fn order(recs: Vec<Rec>) -> Vec<Rec> {
    let mut out: Vec<Rec> = Vec::with_capacity(recs.len());
    for rec in recs {
        let mut place = out.len();
        if rec.moves() {
            while place > 0 {
                let ahead = out[place - 1].at.saturating_sub(rec.at);
                if ahead < nanos(RESOLUTION) || ahead > nanos(LOOK_BACK) {
                    break;
                }
                place -= 1;
            }
        }
        out.insert(place, rec);
    }
    out
}

/// What makes two windows one key: the app plus the site where the title record holds one, else the app plus the
/// masked title.
#[derive(Clone, PartialEq, Eq)]
enum Place {
    Site(Domain),
    Title(Text),
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    app: Option<AppId>,
    place: Place,
}

/// The window that a title record opened: its key, its masked title and its site.
struct Window {
    key: Key,
    title: Text,
    site: Option<Domain>,
}

/// A record of the person's that an episode can hold: its segment, instant, window and observed step, and the
/// input that it counts.
struct Item {
    seg: u64,
    at: i64,
    window: usize,
    step: ObservedStep,
    input: Input,
}

enum Input {
    None,
    Key(Option<KeyClass>),
    Pointer(Pointer),
}

/// The person's records of a call and what the run marks left.
struct Person {
    windows: Vec<Window>,
    items: Vec<Item>,
    run_open: bool,
    start_first: bool,
}

/// Reads the ordered records with the two states of the run marks: a `start` opens a run, an `end` closes it, and
/// a `start` while a run is open or an `end` while none is open changes nothing. Every record while a run is open
/// is dropped, and a run that is open at the end of the call ends with it. A run closes no episode: the cut sees
/// the person's records on both sides of it as one sequence. A keystroke or a pointer record belongs to the window
/// of the title record before it. At the call's start that is the window that the call carries, where it carries one:
/// it makes no item, and a record takes it only while its place is below the carry's `until`. A record with no window
/// since the call's start or since the last run opened, or with the carried window at or after `until`, is dropped.
fn person(ordered: Vec<Rec>, carry: Option<(Window, usize)>) -> Person {
    let mut out = Person {
        windows: Vec::new(),
        items: Vec::new(),
        run_open: false,
        start_first: false,
    };
    let mut current: Option<usize> = None;
    let mut until = 0;
    if let Some((window, reach)) = carry {
        out.windows.push(window);
        current = Some(0);
        until = reach;
    }
    let carried = current;
    for (index, rec) in ordered.into_iter().enumerate() {
        let input = match rec.what {
            What::Mark(Edge::Start) => {
                if !out.run_open {
                    out.run_open = true;
                    out.start_first = index == 0;
                    current = None;
                }
                continue;
            }
            What::Mark(Edge::End) => {
                out.run_open = false;
                continue;
            }
            _ if out.run_open => continue,
            What::Other => continue,
            What::Title { title, site } => {
                out.windows.push(window_of(rec.app.clone(), title, site));
                current = Some(out.windows.len() - 1);
                Input::None
            }
            What::Key(class) => Input::Key(class),
            What::Pointer(pointer) => Input::Pointer(pointer),
        };
        let Some(window) = current else {
            continue;
        };
        if current == carried && rec.place >= until {
            continue;
        }
        let step = step_of(&out.windows[window], rec.app, &input);
        out.items.push(Item {
            seg: rec.seg,
            at: rec.at,
            window,
            step,
            input,
        });
    }
    if !out.run_open {
        out.start_first = false;
    }
    out
}

fn step_of(window: &Window, app: Option<AppId>, input: &Input) -> ObservedStep {
    let class = match input {
        Input::None => None,
        Input::Key(class) => class.map(key_class),
        Input::Pointer(Pointer::Click(_)) => Some(InputClass::Click),
        Input::Pointer(Pointer::Motion(motion, _)) => Some(match motion {
            Motion::Scroll => InputClass::Scroll,
            Motion::Drag => InputClass::Drag,
            Motion::Move => InputClass::Move,
        }),
        Input::Pointer(Pointer::Unclassed(_)) => None,
    };
    ObservedStep {
        app: app.map(|value| Tagged {
            value,
            layer: FloorLayer::Floor,
        }),
        title: Some(Tagged {
            value: window.title.clone(),
            layer: FloorLayer::Floor,
        }),
        input: class.map(|value| Tagged {
            value,
            layer: FloorLayer::Floor,
        }),
        site: window.site.clone().map(|domain| Tagged {
            value: SiteDomain::from(domain),
            layer: AddressFieldLayer::AddressField,
        }),
        text: None,
    }
}

fn key_class(class: KeyClass) -> InputClass {
    match class {
        KeyClass::Printable => InputClass::Printable,
        KeyClass::Editing => InputClass::Editing,
        KeyClass::Navigation => InputClass::Navigation,
        KeyClass::Enter => InputClass::Enter,
        KeyClass::Tab => InputClass::Tab,
        KeyClass::Shortcut => InputClass::Shortcut,
        KeyClass::Other => InputClass::Other,
    }
}

/// Cuts the person's records of a call into episodes, each the indices of its records in the step's order.
/// An episode takes the key of its first record and stays open across records of other keys until more than `GAP`
/// has passed since its own last record; a record of its key within that time continues it, and a later one opens
/// a new episode. A stretch of records in other keys that follows a record of the open episode's key and that a
/// record of that key ends less than `FLICKER` after the stretch's first record is no episode: its records join
/// the episode. Any other stretch in another key is an episode of its own, so episodes of different keys may
/// overlap in time.
fn cut(items: &[Item], windows: &[Window]) -> Vec<Vec<usize>> {
    let key = |i: usize| &windows[items[i].window].key;
    let mut episodes: Vec<Vec<usize>> = Vec::new();
    let mut open: Vec<(usize, i64)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut i = 0;
    while i < items.len() {
        let at = items[i].at;
        open.retain(|&(_, last)| at.saturating_sub(last) <= nanos(GAP));
        let current_slot = current.and_then(|c| open.iter().position(|&(e, _)| e == c));
        let mut take = i..i + 1;
        let mut joins = open
            .iter()
            .position(|&(e, _)| key(episodes[e][0]) == key(i));
        if let Some(c) = current_slot {
            let (episode, last) = open[c];
            let own = key(episodes[episode][0]);
            if own != key(i) {
                let back = (i + 1..items.len())
                    .take_while(|&r| items[r].at.saturating_sub(at) < nanos(FLICKER))
                    .find(|&r| key(r) == own)
                    .filter(|&r| items[r].at.saturating_sub(last) <= nanos(GAP));
                if let Some(r) = back {
                    take = i..r + 1;
                    joins = Some(c);
                }
            }
        }
        let slot = joins.unwrap_or_else(|| {
            episodes.push(Vec::new());
            open.push((episodes.len() - 1, at));
            open.len() - 1
        });
        let episode = open[slot].0;
        episodes[episode].extend(take.clone());
        open[slot].1 = items[take.end - 1].at;
        current = Some(episode);
        i = take.end;
    }
    episodes
}

/// Runs the episode step over one call. `ids` gives the summary id of each episode that the step returns, in the
/// order of the episodes, and is called for no other episode.
pub fn episode_step(call: &Call, stage: &dyn Stage, ids: &mut dyn FnMut() -> SummaryId) -> Outcome {
    let recs = masked(call.entries);
    let keys_on = recs.iter().any(|r| matches!(r.what, What::Key(Some(_))));
    let pointer_on = recs.iter().any(|r| {
        matches!(
            r.what,
            What::Pointer(Pointer::Click(_) | Pointer::Motion(..))
        )
    });
    let person = person(order(recs), carried(call.carry.as_ref()));
    let shape = Shape {
        call,
        keys_on,
        pointer_on,
    };
    let mut episodes = Vec::new();
    for indices in cut(&person.items, &person.windows) {
        let items: Vec<&Item> = indices.iter().map(|&i| &person.items[i]).collect();
        if let Some(episode) = shape.episode(&items, &person.windows, stage, ids) {
            episodes.push(episode);
        }
    }
    Outcome {
        episodes,
        run_open: person.run_open,
        start_first: person.start_first,
    }
}

struct Shape<'c, 'a> {
    call: &'c Call<'a>,
    keys_on: bool,
    pointer_on: bool,
}

impl Shape<'_, '_> {
    /// The episode of the records that the cut gave it, or none when the stage puts it in `other`, when its key
    /// holds no app id, or when a segment of its records has no expiry entry in the call. Its range runs from its
    /// first record to one second after its last, across the records of other keys and any agent's run between them.
    fn episode(
        &self,
        items: &[&Item],
        windows: &[Window],
        stage: &dyn Stage,
        ids: &mut dyn FnMut() -> SummaryId,
    ) -> Option<Episode> {
        let head = items.first()?;
        let window = &windows[head.window];
        let app = window.key.app.clone()?;
        let start = items.iter().map(|i| i.at).min()?;
        let end = items.iter().map(|i| i.at).max()?.saturating_add(SECOND);
        let seg_set: BTreeSet<u64> = items.iter().map(|i| i.seg).collect();
        let mut expires: Option<Instant> = None;
        for seg in &seg_set {
            let at = self
                .call
                .expiry
                .iter()
                .filter(|e| e.seg == *seg)
                .map(|e| e.at)
                .min()?;
            expires = Some(expires.map_or(at, |e| e.min(at)));
        }
        let segments = seg_set
            .into_iter()
            .map(Ordinal::new)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let local = start.saturating_add(i64::from(self.call.utc_offset_s) * SECOND);
        let day = u32::try_from(local.div_euclid(DAY)).ok()?;
        let part = match local.rem_euclid(DAY) / HOUR {
            0..=5 => Part::Night,
            6..=11 => Part::Morning,
            12..=17 => Part::Afternoon,
            _ => Part::Evening,
        };
        let fives = (end - start + FIVE_MINUTES - 1) / FIVE_MINUTES;
        let minutes = u32::try_from((fives * 5).clamp(5, 1440)).ok()?;
        let steps: Vec<ObservedStep> = items.iter().map(|i| i.step.clone()).collect();
        let category = stage.classify(&steps).seed()?;
        Some(Episode {
            id: ids(),
            category,
            site: window
                .site
                .clone()
                .map(|domain| ObservedSite::AddressField { domain }),
            app,
            day,
            part,
            minutes,
            keys: self.keys_on.then(|| key_counts(items)),
            pointer: self.pointer_on.then(|| pointer_counts(items)),
            segments,
            expires: expires?,
            start: Instant(start),
            end: Instant(end),
            steps,
        })
    }
}

fn key_counts(items: &[&Item]) -> KeyCounts {
    let mut k = KeyCounts {
        printable: 0,
        editing: 0,
        navigation: 0,
        enter: 0,
        tab: 0,
        shortcut: 0,
        other: 0,
    };
    for item in items {
        if let Input::Key(Some(class)) = item.input {
            let n = match class {
                KeyClass::Printable => &mut k.printable,
                KeyClass::Editing => &mut k.editing,
                KeyClass::Navigation => &mut k.navigation,
                KeyClass::Enter => &mut k.enter,
                KeyClass::Tab => &mut k.tab,
                KeyClass::Shortcut => &mut k.shortcut,
                KeyClass::Other => &mut k.other,
            };
            *n = n.saturating_add(1);
        }
    }
    k
}

fn pointer_counts(items: &[&Item]) -> PointerCounts {
    let mut p = PointerCounts {
        click: 0,
        scroll: 0,
        drag: 0,
        r#move: 0,
    };
    for item in items {
        match item.input {
            Input::Pointer(Pointer::Click(_)) => p.click = p.click.saturating_add(1),
            Input::Pointer(Pointer::Motion(motion, count)) => {
                let n = count.map_or(0, |c| u32::try_from(c.get()).unwrap_or(u32::MAX));
                let sum = match motion {
                    Motion::Scroll => &mut p.scroll,
                    Motion::Drag => &mut p.drag,
                    Motion::Move => &mut p.r#move,
                };
                *sum = sum.saturating_add(n);
            }
            _ => {}
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use halvo_schema::Position;
    use halvo_seam::record::{CaptureRecord, RecordId, Seq, Title, Why};

    use super::*;

    const APP: &str = "org.chromium.Chromium";

    fn entry(n: u64, at_ms: i64, app: Option<&str>, payload: Payload) -> Entry {
        let id = RecordId::parse(&format!("00000000-0000-7000-8000-{n:012x}")).unwrap();
        let app = app.map(|a| AppId::parse(a).unwrap());
        let record = CaptureRecord::new(
            id,
            Instant(at_ms * 1_000_000),
            Seq::new(n).unwrap(),
            app,
            payload,
        )
        .unwrap();
        Entry {
            pos: Position { seg: 1, idx: n },
            record,
        }
    }

    fn title(text: &str, cut: bool, site: Option<&str>) -> Payload {
        Payload::WindowTitle(Title {
            text: Text::parse(text).unwrap(),
            cut: Some(cut),
            why: Some(Why::TitleChange),
            site: site.map(|s| Domain::parse(s).unwrap()),
        })
    }

    fn page(n: u64, at_s: i64, site: &str) -> Entry {
        entry(n, at_s * 1_000, Some(APP), title(site, false, Some(site)))
    }

    fn mark(n: u64, at_s: i64, edge: Edge) -> Entry {
        entry(n, at_s * 1_000, None, Payload::RunMark(edge))
    }

    const EXPIRY: &[Expiry] = &[Expiry {
        seg: 1,
        at: Instant(i64::MAX),
    }];

    /// A stage that puts every episode in a seed category and keeps each title it gets.
    struct Keeper(RefCell<Vec<String>>);

    impl Stage for Keeper {
        fn classify(&self, steps: &[ObservedStep]) -> Category {
            for step in steps {
                if let Some(title) = &step.title {
                    self.0.borrow_mut().push(title.value.as_str().to_owned());
                }
            }
            Category::ShopOrder
        }
    }

    fn run(entries: &[Entry]) -> (Outcome, Vec<String>) {
        run_carrying(entries, None)
    }

    fn run_carrying(entries: &[Entry], carry: Option<Carry>) -> (Outcome, Vec<String>) {
        let keeper = Keeper(RefCell::new(Vec::new()));
        let mut n = 0u8;
        let mut ids = || {
            n += 1;
            SummaryId::from_random([n; 16])
        };
        let call = Call {
            entries,
            expiry: EXPIRY,
            utc_offset_s: 0,
            carry,
        };
        let outcome = episode_step(&call, &keeper, &mut ids);
        (outcome, keeper.0.into_inner())
    }

    fn key(n: u64, at_s: i64) -> Entry {
        entry(
            n,
            at_s * 1_000,
            Some(APP),
            Payload::Keystroke(Some(KeyClass::Printable)),
        )
    }

    /// The window that a batch carries names its key records in front of its first title record, and the carried
    /// title record adds nothing of its own: the episode begins at the instant of the batch's first key, ends one
    /// second after its last, and holds the batch's three keys as its steps alone, each with the carried window's
    /// title and site. A key at or after the carry's `until` is dropped as with no carry. A batch whose first record
    /// is a title record of its own gets nothing from a carry: its outcome is the outcome with none.
    #[test]
    fn a_carried_window_names_the_keys_in_front_of_the_first_title_and_adds_nothing() {
        let before = page(90, 100, "sheet.test");
        let keys = [key(0, 1_000), key(1, 1_001), key(2, 1_002)];
        let carry = |until| {
            Some(Carry {
                title: &before,
                until,
            })
        };

        let (outcome, titles) = run_carrying(&keys, carry(3));
        assert_eq!(sites(&outcome), ["sheet.test"]);
        let episode = &outcome.episodes[0];
        assert_eq!(episode.start, Instant(1_000 * SECOND));
        assert_eq!(episode.end, Instant(1_003 * SECOND));
        assert_eq!(episode.steps.len(), 3);
        assert!(episode.steps.iter().all(|step| step.input.is_some()));
        assert_eq!(episode.keys.as_ref().map(|k| k.printable), Some(3));
        assert_eq!(titles, ["sheet.test"; 3]);

        let (outcome, _) = run_carrying(&keys, carry(1));
        assert_eq!(outcome.episodes.len(), 1);
        assert_eq!(outcome.episodes[0].steps.len(), 1);
        assert_eq!(run_carrying(&keys, carry(0)).0.episodes, []);
        assert_eq!(run(&keys).0.episodes, []);

        let titled = [page(0, 1_000, "other.test"), key(1, 1_001), key(2, 1_002)];
        assert_eq!(run_carrying(&titled, carry(3)), run(&titled));
    }

    fn sites(outcome: &Outcome) -> Vec<String> {
        outcome
            .episodes
            .iter()
            .map(|e| match &e.site {
                Some(ObservedSite::AddressField { domain }) => domain.as_str().to_owned(),
                other => panic!("an episode without an address-field site: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn mask_comes_first() {
        let entries = [
            entry(
                0,
                0,
                Some(APP),
                title(
                    "Pay with card 4242 4242 4242 4242 - Pinecart",
                    true,
                    Some("pinecart.test"),
                ),
            ),
            entry(
                1,
                2_000,
                Some(APP),
                Payload::Keystroke(Some(KeyClass::Printable)),
            ),
        ];
        let (outcome, seen) = run(&entries);
        assert_eq!(seen.len(), 2, "the stage got one title per step");
        for t in &seen {
            assert!(t.contains("[card]") && !t.contains("4242"), "stage saw {t}");
        }
        assert_eq!(outcome.episodes.len(), 1);
        for step in &outcome.episodes[0].steps {
            let t = step.title.as_ref().unwrap().value.as_str();
            assert!(
                t.contains("[card]") && !t.contains("4242"),
                "episode holds {t}"
            );
        }
    }

    #[test]
    fn gap_and_flicker() {
        let entries = [
            page(0, 0, "a.test"),
            page(1, 290, "a.test"),
            page(2, 300, "b.test"),
            page(3, 315, "a.test"),
            page(4, 700, "a.test"),
        ];
        let (outcome, _) = run(&entries);
        assert_eq!(sites(&outcome), ["a.test", "a.test"]);
        let starts: Vec<i64> = outcome.episodes.iter().map(|e| e.start.0).collect();
        assert_eq!(starts, [0, 700 * 1_000_000_000]);
        assert_eq!(outcome.episodes[0].steps.len(), 4);

        let back_after_gap = [
            page(0, 0, "a.test"),
            page(1, 30, "b.test"),
            page(2, 250, "b.test"),
            page(3, 301, "a.test"),
        ];
        let (outcome, _) = run(&back_after_gap);
        assert_eq!(sites(&outcome), ["a.test", "b.test", "a.test"]);
    }

    #[test]
    fn late_keystroke_order() {
        let entries = [
            entry(0, 10_000, Some(APP), title("Cart - Pinecart", false, None)),
            entry(
                1,
                5_000,
                Some(APP),
                Payload::Keystroke(Some(KeyClass::Enter)),
            ),
            entry(2, 9_500, Some(APP), Payload::Keystroke(Some(KeyClass::Tab))),
        ];
        let ordered: Vec<(&str, i64)> = order(masked(&entries))
            .iter()
            .map(|r| {
                let kind = match r.what {
                    What::Title { .. } => "title",
                    What::Key(_) => "keystroke",
                    _ => "other",
                };
                (kind, r.at / 1_000_000)
            })
            .collect();
        assert_eq!(
            ordered,
            [
                ("keystroke", 5_000),
                ("title", 10_000),
                ("keystroke", 9_500)
            ]
        );
    }

    #[test]
    fn run_marks_two_states() {
        let entries = [
            page(0, 0, "a.test"),
            mark(1, 10, Edge::Start),
            mark(2, 20, Edge::Start),
            page(3, 30, "b.test"),
            mark(4, 40, Edge::End),
            mark(5, 50, Edge::End),
            page(6, 60, "c.test"),
        ];
        let (outcome, _) = run(&entries);
        assert_eq!(sites(&outcome), ["a.test", "c.test"]);
        assert!(!outcome.run_open);
    }

    #[test]
    fn run_inside_episode_keeps_it_open() {
        let entries = [
            page(0, 0, "a.test"),
            mark(1, 10, Edge::Start),
            page(2, 30, "b.test"),
            mark(3, 40, Edge::End),
            page(4, 60, "a.test"),
        ];
        let (outcome, _) = run(&entries);
        assert_eq!(sites(&outcome), ["a.test"]);
        assert_eq!(outcome.episodes[0].steps.len(), 2);
    }
}
