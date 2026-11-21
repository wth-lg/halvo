//! The workflows that Halvo found: the proposals list of the page and every stored workflow.
//!
//! Each command builds one request of the role `interface`, sends its bytes through
//! `Control::relay` and reads the owner's reply with `decode_reply`.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;

use halvo_control::client::ClientError;
use halvo_control::proto::ErrorCode;
use halvo_schema_core::request::{
    decode_reply, ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, ReplyResult,
};
use halvo_schema_core::{
    Decision, RequestId, SiteValue, Workflow, WorkflowId, WorkflowItem, WorkflowState,
};
use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{Runtime, State};

use super::capture::AppState;
use super::control::Control;
use super::names::{Names, WORKFLOWS};
use super::Registration;
use crate::view_state::{read_error, ActionError};

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

/// One stored workflow as the `workflows` read of the derived store hands it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    /// Whether the workflow is in the state `proposed`.
    pub proposed: bool,
    pub category: String,
    /// The display name that the operating system gives for the workflow's app, if it gives one.
    /// It is none for every workflow: nothing asks the operating system for a name yet, and no
    /// row names an app id.
    pub app: Option<String>,
    pub site: Option<String>,
    /// The ids of the summaries that support the workflow, in ascending order.
    pub summaries: Vec<String>,
}

impl Listed {
    /// One item of a page of the read `workflows`. The site is the domain of a workflow whose
    /// site is a domain, and the item's hint for a site that comes from a window title.
    fn of(item: &WorkflowItem) -> Listed {
        let (id, proposed, category, site) = match &item.workflow {
            Workflow::Full(w) => (
                &w.id,
                w.state == WorkflowState::Proposed,
                w.category,
                &w.site,
            ),
            Workflow::Dismissed(w) => (&w.id, false, w.category, &w.site),
        };
        let site = match site {
            Some(SiteValue::Domain { key, .. }) => Some(key.as_str().to_string()),
            Some(SiteValue::Title { .. }) => {
                item.hint.as_ref().map(|hint| hint.as_str().to_string())
            }
            None => None,
        };
        Listed {
            id: id.as_str().to_string(),
            proposed,
            category: category.as_str().to_string(),
            app: None,
            site,
            summaries: item
                .summaries
                .iter()
                .map(|summary| summary.as_str().to_string())
                .collect(),
        }
    }
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

/// The reply of `workflows`: the list, or the state that stands in its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkflowsView {
    /// This part of Halvo does not run: the page says that Halvo is not looking yet.
    NotRunning,
    /// The derived store cannot be opened right now.
    StoreUnavailable,
    /// Halvo is deleting everything.
    Deleting,
    /// The proposals list and every stored workflow in any state.
    Running {
        proposals: Vec<Row>,
        stored: Vec<Row>,
    },
}

fn view(listed: &[Listed]) -> WorkflowsView {
    WorkflowsView::Running {
        proposals: proposals(listed).into_iter().filter_map(Row::of).collect(),
        stored: listed.iter().filter_map(Row::of).collect(),
    }
}

/// The error of a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum DecisionError {
    /// This part of Halvo does not run, so nothing was decided.
    NotRunning,
    /// The workflow was not deleted, because an agent still uses it.
    InUse,
    /// Any other error, as the commands of capture give it.
    #[serde(untagged)]
    Other(ActionError),
}

