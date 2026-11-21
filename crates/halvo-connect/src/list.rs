use std::collections::HashSet;
use std::convert::Infallible;

use serde_json::Value;

use crate::answer::{self, FORM};
use crate::error::{AnswerFault, CallError};
use crate::graph::{Collection, Graph};
use crate::token::Token;

pub(crate) const PAGE_SIZE: usize = 25;
pub(crate) const PAGES_MAX: usize = 10;

pub(crate) struct Chains<T> {
    pub(crate) kept: Vec<T>,
    pub(crate) listed: Vec<String>,
    pub(crate) cut: bool,
    pub(crate) more_pages: bool,
    pub(crate) earlier_page: u32,
}

/// Why `read_left` stopped: a request of the call failed, or the caller's answer for a page did.
pub(crate) enum Stopped<E> {
    Call(CallError),
    Page(E),
}

impl<E> From<CallError> for Stopped<E> {
    fn from(error: CallError) -> Stopped<E> {
        Stopped::Call(error)
    }
}

pub(crate) fn read<T>(
    graph: &Graph,
    token: &Token,
    collection: Collection,
    firsts: &[String],
    bound: usize,
    mut verdict: impl FnMut(&Value) -> Option<T>,
) -> Result<Chains<T>, CallError> {
    let none = |_: &[String]| Ok::<_, Infallible>(HashSet::new());
    read_left(graph, token, collection, firsts, bound, none, |item, _| {
        verdict(item)
    })
    .map_err(|stopped| match stopped {
        Stopped::Call(error) => error,
        Stopped::Page(never) => match never {},
    })
}

/// `read`, with `left` asked once for each page that holds an id that no earlier page of the call gave, with those
/// ids in the page's order, before any item of the page is weighed. An item whose id it answers takes no place of
/// the bound and is weighed with `true`, so that the caller counts it in none of its counts; every other item is
/// weighed with `false`. An error of `left` stops the call before any further request.
pub(crate) fn read_left<T, E>(
    graph: &Graph,
    token: &Token,
    collection: Collection,
    firsts: &[String],
    bound: usize,
    mut left: impl FnMut(&[String]) -> Result<HashSet<String>, E>,
    mut verdict: impl FnMut(&Value, bool) -> Option<T>,
) -> Result<Chains<T>, Stopped<E>> {
    let mut chains = Chains {
        kept: Vec::new(),
        listed: Vec::new(),
        cut: false,
        more_pages: false,
        earlier_page: 0,
    };
    let mut earlier: HashSet<String> = HashSet::new();
    for (chain, first) in firsts.iter().enumerate() {
        let mut url = graph.url(first)?;
        for number in 1..=PAGES_MAX {
            let answer = graph.page(token, &url)?;
            let (items, link) = page(&answer)?;
            let link = link.map(|link| graph.next(link, collection)).transpose()?;
            let here = ids(items)?;
            chains.listed.extend(here.iter().cloned());
            let new: Vec<String> = here
                .iter()
                .filter(|id| !earlier.contains(*id))
                .cloned()
                .collect();
            let alone = if new.is_empty() {
                HashSet::new()
            } else {
                left(&new).map_err(Stopped::Page)?
            };
            for item in items {
                let id = item.get("id").and_then(Value::as_str);
                if id.is_some_and(|id| alone.contains(id)) {
                    verdict(item, true);
                    continue;
                }
                if chains.kept.len() == bound {
                    chains.cut = true;
                    return Ok(chains);
                }
                if id.is_some_and(|id| earlier.contains(id)) {
                    chains.earlier_page = chains.earlier_page.saturating_add(1);
                    continue;
                }
                if let Some(kept) = verdict(item, false) {
                    chains.kept.push(kept);
                }
            }
            earlier.extend(here);
            let Some(next) = link else { break };
            if chains.kept.len() == bound {
                chains.cut = true;
                return Ok(chains);
            }
            if number == PAGES_MAX {
                chains.more_pages = true;
                break;
            }
            url = next;
        }
        if chains.kept.len() == bound && chain + 1 < firsts.len() {
            chains.cut = true;
            return Ok(chains);
        }
    }
    Ok(chains)
}

fn page(answer: &Value) -> Result<(&[Value], Option<&str>), CallError> {
    let object = answer.as_object().ok_or(FORM)?;
    let items = object.get("value").and_then(Value::as_array).ok_or(FORM)?;
    if items.len() > PAGE_SIZE {
        return Err(CallError::Answer(AnswerFault::Page));
    }
    let link = match object.get("@odata.nextLink") {
        None => None,
        Some(link) => Some(link.as_str().ok_or(FORM)?),
    };
    Ok((items, link))
}

fn ids(items: &[Value]) -> Result<Vec<String>, CallError> {
    let mut here = answer::Ids::default();
    let mut ids = Vec::new();
    for item in items {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            let id = answer::checked(id)?;
            here.first(&id)?;
            ids.push(id);
        }
    }
    Ok(ids)
}
