//! The count of a person's hosted calls on the computer: the account of the spend rule (contract
//! T7.2) with its open holds and the end of its cap period, as a value that a caller writes into
//! one row of its store and reads back. Every function here is pure and gives back a new count; a
//! function that needs the time takes the caller's clock as an argument. What is spent and what is
//! held change by `spend::step` alone, and a refusal of `step` refuses the move with its cause and
//! leaves the count as it was. The one other change of a sum is `Count::turn`, which sets what is
//! spent to 0 at the end of a cap period, because `step` has no move for it.

use crate::admit::Room;
use crate::cap::{CapError, DefaultCap};
use crate::prices::{Micro, Prices};
use crate::release;
use crate::spend::{step, Account, Hold, Move, SpendCause, State};
use crate::tier::Tier;

const HEAD: &str = "halvo_count";
const FORM: &str = "1";

/// The id of one hold: the 16 bytes of the request id that the caller minted for the request.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct HoldId(pub [u8; 16]);

/// What the host's answer reported for a request: the tokens in and the tokens out, or `Unsent`
/// for a request of which nothing was sent, which costs 0.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Usage {
    Unsent,
    Reported { tokens_in: u64, tokens_out: u64 },
}

/// Why `Count::read` refuses a text: `text`, it is not a row; `form`, it is a row of another form;
/// `held`, its held figure is not the sum of its open holds; `twice`, one id stands on two holds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RowError {
    Text,
    Form,
    Held,
    Twice,
}

impl RowError {
    pub const fn as_str(self) -> &'static str {
        match self {
            RowError::Text => "text",
            RowError::Form => "form",
            RowError::Held => "held",
            RowError::Twice => "twice",
        }
    }
}

/// The count: the cap, the default of `release::cap()` until the person sets one; whether the
/// account is active; what is spent and what is held, in micro-units; the end of the cap period in
/// milliseconds, as the caller gave it; and the open holds. Its fields are private, so a figure
/// changes by the functions of this module alone.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Count {
    cap: Cap,
    active: bool,
    spent: Micro,
    held: Micro,
    period_end_ms: u64,
    holds: Vec<Open>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Cap {
    Default,
    Person(u64),
}

/// One open hold. Every hold of a count is reserved and dispatched in one move, so an open hold
/// is `sent`, with the cost 0, and the count keeps its amount, its tier and the end of its life.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Open {
    id: HoldId,
    tier: Tier,
    amount: Micro,
    ends_ms: u64,
}

impl Open {
    const fn hold(&self) -> Hold {
        Hold {
            state: State::Sent,
            amount: self.amount,
            cost: 0,
        }
    }
}

impl Count {
    /// A count under the default cap, active, with nothing spent or held and no hold, whose cap
    /// period ends at `period_end_ms`.
    pub fn new(period_end_ms: u64) -> Count {
        Count {
            cap: Cap::Default,
            active: true,
            spent: 0,
            held: 0,
            period_end_ms,
            holds: Vec::new(),
        }
    }

    /// The count with the person's cap of `cap_minor` minor units in the place of the cap it had.
    /// A cap outside the bounds of a cap line (contract T10.4, 1 to 100,000,000) is `amount`; the
    /// bounds are those of `DefaultCap::parse`, which reads them.
    pub fn set_cap(&self, cap_minor: u64) -> Result<Count, CapError> {
        Ok(Count {
            cap: person_cap(cap_minor)?,
            ..self.clone()
        })
    }

    /// The account of the spend rule: the cap in minor units, none when the cap is the default and
    /// `release::cap()` gives none; whether it is active; what is spent; what is held.
    pub fn account(&self) -> Account {
        Account {
            cap_minor: match self.cap {
                Cap::Default => release::cap().map(|cap| cap.amount),
                Cap::Person(cap_minor) => Some(cap_minor),
            },
            active: self.active,
            spent: self.spent,
            held: self.held,
        }
    }

    /// The four values of the cap answer, with the open holds counted in `held`; none when the
    /// count has no cap.
    pub fn room(&self) -> Option<Room> {
        Room::of(&self.account(), self.period_end_ms)
    }

