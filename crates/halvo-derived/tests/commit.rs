mod common;

use common::{
    bodies, count, domain, episode, open, position, raw, store_id, token, Scratch, Seq, TestClock,
    DAY_NS,
};
use halvo_derived::Error;
use halvo_schema_core::record::{Evidence, Instant, Summary};
use halvo_schema_core::request::ErrorName;
use halvo_schema_core::site::SiteValue;
use sha2::{Digest, Sha256};

fn hmac(key: &[u8], message: &[u8]) -> String {
    let mut inner = Sha256::new();
    inner.update(
        key.iter()
            .map(|b| b ^ 0x36)
            .chain([0x36; 32])
            .collect::<Vec<u8>>(),
    );
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(
        key.iter()
            .map(|b| b ^ 0x5c)
            .chain([0x5c; 32])
            .collect::<Vec<u8>>(),
    );
    outer.update(inner.finalize());
    outer
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn commit_batch_stores_each_summary_and_its_evidence_with_the_position() {
    let scratch = Scratch::new("commit");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[
                episode(4410, 20_767, domain("northwind-power.test")),
                episode(4411, 20_767, None),
            ],
            position(4411, 212),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!((outcome.summaries, outcome.evidence), (2, 2));
    drop(store);
    let summaries: Vec<Summary> = bodies(&scratch, "summary")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect();
    assert_eq!(summaries.len(), 2);
    assert!(matches!(summaries[0].site, Some(SiteValue::Domain { .. })));
    assert_eq!(summaries[1].site, None);
    let evidence: Vec<Evidence> = bodies(&scratch, "evidence")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect();
    assert_eq!(evidence[0].summary, summaries[0].id);
    assert_eq!(evidence[0].hint, None);
    assert_eq!(evidence[1].segments[0].get(), 4411);
    let (store, seg, idx): (String, i64, i64) = raw(&scratch)
        .query_row("SELECT store, seg, idx FROM meta", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(
        (store.as_str(), seg, idx),
        (store_id(1).as_str(), 4411, 212)
    );
    assert_eq!(count(&scratch, "SELECT count(*) FROM sources"), 2);
}

#[test]
fn commit_batch_hashes_the_title_token_with_the_key_of_its_own_database() {
    let mut hashes = Vec::new();
    for (name, seed) in [("hash-a", 977), ("hash-b", 1999)] {
        let scratch = Scratch::new(name);
        let clock = TestClock::at_day(20_767);
        let mut store = common::open_with_ids(&scratch, &clock, seed).unwrap();
        store
            .commit_batch(
                &store_id(1),
                &[episode(1, 20_767, token("northwind-power"))],
                position(1, 1),
                &mut Seq(1),
            )
            .unwrap();
        drop(store);
        let summary = &bodies(&scratch, "summary")[0];
        assert!(!summary.contains("northwind-power"));
        let parsed: Summary = serde_json::from_str(summary).unwrap();
        let Some(SiteValue::Title { key }) = parsed.site else {
            panic!("the summary holds no title hash");
        };
        let site_key: Vec<u8> = raw(&scratch)
            .query_row("SELECT site_key FROM meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(key.as_str(), hmac(&site_key, b"northwind-power"));
        let evidence: Evidence = serde_json::from_str(&bodies(&scratch, "evidence")[0]).unwrap();
        assert_eq!(evidence.hint.unwrap().as_str(), "northwind-power");
        hashes.push(key.as_str().to_owned());
    }
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn commit_batch_refuses_a_batch_read_from_another_store() {
    let scratch = Scratch::new("other-store");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, 20_767, None)],
            position(1, 1),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(store.check_store(&store_id(1)), Ok(()));
    assert_eq!(
        store.check_store(&store_id(2)),
        Err(Error::Refused(ErrorName::Store))
    );
    let refused = store.commit_batch(
        &store_id(2),
        &[episode(2, 20_767, None)],
        position(2, 1),
        &mut Seq(1),
    );
    assert_eq!(refused.err(), Some(Error::Refused(ErrorName::Store)));
    drop(store);
    assert_eq!(bodies(&scratch, "summary").len(), 1);
    assert_eq!(count(&scratch, "SELECT seg FROM meta"), 1);
}

#[test]
fn a_commit_at_or_before_the_committed_position_changes_nothing() {
    let scratch = Scratch::new("repeat");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, 20_767, None)],
            position(5, 9),
            &mut Seq(1),
        )
        .unwrap();
    for at in [position(5, 9), position(5, 8), position(4, 100)] {
        let again = store
            .commit_batch(&store_id(1), &[episode(2, 20_767, None)], at, &mut Seq(1))
            .unwrap();
        assert!(again.repeated);
        assert_eq!(again.summaries, 0);
    }
    let later = store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, 20_767, None),
                episode(3, 20_767, None),
                episode(3, 20_767, None),
            ],
            position(6, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!((later.summaries, later.skipped), (1, 2));
    drop(store);
    assert_eq!(bodies(&scratch, "summary").len(), 2);
    assert_eq!(bodies(&scratch, "evidence").len(), 2);
    assert_eq!(count(&scratch, "SELECT seg FROM meta"), 6);
}

