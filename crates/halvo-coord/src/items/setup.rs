use super::chars::{composed, no_form};
use super::compose::setup_with_line;
use super::{bound_field, value_bound, Marking, SetupParam};
use crate::ports::{Refused, Shared};
use crate::types::{
    AgentId, Body, BoundFrom, CandidateFrom, Category, Cause, Class, Item, ItemId, Limits, Name,
    Overlapping, Param, SheetAnswers, Slot, System, WorkflowId,
};

const TYPED_MAX: usize = 1024;

/// The most names of a workbook's worksheets that Which sheet offers as its list (r4 U3.22c, the
/// coordinator's ruling (C)): a bound of its own, the sheet crate's, where every other list keeps
/// `setup_candidates`; a workbook of more names keeps the typed line.
pub const SHEET_NAMES_MAX: usize = 64;

/// The body of the setup flow (I3.5): the setup's `set_bindings` with one slot per parameter of
/// the item, each with the value that the person chose or typed, in the order of the item's
/// parameters. It has no place for an origin, because the coordinator never sends one (the
/// agent-setup run's CP-36, ruling IG-6b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupBody {
    workflow: WorkflowId,
    agent: AgentId,
    slots: Vec<Slot>,
}

impl SetupBody {
    /// The slots, in the order of the item's parameters.
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The body `set_bindings` that `Owner::send` carries.
    pub fn body(&self) -> Body {
        Body::SetBindings {
            workflow: self.workflow.clone(),
            agent: self.agent.clone(),
            slots: self.slots.clone(),
        }
    }
}

/// The state of a setup item (I3.5): open for answers; its body owed, sent and not yet
/// acknowledged, while the item stays open and takes no answer (ruling IG-10); closed at the
/// owner's acknowledgement; or gone, with its workflow or because its agent is not the
/// workflow's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupState {
    /// Takes answers.
    Open,
    /// The body is owed until the owner acknowledges it.
    Owed(SetupBody),
    /// The owner acknowledged the body.
    Closed,
    /// The workflow is gone: nothing more is sent (I5.7).
    Gone,
    /// The owner refused the body with `binding`: the agent is not the workflow's, nothing more
    /// is sent, and a move of the person for it is `unknown_agent` (I3.5, ruling IG-36).
    AgentGone,
}

/// An answer of the person to a setup item (I3.1, I8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupInput<'a> {
    /// The choice `choose` of `agents_answer`: the candidate `n`, from 1, of `param`.
    Choose {
        /// The parameter.
        param: Param,
        /// The candidate's number.
        n: u32,
    },
    /// `agents_setup_typed`: the text of a parameter that has no candidate.
    Typed {
        /// The parameter.
        param: Param,
        /// The text.
        text: &'a str,
    },
    /// `agents_setup_typed` of Which spreadsheet file for an item that asks Which sheet, with
    /// the names of the worksheets that the read of its line answered (`Files::sheets`), none
    /// where they were not read (r4 U3.22c). `Typed` of that question is this with none.
    Book {
        /// The text.
        text: &'a str,
        /// The names that the read answered, in the workbook's order.
        sheets: Option<&'a [String]>,
    },
    /// The choice `none`: the hold stays and the item stays open, with its answers (I3.5,
    /// ruling RD-23 (b)).
    None,
}

/// A move of a setup item: an answer of the person, or what the owner answered to its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMove<'a> {
    /// An answer of the person.
    Input(SetupInput<'a>),
    /// The owner acknowledged the owed body.
    Acknowledged,
    /// The owner refused the owed body by a name after which another value can pass: the item
    /// asks every parameter again (I3.5, ruling IG-34).
    NotTaken,
    /// The owner refused the owed body with `no_workflow`: nothing more is sent.
    Gone,
    /// The owner refused the owed body with `binding`: nothing more is sent (ruling IG-36).
    Binding,
    /// The owner refused the owed body with `overlap` once more: the item asks every parameter
    /// again, as after `NotTaken`, and keeps the refused body in memory, never stored (I3.5 (a);
    /// ruling RD-67 3.2 (a)).
    Overlap,
    /// The kept body goes once more: the open item that keeps a refused body owes it again
    /// (I3.5 (d), (e); ruling RD-67 3.2 (d), (e)).
    Again,
}

/// Why the core did not take a typed line of a parameter (r4 U3.22a): the parameter holds no
/// value, and its entry carries the sentence of the reason until its next answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTaken {
    /// A line of Which spreadsheet file or Where the data comes from that is no full path of
    /// the system in one spelling (`typed_line`).
    Path,
    /// A line of Which cells that names no cells as the sheet crate reads them (`typed_line`).
    Cells,
    /// A line of Where the data comes from whose file lies outside the folder of the line of
    /// Which spreadsheet file that the item holds (`in_folder`; r4 U3.22b).
    Outside,
}

