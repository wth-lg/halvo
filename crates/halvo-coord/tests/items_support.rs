#![allow(dead_code)]

use halvo_coord::types::{
    AgentId, ApprovalId, AskedEffect, BoundField, BoundFrom, Class, ConfirmAsk, Decision,
    DecisionEntry, Digest, EffectAnswer, EffectId, ItemId, Name, RunId, WorkflowId,
};

pub fn effect_id(n: u32) -> EffectId {
    EffectId::parse(&format!("{n:032x}")).unwrap()
}

pub fn digest(n: u32) -> Digest {
    Digest::parse(&format!("{n:064x}")).unwrap()
}

pub fn approval(n: u32) -> ApprovalId {
    ApprovalId::parse(&format!("{n:032x}")).unwrap()
}

pub fn run_id(n: u32) -> RunId {
    RunId::parse(&format!("{:032x}", 0x7200 + n)).unwrap()
}

pub fn agent_id(n: u32) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0x6100 + n)).unwrap()
}

pub fn workflow_id(n: u32) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0x7700 + n)).unwrap()
}

pub fn item_id(n: u32) -> ItemId {
    ItemId::parse(&format!("{:032x}", 0x6900 + n)).unwrap()
}

pub fn name(text: &str) -> Name {
    Name::parse(text).unwrap()
}

pub fn field(name_: &str, from: BoundFrom, value: &str) -> BoundField {
    BoundField {
        name: name(name_),
        from,
        value: vec![value.to_owned()],
    }
}

/// The bound fields of a class in the order of A-A9.3, with plain values.
pub fn fields_of(class: Class) -> Vec<BoundField> {
    let names: &[(&str, BoundFrom)] = match class {
        Class::Payment => &[
            ("payee", BoundFrom::Slot),
            ("amount", BoundFrom::Extract),
            ("instrument", BoundFrom::Slot),
            ("origin", BoundFrom::Const),
        ],
        Class::Purchase => &[
            ("seller", BoundFrom::Slot),
            ("items", BoundFrom::Extract),
            ("amount", BoundFrom::Extract),
            ("instrument", BoundFrom::Slot),
            ("origin", BoundFrom::Const),
        ],
        Class::Send => &[
            ("to", BoundFrom::Extract),
            ("subject", BoundFrom::Extract),
            ("body", BoundFrom::Model),
        ],
        Class::Delete => &[("items", BoundFrom::Extract)],
        Class::Submit => &[("origin", BoundFrom::Const), ("fields", BoundFrom::Slot)],
        Class::Sign => &[("document", BoundFrom::Slot), ("party", BoundFrom::Slot)],
        _ => &[],
    };
    names
        .iter()
        .map(|(n, f)| field(n, *f, &format!("value of {n}")))
        .collect()
}

/// An effect numbered `n` with the digest numbered `d`.
pub fn asked(n: u32, class: Class, d: u32, digest_ok: bool) -> AskedEffect {
    AskedEffect {
        effect: effect_id(n),
        class,
        digest: digest(d),
        digest_ok,
        fields: fields_of(class),
        new_origin: None,
    }
}

pub fn ask_of(run: u32, agent: u32, effects: Vec<AskedEffect>) -> ConfirmAsk {
    ConfirmAsk::new(run_id(run), agent_id(agent), effects).unwrap()
}

/// A request of run 1 of agent 1 whose effect `i` (from 1) has the class `classes[i - 1]`,
/// the effect id `i` and the digest `i`.
pub fn ask(classes: &[Class]) -> ConfirmAsk {
    ask_of(
        1,
        1,
        classes
            .iter()
            .zip(1..)
            .map(|(c, i)| asked(i, *c, i, true))
            .collect(),
    )
}

/// A decision with one entry per effect of `ask`, the approval ids counted from `first`.
pub fn decision_for(ask: &ConfirmAsk, answer: EffectAnswer, first: u32) -> Decision {
    Decision::new(
        ask.effects()
            .iter()
            .zip(first..)
            .map(|(e, n)| DecisionEntry {
                approval: approval(n),
                effect: e.effect.clone(),
                digest: e.digest.clone(),
                answer,
            })
            .collect(),
    )
    .unwrap()
}
