tokens! {
    /// The one cause of [`check_sources`].
    pub enum SourceCause {
        Source = "source",
    }
}

/// The source figures of one `learn_edit` revision (L1.3), as plain values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSources {
    /// The workflow id.
    pub workflow: String,
    /// The agent id.
    pub agent: String,
    /// The run ids of `runs`, in order.
    pub runs: Vec<String>,
    /// The lowest day of the runs.
    pub first: u32,
    /// The highest day of the runs.
    pub last: u32,
    /// The clean uses.
    pub clean: u32,
}

/// The `learn_run` rev 2 that the commit stores, as plain values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSource {
    /// The workflow id.
    pub workflow: String,
    /// The agent id.
    pub agent: String,
    /// The run id.
    pub run: String,
    /// The run's day.
    pub day: u32,
}

/// What [`check_sources`] reads (L3.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFacts {
    /// The revision to be stored.
    pub edit: EditSources,
    /// The revision before it, or `None` at rev 1.
    pub before: Option<EditSources>,
    /// The `learn_run` rev 2 that the same commit stores, or `None` when it stores none.
    pub run: Option<RunSource>,
}

/// `Ok` when a `learn_edit` revision keeps the source rule (L1.5, L3.7), else `source`. The
/// revision adds a run at rev 1 or when its `runs` is one longer than the revision before.
/// One that adds no run answers `source` when `runs`, `first` or `last` differ from the
/// revision before or `clean` is higher. One that adds a run answers `source` when the added
/// id is not the commit's run of the same workflow and agent, when the ids before it are not
/// the `runs` of the revision before, or when `first` is not the lower, or `last` not the
/// higher, of that revision's value and the added run's day (both the run's day at rev 1).
/// Every revision also answers `source` when its workflow or agent differs from the revision
/// before.
pub fn check_sources(facts: &SourceFacts) -> Result<(), SourceCause> {
    let edit = &facts.edit;
    let same_owner = facts
        .before
        .as_ref()
        .is_none_or(|before| before.workflow == edit.workflow && before.agent == edit.agent);
    let kept = match &facts.before {
        Some(before) if edit.runs.len() != before.runs.len() + 1 => {
            edit.runs == before.runs
                && edit.first == before.first
                && edit.last == before.last
                && edit.clean <= before.clean
        }
        before => adds_run(edit, before.as_ref(), facts.run.as_ref()),
    };
    if same_owner && kept {
        Ok(())
    } else {
        Err(SourceCause::Source)
    }
}

fn adds_run(edit: &EditSources, before: Option<&EditSources>, run: Option<&RunSource>) -> bool {
    let (Some(run), Some((added, earlier))) = (run, edit.runs.split_last()) else {
        return false;
    };
    let (earlier_runs, first, last) = match before {
        Some(before) => (
            before.runs.as_slice(),
            before.first.min(run.day),
            before.last.max(run.day),
        ),
        None => (&[][..], run.day, run.day),
    };
    *added == run.run
        && run.workflow == edit.workflow
        && run.agent == edit.agent
        && earlier == earlier_runs
        && edit.first == first
        && edit.last == last
}
