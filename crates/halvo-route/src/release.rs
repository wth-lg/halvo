//! The compiled-in data of contract T1.6: the ruling with its table of hosts, the policy table and
//! the default cap, each of which fails closed when its file does not parse.

use std::sync::OnceLock;

use crate::cap::DefaultCap;
use crate::policy::Policy;
use crate::ruling::Ruling;

const RULING: &str = include_str!("../data/ruling.txt");
const HOSTS: &str = include_str!("../data/hosts.tsv");
const POLICY: &str = include_str!("../data/policy.tsv");
const CAP: &str = include_str!("../data/cap.txt");

/// The ruling of `crates/halvo-route/data/ruling.txt`, read without its last LF with the table of
/// hosts of `crates/halvo-route/data/hosts.tsv`, or `Ruling::None` when the line does not parse,
/// when the table does not parse, when the table holds no row, and when the line is `ruling=none`.
pub fn ruling() -> Ruling {
    ruling_of(RULING, HOSTS)
}

/// The table of `crates/halvo-route/data/policy.tsv`, read once by `Policy::parse_for` with the
/// tiers that `ruling()` allows, or `Policy::hand_back_all()` when it does not parse.
pub fn policy() -> &'static Policy {
    static TABLE: OnceLock<Policy> = OnceLock::new();
    TABLE.get_or_init(|| policy_of(POLICY, &ruling()))
}

/// The cap of `crates/halvo-route/data/cap.txt`, read without its last LF, or none when that line
/// does not parse.
pub fn cap() -> Option<DefaultCap> {
    cap_of(CAP)
}

fn ruling_of(text: &str, hosts: &str) -> Ruling {
    text.strip_suffix('\n')
        .map_or(Ruling::None, |line| Ruling::read_with_table(line, hosts))
}

fn policy_of(text: &str, ruling: &Ruling) -> Policy {
    Policy::parse_for(text, ruling.allowed()).unwrap_or_else(|_| Policy::hand_back_all())
}

