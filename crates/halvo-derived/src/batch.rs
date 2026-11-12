use std::collections::BTreeSet;

use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, StoreId};
use halvo_schema_core::kinds::{KindEntry, RetentionClass, KINDS};
use halvo_schema_core::record::{Episode, Evidence, Summary};
use halvo_schema_core::request::ErrorName;
use halvo_schema_core::site::{AppId, DomainSource, ObservedSite, SiteDomain, SiteValue};
use halvo_sqlcipher::rusqlite;

use crate::db::{one, NewRow, Now, Tx};
use crate::error::{Error, Fault};
use crate::hash::site_hash;
use crate::host::IdSource;
use crate::sql;
use crate::Store;

/// What one call of `commit_batch` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitOutcome {
    /// The position was at or before the committed position: the call changed nothing and ran no step.
    pub repeated: bool,
    /// Summaries stored.
    pub summaries: u64,
    /// Episodes skipped because their summary id is stored already or stands earlier in the same batch.
    pub skipped: u64,
    /// Episodes whose summary lies outside its bound of 93 days, or after the clock's day, and so is not stored.
    pub outside: u64,
    /// Evidence rows stored.
    pub evidence: u64,
    /// Evidence rows not stored because their expiry has passed.
    pub dropped: u64,
}

/// The entry of a kind of the workflow run.
pub(crate) fn this_run(name: &str) -> Result<&'static KindEntry, Fault> {
    KINDS
        .iter()
        .find(|entry| entry.name == name)
        .ok_or(Fault::Row)
}

pub(crate) fn bound_days(entry: &KindEntry) -> Option<u16> {
    match entry.class {
        RetentionClass::Bounded { days } => Some(days),
        RetentionClass::Window | RetentionClass::Kept => None,
    }
}

/// The key of a summary or a workflow: its category and site key or, where the site key is null, its category and
/// app id.
pub(crate) fn key_of(category: SeedCategory, site: &Option<SiteValue>, app: &AppId) -> String {
    match site {
        Some(SiteValue::Domain { key, source }) => {
            format!("{category} {} {}", source.as_str(), key.as_str())
        }
        Some(SiteValue::Title { key }) => format!("{category} title {}", key.as_str()),
        None => format!("{category} app {}", app.as_str()),
    }
}

fn malformed<T, E>(_: E) -> Result<T, Error> {
    Err(Error::Refused(ErrorName::Malformed))
}

struct Records {
    summary: Summary,
    summary_json: String,
    evidence_json: String,
    key: String,
}

impl Records {
    fn of(episode: &Episode, site_key: &[u8; 32]) -> Result<Records, Error> {
        let (site, hint) = match &episode.site {
            None => (None, None),
            Some(ObservedSite::AddressField { domain }) => (
                Some(SiteValue::Domain {
                    key: SiteDomain::from(domain.clone()),
                    source: DomainSource::AddressField,
                }),
                None,
            ),
            Some(ObservedSite::Title { token }) => (
                Some(SiteValue::Title {
                    key: site_hash(site_key, token)?,
                }),
                Some(token.clone()),
            ),
        };
        let summary = Summary {
            id: episode.id.clone(),
            category: episode.category,
            site,
            app: episode.app.clone(),
            day: episode.day,
            part: episode.part,
            minutes: episode.minutes,
            keys: episode.keys,
            pointer: episode.pointer,
        };
        let evidence = Evidence {
            summary: episode.id.clone(),
            segments: episode.segments.clone(),
            expires: episode.expires,
            start: episode.start,
            end: episode.end,
            hint,
            steps: episode.steps.clone(),
        };
        let summary_json = serde_json::to_string(&summary).or_else(malformed)?;
        let evidence_json = serde_json::to_string(&evidence).or_else(malformed)?;
        let key = key_of(summary.category, &summary.site, &summary.app);
        Ok(Records {
            summary,
            summary_json,
            evidence_json,
            key,
        })
    }
}