impl NotTaken {
    /// The words key of the sentence under the line on `system`.
    fn key(self, system: System) -> &'static str {
        match (self, system) {
            (NotTaken::Path, System::Mac) => "param_typed.not_path_mac",
            (NotTaken::Path, System::Windows) => "param_typed.not_path_windows",
            (NotTaken::Cells, _) => "param_typed.not_cells",
            (NotTaken::Outside, _) => "param_typed.not_in_folder",
        }
    }
}

/// A setup item that the coordinator holds: its item id, its workflow and agent, its
/// parameters with their candidates, the value given for each so far, the refusal by entry that
/// the reader of senders gave the newest answer for each, the reason why the core did not take
/// the newest typed line of each, whether the owner refused its answers by a name after which
/// another value can pass, whether it asks again answers that its agent has stored, the stored
/// answers that it holds as lines of the core (`holding`), and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    item: ItemId,
    workflow: WorkflowId,
    agent: AgentId,
    category: Category,
    params: Vec<SetupParam>,
    given: Vec<Option<Vec<String>>>,
    stored: SheetAnswers,
    refused: Vec<Option<Refused>>,
    untaken: Vec<Option<NotTaken>>,
    not_taken: bool,
    again: bool,
    step: u32,
    state: SetupState,
    kept: Option<SetupBody>,
    shared: Option<Shared>,
    waits: Option<AgentId>,
}

impl Setup {
    /// An open setup item that asks `params` of an agent of `category`, each with its candidates
    /// as the item shows them; `malformed` for no parameter, a parameter that the one table
    /// (`params`) does not give the category, or a parameter asked twice.
    pub fn new(
        item: ItemId,
        workflow: WorkflowId,
        agent: AgentId,
        category: Category,
        params: Vec<SetupParam>,
    ) -> Result<Setup, Cause> {
        let table = super::params(category);
        let fits = !params.is_empty()
            && params.iter().enumerate().all(|(i, p)| {
                table.contains(&p.param) && !params[..i].iter().any(|q| q.param == p.param)
            });
        if !fits {
            return Err(Cause::Malformed);
        }
        Ok(Setup {
            item,
            workflow,
            agent,
            category,
            given: vec![None; params.len()],
            stored: SheetAnswers::default(),
            refused: vec![None; params.len()],
            untaken: vec![None; params.len()],
            params,
            not_taken: false,
            again: false,
            step: 0,
            state: SetupState::Open,
            kept: None,
            shared: None,
            waits: None,
        })
    }

    /// The item as one that asks again the answers that its agent has stored, which the person's
    /// Change its answers put up with no row (r4 U3.22a): the answer `none` closes it, so that
    /// the stored answers stand.
    pub fn asked_again(self) -> Setup {
        Setup {
            again: true,
            ..self
        }
    }

    /// Whether the item asks again the answers that its agent has stored (`asked_again`).
    pub fn is_asked_again(&self) -> bool {
        self.again
    }

    /// The item holding the stored answers `answers` of its agent, which the read at the press of
    /// Change its answers gave (r4 U3.22g), as lines that the core holds: of each question that the
    /// item asks, the stored answer where it is a line that the core takes as that question's
    /// typed line on this build's system as it stands (`kept_line`), and none otherwise. A line so
    /// held is no answer given at the item: the one body still waits for a line taken at the item
    /// for every question, so that a press sends each line that shows text. The lines go at the
    /// move that makes the body, at the close, and at each move after which the item asks every
    /// parameter again (`setup_step`).
    pub fn holding(self, answers: &SheetAnswers) -> Setup {
        let kept = |param: Param| {
            answers
                .of(param)
                .filter(|line| {
                    self.at(param).is_ok() && kept_line(self.category, param, line, System::HOST)
                })
                .map(str::to_owned)
        };
        let stored = SheetAnswers {
            workbook: kept(Param::Workbook),
            sheet: kept(Param::Sheet),
            range: kept(Param::Range),
            source: kept(Param::Source),
        };
        Setup { stored, ..self }
    }

    /// The stored line of `param` that the item holds (`holding`); none for every other.
    pub fn stored(&self, param: Param) -> Option<&str> {
        self.stored.of(param)
    }

    /// The body that the owner refused with `overlap`, kept in memory and never stored, which
    /// the flow sends once more after a read with no entry or a switch-off of an agent that the
    /// sentence names; none after any other move that ends it (I3.5 (a); ruling RD-67 3.2 (a)).
    pub fn kept(&self) -> Option<&SetupBody> {
        self.kept.as_ref()
    }

