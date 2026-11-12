mod common;

use common::{config, config_json};
use halvo_policy::{decide, decide_row, Checkpoint, Decision};
use halvo_schema::executable::ActionClass;

#[test]
fn decide_carries_the_class_and_the_row_of_the_configuration() {
    let autonomies = ["observe", "draft_and_confirm", "unattended"];
    let allows: [&[&str]; 4] = [
        &[],
        &["send"],
        &["delete", "submit"],
        &["send", "delete", "submit"],
    ];
    let caps = [
        ("yes", "yes", "yes"),
        ("no", "yes", "no"),
        ("yes", "no", "yes"),
        ("no", "no", "no"),
    ];
    let mut tried = 0;
    for autonomy in autonomies {
        for allow in allows {
            for cap in caps {
                let c = config(&config_json("email", autonomy, allow, cap, "graph_mail"));
                for &class in ActionClass::ALL {
                    for checkpoint in [Checkpoint::Yes, Checkpoint::No] {
                        let decided = decide(&c, class, checkpoint);
                        assert_eq!(decided.class(), class);
                        assert_eq!(
                            decided.decision(),
                            decide_row(c.autonomy, class, &c.allow, &c.caps, checkpoint),
                            "{autonomy} {allow:?} {cap:?} {class:?} {checkpoint:?}"
                        );
                        tried += 1;
                    }
                }
            }
        }
    }
    assert_eq!(tried, 3 * 4 * 4 * 10 * 2);
}

#[test]
fn decide_gives_run_for_an_allowed_class_of_an_unattended_split_agent_and_confirm_without_the_split(
) {
    let split = config(&config_json(
        "spreadsheet_fill",
        "unattended",
        &["send"],
        ("no", "yes", "no"),
        "file",
    ));
    assert_eq!(
        decide(&split, ActionClass::Send, Checkpoint::No).decision(),
        Decision::Run
    );
    assert_eq!(
        decide(&split, ActionClass::Delete, Checkpoint::No).decision(),
        Decision::Confirm
    );
    assert_eq!(
        decide(&split, ActionClass::Payment, Checkpoint::No).decision(),
        Decision::Confirm
    );
    let joined = config(&config_json(
        "email",
        "unattended",
        &["send"],
        ("yes", "yes", "no"),
        "graph_mail",
    ));
    assert_eq!(
        decide(&joined, ActionClass::Send, Checkpoint::No).decision(),
        Decision::Confirm
    );
}
