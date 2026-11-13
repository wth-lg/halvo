use halvo_schema::executable::names::LocatorBy;
use halvo_schema::executable::step::Target;
use halvo_schema::WorkflowId;

use crate::case::{Case, Form, Kind, Occurrence, Step, Verb};
use crate::cli::{Flags, Refusal, Report, StoreName};
use crate::page::{Control, Role};
use crate::store::{Family, MemoryStore};
use crate::stub::{play_all, Arm, Play};

/// The markers of contracts.md L11.4: each seeded workflow's learned labels hold its marker.
pub const MARKERS: [&str; 2] = ["Zq7-halvo-marker-learn-a", "Zq7-halvo-marker-learn-b"];

fn button(name: &str, test_id: &str, id: &str) -> Control {
    Control {
        id: id.to_owned(),
        role: Role::Button,
        name: name.to_owned(),
        test_id: test_id.to_owned(),
        passes: Vec::new(),
    }
}

/// The scripted case that seeds one workflow: its one step's control takes the marker as its name and the
/// learned edit becomes `active` after two clean uses; then the control is renamed again, the new edit is served
/// once where another control carries its name, and that counted failure reverts it.
pub fn seed_case(marker: &str) -> Case {
    let names = [
        "Pay now".to_owned(),
        marker.to_owned(),
        format!("{marker} second"),
        format!("{marker} third"),
    ];
    let pages: [Vec<Control>; 6] = [
        vec![button(&names[0], "pay-1", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[2], "pay-3", "go")],
        vec![
            button(&names[3], "pay-4", "go"),
            button(&names[2], "other-1", "other"),
        ],
    ];
    Case {
        name: "seed".to_owned(),
        kind: Kind::Benign,
        form: Form::Renamed,
        shows: 0,
        steps: vec![Step {
            n: 1,
            verb: Verb::Click,
            control: "go".to_owned(),
        }],
        occurrences: (20_000u32..)
            .zip(pages)
            .map(|(day, page)| Occurrence {
                day,
                page,
                repair: vec![vec!["go".to_owned()]],
                flaky: Vec::new(),
                restart: false,
            })
            .collect(),
    }
}

/// The play of the second seeded workflow, on the store that holds both seeded workflows, each with its agent,
/// its compile and its learned edits (L10.4).
pub fn seed(cases: &[Case; 2]) -> Result<Play<'_>, String> {
    let mut first = Play::new(&cases[0], Arm::Admit, 1, MemoryStore::new());
    play_all(&mut first, false)?;
    let mut second = Play::new(&cases[1], Arm::Admit, 2, first.host.store);
    play_all(&mut second, false)?;
    Ok(second)
}

fn rows_naming(store: &MemoryStore, workflow: &str) -> usize {
    store
        .rows(Family::Learn)
        .all()
        .filter(|(_, json)| {
            serde_json::from_str::<serde_json::Value>(json).is_ok_and(|v| v["workflow"] == workflow)
        })
        .count()
}

/// `learn-gate delete --what workflow|all --data-dir DIR [--store memory|derived]` (L10.4).
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    let what = flags
        .value("--what")?
        .ok_or_else(|| Refusal::usage("delete takes --what workflow|all"))?;
    flags
        .value("--data-dir")?
        .ok_or_else(|| Refusal::usage("delete takes --data-dir DIR"))?;
    let store = flags.value("--store")?.map(StoreName::parse).transpose()?;
    flags.done()?;
    if what != "workflow" && what != "all" {
        return Err(Refusal::usage("--what takes workflow or all"));
    }
    store.unwrap_or(StoreName::Memory).memory_only()?;
    if what == "all" {
        return Ok(Report {
            lines: vec!["delete what=all store=memory exit=- files_left=- result=not-run".into()],
            ok: true,
        });
    }
    let fail = |detail: String| Refusal::new(crate::cli::Cause::Store, detail);
    let cases = MARKERS.map(seed_case);
    let mut play = seed(&cases).map_err(fail)?;
    let first = crate::stub::id('3', 1);
    let id = WorkflowId::parse(&first).map_err(|_| fail("a workflow id".into()))?;
    play.host.store.delete_workflow(&id);
    let rows_left = rows_naming(&play.host.store, &first);
    let other_rows = rows_naming(&play.host.store, play.workflow());
    let served = play.served_now().map_err(fail)?;
    let learned = served.first().and_then(|step| match &step.target {
        Some(Target::Web(t)) => t.locators.first(),
        _ => None,
    });
    let other_served = learned.is_some_and(|l| {
        l.by == LocatorBy::Role
            && l.name
                .as_ref()
                .is_some_and(|n| n.as_str().contains(MARKERS[1]))
    });
    let ok = rows_left == 0 && other_rows > 0 && other_served;
    Ok(Report {
        lines: vec![format!(
            "delete what=workflow store=memory rows_left={rows_left} other_rows={other_rows} other_served={} result={}",
            if other_served { "yes" } else { "no" },
            if ok { "ok" } else { "miss" }
        )],
        ok,
    })
}
