//! The workflows that Halvo found: the proposals list of the page and every stored workflow.
//!
//! No relay to the derived store stands yet, so the commands answer from a stand-in: `workflows`
//! says that this part of Halvo does not run, and every decision answers `not_running`.

use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::Runtime;

use super::names::{Names, WORKFLOWS};
use super::Registration;

/// The ten seed categories by id, each with the text that a person reads, in the order of
/// `SeedCategory` in halvo-schema-core: the order in which the proposals list breaks a tie of
/// counts (request WR1 of the interface run).
pub const CATEGORY_TEXTS: [(&str, &str); 10] = [
    ("utility_bill_payment", "Paying a utility bill"),
    ("tenant_rent", "Handling a tenant's rent"),
    ("lease_renewal_or_change", "Renewing or changing a lease"),
    (
        "tenant_application_review",
        "Reviewing a tenant application",
    ),
    ("email", "Writing or answering email"),
    ("spreadsheet_fill", "Filling in a spreadsheet"),
    ("shop_order", "Ordering from a shop"),
    ("calendar_cleanup", "Tidying a calendar"),
    ("ride_order", "Ordering a ride"),
    ("daily_chart_check", "Checking a chart"),
];

/// The most proposals that the list holds.
pub const MAX_PROPOSALS: usize = 4;

/// What was seen, as a row says it (request WR1 of the interface run): the text of the category,
/// then ` in ` and the app's display name, then ` on ` and the site, a part that is not known
/// left out. `app` is the name that the operating system gives for the workflow's app, never its
/// app id. None for a category id that is not one of the ten.
#[expect(
    dead_code,
    reason = "request WR1: the agent-setup run names its agents with it; no caller stands on main"
)]
pub fn row_text(category: &str, app: Option<&str>, site: Option<&str>) -> Option<String> {
    let mut text = CATEGORY_TEXTS[category_rank(category)?].1.to_string();
    if let Some(app) = app.filter(|a| !a.is_empty()) {
        text.push_str(" in ");
        text.push_str(app);
    }
    if let Some(site) = site.filter(|s| !s.is_empty()) {
        text.push_str(" on ");
        text.push_str(site);
    }
    Some(text)
}

fn category_rank(id: &str) -> Option<usize> {
    CATEGORY_TEXTS.iter().position(|(known, _)| *known == id)
}

/// One stored workflow as the `workflows` read of the derived store hands it out, with the app
/// resolved to its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    /// Whether the workflow is in the state `proposed`.
    pub proposed: bool,
    pub category: String,
    /// The display name that the operating system gives for the workflow's app, if it gives one.
    pub app: Option<String>,
    pub site: Option<String>,
    /// The ids of the summaries that support the workflow, in ascending order.
    pub summaries: Vec<String>,
}

/// The proposals list: the workflows in the state `proposed` with the most supporting summaries,
/// at most `MAX_PROPOSALS`; a tie of counts goes by the order of the category in
/// `CATEGORY_TEXTS`, then by the summary ids, the first one first, then by the workflow id. A
/// workflow whose category is not one of the ten is left out.
pub fn proposals(listed: &[Listed]) -> Vec<&Listed> {
    let mut found: Vec<(usize, &Listed)> = listed
        .iter()
        .filter(|w| w.proposed)
        .filter_map(|w| category_rank(&w.category).map(|rank| (rank, w)))
        .collect();
    found.sort_by(|(rank_a, a), (rank_b, b)| {
        b.summaries
            .len()
            .cmp(&a.summaries.len())
            .then(rank_a.cmp(rank_b))
            .then_with(|| a.summaries.cmp(&b.summaries))
            .then_with(|| a.id.cmp(&b.id))
    });
    found
        .into_iter()
        .take(MAX_PROPOSALS)
        .map(|(_, w)| w)
        .collect()
}

/// A row of the page: the workflow id, the category id, the app's display name, the site and the
/// number of supporting summaries. The page composes the text from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub id: String,
    pub category: &'static str,
    pub app: Option<String>,
    pub site: Option<String>,
    pub count: usize,
}

impl Row {
    fn of(w: &Listed) -> Option<Row> {
        Some(Row {
            id: w.id.clone(),
            category: CATEGORY_TEXTS[category_rank(&w.category)?].0,
            app: w.app.clone(),
            site: w.site.clone(),
            count: w.summaries.len(),
        })
    }
}

/// The reply of `workflows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkflowsView {
    /// This part of Halvo does not run: the page says that Halvo is not looking yet.
    NotRunning,
    /// The proposals list and every stored workflow in any state.
    Running {
        proposals: Vec<Row>,
        stored: Vec<Row>,
    },
}

fn view(listed: Option<&[Listed]>) -> WorkflowsView {
    match listed {
        None => WorkflowsView::NotRunning,
        Some(listed) => WorkflowsView::Running {
            proposals: proposals(listed).into_iter().filter_map(Row::of).collect(),
            stored: listed.iter().filter_map(Row::of).collect(),
        },
    }
}

/// The error of a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum DecisionError {
    /// This part of Halvo does not run, so nothing was decided.
    NotRunning,
}

/// The stand-in of the relay: it lists no workflow and says that this part does not run.
fn stand_in_read() -> Option<Vec<Listed>> {
    None
}

/// The stand-in of the relay: it takes no decision.
fn stand_in_decide(_workflow: &str) -> Result<(), DecisionError> {
    Err(DecisionError::NotRunning)
}

pub struct Workflows;

impl Registration for Workflows {
    fn names(&self) -> &'static Names {
        &WORKFLOWS
    }
}

/// Answers an invoke of a command of `names::WORKFLOWS`; returns false for any other name.
pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    let handle: fn(Invoke<R>) -> bool = tauri::generate_handler![
        workflows,
        approve_workflow,
        dismiss_workflow,
        delete_workflow,
    ];
    handle(invoke)
}

#[tauri::command]
pub async fn workflows() -> WorkflowsView {
    view(stand_in_read().as_deref())
}

/// Moves a proposed workflow to approved. It takes the workflow id and nothing else.
#[tauri::command]
pub async fn approve_workflow(workflow: String) -> Result<(), DecisionError> {
    stand_in_decide(&workflow)
}

/// Sets a proposed workflow aside: the dismissal of the store. It takes the workflow id and
/// nothing else.
#[tauri::command]
pub async fn dismiss_workflow(workflow: String) -> Result<(), DecisionError> {
    stand_in_decide(&workflow)
}

/// Deletes a stored workflow and everything that supported it. It takes the workflow id and
/// nothing else.
#[tauri::command]
pub async fn delete_workflow(workflow: String) -> Result<(), DecisionError> {
    stand_in_decide(&workflow)
}
