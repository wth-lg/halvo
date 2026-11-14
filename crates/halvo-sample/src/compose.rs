//! `compose` (contracts.md C12.10): writes the explicit script of one author's two bodies from site data and a
//! rates file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use halvo_schema::{AppId, Category, Domain, SeedCategory};

use crate::form::{
    create_out_dir, instant, parse_seed, sha256_hex, whole_seconds_between, write_new, Refusal,
    DAY, LAST_DAY, MILLI, SECOND,
};
use crate::rng::Draw;
use crate::script::{Edge, Episode, Header, Item, Line, Mark, Noise, Sites, Step, TaskKind};
use crate::site_data::{encode_value, fill, parse_service, Role, Service};
use crate::text::site_masked;

/// The rates file (C12.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rates {
    pub sides: (Sites, Sites),
    pub seed: u64,
    pub browser: String,
    pub streams: u64,
    pub planted: u64,
    pub interleave: u8,
    pub noise: u8,
    pub near_miss: u8,
    pub day1: i64,
    pub canaries: Vec<String>,
}

fn sides_text(s: (Sites, Sites)) -> String {
    format!("{},{}", s.0.as_str(), s.1.as_str())
}

impl Rates {
    /// The rates file as its ten lines.
    pub fn write(&self) -> String {
        let canaries: Vec<String> = self.canaries.iter().map(|c| format!("\"{c}\"")).collect();
        format!(
            "sides = \"{}\"\nseed = {}\nbrowser = \"{}\"\nstreams = {}\nplanted = {}\ninterleave = {}\nnoise = {}\nnear_miss = {}\nday1 = {}\ncanaries = [{}]\n",
            sides_text(self.sides),
            self.seed,
            self.browser,
            self.streams,
            self.planted,
            self.interleave,
            self.noise,
            self.near_miss,
            self.day1,
            canaries.join(", ")
        )
    }

    /// Reads a rates file: exactly the ten lines of C12.10 in their order, taken only when it is byte-equal to
    /// the text that `write` gives for what was read; `streams` and `planted` even and `streams` above 0, each
    /// step from 1 to 5, `day1` a whole second, the browser an app id with no `"` and no `\`, and 0 or 3
    /// canaries, each a domain, none twice.
    pub fn parse(bytes: &[u8]) -> Option<Rates> {
        let text = std::str::from_utf8(bytes).ok()?;
        let lines: Vec<&str> = text.strip_suffix('\n')?.split('\n').collect();
        if lines.len() != 10 {
            return None;
        }
        let value = |i: usize, key: &str| lines[i].strip_prefix(key)?.strip_prefix(" = ");
        let quoted = |v: &str| -> Option<String> {
            let inner = v.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.contains('"') && !inner.contains('\\')).then(|| inner.to_owned())
        };
        let sides = match quoted(value(0, "sides")?)?.as_str() {
            "seen,unseen" => (Sites::Seen, Sites::Unseen),
            "check,heldout" => (Sites::Check, Sites::Heldout),
            _ => return None,
        };
        let seed = parse_seed(value(1, "seed")?)?;
        let browser = quoted(value(2, "browser")?)?;
        AppId::parse(&browser).ok()?;
        let streams = parse_seed(value(3, "streams")?)?;
        let planted = parse_seed(value(4, "planted")?)?;
        let step = |i: usize, key: &str| -> Option<u8> {
            let v = value(i, key)?;
            (v.len() == 1 && (b'1'..=b'5').contains(&v.as_bytes()[0]))
                .then(|| v.as_bytes()[0] - b'0')
        };
        let interleave = step(5, "interleave")?;
        let noise = step(6, "noise")?;
        let near_miss = step(7, "near_miss")?;
        let day1: i64 = value(8, "day1")?.parse().ok()?;
        let list = value(9, "canaries")?;
        let inner = list.strip_prefix('[')?.strip_suffix(']')?;
        let canaries: Vec<String> = if inner.is_empty() {
            Vec::new()
        } else {
            inner.split(", ").map(quoted).collect::<Option<_>>()?
        };
        let distinct = canaries.iter().collect::<BTreeSet<&String>>().len();
        let rates = Rates {
            sides,
            seed,
            browser,
            streams,
            planted,
            interleave,
            noise,
            near_miss,
            day1,
            canaries,
        };
        let ok = rates.write().as_bytes() == bytes
            && streams > 0
            && streams % 2 == 0
            && planted % 2 == 0
            && day1 % SECOND == 0
            && (rates.canaries.is_empty() || rates.canaries.len() == 3)
            && distinct == rates.canaries.len()
            && rates.canaries.iter().all(|c| Domain::parse(c).is_ok());
        ok.then_some(rates)
    }
}