fn cap_of(text: &str) -> Option<DefaultCap> {
    text.strip_suffix('\n')
        .and_then(|line| DefaultCap::parse(line).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap::{CapStatus, Period};
    use crate::tier::Tier;

    fn with_line(text: &str, from: &str, to: &str) -> String {
        let changed = text.replacen(from, to, 1);
        assert_ne!(changed, text, "{from:?}");
        changed
    }

    #[test]
    fn release_ruling_fails_closed() {
        let header = "tier\torigin\tmodel\tadapter\tsource\tdate\n";
        let row = "frontier\thttps://top.invalid\ttop-model\tadapter-b\tfixture:test\t2026-11-22\n";
        let one = format!("{header}{row}");
        assert_eq!(ruling_of("ruling=none\n", HOSTS), Ruling::None);
        assert_eq!(ruling_of("ruling=hosted\n", HOSTS), Ruling::None);
        assert_eq!(ruling_of("ruling=none\n", &one), Ruling::None);
        let open = ruling_of("ruling=hosted\n", &one);
        assert_eq!(open.allowed(), &[Tier::OnDevice, Tier::Frontier]);
        assert_eq!(open.rows().len(), 1);
        for text in [
            "",
            "\n",
            "ruling=none",
            "ruling=hosted",
            "ruling=none\n\n",
            "ruling=hosted\n\n",
            "ruling=none\r\n",
            "ruling=hosted\r\n",
            "ruling=none\r",
            "ruling=none\t",
            "ruling=none ",
            "ruling=none \n",
            "\u{feff}ruling=none\n",
            "\u{feff}ruling=hosted\n",
            "ruling=test_hosted\n",
            "ruling=all\n",
        ] {
            for hosts in [HOSTS, one.as_str()] {
                assert_eq!(ruling_of(text, hosts), Ruling::None, "{text:?}");
                assert_eq!(ruling_of(text, hosts).allowed(), &[Tier::OnDevice]);
                assert!(ruling_of(text, hosts).rows().is_empty());
            }
        }
        let low = row.replacen("frontier", "low_cost_hosted", 1);
        let mut tables = vec![
            String::new(),
            header.trim_end().to_owned(),
            one.trim_end().to_owned(),
            one.replace('\n', "\r\n"),
            format!("{one}\n"),
            format!("\u{feff}{one}"),
            one.replacen("source\tdate", "date\tsource", 1),
            format!("{header}{row}{row}"),
            format!("{header}{low}{low}"),
            one.replacen("\t2026-11-22", "\t2026-11-22\t", 1),
            one.replacen("\tfixture:test", "", 1),
        ];
        for (from, to) in [
            ("frontier\t", "on_device\t"),
            ("frontier\t", "Frontier\t"),
            ("frontier\t", "hand_back\t"),
            ("https://top.invalid", "http://top.invalid"),
            ("https://top.invalid", "https://top.invalid:443"),
            ("https://top.invalid", "https://top.invalid/"),
            ("https://top.invalid", "https://top.invalid/v1"),
            ("https://top.invalid", "https://127.0.0.1"),
            ("https://top.invalid", "https://10.0.0.1"),
            ("https://top.invalid", "https://localhost"),
            ("https://top.invalid", "https://top.localhost"),
            ("https://top.invalid", "https://TOP.invalid"),
            ("https://top.invalid", "https://top..invalid"),
            ("https://top.invalid", "https://top.invalid."),
            ("https://top.invalid", "https://[::1]"),
            ("https://top.invalid", "https://user@top.invalid"),
            ("https://top.invalid", "top.invalid"),
            ("https://top.invalid", ""),
            ("\ttop-model\t", "\ttop model\t"),
            ("\ttop-model\t", "\t\t"),
            ("\ttop-model\t", "\ttop/model\t"),
            ("\tadapter-b\t", "\tadapter\"b\t"),
            ("\tfixture:test\t", "\t-\t"),
            ("\tfixture:test\t", "\t\t"),
            ("\tfixture:test\t", "\tfixture test\t"),
            ("\t2026-11-22", "\t2026-11-31"),
            ("\t2026-11-22", "\t2026-02-29"),
            ("\t2026-11-22", "\t26-11-22"),
        ] {
            tables.push(one.replacen(from, to, 1));
        }
        tables.push(one.replacen("top-model", &"m".repeat(65), 1));
        for table in &tables {
            assert_ne!(table, &one);
            assert_eq!(
                ruling_of("ruling=hosted\n", table),
                Ruling::None,
                "{table:?}"
            );
        }
    }

    #[test]
    fn release_policy_fails_closed() {
        let all = Policy::hand_back_all();
        let base = all.to_text();
        let cell = "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\n";

        let device = with_line(
            &base,
            cell,
            "utility_bill_payment\tcompile\ton_device\tlow\tline_met\t4\t4\n",
        );
        let parsed = Policy::parse_for(&device, &[Tier::OnDevice]).expect("device table");
        assert_ne!(parsed, all);
        assert_eq!(policy_of(&device, &Ruling::None), parsed);

        for tier in ["low_cost_hosted", "frontier"] {
            let hosted = with_line(
                &base,
                cell,
                &format!("utility_bill_payment\tcompile\t{tier}\tlow\tline_met\t4\t4\n"),
            );
            let open = Policy::parse_for(&hosted, &Tier::ALL).expect("hosted table");
            assert_ne!(open, all);
            assert_eq!(policy_of(&hosted, &Ruling::None), all, "{tier}");
        }

        let crlf = device.replace('\n', "\r\n");
        let unended = device.strip_suffix('\n').expect("LF");
        let blank = format!("{device}\n");
        let short = with_line(&device, "other\treplay\tnone\t-\tno_model\t0\t0\n", "");
        for text in [
            "",
            "\n",
            crlf.as_str(),
            unended,
            blank.as_str(),
            short.as_str(),
        ] {
            assert_eq!(policy_of(text, &Ruling::None), all, "{text:?}");
        }
    }

    #[test]
    fn release_cap_fails_closed() {
        let line = "default_cap amount=2000 period=month status=provisional";
        assert_eq!(
            cap_of(&format!("{line}\n")),
            Some(DefaultCap {
                amount: 2000,
                period: Period::Month,
                status: CapStatus::Provisional,
            })
        );
        for text in [
            String::new(),
            "\n".to_owned(),
            line.to_owned(),
            format!("{line}\n\n"),
            format!("{line}\r\n"),
            format!("{line}\r"),
            format!("{line}\t"),
            format!("{line} "),
            format!("{line}\u{a0}"),
            format!("{line}\u{2028}"),
            format!("{line}\t\n"),
            format!("{line} \n"),
            format!("\u{feff}{line}\n"),
            "default_cap amount=0 period=month status=provisional\n".to_owned(),
            "default_cap amount=2000 period=year status=provisional\n".to_owned(),
            "default_cap amount=2000 period=month status=final\n".to_owned(),
        ] {
            assert_eq!(cap_of(&text), None, "{text:?}");
        }
    }
}
