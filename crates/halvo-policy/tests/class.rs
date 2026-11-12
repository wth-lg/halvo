mod common;

use std::num::NonZeroU32;

use common::{api, file, web, web_credential, web_request};
use halvo_policy::{check_playbook, class_of};
use halvo_schema::executable::names::Cause;
use halvo_schema::executable::ActionClass as C;
use halvo_schema::SeedCategory as S;

fn request_class(category: S) -> C {
    match category {
        S::UtilityBillPayment | S::TenantRent => C::Payment,
        S::ShopOrder | S::RideOrder => C::Purchase,
        S::LeaseRenewalOrChange => C::Sign,
        S::TenantApplicationReview
        | S::Email
        | S::SpreadsheetFill
        | S::CalendarCleanup
        | S::DailyChartCheck => C::Submit,
    }
}

#[test]
fn a_web_handback_or_a_credential_target_is_credential_before_every_other_row() {
    for &category in S::ALL {
        assert_eq!(
            class_of(category, &web(1, "handback", C::Credential)),
            C::Credential
        );
        assert_eq!(
            class_of(category, &web_credential(1, "none", C::Credential)),
            C::Credential
        );
        assert_eq!(
            class_of(category, &web_credential(1, "request", C::Credential)),
            C::Credential,
            "a credential target that issues a request is still credential in {category:?}"
        );
    }
}

#[test]
fn a_web_request_takes_the_class_of_its_category() {
    for &category in S::ALL {
        let class = request_class(category);
        for verb in ["click", "press"] {
            assert_eq!(
                class_of(category, &web_request(1, verb, class)),
                class,
                "{verb} in {category:?}"
            );
        }
    }
    assert_eq!(
        class_of(S::UtilityBillPayment, &web_request(1, "click", C::Payment)),
        C::Payment
    );
    assert_eq!(
        class_of(S::TenantRent, &web_request(1, "click", C::Payment)),
        C::Payment
    );
    assert_eq!(
        class_of(S::ShopOrder, &web_request(1, "click", C::Purchase)),
        C::Purchase
    );
    assert_eq!(
        class_of(S::RideOrder, &web_request(1, "click", C::Purchase)),
        C::Purchase
    );
    assert_eq!(
        class_of(S::LeaseRenewalOrChange, &web_request(1, "click", C::Sign)),
        C::Sign
    );
    assert_eq!(
        class_of(S::Email, &web_request(1, "click", C::Submit)),
        C::Submit
    );
}

#[test]
fn a_stored_class_never_lowers_the_class_of_a_request() {
    for &category in S::ALL {
        let step = web_request(1, "click", C::Submit);
        assert_eq!(class_of(category, &step), request_class(category));
        let step = web_request(1, "click", C::Prepare);
        assert_eq!(class_of(category, &step), request_class(category));
    }
}

#[test]
fn a_web_step_without_a_request_is_read_or_prepare_by_its_verb() {
    for &category in S::ALL {
        for verb in ["navigate", "extract", "wait"] {
            assert_eq!(
                class_of(category, &web(1, verb, C::Read)),
                C::Read,
                "{verb}"
            );
        }
        for verb in ["click", "fill", "select", "press", "upload"] {
            assert_eq!(
                class_of(category, &web(1, verb, C::Prepare)),
                C::Prepare,
                "{verb}"
            );
        }
    }
}

#[test]
fn an_api_step_takes_the_class_of_its_operation() {
    for &category in S::ALL {
        assert_eq!(class_of(category, &api(1, "mail_list", C::Read)), C::Read);
        assert_eq!(
            class_of(category, &api(1, "calendar_list", C::Read)),
            C::Read
        );
        assert_eq!(
            class_of(category, &api(1, "mail_draft_reply", C::Prepare)),
            C::Prepare
        );
        assert_eq!(class_of(category, &api(1, "mail_send", C::Send)), C::Send);
        assert_eq!(
            class_of(category, &api(1, "calendar_delete", C::Delete)),
            C::Delete
        );
    }
}

#[test]
fn a_file_read_is_read_and_a_file_write_is_local_edit() {
    for &category in S::ALL {
        assert_eq!(class_of(category, &file(1, "read", C::Read)), C::Read);
        assert_eq!(
            class_of(category, &file(1, "write", C::LocalEdit)),
            C::LocalEdit
        );
    }
}

fn n(k: u32) -> NonZeroU32 {
    match NonZeroU32::new(k) {
        Some(n) => n,
        None => panic!("zero"),
    }
}

fn bill(stored_pay: C) -> Vec<halvo_schema::executable::ExecutableStep> {
    vec![
        web(1, "navigate", C::Read),
        web(2, "handback", C::Credential),
        web(3, "extract", C::Read),
        web(4, "fill", C::Prepare),
        web_request(5, "click", stored_pay),
        web(6, "wait", C::Read),
    ]
}

#[test]
fn check_playbook_takes_a_playbook_whose_stored_classes_are_the_computed_ones() {
    assert_eq!(
        check_playbook(S::UtilityBillPayment, &bill(C::Payment)),
        Ok(())
    );
    let reply = vec![
        api(1, "mail_list", C::Read),
        api(2, "mail_draft_reply", C::Prepare),
        api(3, "mail_send", C::Send),
    ];
    assert_eq!(check_playbook(S::Email, &reply), Ok(()));
    let fill = vec![file(1, "read", C::Read), file(2, "write", C::LocalEdit)];
    assert_eq!(check_playbook(S::SpreadsheetFill, &fill), Ok(()));
    assert_eq!(check_playbook(S::SpreadsheetFill, &[]), Ok(()));
}

#[test]
fn check_playbook_refuses_with_class_at_the_first_stored_class_that_differs() {
    let refused = check_playbook(S::UtilityBillPayment, &bill(C::Submit));
    let Err(refusal) = refused else {
        panic!("a payment step stored as submit passed");
    };
    assert_eq!(refusal.n, n(5));
    assert_eq!(refusal.cause(), Cause::Class);

    let bill = bill(C::Payment);
    assert_eq!(
        check_playbook(S::ShopOrder, &bill).map_err(|r| r.n),
        Err(n(5)),
        "the bill's payment step is a purchase in shop_order"
    );

    let steps = vec![
        web(1, "navigate", C::Read),
        web(2, "fill", C::Read),
        web(3, "wait", C::Prepare),
    ];
    assert_eq!(check_playbook(S::Email, &steps).map_err(|r| r.n), Err(n(2)));

    let steps = vec![
        api(1, "mail_list", C::Read),
        api(2, "mail_send", C::Send),
        api(3, "mail_list", C::Prepare),
    ];
    assert_eq!(check_playbook(S::Email, &steps).map_err(|r| r.n), Err(n(3)));
    let steps = vec![file(1, "write", C::Read)];
    assert_eq!(
        check_playbook(S::SpreadsheetFill, &steps).map_err(|r| r.n),
        Err(n(1))
    );
}
