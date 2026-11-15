use halvo_schema::kinds::registry;
use halvo_schema::{ErrorName, FieldType, Instant, RetentionClass, TypedRecord};
use serde_json::Value;

use crate::errors::Own;
use crate::host::{Host, Now};

pub(crate) fn of(host: &dyn Host, now: Now) -> u32 {
    host.day(now.at)
}

pub(crate) fn commit(
    host: &mut dyn Host,
    now: Now,
    records: Vec<TypedRecord>,
) -> Result<(), ErrorName> {
    let today = of(&*host, now);
    for record in &records {
        check(&*host, today, record)?;
    }
    host.commit(records)
}

fn check(host: &dyn Host, today: u32, record: &TypedRecord) -> Result<(), ErrorName> {
    let Some(entry) = registry().find(|e| e.name == record.kind) else {
        return Ok(());
    };
    let Some(days) = bound(record.kind) else {
        return Ok(());
    };
    let value: Value = serde_json::from_str(&record.json).map_err(|_| Own::Day)?;
    let day = value
        .get("day")
        .and_then(Value::as_u64)
        .and_then(|d| u32::try_from(d).ok())
        .ok_or(Own::Day)?;
    if entry.fields.contains(&("at", FieldType::Instant)) {
        let at = value
            .get("at")
            .and_then(Value::as_i64)
            .filter(|at| *at >= 0)
            .ok_or(Own::Day)?;
        if host.day(Instant(at)) != day {
            return Err(Own::Day.into());
        }
    }
    if !within(today, days, day) {
        return Err(Own::Day.into());
    }
    Ok(())
}

pub(crate) fn bound(kind: &str) -> Option<u16> {
    match registry().find(|e| e.name == kind)?.class {
        RetentionClass::Bounded { days } => Some(days),
        _ => None,
    }
}

pub(crate) fn within(today: u32, days: u16, day: u32) -> bool {
    let age = i64::from(today) - i64::from(day);
    (0..i64::from(days)).contains(&age)
}