    /// What the item holds to show of the newest refusal `overlap` of its body: the entries of
    /// the read `Owner::overlaps` that followed it, or that the read did not answer; none where
    /// no such refusal stands (I3.5 (c), (d); ruling RD-67 3.2). The sentence's words are U2.4d's.
    pub fn shared(&self) -> Option<&Shared> {
        self.shared.as_ref()
    }

    /// Takes the answer of the read that follows the newest refusal `overlap` in place of an
    /// earlier one, for an item that keeps a refused body; nothing otherwise.
    pub(crate) fn take_shared(&mut self, shared: Option<Shared>) {
        if self.kept.is_some() {
            self.shared = shared;
        }
    }

    /// The agent that the person switched off whose `paused` the owner has not acknowledged yet,
    /// for an open item whose kept body that switch-off freed: the body waits and goes at the
    /// first tick at which that agent's workflow is listed `paused` or has left the list (I3.5
    /// (e); ruling RD-160 (2)). Every move of the item but the answer `none` ends the wait.
    pub(crate) fn waits(&self) -> Option<&AgentId> {
        self.waits.as_ref()
    }

    /// Holds the kept body of an open item until the `paused` of `off` is acknowledged; nothing
    /// for an item that is not open or keeps no body.
    pub(crate) fn wait_for(&mut self, off: &AgentId) {
        if self.state == SetupState::Open && self.kept.is_some() {
            self.waits = Some(off.clone());
        }
    }

    /// The step mark (I3.4, ruling RD-87 1): 0 at the first ask, and one more each time the owner
    /// refuses the body by a name after which the item asks every parameter again; no other move
    /// changes it.
    pub fn step(&self) -> u32 {
        self.step
    }

    /// The item id.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// The workflow.
    pub fn workflow(&self) -> &WorkflowId {
        &self.workflow
    }

    /// The agent.
    pub fn agent(&self) -> &AgentId {
        &self.agent
    }

    /// The parameters with their candidates.
    pub fn params(&self) -> &[SetupParam] {
        &self.params
    }

    /// The state.
    pub fn state(&self) -> &SetupState {
        &self.state
    }

    /// Whether the owner refused the item's answers by a name after which another value can
    /// pass, so that the item asks every parameter again with the line `item.setup.not_taken`.
    pub fn not_taken(&self) -> bool {
        self.not_taken
    }

    /// The value that the item holds for `param`, as the person gave it; none while it holds
    /// none, and for a parameter that it does not ask.
    pub fn given(&self, param: Param) -> Option<&[String]> {
        let i = self.at(param).ok()?;
        self.given[i].as_deref()
    }

    /// Takes the refusal by entry that the reader of senders gave the answer for `param`: the
    /// parameter holds no value and carries the refusal until its next answer, and nothing else
    /// of the item changes (ruling RD-81 9.2); nothing for a parameter that the item does not
    /// ask.
    pub(crate) fn refuse(&mut self, param: Param, refused: Refused) {
        if let Ok(i) = self.at(param) {
            self.given[i] = None;
            self.refused[i] = Some(refused);
        }
    }

    /// The item as the page gets it (I3.4, I3.5), for the agent of the name `name` (I2.3):
    /// `compose_setup` of its parameters, with the line `item.setup.not_taken` while it asks
    /// again, the key `refused` of each parameter whose newest answer the reader of senders
    /// refused (ruling RD-81 9.3), and the key `not_taken` of each parameter whose newest typed
    /// line the core did not take, with the sentence of its reason on this build's system (r4
    /// U3.22a). Where the item holds what the read after the newest refusal
    /// `overlap` gave, the overlap sentence stands in the place of `item.setup.not_taken`, with
    /// the name of the other agent's row from `name_of` (I3.5 (c)).
    pub fn page_item(
        &self,
        name: &str,
        name_of: impl Fn(&AgentId) -> Option<String>,
        limits: &Limits,
    ) -> Result<Item, Cause> {
        let line = match &self.shared {
            Some(shared) => Some(Overlapping::of(shared, name_of).setup_line()?),
            None if self.not_taken => Some(crate::words::text("item.setup.not_taken", &[])?),
            None => None,
        };
        let item = setup_with_line(&self.item, &self.agent, name, &self.params, line, limits)?;
        let refused = self.params.iter().zip(&self.refused);
        let mut item = refused
            .filter_map(|(p, r)| r.map(|r| (p.param, r)))
            .fold(item.at_step(self.step), |item, (param, r)| {
                item.refused_at(param, r)
            });
        for (p, why) in self.params.iter().zip(&self.untaken) {
            if let Some(why) = why {
                let sentence = crate::words::text(why.key(System::HOST), &[])?;
                item = item.not_taken_at(p.param, sentence);
            }
        }
        for p in &self.params {
            if let Some(line) = self.stored(p.param) {
                item = item.stored_at(p.param, line);
            }
        }
        Ok(item)
    }

