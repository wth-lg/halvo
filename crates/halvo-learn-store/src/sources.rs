use halvo_learn::{check_sources, EditSources, RunSource, SourceFacts};
use halvo_schema::learning::record::{LearnEdit, LearnRun};

fn sources(row: &LearnEdit) -> EditSources {
    EditSources {
        workflow: row.workflow.as_str().to_owned(),
        agent: row.agent.as_str().to_owned(),
        runs: row.runs.iter().map(|r| r.as_str().to_owned()).collect(),
        first: row.first.get(),
        last: row.last.get(),
        clean: row.clean,
    }
}

/// `check_sources` (contracts.md L1.5, L3.7) for a `learn_edit` revision to be stored, with the revision before
/// it and the `learn_run` rev 2 that the same commit stores.
pub(crate) fn kept(row: &LearnEdit, before: Option<&LearnEdit>, run: Option<&LearnRun>) -> bool {
    let facts = SourceFacts {
        edit: sources(row),
        before: before.map(sources),
        run: run.map(|r| RunSource {
            workflow: r.workflow.as_str().to_owned(),
            agent: r.agent.as_str().to_owned(),
            run: r.run.as_str().to_owned(),
            day: r.day.get(),
        }),
    };
    check_sources(&facts).is_ok()
}