    /// A hold of `Prices::hold` for the tier, the body's bytes and the output limit, whose life
    /// ends at `ends_ms`: `step`'s `Reserve` and then its `Dispatch`. A reserve with the id of an
    /// open hold is `step`'s `repeat` or `other_arguments`.
    pub fn reserve(
        &self,
        id: HoldId,
        prices: &Prices,
        tier: Tier,
        body_bytes: u64,
        max_output_tokens: u64,
        ends_ms: u64,
    ) -> Result<Count, SpendCause> {
        let amount = prices.hold(tier, body_bytes, max_output_tokens);
        let before = self.open(id).map_or_else(Hold::new, Open::hold);
        let (account, held) = step(&self.account(), &before, Move::Reserve { amount })?;
        let (account, sent) = step(&account, &held, Move::Dispatch)?;
        let mut next = self.with(&account);
        next.holds.push(Open {
            id,
            tier,
            amount: sent.amount,
            ends_ms,
        });
        Ok(next)
    }

    /// The settle of the open hold `id` by `step`, at `Prices::cost` of the hold's tier for the
    /// tokens reported, or at 0 for a request of which nothing was sent. An id that is not open is
    /// `step`'s `no_hold`.
    pub fn settle(&self, id: HoldId, prices: &Prices, usage: Usage) -> Result<Count, SpendCause> {
        let open = self.open(id);
        let cost = match (open, usage) {
            (
                Some(open),
                Usage::Reported {
                    tokens_in,
                    tokens_out,
                },
            ) => prices.cost(open.tier, tokens_in, tokens_out),
            _ => 0,
        };
        let before = open.map_or_else(Hold::new, Open::hold);
        let (account, _) = step(&self.account(), &before, Move::Settle { cost })?;
        let mut next = self.with(&account);
        next.holds.retain(|each| each.id != id);
        Ok(next)
    }

    /// The expiry by `step`, at its full amount, of every open hold whose life ends at or before
    /// `now_ms`, the caller's clock. When `step` refuses one, the expiry is refused and no hold
    /// expires.
    pub fn expire(&self, now_ms: u64) -> Result<Count, SpendCause> {
        let mut account = self.account();
        for open in self.holds.iter().filter(|each| each.ends_ms <= now_ms) {
            (account, _) = step(&account, &open.hold(), Move::Expire)?;
        }
        let mut next = self.with(&account);
        next.holds.retain(|each| each.ends_ms > now_ms);
        Ok(next)
    }

    /// The turn of the cap period at the new end `period_end_ms` that the caller gives. When it is
    /// later than the count's end, what is spent becomes 0 and the end becomes the new one; the cap
    /// and every open hold stay, and so does what is held. Otherwise the count stays as it is. This
    /// is the one change of a sum that is not a move of `step`, which has no move for it.
    pub fn turn(&self, period_end_ms: u64) -> Count {
        if period_end_ms <= self.period_end_ms {
            return self.clone();
        }
        Count {
            spent: 0,
            period_end_ms,
            ..self.clone()
        }
    }

    /// The row of the count, LF after each line: `halvo_count form=1 cap=<default or minor units>
    /// active=<yes or no> spent=<n> held=<n> period_end_ms=<n>`, then one line per open hold,
    /// `hold id=<32 lower-case hexadecimal digits> tier=<tier> amount=<n> ends_ms=<n>`.
    pub fn row(&self) -> String {
        let cap = match self.cap {
            Cap::Default => "default".to_owned(),
            Cap::Person(cap_minor) => cap_minor.to_string(),
        };
        let active = if self.active { "yes" } else { "no" };
        let mut row = format!(
            "{HEAD} form={FORM} cap={cap} active={active} spent={} held={} period_end_ms={}\n",
            self.spent, self.held, self.period_end_ms
        );
        for open in &self.holds {
            let id: String = open.id.0.iter().map(|byte| format!("{byte:02x}")).collect();
            row.push_str(&format!(
                "hold id={id} tier={} amount={} ends_ms={}\n",
                open.tier.as_str(),
                open.amount,
                open.ends_ms
            ));
        }
        row
    }