/// The id of a request, from 16 bytes of the keys of std's `RandomState`, which the system seeds
/// per process, as halvo-agent-gate mints an id. The owner keeps no log of request ids.
fn request_id() -> RequestId {
    let mut bytes = [0u8; 16];
    for half in bytes.chunks_mut(8) {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(0);
        half.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    RequestId::from_random(bytes)
}

/// What a request got.
enum Asked {
    /// The owner's reply: its result or its error name.
    Owner(Result<ReplyResult, ErrorName>),
    /// Bytes that `decode_reply` refuses.
    NoReply,
    /// No answer of the owner: the error of the control channel or of the daemon.
    Failed(ClientError),
}

fn ask(control: &dyn Control, request: &[u8]) -> Asked {
    let made = control.places().make();
    match control.relay(made, request) {
        Ok(bytes) => match decode_reply(&bytes) {
            Ok(reply) => Asked::Owner(reply.outcome),
            Err(_) => Asked::NoReply,
        },
        Err(error) => Asked::Failed(error),
    }
}

/// Whether the error says that this part of Halvo does not run: no daemon answers, or the one
/// that answers does not know the request.
fn not_running(error: &ClientError) -> bool {
    match error {
        ClientError::Unreachable { .. } | ClientError::Unsupported => true,
        ClientError::Daemon(reply) => matches!(
            reply.code(),
            ErrorCode::UnsupportedOp | ErrorCode::UnsupportedVersion
        ),
        _ => false,
    }
}

/// Any other error of the channel or of the daemon, as the commands of capture read it.
fn action_error(error: &ClientError) -> ActionError {
    let reading = read_error(error);
    reading.command_error.unwrap_or(ActionError::NotAnswering {
        service: reading.service,
    })
}

/// Reads every page of the read `workflows`, the `after` of each being the `next` of the page
/// before, until a page has no `next`. An answer without a list gives its state and no row.
pub fn read(control: &dyn Control) -> Result<WorkflowsView, ActionError> {
    let role = InterfaceRole::claim();
    let mut listed = Vec::new();
    let mut after: Option<WorkflowId> = None;
    loop {
        let request = InterfaceRead::workflows(&role, request_id(), after.clone())
            .to_bytes()
            .map_err(|_| ActionError::Internal)?;
        let page = match ask(control, &request) {
            Asked::Owner(Ok(ReplyResult::Workflows(page))) => page,
            Asked::Owner(_) | Asked::NoReply => return Err(ActionError::BrokenReply),
            Asked::Failed(error) if not_running(&error) => return Ok(WorkflowsView::NotRunning),
            Asked::Failed(ClientError::Daemon(reply))
                if reply.code() == ErrorCode::StoreUnavailable =>
            {
                return Ok(WorkflowsView::StoreUnavailable)
            }
            Asked::Failed(ClientError::Daemon(reply)) if reply.code() == ErrorCode::Busy => {
                return Ok(WorkflowsView::Deleting)
            }
            Asked::Failed(error) => return Err(action_error(&error)),
        };
        listed.extend(page.workflows.iter().map(Listed::of));
        match page.next {
            Some(next) if after.as_ref().is_none_or(|before| next > *before) => {
                after = Some(next);
            }
            Some(_) => return Err(ActionError::BrokenReply),
            None => return Ok(view(&listed)),
        }
    }
}

/// Sends one decision, which holds the workflow id and nothing else.
pub fn decide(
    control: &dyn Control,
    decision: Decision,
    workflow: &str,
) -> Result<(), DecisionError> {
    let workflow = WorkflowId::parse(workflow)
        .map_err(|_| DecisionError::Other(ActionError::InvalidArgument))?;
    let role = InterfaceRole::claim();
    let request = match decision {
        Decision::Approve => InterfaceDecision::approve(&role, request_id(), workflow),
        Decision::Dismiss => InterfaceDecision::dismiss(&role, request_id(), workflow),
        Decision::Delete => InterfaceDecision::delete(&role, request_id(), workflow),
    }
    .to_bytes()
    .map_err(|_| DecisionError::Other(ActionError::Internal))?;
    match ask(control, &request) {
        // The owner decided, or the workflow is gone.
        Asked::Owner(Ok(ReplyResult::Null) | Err(ErrorName::NoWorkflow)) => Ok(()),
        // The owner's guard holds a delete while an agent uses the workflow.
        Asked::Owner(Err(ErrorName::State)) if decision == Decision::Delete => {
            Err(DecisionError::InUse)
        }
        // An approval or a dismissal of a workflow that is no longer proposed counts as done:
        // the page reads again and shows what is stored.
        Asked::Owner(Err(ErrorName::State)) => Ok(()),
        Asked::Owner(_) | Asked::NoReply => Err(DecisionError::Other(ActionError::BrokenReply)),
        // No daemon, or one that does not know the request: nothing was decided.
        Asked::Failed(error) if not_running(&error) => Err(DecisionError::NotRunning),
        Asked::Failed(error) => Err(DecisionError::Other(action_error(&error))),
    }
}

async fn decided(
    state: &AppState,
    decision: Decision,
    workflow: String,
) -> Result<(), DecisionError> {
    let control = Arc::clone(&state.control);
    tauri::async_runtime::spawn_blocking(move || decide(control.as_ref(), decision, &workflow))
        .await
        .map_err(|_| DecisionError::Other(ActionError::Internal))?
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
pub async fn workflows(state: State<'_, AppState>) -> Result<WorkflowsView, ActionError> {
    let control = Arc::clone(&state.control);
    tauri::async_runtime::spawn_blocking(move || read(control.as_ref()))
        .await
        .map_err(|_| ActionError::Internal)?
}

/// Moves a proposed workflow to approved. It takes the workflow id and nothing else.
#[tauri::command]
pub async fn approve_workflow(
    state: State<'_, AppState>,
    workflow: String,
) -> Result<(), DecisionError> {
    decided(&state, Decision::Approve, workflow).await
}

/// Sets a proposed workflow aside: the dismissal of the store. It takes the workflow id and
/// nothing else.
#[tauri::command]
pub async fn dismiss_workflow(
    state: State<'_, AppState>,
    workflow: String,
) -> Result<(), DecisionError> {
    decided(&state, Decision::Dismiss, workflow).await
}

/// Deletes a stored workflow and everything that supported it. It takes the workflow id and
/// nothing else.
#[tauri::command]
pub async fn delete_workflow(
    state: State<'_, AppState>,
    workflow: String,
) -> Result<(), DecisionError> {
    decided(&state, Decision::Delete, workflow).await
}
