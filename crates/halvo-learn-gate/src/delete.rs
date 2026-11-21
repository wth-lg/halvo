use std::path::Path;

use halvo_schema::executable::names::LocatorBy;
use halvo_schema::executable::step::Target;
use halvo_schema::WorkflowId;

use crate::case::{Case, Form, Kind, Occurrence, Step, Verb};
use crate::cli::{Cause, Flags, Refusal, Report, StoreName};
use crate::derived::{process_key, DerivedHost};
use crate::host::{GateHost, PlayHost};
use crate::page::{Control, Role};
use crate::store::MemoryStore;
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

/// The play of the second seeded workflow on the derived store under `data_dir`, opened with a key that this
/// process makes, which holds both seeded workflows, each made by the test proposer and an approval, with its
/// agent, its compile and its learned edits (L10.1, L10.4), and the id of the first workflow as the store gave it.
/// The host keeps every report of the store from its first open on (`DerivedHost::with_reports`), which
/// `privacy::derived_outcomes` reads.
pub fn seed_derived<'c>(
    cases: &'c [Case; 2],
    data_dir: &Path,
) -> Result<(Play<'c, DerivedHost>, String), String> {
    let host = DerivedHost::new(
        data_dir.to_path_buf(),
        process_key(),
        cases[0].occurrences[0].day,
    )
    .with_reports();
    let mut first = Play::with_host(&cases[0], Arm::Admit, 1, host);
    play_all(&mut first, false)?;
    let first_workflow = first.workflow().to_owned();
    let mut second = Play::with_host(&cases[1], Arm::Admit, 2, first.host);
    play_all(&mut second, false)?;
    Ok((second, first_workflow))
}

/// The rows of the family `learn` that name a workflow, each as (kind, bytes), in byte order.
fn rows_naming<H: PlayHost>(
    host: &mut H,
    workflow: &str,
) -> Result<Vec<(&'static str, String)>, String> {
    let mut rows = Vec::new();
    for entry in halvo_schema::learning::KINDS {
        for json in host.learn_rows(entry.name)? {
            if serde_json::from_str::<serde_json::Value>(&json)
                .is_ok_and(|v| v["workflow"] == workflow)
            {
                rows.push((entry.name, json));
            }
        }
    }
    rows.sort();
    Ok(rows)
}

/// A cascade of the store: deletes a workflow with the rows that name it and answers whether it was stored.
pub type Cascade<'a> = &'a mut dyn FnMut(&mut MemoryStore, &WorkflowId) -> bool;

/// The line of `delete --what workflow` on the memory store with the cascade given, and whether it reads
/// `result=ok` (L10.4; the coordinator's ruling LM-54 E5): `line_after` on the play of `seed`, whose cascade calls
/// the given one on the memory store and sends no decision.
pub fn workflow_line(cascade: Cascade<'_>) -> Result<(String, bool), String> {
    let cases = seed_cases();
    let (play, first) = seed(&cases)?;
    line_after(
        StoreName::Memory,
        play,
        &first,
        &mut |host: &mut GateHost, id| Ok(cascade(&mut host.store, id)),
    )
}

/// The line of `delete --what workflow` on the derived store under `data_dir`, and whether it reads `result=ok`
/// (L10.4): `line_after` on the play of `seed_derived`, whose cascade is the decision `delete` of the first
/// workflow, the store's own (W-C3.10, contracts.md L5.1).
pub fn derived_workflow_line(data_dir: &Path) -> Result<(String, bool), String> {
    let cases = seed_cases();
    let (play, first) = seed_derived(&cases, data_dir)?;
    line_after(
        StoreName::Derived,
        play,
        &first,
        &mut |host: &mut DerivedHost, id| host.delete_workflow(id),
    )
}

/// The line of `delete --what workflow` after a seed whose second play is `play` and whose first workflow is
/// `first`, with the cascade given. After the seed's last request, with no open and no request until the counts
/// after the cascade but the reads of the rows, it counts the rows of the family `learn` that name the first
/// workflow (by the id that its play made) and keeps those of the second, each by kind and bytes; it calls the
/// cascade; then `rows_left` counts the first workflow's rows and `other_rows` the second's, and `other_served` is
/// yes when the first locator served to the second agent is a role locator whose name is the second marker
/// exactly, the name of its `active` edit. `result=ok` needs all of: rows of the first workflow before the
/// cascade, a cascade that answered that the workflow was stored, `rows_left` 0, the second workflow's rows the
/// same rows as before by kind and bytes and more than none, and `other_served` yes. The line counts no row of the
/// family `agent` on either side and does not read the workflow record.
pub fn line_after<H: PlayHost>(
    store: StoreName,
    mut play: Play<'_, H>,
    first: &str,
    cascade: &mut dyn FnMut(&mut H, &WorkflowId) -> Result<bool, String>,
) -> Result<(String, bool), String> {
    let id = WorkflowId::parse(first).map_err(|_| "the first workflow's id".to_owned())?;
    let second = play.workflow().to_owned();
    let before = rows_naming(&mut play.host, first)?.len();
    let kept = rows_naming(&mut play.host, &second)?;
    let stored = cascade(&mut play.host, &id)?;
    let rows_left = rows_naming(&mut play.host, first)?.len();
    let after = rows_naming(&mut play.host, &second)?;
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
        "delete what=workflow store={} rows_left={rows_left} other_rows={other_rows} other_served={} result={}",
        store.as_str(),
        if other_served { "yes" } else { "no" },
        if ok { "ok" } else { "miss" }
    );
    Ok((line, ok))
}

/// `learn-gate delete --what workflow|all --data-dir DIR [--store memory|derived]` (L10.4). `--what all` reads
/// `not-run` on either store while `halvo-capture --delete-all --data-dir DIR` is not built (RC2), and touches
/// nothing; `--what workflow` on the derived store seeds the store under DIR, which stays there.
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    let what = flags
        .value("--what")?
        .ok_or_else(|| Refusal::usage("delete takes --what workflow|all"))?;
    let data_dir = flags
        .value("--data-dir")?
        .ok_or_else(|| Refusal::usage("delete takes --data-dir DIR"))?;
    let store = flags.value("--store")?.map(StoreName::parse).transpose()?;
    flags.done()?;
    if what != "workflow" && what != "all" {
        return Err(Refusal::usage("--what takes workflow or all"));
    }
    let store = store.unwrap_or(StoreName::Memory);
    if what == "all" {
        return Ok(Report {
            lines: vec![format!(
                "delete what=all store={} exit=- files_left=- result=not-run",
                store.as_str()
            )],
            ok: true,
        });
    }
    let (line, ok) = match store {
        StoreName::Memory => workflow_line(&mut |store, id| store.delete_workflow(id)),
        StoreName::Derived => derived_workflow_line(Path::new(data_dir)),
    }
    .map_err(|detail| Refusal::new(Cause::Store, detail))?;
    Ok(Report {
        lines: vec![line],
        ok,
    })
}