    /// Reads a row that `row` wrote. A text whose first line names another form of the row is
    /// `form`; any other text that is not in the form that `row` writes is `text`, an empty one
    /// among them; a held figure that is not the sum of the open holds is `held`; an id on two
    /// holds is `twice`.
    pub fn read(text: &str) -> Result<Count, RowError> {
        let first = text.split('\n').next().unwrap_or_default();
        let mut words = first.split(' ');
        if words.next() != Some(HEAD) {
            return Err(RowError::Text);
        }
        match words.next().and_then(|word| word.strip_prefix("form=")) {
            Some(FORM) => {}
            Some(form) if decimal(form).is_some() => return Err(RowError::Form),
            _ => return Err(RowError::Text),
        }
        let mut lines = text.strip_suffix('\n').ok_or(RowError::Text)?.split('\n');
        lines.next();
        let count = head(&mut words).ok_or(RowError::Text)?;
        let holds = lines
            .map(hold_line)
            .collect::<Option<Vec<Open>>>()
            .ok_or(RowError::Text)?;
        if holds
            .iter()
            .enumerate()
            .any(|(at, open)| holds[..at].iter().any(|each| each.id == open.id))
        {
            return Err(RowError::Twice);
        }
        let open: u128 = holds.iter().map(|open| u128::from(open.amount)).sum();
        if open != u128::from(count.held) {
            return Err(RowError::Held);
        }
        Ok(Count { holds, ..count })
    }

    fn open(&self, id: HoldId) -> Option<&Open> {
        self.holds.iter().find(|each| each.id == id)
    }

    fn with(&self, account: &Account) -> Count {
        Count {
            spent: account.spent,
            held: account.held,
            ..self.clone()
        }
    }
}

fn person_cap(cap_minor: u64) -> Result<Cap, CapError> {
    DefaultCap::parse(&format!(
        "default_cap amount={cap_minor} period=month status=provisional"
    ))
    .map(|_| Cap::Person(cap_minor))
}

fn head<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Count> {
    let cap = match field(words, "cap")? {
        "default" => Cap::Default,
        cap_minor => person_cap(decimal(cap_minor)?).ok()?,
    };
    let active = match field(words, "active")? {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    let spent = decimal(field(words, "spent")?)?;
    let held = decimal(field(words, "held")?)?;
    let period_end_ms = decimal(field(words, "period_end_ms")?)?;
    if words.next().is_some() {
        return None;
    }
    Some(Count {
        cap,
        active,
        spent,
        held,
        period_end_ms,
        holds: Vec::new(),
    })
}

fn hold_line(line: &str) -> Option<Open> {
    let mut words = line.split(' ');
    if words.next() != Some("hold") {
        return None;
    }
    let id = hex_id(field(&mut words, "id")?)?;
    let tier = Tier::parse(field(&mut words, "tier")?).ok()?;
    let amount = decimal(field(&mut words, "amount")?)?;
    let ends_ms = decimal(field(&mut words, "ends_ms")?)?;
    if words.next().is_some() || tier == Tier::OnDevice || amount == 0 {
        return None;
    }
    Some(Open {
        id,
        tier,
        amount,
        ends_ms,
    })
}

fn field<'a>(words: &mut impl Iterator<Item = &'a str>, key: &str) -> Option<&'a str> {
    words.next()?.strip_prefix(key)?.strip_prefix('=')
}

fn decimal(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical {
        text.parse().ok()
    } else {
        None
    }
}

fn hex_id(text: &str) -> Option<HoldId> {
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    if bytes.len() != 32 {
        return None;
    }
    let mut id = [0u8; 16];
    for (at, byte) in id.iter_mut().enumerate() {
        *byte = digit(bytes[2 * at])? * 16 + digit(bytes[2 * at + 1])?;
    }
    Some(HoldId(id))
}
