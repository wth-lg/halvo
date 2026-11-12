//! The compiled-in data of contract T1.6: the ruling, the policy table and the default cap, each of
//! which fails closed when its file does not parse.

use std::sync::OnceLock;

use crate::cap::DefaultCap;
use crate::policy::Policy;
use crate::ruling::Ruling;

const RULING: &str = include_str!("../data/ruling.txt");
const POLICY: &str = include_str!("../data/policy.tsv");
const CAP: &str = include_str!("../data/cap.txt");

/// The ruling of `crates/halvo-route/data/ruling.txt`, read without its last LF, or `Ruling::None`
/// when that line does not parse.
pub fn ruling() -> Ruling {
    ruling_of(RULING)
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

fn ruling_of(text: &str) -> Ruling {
    text.strip_suffix('\n')
        .and_then(|line| Ruling::parse(line).ok())
        .unwrap_or(Ruling::None)
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
        assert_eq!(ruling_of("ruling=none\n"), Ruling::None);
        for text in [
            "",
            "\n",
            "ruling=none",
            "ruling=none\n\n",
            "ruling=none\r\n",
            "ruling=none\r",
            "ruling=none\t",
            "ruling=none ",
            "ruling=none \n",
            "\u{feff}ruling=none\n",
            "ruling=hosted\n",
            "ruling=test_hosted\n",
            "ruling=all\n",
        ] {
            assert_eq!(ruling_of(text), Ruling::None, "{text:?}");
            assert_eq!(ruling_of(text).allowed(), &[Tier::OnDevice]);
            assert!(ruling_of(text).origins().is_empty());
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