/// The table of constants behind the three rates: for each rate and step, what it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTable {
    pub cut_per_mille: u64,
    pub stretch_min_ms: u64,
    pub stretch_max_ms: u64,
    pub noise_max_per_day: u64,
    pub near_miss_per_mille: u64,
}

/// The share of episodes cut into more than one stretch, per mille, for each `interleave` step.
pub const CUT_PER_MILLE: [u64; 5] = [100, 200, 300, 400, 500];
/// The shortest stretch in milliseconds, for each `interleave` step.
pub const STRETCH_MIN_MS: [u64; 5] = [14_000, 14_000, 14_000, 14_000, 14_000];
/// The longest stretch in milliseconds, for each `interleave` step.
pub const STRETCH_MAX_MS: [u64; 5] = [70_000, 70_000, 70_000, 70_000, 70_000];
/// The most noise items of an active scripted day, for each `noise` step.
pub const NOISE_MAX_PER_DAY: [u64; 5] = [1, 2, 3, 4, 5];
/// The near-miss routines per planted routine of a category, per mille, for each `near_miss` step; step 1
/// gives a quarter (C10.7).
pub const NEAR_MISS_PER_MILLE: [u64; 5] = [250, 275, 300, 325, 350];

/// The fifteen lines of `compose --ranges`.
pub fn ranges() -> Vec<String> {
    let mut out = Vec::new();
    for s in 0..5 {
        out.push(format!(
            "range interleave {} cut_per_mille={} stretch_min_ms={} stretch_max_ms={}",
            s + 1,
            CUT_PER_MILLE[s],
            STRETCH_MIN_MS[s],
            STRETCH_MAX_MS[s]
        ));
    }
    for (s, n) in NOISE_MAX_PER_DAY.iter().enumerate() {
        out.push(format!("range noise {} noise_max_per_day={n}", s + 1));
    }
    for (s, n) in NEAR_MISS_PER_MILLE.iter().enumerate() {
        out.push(format!("range near_miss {} near_miss_per_mille={n}", s + 1));
    }
    out
}

/// Episodes of a planted routine: from 3 to this number.
pub const PLANTED_EPISODES_MAX: u64 = 6;
/// Episodes of a near-miss routine: from 3 to this number.
pub const NEAR_MISS_EPISODES_MAX: u64 = 5;
/// Days of a non-seed routine: from 3 to 8 (C10.5).
pub const NON_SEED_DAYS: (u64, u64) = (3, 8);
/// The first block of a day starts from 07:00 to 11:00, in milliseconds after midnight.
pub const DAY_START_MS: (u64, u64) = (25_200_000, 39_600_000);
/// The gap between two blocks of a day, in milliseconds.
pub const GAP_MS: (u64, u64) = (60_000, 1_800_000);
/// The gap between a mark and the episode next to it, and between two episodes of the marked run.
pub const MARK_GAP_MS: (u64, u64) = (1_000, 5_000);