fn summary_stored(tx: &Tx<'_>, id: &str) -> Result<bool, Fault> {
    Ok(one(tx.conn, sql::SUMMARY_STORED, [id], |r| r.get::<_, i64>(0))?.is_some())
}

impl Store {
    /// `Ok` when an analysis write that names `store` may be taken: the database is bound to no store id yet, or to
    /// this one; else the error name `store`. The owner checks every write of a batch with it.
    pub fn check_store(&self, store: &StoreId) -> Result<(), Error> {
        self.db.arm();
        match self.db.meta()?.store {
            Some(bound) if bound != store.as_str() => Err(Error::Refused(ErrorName::Store)),
            _ => Ok(()),
        }
    }

    /// Commits a batch: in one transaction the position and, for each episode in order, its summary and its
    /// evidence, with the title-derived site token hashed into the summary's site key under the key of the site
    /// hash and kept in plain text as the evidence's hint.
    ///
    /// The database is bound to the store id of its first commit, and a batch of another store id is refused with
    /// the error name `store`. A position at or before the committed one changes nothing (`repeated`). An episode
    /// whose summary id is stored, or stands earlier in the batch, is skipped. A summary outside its bound, a day more
    /// than 92 days before the clock's day or after it, is not stored with its evidence; an evidence row whose expiry
    /// has passed is not stored. A batch that holds a value that its reader would refuse is refused whole with the
    /// error name `malformed`, and nothing of it is stored.
    pub fn commit_batch(
        &mut self,
        store: &StoreId,
        episodes: &[Episode],
        position: Position,
        _ids: &mut dyn IdSource,
    ) -> Result<CommitOutcome, Error> {
        let db = &self.db;
        db.arm();
        let meta = db.meta()?;
        if meta
            .store
            .as_deref()
            .is_some_and(|bound| bound != store.as_str())
        {
            return Err(Error::Refused(ErrorName::Store));
        }
        let (Ok(seg), Ok(idx)) = (i64::try_from(position.seg), i64::try_from(position.idx)) else {
            return Err(Error::Refused(ErrorName::Malformed));
        };
        if meta.position.is_some_and(|committed| position <= committed) {
            return Ok(CommitOutcome {
                repeated: true,
                ..CommitOutcome::default()
            });
        }
        let summary_entry = this_run("summary")?;
        let evidence_entry = this_run("evidence")?;
        let days = bound_days(summary_entry).ok_or(Fault::Row)?;
        let now: Now = db.now();
        db.write(|tx| {
            let mut out = CommitOutcome::default();
            let mut seen = BTreeSet::new();
            for episode in episodes {
                if !seen.insert(episode.id.as_str()) || summary_stored(tx, episode.id.as_str())? {
                    out.skipped += 1;
                    continue;
                }
                let records = Records::of(episode, &db.site_key)?;
                if !now.keeps_day(days, records.summary.day) {
                    out.outside += 1;
                    continue;
                }
                tx.insert(&NewRow {
                    entry: summary_entry,
                    day: Some(records.summary.day),
                    expires: None,
                    workflow: None,
                    summary: Some(episode.id.as_str()),
                    wkey: Some(&records.key),
                    body: &records.summary_json,
                    segments: &[],
                })?;
                out.summaries += 1;
                if !now.keeps_expiry(episode.expires.0) {
                    out.dropped += 1;
                    continue;
                }
                let segments: Vec<u64> = episode.segments.iter().map(|o| o.get()).collect();
                tx.insert(&NewRow {
                    entry: evidence_entry,
                    day: None,
                    expires: Some(episode.expires.0),
                    workflow: None,
                    summary: Some(episode.id.as_str()),
                    wkey: None,
                    body: &records.evidence_json,
                    segments: &segments,
                })?;
                out.evidence += 1;
            }
            tx.exec(
                sql::META_POSITION,
                rusqlite::params![store.as_str(), seg, idx],
            )?;
            Ok::<_, Error>(out)
        })
    }
}