    /// The line of Which spreadsheet file whose worksheets' names the core reads before it takes
    /// `text` as that question's answer (r4 U3.22c): where the item is open and asks Which sheet,
    /// `param` is Which spreadsheet file, and `text` is a line that the core takes on this build's
    /// system (`typed_line`) and that differs from the line that the item holds; none otherwise.
    pub fn sheets_due(&self, param: Param, text: &str) -> Option<String> {
        if param != Param::Workbook || self.state != SetupState::Open {
            return None;
        }
        let (w, _) = (self.at(Param::Workbook).ok()?, self.at(Param::Sheet).ok()?);
        typed(text).ok()?;
        let line = typed_line(param, text, System::HOST).ok()?;
        let held = self.given[w].as_ref().and_then(|g| g.first());
        (held != Some(&line)).then_some(line)
    }

    /// Whether Which sheet offers the names of a workbook's worksheets as its list.
    fn sheet_list(&self) -> Option<usize> {
        let s = self.at(Param::Sheet).ok()?;
        let list = &self.params[s].candidates;
        (!list.is_empty() && list.iter().all(|(_, from)| *from == CandidateFrom::Files))
            .then_some(s)
    }

    /// Which sheet after the item's line of Which spreadsheet file changed (r4 U3.22c): with
    /// `names` that fit (`fit_names`), they are its list, in the workbook's order, and the sheet
    /// that the item held is dropped, chosen or typed, so that the person chooses from the new
    /// list; without them, a list of an older line goes with the sheet chosen from it, the typed
    /// line comes back, and a typed sheet stays. Nothing for an item that does not ask it.
    fn take_sheets(&mut self, names: Option<&[String]>) {
        let Ok(s) = self.at(Param::Sheet) else {
            return;
        };
        match names.filter(|names| fit_names(names)) {
            Some(names) => {
                self.params[s].candidates = names
                    .iter()
                    .map(|name| (vec![name.clone()], CandidateFrom::Files))
                    .collect();
                self.given[s] = None;
                self.refused[s] = None;
                self.untaken[s] = None;
            }
            None if self.sheet_list().is_some() => {
                self.params[s].candidates.clear();
                self.given[s] = None;
            }
            None => {}
        }
    }

    fn at(&self, param: Param) -> Result<usize, Cause> {
        self.params
            .iter()
            .position(|p| p.param == param)
            .ok_or(Cause::Malformed)
    }

    /// The data file's line against the workbook's line (r4 U3.22b): where the item holds a line of
    /// each and the data file lies outside the workbook's folder (`in_folder`), the data file's
    /// question holds no value and carries `NotTaken::Outside` until its next answer.
    fn place_source(&mut self, system: System) {
        let (Ok(w), Ok(d)) = (self.at(Param::Workbook), self.at(Param::Source)) else {
            return;
        };
        let lines = self.given[w].as_ref().zip(self.given[d].as_ref());
        let outside = lines.is_some_and(|(workbook, data)| {
            workbook
                .first()
                .zip(data.first())
                .is_none_or(|(workbook, data)| !in_folder(workbook, data, system))
        });
        if outside {
            self.given[d] = None;
            self.untaken[d] = Some(NotTaken::Outside);
        }
    }

    fn body(&self) -> Option<SetupBody> {
        let mut slots = Vec::new();
        for (p, given) in self.params.iter().zip(&self.given) {
            slots.push(Slot {
                name: Name::parse(p.param.name()).ok()?,
                value: given.clone()?,
            });
        }
        Some(SetupBody {
            workflow: self.workflow.clone(),
            agent: self.agent.clone(),
            slots,
        })
    }
}

/// The deciding field of I4.3 that the slot `param` of an agent of `category` binds, with its
/// class (ruling RD-90 items 3 and 6): a field of the class of the category's effects that
/// A-A9.3 binds from a slot and the table marks as deciding, so the payee and the instrument of a
/// payment and the seller and the instrument of a purchase; none for every other parameter.
fn deciding(category: Category, param: Param) -> Option<(Class, &'static str)> {
    let class = match category {
        Category::UtilityBillPayment => Class::Payment,
        Category::ShopOrder => Class::Purchase,
        _ => return None,
    };
    let field = param.name();
    match bound_field(class, field) {
        Some((sources, Marking::Deciding)) if sources.contains(&BoundFrom::Slot) => {
            Some((class, field))
        }
        _ => None,
    }
}