/// The arguments of `compose`.
#[derive(Clone, Debug)]
pub struct ComposeArgs {
    pub rates: PathBuf,
    pub out: PathBuf,
    pub sites: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct Task {
    stream: usize,
    id: String,
    service: usize,
    category: Category,
    kind: TaskKind,
    flow: usize,
    values: Vec<String>,
    days: Vec<u32>,
}

struct Plan {
    services: Vec<Service>,
    halves: [Vec<usize>; 2],
    canary_services: Vec<usize>,
}

fn short(detail: &str) -> Refusal {
    Refusal::new("short", detail.to_owned())
}

/// Reads the site data files in order; a file that is missing or not of C11.1, or that names a service an
/// earlier file names, is refused with `site-data`.
pub fn read_services(paths: &[PathBuf]) -> Result<Vec<Service>, Refusal> {
    let mut out: Vec<Service> = Vec::new();
    for p in paths {
        let fault = || Refusal::new("site-data", p.display().to_string());
        let bytes = fs::read(p).map_err(|_| fault())?;
        let s = parse_service(&bytes).ok_or_else(fault)?;
        if out.iter().any(|o| o.service == s.service) {
            return Err(fault());
        }
        out.push(s);
    }
    Ok(out)
}

fn plan_services(rates: &Rates, services: Vec<Service>) -> Result<Plan, Refusal> {
    let mut canary_services = Vec::new();
    for c in &rates.canaries {
        let i = services
            .iter()
            .position(|s| s.category == Category::Other && s.host.as_deref() == Some(c.as_str()))
            .ok_or_else(|| {
                Refusal::new("site-data", format!("canary {}", canary_index(rates, c)))
            })?;
        canary_services.push(i);
    }
    let mut halves: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for category in Category::ALL {
        let mut list: Vec<usize> = (0..services.len())
            .filter(|i| services[*i].category == *category && !canary_services.contains(i))
            .collect();
        list.sort_by(|a, b| {
            (services[*a].web, &services[*a].service)
                .cmp(&(services[*b].web, &services[*b].service))
        });
        for (k, i) in list.into_iter().enumerate() {
            halves[k % 2].push(i);
        }
    }
    Ok(Plan {
        services,
        halves,
        canary_services,
    })
}

fn canary_index(rates: &Rates, c: &str) -> usize {
    rates.canaries.iter().position(|x| x == c).unwrap_or(0)
}

fn seed_index(c: Category) -> usize {
    c.seed()
        .and_then(|s| SeedCategory::ALL.iter().position(|x| *x == s))
        .unwrap_or(10)
}

fn app_of(rates: &Rates, s: &Service) -> String {
    if s.web {
        rates.browser.clone()
    } else {
        s.app.clone().unwrap_or_default()
    }
}

/// One step of a composed script: its id and its line.
fn step_of(rates: &Rates, s: &Service, page: &str, values: &[String]) -> Step {
    let p = s.page(page).cloned().unwrap_or_else(|| s.pages[0].clone());
    let value = |name: &str| {
        let i = s.parameters.iter().position(|n| n == name).unwrap_or(0);
        values.get(i).cloned().unwrap_or_default()
    };
    let names = s.names_of(&p);
    let query: Vec<String> = names
        .iter()
        .map(|n| format!("{n}={}", encode_value(&value(n))))
        .collect();
    let path = if query.is_empty() {
        format!("/{}", p.page)
    } else {
        format!("/{}?{}", p.page, query.join("&"))
    };
    let title = fill(&p.title, value);
    let id = format!(
        "p{}",
        &sha256_hex(format!("{}\n{}\n{}", s.service, p.page, path).as_bytes())[..15]
    );
    Step {
        step: id,
        app: app_of(rates, s),
        site: s.host.clone(),
        titles: vec![title],
        paths: if s.web { vec![path] } else { Vec::new() },
        password_input: p.password_input,
        card_title: p.card_title,
    }
}

struct Composer<'a> {
    rates: &'a Rates,
    plan: &'a Plan,
    draw: Draw,
    half_size: usize,
    tasks: Vec<Task>,
    used: Vec<BTreeSet<usize>>,
}