#[test]
fn commit_batch_stores_no_row_past_the_bound_of_its_class() {
    let scratch = Scratch::new("bound-at-commit");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    let mut expired = episode(4, 20_767, None);
    expired.expires = Instant(clock.now());
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, 20_767 - 92, None),
                episode(2, 20_767 - 93, None),
                episode(3, 20_768, None),
                expired,
            ],
            position(1, 1),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(
        (
            outcome.summaries,
            outcome.outside,
            outcome.evidence,
            outcome.dropped
        ),
        (2, 2, 1, 1)
    );
    drop(store);
    let days: Vec<u32> = bodies(&scratch, "summary")
        .iter()
        .map(|b| serde_json::from_str::<Summary>(b).unwrap().day)
        .collect();
    assert_eq!(days, vec![20_767 - 92, 20_767]);
    assert_eq!(bodies(&scratch, "evidence").len(), 1);
    assert_eq!(clock.now() / DAY_NS, 20_767);
}

#[test]
fn a_batch_with_a_value_that_its_reader_refuses_is_refused_whole() {
    let scratch = Scratch::new("malformed");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    let mut bad = episode(2, 20_767, None);
    bad.minutes = 7;
    let refused = store.commit_batch(
        &store_id(1),
        &[episode(1, 20_767, None), bad],
        position(1, 1),
        &mut Seq(1),
    );
    assert_eq!(refused.err(), Some(Error::Refused(ErrorName::Malformed)));
    drop(store);
    assert_eq!(bodies(&scratch, "summary").len(), 0);
    assert_eq!(
        count(
            &scratch,
            "SELECT count(*) FROM meta WHERE seg IS NULL AND store IS NULL"
        ),
        1
    );
}

#[test]
fn an_episode_that_names_one_segment_twice_is_refused_by_its_reader() {
    let scratch = Scratch::new("episode-segment-twice");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    let mut twice = episode(7, 20_767, None);
    twice.segments = vec![
        halvo_schema_core::record::Ordinal::new(7).unwrap(),
        halvo_schema_core::record::Ordinal::new(7).unwrap(),
    ];
    let refused = store.commit_batch(&store_id(1), &[twice], position(1, 1), &mut Seq(1));
    assert_eq!(refused.err(), Some(Error::Refused(ErrorName::Malformed)));
    let text = format!(
        r#"{{"id":"{}","category":"email","site":null,"app":"com.example.browser","day":20767,"part":"evening","minutes":5,"keys":null,"pointer":null,"segments":[7,7],"expires":1,"start":1,"end":2,"steps":[]}}"#,
        "7".repeat(32)
    );
    assert!(serde_json::from_str::<halvo_schema_core::record::Episode>(&text).is_err());
    let once = text.replace("[7,7]", "[7]");
    assert!(serde_json::from_str::<halvo_schema_core::record::Episode>(&once).is_ok());
    drop(store);
    assert_eq!(bodies(&scratch, "summary").len(), 0);
}
