use std::collections::HashSet;

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

pub(crate) fn read<T>(
    graph: &Graph,
    token: &Token,
    collection: Collection,
    firsts: &[String],
    bound: usize,
    mut verdict: impl FnMut(&Value) -> Option<T>,
) -> Result<Chains<T>, CallError> {
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
            for item in items {
                if chains.kept.len() == bound {
                    chains.cut = true;
                    return Ok(chains);
                }
                let id = item.get("id").and_then(Value::as_str);
                if id.is_some_and(|id| earlier.contains(id)) {
                    chains.earlier_page = chains.earlier_page.saturating_add(1);
                    continue;
                }
                if let Some(kept) = verdict(item) {
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