impl Composer<'_> {
    fn stream_id(&self, i: usize) -> String {
        format!("s{:03}", i + 1)
    }

    fn half_of(&self, stream: usize) -> usize {
        stream / self.half_size
    }

    fn services_in(&self, half: usize, category: Category) -> Vec<usize> {
        self.plan.halves[half]
            .iter()
            .copied()
            .filter(|i| self.plan.services[*i].category == category)
            .collect()
    }

    fn add(&mut self, stream: usize, service: usize, kind: TaskKind) {
        let s = &self.plan.services[service];
        let n = self.tasks.iter().filter(|t| t.stream == stream).count();
        self.tasks.push(Task {
            stream,
            id: format!("{}-t{:02}", self.stream_id(stream), n + 1),
            service,
            category: s.category,
            kind,
            flow: 0,
            values: Vec::new(),
            days: Vec::new(),
        });
        self.used[stream].insert(service);
    }

    fn block_twice(&mut self, stream: usize, service: usize) {
        let s = &self.plan.services[service];
        if s.web {
            return;
        }
        let app = app_of(self.rates, s);
        let half = self.half_of(stream);
        for other in self.services_in(half, s.category) {
            if other != service && app_of(self.rates, &self.plan.services[other]) == app {
                self.used[stream].insert(other);
            }
        }
    }

    fn free(&self, stream: usize, category: Category) -> Vec<usize> {
        self.services_in(self.half_of(stream), category)
            .into_iter()
            .filter(|i| !self.used[stream].contains(i))
            .collect()
    }

    fn fixed(&mut self, stream: usize, category: Category, kind: TaskKind) -> Result<(), Refusal> {
        let free = self.free(stream, category);
        if free.is_empty() {
            return Err(short(category.as_str()));
        }
        let pick = free[self.draw.below(free.len() as u64) as usize];
        self.add(stream, pick, kind);
        if kind == TaskKind::Twice {
            self.block_twice(stream, pick);
        }
        Ok(())
    }

    fn plan_half(&mut self, half: usize) -> Result<(), Refusal> {
        let first = half * self.half_size;
        let streams: Vec<usize> = (first..first + self.half_size).collect();
        let seed = SeedCategory::ALL;
        let mut mandatory = [0u64; 10];
        for (k, s) in streams.iter().enumerate() {
            self.fixed(*s, seed[(3 * k) % 10].into(), TaskKind::OneOff)?;
            self.fixed(*s, seed[(3 * k + 1) % 10].into(), TaskKind::Twice)?;
            self.fixed(*s, seed[(3 * k + 2) % 10].into(), TaskKind::Marked)?;
            let mut placed = false;
            for j in 3..13 {
                let c: Category = seed[(3 * k + j) % 10].into();
                if !self.free(*s, c).is_empty() {
                    self.fixed(*s, c, TaskKind::NearMiss)?;
                    mandatory[(3 * k + j) % 10] += 1;
                    placed = true;
                    break;
                }
            }
            if !placed {
                return Err(short("near_miss"));
            }
        }
        let planted = self.rates.planted / 2;
        let near =
            (planted * NEAR_MISS_PER_MILLE[usize::from(self.rates.near_miss) - 1]).div_ceil(1000);
        for (c, already) in seed.iter().zip(mandatory) {
            let category: Category = (*c).into();
            for (kind, count) in [
                (TaskKind::NearMiss, near.saturating_sub(already)),
                (TaskKind::Planted, planted),
            ] {
                let slots: Vec<(usize, usize)> = streams
                    .iter()
                    .flat_map(|s| self.free(*s, category).into_iter().map(move |v| (*s, v)))
                    .collect();
                if (slots.len() as u64) < count {
                    return Err(short(category.as_str()));
                }
                for (s, v) in self.draw.pick(&slots, count as usize) {
                    self.add(s, v, kind);
                }
            }
        }
        let others = self.services_in(half, Category::Other);
        if others.is_empty() {
            return Err(short(Category::Other.as_str()));
        }
        for s in &streams {
            let pick = others[self.draw.below(others.len() as u64) as usize];
            self.add(*s, pick, TaskKind::NonSeed);
        }
        Ok(())
    }

    fn shape_tasks(&mut self) {
        let days: Vec<u32> = (1..=LAST_DAY).collect();
        for t in 0..self.tasks.len() {
            let s = &self.plan.services[self.tasks[t].service];
            let role = match self.tasks[t].kind {
                TaskKind::NearMiss => Some(Role::NearMiss),
                TaskKind::NonSeed => None,
                _ => Some(Role::Task),
            };
            let flow = match role {
                Some(r) => s.flows.iter().position(|f| f.role == r).unwrap_or(0),
                None => self.draw.below(s.flows.len() as u64) as usize,
            };
            let values: Vec<String> = s
                .parameters
                .iter()
                .map(|n| {
                    let list = s.values_of(n);
                    list[self.draw.below(list.len() as u64) as usize].clone()
                })
                .collect();
            let n = match self.tasks[t].kind {
                TaskKind::Planted => self.draw.between(3, PLANTED_EPISODES_MAX),
                TaskKind::NearMiss => self.draw.between(3, NEAR_MISS_EPISODES_MAX),
                TaskKind::NonSeed => self.draw.between(NON_SEED_DAYS.0, NON_SEED_DAYS.1),
                TaskKind::OneOff => 1,
                TaskKind::Twice => 2,
                TaskKind::Marked => 1,
            };
            let mut chosen = self.draw.pick(&days, n as usize);
            chosen.sort_unstable();
            let task = &mut self.tasks[t];
            task.flow = flow;
            task.values = values;
            task.days = chosen;
        }
    }
}

/// One unit of a day's layout: the stretches of an episode, or one noise stretch.
struct Unit {
    task: Option<usize>,
    stretches: Vec<(Step, u64)>,
}

struct Composed {
    lines: BTreeMap<usize, Vec<(i64, Line)>>,
    marked: BTreeMap<usize, Vec<(i64, Line)>>,
    steps: BTreeMap<String, Step>,
    stretch_ms: Vec<u64>,
    episode_ms: Vec<u64>,
    episodes: usize,
}

fn median_seconds(list: &mut [u64]) -> u64 {
    if list.is_empty() {
        return 0;
    }
    list.sort_unstable();
    list[(list.len() - 1) / 2] / 1000
}

fn input_counts(draw: &mut Draw, step: &Step, room: u64) -> (u64, u64, u64) {
    let keys = if step.password_input {
        0
    } else {
        draw.between(0, room.min(2))
    };
    let clicks = draw.between(0, (room - keys).min(2));
    let scrolls = draw.between(0, room.min(1));
    (keys, clicks, scrolls)
}

