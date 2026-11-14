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

/// The scripted case that seeds one workflow, with its first occurrence on day `first`: its one step's control
/// takes the marker as its name and the learned edit becomes `active` after two clean uses; then the control is
/// renamed again, and the new edit is served once where another control carries its name, which reverts it; on
/// that last occurrence the control holds the marker again, so the repair proposes the locator of the `active`
/// edit, which is refused, and the `active` edit is the one in force.
pub fn seed_case(marker: &str, first: u32) -> Case {
    let names = [
        "Pay now".to_owned(),
        marker.to_owned(),
        format!("{marker} second"),
    ];
    let pages: [Vec<Control>; 6] = [
        vec![button(&names[0], "pay-1", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[1], "pay-2", "go")],
        vec![button(&names[2], "pay-3", "go")],
        vec![
            button(&names[1], "pay-2", "go"),
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
        occurrences: (first..)
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

/// The two seeded cases: the second starts on the last day of the first, so that no open of its play removes a
/// row of the first for lying ahead of the open's day.
pub fn seed_cases() -> [Case; 2] {
    let first = seed_case(MARKERS[0], SEED_DAY);
    let last = first.occurrences[first.occurrences.len() - 1].day;
    [first, seed_case(MARKERS[1], last)]
}

/// The first day of the first seeded case.
pub const SEED_DAY: u32 = 20_000;

/// The play of the second seeded workflow, on the store that holds both seeded workflows, each with its agent,
/// its compile and its learned edits (L10.4), and the id of the first workflow as its play made it.
pub fn seed(cases: &[Case; 2]) -> Result<(Play<'_>, String), String> {
    let mut first = Play::new(&cases[0], Arm::Admit, 1, MemoryStore::new());
    play_all(&mut first, false)?;
    let first_workflow = first.workflow().to_owned();
    let mut second = Play::new(&cases[1], Arm::Admit, 2, first.host.store);
    play_all(&mut second, false)?;
    Ok((second, first_workflow))
}

/// The rows of the family `learn` that name a workflow, each as (kind, bytes), in byte order.
fn rows_naming(store: &MemoryStore, workflow: &str) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = store
        .rows(Family::Learn)
        .all()
        .filter(|(_, json)| {
            serde_json::from_str::<serde_json::Value>(json).is_ok_and(|v| v["workflow"] == workflow)
        })
        .map(|(kind, json)| (kind, json.to_owned()))
        .collect();
    rows.sort();
    rows
}

/// A cascade of the store: deletes a workflow with the rows that name it and answers whether it was stored.
pub type Cascade<'a> = &'a mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool;

/// The line of `delete --what workflow` with the cascade given, and whether it reads `result=ok` (L10.4; the
/// coordinator's ruling LM-54 E5). After the seed's last request, with no open and no request until the counts
/// after the cascade, it counts the rows of the family `learn` that name the first workflow (by the id that its
/// play made) and keeps those of the second, each by kind and bytes; it calls the cascade; then `rows_left`
/// counts the first workflow's rows and `other_rows` the second's, and `other_served` is yes when the first
/// locator served to the second agent is a role locator whose name is the second marker exactly, the name of
/// its `active` edit. `result=ok` needs all of: rows of the first workflow before the cascade, a cascade that
/// answered that the workflow was stored, `rows_left` 0, the second workflow's rows the same rows as before by
/// kind and bytes and more than none, and `other_served` yes. The line counts no row of the family `agent` on
/// either side and does not read the workflow record; on the memory store it calls the store's cascade and
/// sends no decision.
pub fn workflow_line(cascade: Cascade<'_>) -> Result<(String, bool), String> {
    let cases = seed_cases();
    let (mut play, first) = seed(&cases)?;
    let id = WorkflowId::parse(&first).map_err(|_| "the first workflow's id".to_owned())?;
    let second = play.workflow().to_owned();
    let before = rows_naming(&play.host.store, &first).len();
    let kept = rows_naming(&play.host.store, &second);
    let stored = cascade(&mut play.host.store, &id);
    let rows_left = rows_naming(&play.host.store, &first).len();
    let after = rows_naming(&play.host.store, &second);
    let other_rows = after.len();
    let served = play.served_now()?;
    let learned = served.first().and_then(|step| match &step.target {
        Some(Target::Web(t)) => t.locators.first(),
        _ => None,
    });
    let other_served = learned.is_some_and(|l| {
        l.by == LocatorBy::Role && l.name.as_ref().is_some_and(|n| n.as_str() == MARKERS[1])
    });
    let ok =
        before > 0 && stored && rows_left == 0 && after == kept && other_rows > 0 && other_served;
    let line = format!(
        "delete what=workflow store=memory rows_left={rows_left} other_rows={other_rows} other_served={} result={}",
        if other_served { "yes" } else { "no" },
        if ok { "ok" } else { "miss" }
    );
    Ok((line, ok))
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
    let (line, ok) = workflow_line(&mut |store, id| store.delete_workflow(id))
        .map_err(|detail| Refusal::new(crate::cli::Cause::Store, detail))?;
    Ok(Report {
        lines: vec![line],
        ok,
    })
}
