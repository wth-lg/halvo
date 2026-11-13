use halvo_coord::roster::{lines, names, order, plain_site, row, status, Placed};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{Category, Limits};
use serde_json::Value;

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/tests/fixtures/coord");

fn fixture(state: State) -> Value {
    let path = format!("{TRACKED}/state-{}.json", state.name());
    let text = std::fs::read_to_string(&path).expect("a tracked state fixture");
    serde_json::from_str(&text).expect("a state fixture is JSON")
}

fn rows(state: State) -> Value {
    let all = facts(state);
    let refs: Vec<_> = all.iter().collect();
    let names = names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap();
    let mut placed: Vec<Placed> = all
        .iter()
        .zip(&names)
        .map(|(f, name)| Placed {
            row: row(f, status(f, &Limits::PRODUCT, None), name).unwrap(),
            raised: None,
        })
        .collect();
    order(&mut placed);
    serde_json::to_value(placed.into_iter().map(|p| p.row).collect::<Vec<_>>()).unwrap()
}

#[test]
fn roster_fixtures() {
    for state in State::ALL {
        let file = fixture(*state);
        assert_eq!(
            rows(*state),
            file["snapshot"]["rows"],
            "the rows of the state {}",
            state.name()
        );
    }
    let card = fixture(State::Card);
    let first = facts(State::Card).remove(0);
    assert_eq!(
        serde_json::to_value(lines(&first.config)).unwrap(),
        card["card"]["lines"]
    );
}

#[test]
fn the_label_of_a_site_is_its_host_without_www_and_its_last_label_in_upper_case_first() {
    assert_eq!(plain_site("site1.example").as_deref(), Some("Site1"));
    assert_eq!(plain_site("www.bank.example").as_deref(), Some("Bank"));
    assert_eq!(plain_site("shop.co.uk").as_deref(), Some("Shop.co"));
    assert_eq!(plain_site("example"), None);
    assert_eq!(plain_site(""), None);
}

#[test]
fn two_agents_with_one_name_get_the_ordinals_two_and_three_by_agent_id() {
    let mut three = facts(State::Ten);
    three.truncate(1);
    let mut second = three[0].clone();
    second.agent = halvo_coord::types::AgentId::parse(&format!("{:032x}", 0x50)).unwrap();
    let mut third = three[0].clone();
    third.agent = halvo_coord::types::AgentId::parse(&format!("{:032x}", 0x40)).unwrap();
    three.push(second);
    three.push(third);
    let refs: Vec<_> = three.iter().collect();
    let got = names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap();
    let plain = "Paying a utility bill on Site1";
    assert_eq!(
        got,
        [
            plain.to_owned(),
            format!("{plain} (3)"),
            format!("{plain} (2)")
        ]
    );
    let none = names(&refs[..1], |_| None).unwrap();
    assert_eq!(none, ["Paying a utility bill"]);
    assert_eq!(Category::ALL.len(), 10);
}

#[test]
fn inside_needs_you_the_oldest_open_item_comes_first_and_elsewhere_the_name() {
    let all = facts(State::Ten);
    let refs: Vec<_> = all.iter().collect();
    let names = names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap();
    let mut a = all[4].clone();
    let mut b = all[0].clone();
    for (f, n) in [(&mut a, 1u32), (&mut b, 2u32)] {
        f.doing = halvo_coord::types::Doing::Idle(Some(halvo_coord::types::OwnItem {
            item: halvo_coord::types::ItemId::parse(&format!("{n:032x}")).unwrap(),
            kind: halvo_coord::types::OwnItemKind::Ready,
        }));
    }
    let place = |f: &halvo_coord::types::Facts<_>, name: &str, raised| Placed {
        row: row(f, status(f, &Limits::PRODUCT, None), name).unwrap(),
        raised,
    };
    let mut placed = vec![
        place(&b, &names[0], Some(20)),
        place(&a, &names[4], Some(10)),
        place(&all[7], &names[7], None),
        place(&all[5], &names[5], None),
    ];
    order(&mut placed);
    let got: Vec<&str> = placed.iter().map(|p| p.row.name.as_str()).collect();
    assert_eq!(
        got,
        [
            names[4].as_str(),
            names[0].as_str(),
            names[5].as_str(),
            names[7].as_str()
        ]
    );
}

#[test]
fn the_line_time_rounds_wall_s_up_to_whole_minutes() {
    let mut f = facts(State::Ten).remove(0);
    let text = |wall: u64| {
        let record = format!(
            "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"utility_bill_payment\",\"tier\":\"on_device\",\"reasoning\":\"high\",\"autonomy\":\"observe\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":{wall},\"model_tokens\":0}},\"budget\":null,\"permissions\":[\"user_files\"],\"runtime\":\"web\",\"playbook\":null}}",
            f.workflow.id, f.agent
        );
        halvo_coord::mem::Config::parse(&record).unwrap()
    };
    f.config = text(61);
    let got = lines(&f.config);
    let keys: Vec<&str> = got.iter().map(|l| l.key.name()).collect();
    assert_eq!(keys, ["level", "runs_on", "asks", "time", "files"]);
    assert_eq!(
        got[3].text,
        halvo_coord::words::text("line.time", &[("minutes", "2")]).unwrap()
    );
    assert_eq!(
        lines(&text(120))[3].text,
        halvo_coord::words::text("line.time", &[("minutes", "2")]).unwrap()
    );
}