/// Whether `text`, given for `param` of an agent of `category`, holds to the predicate of a
/// deciding field (ruling RD-90 items 2 and 6): it equals its own composed form at the bound of
/// its field (`chars::composed` at `value_bound`), as a raised value of that field must, so that
/// a person's own value never trips the refusal at the raise later. Every text holds for a
/// parameter that binds no deciding field.
pub(super) fn fits(category: Category, param: Param, text: &str) -> bool {
    match deciding(category, param) {
        Some((class, field)) => {
            value_bound(class, field, BoundFrom::Slot).is_ok_and(|most| composed(text, most))
        }
        None => true,
    }
}

/// A typed answer: one line of at most 1,024 bytes that holds something other than white space
/// (I3.5); else `malformed`.
fn typed(text: &str) -> Result<(), Cause> {
    let one_line = !text.contains(['\n', '\r']);
    if one_line && text.len() <= TYPED_MAX && !text.trim().is_empty() {
        Ok(())
    } else {
        Err(Cause::Malformed)
    }
}

/// Whether `line`, a stored answer of `param` of an agent of `category`, is a line that the core
/// takes as that question's typed line on `system` as it stands (r4 U3.22g): a typed answer
/// (`typed`) that holds to the predicate of a deciding field (`fits`), with no character that has
/// no visible form (`chars::no_form`), and whose line by `typed_line` is the very text, so that
/// what the card shows is what a press sends, byte for byte.
fn kept_line(category: Category, param: Param, line: &str, system: System) -> bool {
    typed(line).is_ok()
        && fits(category, param, line)
        && !line.chars().any(no_form)
        && typed_line(param, line, system).is_ok_and(|taken| taken == line)
}

/// Whether the names of a workbook's worksheets fit Which sheet's list (r4 U3.22c, the
/// coordinator's ruling (B), (C)): one to `SHEET_NAMES_MAX` names, each a line that a typed line
/// could be (`typed`: one line of at most 1,024 bytes that holds something other than white
/// space) with no character that has no visible form (`chars::no_form`: control and format
/// characters, the direction marks among them). One name that does not fit, and there is no list:
/// never a list that lacks a sheet.
pub(crate) fn fit_names(names: &[String]) -> bool {
    (1..=SHEET_NAMES_MAX).contains(&names.len())
        && names
            .iter()
            .all(|name| typed(name).is_ok() && !name.chars().any(no_form))
}

/// The line that the core takes of a typed answer `text` of `param` on `system`, or why it does
/// not take it (r4 U3.22a). For Which spreadsheet file and Where the data comes from: the line
/// without the spaces around it and then without ONE pair of quotation marks around the whole of
/// it, where what is left is a full path of the system in one spelling (`full_path`), else
/// `NotTaken::Path`. For Which cells: the line as it is typed, where it names cells as the sheet
/// crate reads them (`cells`), else `NotTaken::Cells`. Every other parameter's line as it is
/// typed.
pub(crate) fn typed_line(param: Param, text: &str, system: System) -> Result<String, NotTaken> {
    match param {
        Param::Workbook | Param::Source => {
            let line = text.trim();
            let line = line
                .strip_prefix('"')
                .and_then(|l| l.strip_suffix('"'))
                .unwrap_or(line);
            if full_path(line, system) {
                Ok(line.to_owned())
            } else {
                Err(NotTaken::Path)
            }
        }
        Param::Range if cells(text) => Ok(text.to_owned()),
        Param::Range => Err(NotTaken::Cells),
        _ => Ok(text.to_owned()),
    }
}

/// Whether `text` names cells as the sheet crate's `Range::parse` takes them (ONE table that a
/// test of each crate reads, `tests/typed/cells.txt`): `B2:D6`, or one cell `B2`; a column of 1
/// to 3 letters of either case up to `XFD`; a row from 1 to 1,048,576 with no leading zero; the
/// first cell's row and column at most the last cell's.
fn cells(text: &str) -> bool {
    let (first, last) = text.split_once(':').unwrap_or((text, text));
    match (cell(first), cell(last)) {
        (Some(first), Some(last)) => first.0 <= last.0 && first.1 <= last.1,
        _ => false,
    }
}