impl Composer<'_> {
    fn steps_of_task(&self, t: &Task) -> Vec<Step> {
        let s = &self.plan.services[t.service];
        s.flows[t.flow]
            .pages
            .iter()
            .map(|p| step_of(self.rates, s, p, &t.values))
            .collect()
    }

    fn noise_pool(&self, half: usize, seed_keys: &BTreeSet<(String, String)>) -> Vec<Step> {
        let mut out = Vec::new();
        for i in self.services_in(half, Category::Other) {
            out.extend(self.pages_of(i, seed_keys));
        }
        out
    }

    fn pages_of(&self, service: usize, seed_keys: &BTreeSet<(String, String)>) -> Vec<Step> {
        let s = &self.plan.services[service];
        let mut pages: Vec<&str> = Vec::new();
        for f in &s.flows {
            for p in &f.pages {
                if !pages.contains(&p.as_str()) {
                    pages.push(p);
                }
            }
        }
        pages
            .into_iter()
            .map(|p| step_of(self.rates, s, p, &[]))
            .filter(|st| {
                let key = (st.app.clone(), st.titles[0].clone());
                st.site.is_some() || !seed_keys.contains(&key)
            })
            .collect()
    }

    fn compose(&mut self) -> Result<Composed, Refusal> {
        let cut = CUT_PER_MILLE[usize::from(self.rates.interleave) - 1];
        let (lo, hi) = (
            STRETCH_MIN_MS[usize::from(self.rates.interleave) - 1],
            STRETCH_MAX_MS[usize::from(self.rates.interleave) - 1],
        );
        let noise_max = NOISE_MAX_PER_DAY[usize::from(self.rates.noise) - 1];
        let mut seed_keys: BTreeSet<(String, String)> = BTreeSet::new();
        for t in &self.tasks {
            if t.category != Category::Other {
                for st in self.steps_of_task(t) {
                    if st.site.is_none() {
                        seed_keys.insert((st.app.clone(), st.titles[0].clone()));
                    }
                }
            }
        }
        let pools = [
            self.noise_pool(0, &seed_keys),
            self.noise_pool(1, &seed_keys),
        ];
        let canary_pools: Vec<Vec<Step>> = self
            .plan
            .canary_services
            .iter()
            .map(|i| self.pages_of(*i, &seed_keys))
            .collect();
        let mut out = Composed {
            lines: BTreeMap::new(),
            marked: BTreeMap::new(),
            steps: BTreeMap::new(),
            stretch_ms: Vec::new(),
            episode_ms: Vec::new(),
            episodes: 0,
        };
        let total = self.half_size * 2;
        for stream in 0..total {
            let half = self.half_of(stream);
            let sid = self.stream_id(stream);
            let tasks: Vec<usize> = (0..self.tasks.len())
                .filter(|t| self.tasks[*t].stream == stream)
                .collect();
            let non_seed = tasks
                .iter()
                .find(|t| self.tasks[**t].kind == TaskKind::NonSeed)
                .map(|t| self.tasks[*t].service);
            let own_pool: Vec<Step> = {
                let pool = &pools[half];
                let skip = non_seed.map(|i| self.plan.services[i].host.clone());
                let narrowed: Vec<Step> = pool
                    .iter()
                    .filter(|st| {
                        skip.as_ref().is_none_or(|h| st.site != *h)
                            || non_seed.is_some_and(|i| !self.plan.services[i].web)
                    })
                    .cloned()
                    .collect();
                if narrowed.is_empty() {
                    pool.clone()
                } else {
                    narrowed
                }
            };
            let mut active: BTreeSet<u32> = BTreeSet::new();
            for t in &tasks {
                active.extend(self.tasks[*t].days.iter().copied());
            }
            let active: Vec<u32> = active.into_iter().collect();
            let mut canary_days: Vec<(u32, usize)> = Vec::new();
            for c in 0..canary_pools.len() {
                let d = active[self.draw.below(active.len() as u64) as usize];
                canary_days.push((d, c));
            }
            let mut episode_seq: Vec<(i64, String, usize, Vec<Item>, u32)> = Vec::new();
            for day in &active {
                let mut blocks: Vec<usize> = tasks
                    .iter()
                    .copied()
                    .filter(|t| {
                        self.tasks[*t].days.contains(day) && self.tasks[*t].kind != TaskKind::Marked
                    })
                    .collect();
                self.draw.shuffle(&mut blocks);
                let marked = tasks.iter().copied().find(|t| {
                    self.tasks[*t].kind == TaskKind::Marked && self.tasks[*t].days[0] == *day
                });
                let mut units: Vec<Unit> = Vec::new();
                for t in blocks {
                    let stretches = self
                        .steps_of_task(&self.tasks[t])
                        .into_iter()
                        .map(|st| (st, self.draw.between(lo, hi)))
                        .collect();
                    units.push(Unit {
                        task: Some(t),
                        stretches,
                    });
                }
                let n_noise = self.draw.between(0, noise_max);
                for _ in 0..n_noise {
                    if own_pool.is_empty() {
                        break;
                    }
                    let st = own_pool[self.draw.below(own_pool.len() as u64) as usize].clone();
                    let len = self.draw.between(lo, hi);
                    let at = self.draw.below(units.len() as u64 + 1) as usize;
                    units.insert(
                        at,
                        Unit {
                            task: None,
                            stretches: vec![(st, len)],
                        },
                    );
                }
                for (d, c) in &canary_days {
                    if d == day && !canary_pools[*c].is_empty() {
                        let pool = &canary_pools[*c];
                        let st = pool[self.draw.below(pool.len() as u64) as usize].clone();
                        let len = self.draw.between(lo, hi);
                        let at = self.draw.below(units.len() as u64 + 1) as usize;
                        units.insert(
                            at,
                            Unit {
                                task: None,
                                stretches: vec![(st, len)],
                            },
                        );
                    }
                }
                let mut cursor =
                    (self.draw.between(DAY_START_MS.0, DAY_START_MS.1) * 1_000_000) as i64;
                let mut i = 0;
                let marked_at = marked.map(|_| self.draw.below(units.len() as u64 + 1) as usize);
                while i <= units.len() {
                    if marked_at == Some(i) {
                        if let Some(t) = marked {
                            cursor = self.lay_marked(
                                &mut out,
                                stream,
                                &sid,
                                t,
                                *day,
                                cursor,
                                lo,
                                hi,
                                &mut episode_seq,
                            )?;
                            cursor += (self.draw.between(GAP_MS.0, GAP_MS.1) * 1_000_000) as i64;
                        }
                    }
                    if i == units.len() {
                        break;
                    }
                    let interrupt = units[i].task.is_some()
                        && i + 1 < units.len()
                        && self.draw.below(1000) < cut
                        && marked_at != Some(i + 1);
                    let mut pieces: Vec<(Option<usize>, Step, u64)> = Vec::new();
                    if interrupt {
                        let (a, b) = (&units[i], &units[i + 1]);
                        let k = a.stretches.len();
                        let mut first: Vec<(Step, u64)> = a.stretches.clone();
                        let mut rest: Vec<(Step, u64)> = Vec::new();
                        if k >= 2 {
                            let j = self.draw.between(1, k as u64 - 1) as usize;
                            rest = first.split_off(j);
                        } else {
                            let (st, len) = first[0].clone();
                            let part = self.draw.between(len / 4, len * 3 / 4);
                            first[0].1 = part;
                            rest.push((st, len - part));
                        }
                        pieces.extend(first.into_iter().map(|(s, l)| (a.task, s, l)));
                        pieces.extend(b.stretches.iter().cloned().map(|(s, l)| (b.task, s, l)));
                        pieces.extend(rest.into_iter().map(|(s, l)| (a.task, s, l)));
                        i += 2;
                    } else {
                        pieces.extend(
                            units[i]
                                .stretches
                                .iter()
                                .cloned()
                                .map(|(s, l)| (units[i].task, s, l)),
                        );
                        i += 1;
                    }
                    let mut by_task: BTreeMap<usize, Vec<Item>> = BTreeMap::new();
                    for (task, st, len) in pieces {
                        let start = cursor;
                        let end = start + (len as i64) * MILLI;
                        if end > DAY {
                            return Err(short("day"));
                        }
                        let room = whole_seconds_between(
                            instant(self.rates.day1, *day, start).unwrap_or(0),
                            instant(self.rates.day1, *day, end).unwrap_or(0),
                        ) as u64;
                        let (keys, clicks, scrolls) = input_counts(&mut self.draw, &st, room);
                        let item = Item {
                            step: st.step.clone(),
                            title: 0,
                            start,
                            end,
                            keys,
                            clicks,
                            scrolls,
                        };
                        out.stretch_ms.push(len);
                        out.steps.insert(st.step.clone(), st);
                        cursor = end;
                        match task {
                            Some(t) => by_task.entry(t).or_default().push(item),
                            None => out.lines.entry(stream).or_default().push((
                                start,
                                Line::Noise(Noise {
                                    stream: sid.clone(),
                                    day: *day,
                                    item,
                                }),
                            )),
                        }
                    }
                    for (t, items) in by_task {
                        episode_seq.push((items[0].start, String::new(), t, items, *day));
                    }
                    cursor += (self.draw.between(GAP_MS.0, GAP_MS.1) * 1_000_000) as i64;
                }
            }
            self.emit_episodes(&mut out, stream, &sid, episode_seq);
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn lay_marked(
        &mut self,
        out: &mut Composed,
        stream: usize,
        sid: &str,
        t: usize,
        day: u32,
        mut cursor: i64,
        lo: u64,
        hi: u64,
        seq: &mut Vec<(i64, String, usize, Vec<Item>, u32)>,
    ) -> Result<i64, Refusal> {
        let steps = self.steps_of_task(&self.tasks[t]);
        let mark = |edge: Edge, at: i64| {
            Line::Mark(Mark {
                stream: sid.to_owned(),
                day,
                edge,
                at,
            })
        };
        out.marked
            .entry(stream)
            .or_default()
            .push((cursor, mark(Edge::Start, cursor)));
        for _ in 0..5 {
            cursor += (self.draw.between(MARK_GAP_MS.0, MARK_GAP_MS.1) * 1_000_000) as i64;
            let mut items = Vec::new();
            for st in &steps {
                let len = self.draw.between(lo, hi);
                let start = cursor;
                let end = start + (len as i64) * MILLI;
                if end >= DAY {
                    return Err(short("day"));
                }
                let room = whole_seconds_between(
                    instant(self.rates.day1, day, start).unwrap_or(0),
                    instant(self.rates.day1, day, end).unwrap_or(0),
                ) as u64;
                let (keys, clicks, scrolls) = input_counts(&mut self.draw, st, room);
                items.push(Item {
                    step: st.step.clone(),
                    title: 0,
                    start,
                    end,
                    keys,
                    clicks,
                    scrolls,
                });
                out.stretch_ms.push(len);
                out.steps.insert(st.step.clone(), st.clone());
                cursor = end;
            }
            seq.push((items[0].start, String::new(), t, items, day));
        }
        cursor += (self.draw.between(MARK_GAP_MS.0, MARK_GAP_MS.1) * 1_000_000) as i64;
        if cursor >= DAY {
            return Err(short("day"));
        }
        out.marked
            .entry(stream)
            .or_default()
            .push((cursor, mark(Edge::End, cursor)));
        Ok(cursor)
    }

    fn emit_episodes(
        &self,
        out: &mut Composed,
        stream: usize,
        sid: &str,
        mut seq: Vec<(i64, String, usize, Vec<Item>, u32)>,
    ) {
        seq.sort_by_key(|e| (e.4, e.0));
        for (n, (start, _, t, items, day)) in seq.into_iter().enumerate() {
            let task = &self.tasks[t];
            let total: u64 = items
                .iter()
                .map(|i| ((i.end - i.start) / MILLI) as u64)
                .sum();
            out.episode_ms.push(total);
            out.episodes += 1;
            let line = Line::Episode(Episode {
                stream: sid.to_owned(),
                episode: format!("{sid}-e{:04}", n + 1),
                task: task.id.clone(),
                category: task.category,
                kind: task.kind,
                day,
                ranges: items,
            });
            let target = if task.kind == TaskKind::Marked {
                &mut out.marked
            } else {
                &mut out.lines
            };
            target.entry(stream).or_default().push((start, line));
        }
    }
}

fn file_text(
    header: &str,
    stream_line: &str,
    events: &[(u32, i64, Line)],
    steps: &BTreeMap<String, Step>,
) -> String {
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for (_, _, l) in events {
        match l {
            Line::Episode(e) => used.extend(e.ranges.iter().map(|i| i.step.as_str())),
            Line::Noise(n) => {
                used.insert(n.item.step.as_str());
            }
            _ => {}
        }
    }
    let mut text = format!("{header}\n{stream_line}\n");
    for s in used {
        text.push_str(&Line::Step(steps[s].clone()).write());
        text.push('\n');
    }
    for (_, _, l) in events {
        text.push_str(&l.write());
        text.push('\n');
    }
    text
}

fn day_of_line(l: &Line) -> u32 {
    match l {
        Line::Episode(e) => e.day,
        Line::Noise(n) => n.day,
        Line::Mark(m) => m.day,
        _ => 0,
    }
}

/// Runs `compose` and returns the lines it prints.
pub fn compose(args: &ComposeArgs) -> Result<Vec<String>, Refusal> {
    let bytes = fs::read(&args.rates)
        .map_err(|_| Refusal::new("rates", args.rates.display().to_string()))?;
    let rates = Rates::parse(&bytes)
        .ok_or_else(|| Refusal::new("rates", args.rates.display().to_string()))?;
    let services = read_services(&args.sites)?;
    let plan = plan_services(&rates, services)?;
    let half_size = (rates.streams / 2) as usize;
    let mut c = Composer {
        rates: &rates,
        plan: &plan,
        draw: Draw::named(&format!(
            "halvo-sample compose {} {}",
            rates.seed,
            sides_text(rates.sides)
        )),
        half_size,
        tasks: Vec::new(),
        used: vec![BTreeSet::new(); half_size * 2],
    };
    c.plan_half(0)?;
    c.plan_half(1)?;
    c.shape_tasks();
    let mut composed = c.compose()?;
    check_halves(&c, &composed)?;
    let header = Line::Header(Header {
        day1: rates.day1,
        canaries: rates.canaries.clone(),
    })
    .write();
    let mut files: Vec<(String, String)> = Vec::new();
    for stream in 0..half_size * 2 {
        let sid = c.stream_id(stream);
        let stream_line = Line::Stream {
            stream: sid.clone(),
            sites: rates.sides.0,
        }
        .write();
        let order = |list: Option<&Vec<(i64, Line)>>| -> Vec<(u32, i64, Line)> {
            let mut v: Vec<(u32, i64, Line)> = list
                .map(|l| {
                    l.iter()
                        .map(|(t, x)| (day_of_line(x), *t, x.clone()))
                        .collect()
                })
                .unwrap_or_default();
            v.sort_by_key(|(d, t, _)| (*d, *t));
            v
        };
        let main = order(composed.lines.get(&stream));
        files.push((
            format!("{sid}.jsonl"),
            file_text(&header, &stream_line, &main, &composed.steps),
        ));
        let marked = order(composed.marked.get(&stream));
        if !marked.is_empty() {
            files.push((
                format!("{sid}.marked.jsonl"),
                file_text(&header, &stream_line, &marked, &composed.steps),
            ));
        }
    }
    create_out_dir(&args.out)?;
    for (name, text) in &files {
        let path: PathBuf = args.out.join(name);
        write_new(&path, text.as_bytes())
            .map_err(|_| Refusal::new("output", path.display().to_string()))?;
    }
    let per = |kind: TaskKind| -> String {
        let mut counts = [0u64; 10];
        for t in &c.tasks {
            if t.kind == kind && t.category != Category::Other {
                counts[seed_index(t.category)] += 1;
            }
        }
        counts
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let planted = per(TaskKind::Planted);
    let near = per(TaskKind::NearMiss);
    let twice = per(TaskKind::Twice);
    Ok(vec![
        format!("compose seed={} sides={}", rates.seed, sides_text(rates.sides)),
        format!(
            "composed streams={} tasks={} episodes={} planted={planted} near_miss={near} twice={twice}",
            half_size * 2,
            c.tasks.len(),
            composed.episodes
        ),
        format!(
            "medians stretch_seconds={} episode_seconds={}",
            median_seconds(&mut composed.stretch_ms),
            median_seconds(&mut composed.episode_ms)
        ),
    ])
}

/// The two halves share no service but a canary's and no episode template, and inside a half each stream after
/// the first shares a site that is no canary with an earlier stream of that half.
fn check_halves(c: &Composer, composed: &Composed) -> Result<(), Refusal> {
    let mut templates: [BTreeMap<Vec<String>, Category>; 2] = [BTreeMap::new(), BTreeMap::new()];
    let mut sites: Vec<BTreeSet<String>> = vec![BTreeSet::new(); c.half_size * 2];
    let canaries: BTreeSet<&str> = c.rates.canaries.iter().map(String::as_str).collect();
    for (stream, list) in composed.lines.iter().chain(composed.marked.iter()) {
        let half = c.half_of(*stream);
        for (_, l) in list {
            let items: Vec<&Item> = match l {
                Line::Episode(e) => e.ranges.iter().collect(),
                Line::Noise(n) => vec![&n.item],
                _ => Vec::new(),
            };
            for i in &items {
                if let Some(s) = &composed.steps[&i.step].site {
                    if !canaries.contains(s.as_str()) {
                        sites[*stream].insert(s.clone());
                    }
                }
            }
            if let Line::Episode(e) = l {
                let t: Vec<String> = crate::text::template(items.iter().map(|i| {
                    let st = &composed.steps[&i.step];
                    site_masked(&st.titles[i.title], st.site.as_deref())
                }));
                templates[half].entry(t).or_insert(e.category);
            }
        }
    }
    if let Some(category) = templates[0]
        .iter()
        .find(|(t, _)| templates[1].contains_key(*t))
        .map(|(_, c)| *c)
    {
        return Err(short(category.as_str()));
    }
    for half in 0..2 {
        let first = half * c.half_size;
        for s in first + 1..first + c.half_size {
            let shared = (first..s).any(|e| !sites[s].is_disjoint(&sites[e]));
            if !shared {
                return Err(short("link"));
            }
        }
    }
    Ok(())
}