/// The row and the column of one cell of the A1 form, each from 1.
fn cell(text: &str) -> Option<(u32, u32)> {
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (column, row) = text.split_at(letters);
    let digits = !row.is_empty() && row.bytes().all(|b| b.is_ascii_digit());
    if !(1..=3).contains(&letters) || !digits || row.starts_with('0') {
        return None;
    }
    let column = column.bytes().fold(0u32, |acc, b| {
        acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    let row: u32 = row.parse().ok()?;
    (column <= 16_384 && row <= 1_048_576).then_some((row, column))
}

/// Whether `path` is a full path of `system` in one spelling, by the agent store's own rule for
/// the slot `workbook` (halvo-agent-store `workbook_path` in the native form; ONE table that a
/// test of each crate reads, `tests/typed/paths.txt`): on a Mac it starts with `/`, whose
/// separator is `/`; on Windows it starts with a drive letter, `:` and a separator, and its
/// separators are `\` and `/`; and no step after the root is empty, `.` or `..`.
fn full_path(path: &str, system: System) -> bool {
    let (steps, separators): (Option<&str>, &[char]) = match system {
        System::Mac => (path.strip_prefix('/'), &['/']),
        System::Windows => (drive(path), &['/', '\\']),
    };
    steps.is_some_and(|steps| {
        steps
            .split(separators)
            .all(|step| !step.is_empty() && step != "." && step != "..")
    })
}

/// Whether the data file `data` lies in the folder of the workbook `workbook`, both full paths of
/// `system` (`full_path`), by the sheet crate's own rule for the place of a data file (r4 U3.22b;
/// halvo-sheet `Folder::read_source` over the workbook's folder as the runner gives it, which
/// takes a path below that folder and refuses every other as `data_outside`; ONE table that a
/// test of each crate reads, `tests/typed/folders.txt`): the steps of `data` start with the steps
/// of the workbook's folder, every step of `workbook` but its last, and hold at least one step
/// more, so that a file in a folder below the workbook's lies in it too. On Windows the drive's
/// letter is read in either case and `\` and `/` are one separator; every step is compared as it
/// is spelled, so a folder named by another spelling (a link, another letter case) is outside.
pub(crate) fn in_folder(workbook: &str, data: &str, system: System) -> bool {
    match (steps(workbook, system), steps(data, system)) {
        (Some((root, book)), Some((data_root, data))) => {
            let folder = &book[..book.len().saturating_sub(1)];
            root == data_root && data.len() > folder.len() && data.starts_with(folder)
        }
        _ => false,
    }
}

/// The root of a full path of `system`, the drive's letter in upper case on Windows and none on
/// a Mac, and its steps after the root; none for a path that does not start at a root.
fn steps(path: &str, system: System) -> Option<(Option<u8>, Vec<&str>)> {
    match system {
        System::Mac => Some((None, path.strip_prefix('/')?.split('/').collect())),
        System::Windows => {
            let rest = drive(path)?;
            let letter = path.as_bytes()[0].to_ascii_uppercase();
            Some((Some(letter), rest.split(['/', '\\']).collect()))
        }
    }
}

/// What follows the drive of a path of the Windows form: a letter, `:` and a separator.
fn drive(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    let rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    rooted.then(|| &path[3..])
}

/// The folder in which the chooser of `param` opens for the set-up item `setup` (I8.1
/// `agents_choose_file`; r4 U3.22b), or why the core refuses the call, in the order in which
/// `setup_step` refuses a typed line of `param` there: `unknown_agent` for an item whose agent
/// the owner refused with `binding`, `stale` for an item that is closed or whose workflow is
/// gone, `state` while its body is owed, and `malformed` for a parameter that it does not ask, that
/// has candidates, or that is neither Which spreadsheet file nor Where the data comes from. The
/// chooser of Where the data comes from opens in the folder of the line of Which spreadsheet file
/// that the item holds (`folder_of`); the other, and that one while the item holds no such line,
/// where the system opens it.
pub fn choose_in(setup: &Setup, param: Param, system: System) -> Result<Option<String>, Cause> {
    match setup.state {
        SetupState::AgentGone => return Err(Cause::UnknownAgent),
        SetupState::Closed | SetupState::Gone => return Err(Cause::Stale),
        SetupState::Owed(_) => return Err(Cause::State),
        SetupState::Open => {}
    }
    let i = setup.at(param)?;
    if !setup.params[i].candidates.is_empty() || !matches!(param, Param::Workbook | Param::Source) {
        return Err(Cause::Malformed);
    }
    Ok(match param {
        Param::Source => setup
            .given(Param::Workbook)
            .and_then(|line| line.first())
            .map(|workbook| folder_of(workbook, system).to_owned()),
        _ => None,
    })
}

/// The folder of `path`, a full path of `system` (`full_path`): every step but its last, with the
/// separator of the root where the folder is the root.
fn folder_of(path: &str, system: System) -> &str {
    let (root, separators): (usize, &[char]) = match system {
        System::Mac => (1, &['/']),
        System::Windows => (3, &['/', '\\']),
    };
    match path.rfind(separators) {
        Some(at) if at >= root => &path[..at],
        _ => &path[..root.min(path.len())],
    }
}

/// The decision of the setup flow (I3.5, I5.10, ruling IG-6c): the setup after `mv`, or the
/// cause of its refusal, first that fits. An item whose agent the owner refused with `binding`
/// refuses an answer of the person with `unknown_agent` (ruling IG-36) and every other move with
/// `stale`; a closed item and one whose workflow is gone refuse every move with `stale`. While the
/// body is owed, an answer is refused with `state` (ruling IG-10), the acknowledgement closes the
/// item, `not_taken` makes it open again with every answer cleared, its step mark one more, and
/// the line `item.setup.not_taken` (ruling IG-34), and `gone` ends it. An open item refuses the
/// acknowledgement and `not_taken` with `state`, keeps everything at `none` (I3.5, ruling RD-23
/// (b)) but for an item asked again, which `none` closes (`Setup::asked_again`, r4 U3.22a), and
/// takes `choose` of a candidate that it shows, or a typed line for a parameter with no
/// candidate that holds, for a parameter that binds a deciding field, to that field's predicate
/// (`fits`; ruling RD-90 item 6) (`malformed` otherwise); a later answer for a parameter replaces
/// the earlier one. Such a typed line goes in the form that `typed_line` gives on this build's
/// system; a line that `typed_line` does not take is not taken: the parameter holds no value and
/// carries the reason until its next answer, so that no body is made, and the move is no refusal
/// (r4 U3.22a).
/// While a parameter has no answer the item stays open; the answer that gives the last one makes
/// the one body `set_bindings`, with the slots of the given values, which is owed from then on.
/// `overlap` (I3.5 (a); ruling RD-67 3.2) makes an owed item open as `not_taken` does and keeps
/// its body in memory, never stored, and `again` makes an open item that keeps a body owe it once
/// more, `state` for an item that keeps none; the answer that makes a new body, `not_taken` and
/// the close end the kept body and what the item holds of its read, and an answer of the person
/// stops the read that a tick asks again; every move but the answer `none` ends the wait of a
/// freed body (`Setup::waits`). An answer that it takes for a parameter ends the refusal by
/// entry that the parameter carried, and a move after which the item asks every parameter again
/// ends every such refusal (ruling RD-81 9.2).
/// Which sheet of an item that asks it (r4 U3.22c): where a typed line of Which spreadsheet file
/// changes the line that the item holds, `Book` with names that fit (`fit_names`) makes them Which
/// sheet's list, in the workbook's order, and drops the sheet that the item held, chosen or typed;
/// without such names (`Typed`, or `Book` with none) a list of an older line goes with the sheet
/// chosen from it and the typed line comes back, while a typed sheet stays. While the list stands,
/// a pick of it reaches the core by the sheet's NAME, never by its place (the coordinator's ruling
/// of its log line 19505): `choose` of Which sheet is refused with `malformed`, and a typed line of
/// Which sheet is taken where it equals a name of the PRESENT list and leaves the question with no
/// answer otherwise, with no refusal, so that no set-up ends. A move after which the item asks
/// every parameter again drops the list.
/// The stored lines that the item holds (`Setup::holding`, r4 U3.22g) go at every move after which
/// the item is no longer open, the one that makes its body among them, and at every move after
/// which it asks every parameter again, so that it then starts with empty lines.
pub fn setup_step(setup: &Setup, mv: SetupMove<'_>) -> Result<Setup, Cause> {
    let mut next = step_of(setup, mv)?;
    if next.state != SetupState::Open || next.step != setup.step {
        next.stored = SheetAnswers::default();
    }
    Ok(next)
}

/// `setup_step` but for the stored lines.
fn step_of(setup: &Setup, mv: SetupMove<'_>) -> Result<Setup, Cause> {
    let mut next = setup.clone();
    if !matches!(mv, SetupMove::Input(SetupInput::None)) {
        next.waits = None;
    }
    match (&setup.state, mv) {
        (SetupState::AgentGone, SetupMove::Input(_)) => return Err(Cause::UnknownAgent),
        (SetupState::Closed | SetupState::Gone | SetupState::AgentGone, _) => {
            return Err(Cause::Stale)
        }
        (SetupState::Owed(_), SetupMove::Acknowledged) => next.state = SetupState::Closed,
        (SetupState::Owed(_), SetupMove::NotTaken) => {
            next.take_sheets(None);
            next.given = vec![None; setup.params.len()];
            next.refused = vec![None; setup.params.len()];
            next.untaken = vec![None; setup.params.len()];
            next.not_taken = true;
            next.step = setup.step.saturating_add(1);
            next.state = SetupState::Open;
            next.kept = None;
            next.shared = None;
            return Ok(next);
        }
        (SetupState::Owed(body), SetupMove::Overlap) => {
            next.take_sheets(None);
            next.given = vec![None; setup.params.len()];
            next.refused = vec![None; setup.params.len()];
            next.untaken = vec![None; setup.params.len()];
            next.not_taken = true;
            next.step = setup.step.saturating_add(1);
            next.state = SetupState::Open;
            next.kept = Some(body.clone());
            next.shared = None;
            return Ok(next);
        }
        (SetupState::Open, SetupMove::Again) => {
            let Some(kept) = setup.kept.clone() else {
                return Err(Cause::State);
            };
            next.state = SetupState::Owed(kept);
            return Ok(next);
        }
        (SetupState::Open, SetupMove::Overlap) => return Err(Cause::State),
        (_, SetupMove::Gone) => next.state = SetupState::Gone,
        (_, SetupMove::Binding) => next.state = SetupState::AgentGone,
        (SetupState::Owed(_), _)
        | (SetupState::Open, SetupMove::Acknowledged | SetupMove::NotTaken) => {
            return Err(Cause::State)
        }
        (SetupState::Open, SetupMove::Input(SetupInput::None)) if setup.again => {
            next.state = SetupState::Closed;
            return Ok(next);
        }
        (SetupState::Open, SetupMove::Input(SetupInput::None)) => {}
        (SetupState::Open, SetupMove::Input(SetupInput::Choose { param, .. }))
            if param == Param::Sheet && setup.sheet_list().is_some() =>
        {
            return Err(Cause::Malformed);
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Choose { param, n })) => {
            let i = setup.at(param)?;
            let candidates = &setup.params[i].candidates;
            let k = usize::try_from(n).map_err(|_| Cause::Malformed)?;
            let (value, _) = k
                .checked_sub(1)
                .and_then(|k| candidates.get(k))
                .ok_or(Cause::Malformed)?;
            next.given[i] = Some(value.clone());
            next.refused[i] = None;
            next.untaken[i] = None;
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Typed { param, text }))
            if param == Param::Sheet && setup.sheet_list().is_some() =>
        {
            let i = setup.at(param)?;
            typed(text)?;
            let named = setup.params[i]
                .candidates
                .iter()
                .find(|(value, _)| value.len() == 1 && value[0] == text);
            next.given[i] = named.map(|(value, _)| value.clone());
            next.refused[i] = None;
            next.untaken[i] = None;
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Book { text, sheets })) => {
            next = taken_line(setup, next, Param::Workbook, text)?;
            let held = next.given(Param::Workbook).is_some();
            if next.given(Param::Workbook) != setup.given(Param::Workbook) {
                next.take_sheets(sheets.filter(|_| held));
            }
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Typed { param, text })) => {
            next = taken_line(setup, next, param, text)?;
            if param == Param::Workbook
                && next.given(Param::Workbook) != setup.given(Param::Workbook)
            {
                next.take_sheets(None);
            }
        }
    }
    if matches!(
        mv,
        SetupMove::Input(
            SetupInput::Choose { .. } | SetupInput::Typed { .. } | SetupInput::Book { .. }
        )
    ) {
        if let Some(Shared::Unread { again }) = next.shared.as_mut() {
            *again = false;
        }
    }
    if next.state == SetupState::Open {
        if let Some(body) = next.body() {
            next.state = SetupState::Owed(body);
            next.kept = None;
            next.shared = None;
        }
    }
    Ok(next)
}

/// `next` with the typed line `text` of `param` taken as `setup_step` takes a typed line of a
/// parameter with no candidate, or the refusal of that move.
fn taken_line(setup: &Setup, mut next: Setup, param: Param, text: &str) -> Result<Setup, Cause> {
    let i = setup.at(param)?;
    if !setup.params[i].candidates.is_empty() {
        return Err(Cause::Malformed);
    }
    typed(text)?;
    if !fits(setup.category, param, text) {
        return Err(Cause::Malformed);
    }
    match typed_line(param, text, System::HOST) {
        Ok(line) => {
            next.given[i] = Some(vec![line]);
            next.untaken[i] = None;
        }
        Err(why) => {
            next.given[i] = None;
            next.untaken[i] = Some(why);
        }
    }
    next.refused[i] = None;
    if matches!(param, Param::Workbook | Param::Source) {
        next.place_source(System::HOST);
    }
    Ok(next)
}
