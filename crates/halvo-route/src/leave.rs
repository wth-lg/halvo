//! What may leave the machine, contract T9.2: the grant without which no byte of a model request
//! leaves, and `grant`, the one code that makes one.

use crate::ruling::Ruling;
use crate::tier::Tier;

/// A kind of part of a model request. Only `text` may leave (D13).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PartKind {
    Text,
    Image,
    FileRef,
    Url,
    StoredConversation,
}

impl PartKind {
    pub const ALL: [PartKind; 5] = [
        PartKind::Text,
        PartKind::Image,
        PartKind::FileRef,
        PartKind::Url,
        PartKind::StoredConversation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            PartKind::Text => "text",
            PartKind::Image => "image",
            PartKind::FileRef => "file_ref",
            PartKind::Url => "url",
            PartKind::StoredConversation => "stored_conversation",
        }
    }
}

/// Whether the person's agreement to hosted steps is recorded.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Consent {
    Recorded,
    Missing,
}

impl Consent {
    pub const ALL: [Consent; 2] = [Consent::Recorded, Consent::Missing];

    pub const fn as_str(self) -> &'static str {
        match self {
            Consent::Recorded => "recorded",
            Consent::Missing => "missing",
        }
    }
}

/// Why `grant` makes no grant, in the order in which it tests.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LeaveCause {
    Local,
    Ruling,
    Ceiling,
    Part,
    Origin,
    Consent,
}

impl LeaveCause {
    pub const ALL: [LeaveCause; 6] = [
        LeaveCause::Local,
        LeaveCause::Ruling,
        LeaveCause::Ceiling,
        LeaveCause::Part,
        LeaveCause::Origin,
        LeaveCause::Consent,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            LeaveCause::Local => "local",
            LeaveCause::Ruling => "ruling",
            LeaveCause::Ceiling => "ceiling",
            LeaveCause::Part => "part",
            LeaveCause::Origin => "origin",
            LeaveCause::Consent => "consent",
        }
    }
}

/// The leave for one model request to leave the machine. It has a private field, no public maker
/// but `grant`, and neither `Clone` nor `Copy`, so one grant serves one request. While the ruling
/// is `none` no build that is not a test build of this crate can hold one.
///
/// A caller outside this module cannot write one:
///
/// ```compile_fail,E0451
/// let grant = halvo_route::leave::LeaveGrant { _sealed: () };
/// ```
///
/// ```compile_fail,E0277
/// let grant: halvo_route::leave::LeaveGrant = Default::default();
/// ```
///
/// ```compile_fail,E0599
/// let grant = halvo_route::leave::LeaveGrant::new();
/// ```
///
/// cannot copy one:
///
/// ```compile_fail,E0599
/// use halvo_route::leave::LeaveGrant;
/// fn copy(grant: LeaveGrant) -> (LeaveGrant, LeaveGrant) {
///     let other = grant.clone();
///     (grant, other)
/// }
/// ```
///
/// ```compile_fail,E0382
/// use halvo_route::leave::LeaveGrant;
/// fn twice(grant: LeaveGrant) -> (LeaveGrant, LeaveGrant) {
///     (grant, grant)
/// }
/// ```
///
/// cannot reach its field:
///
/// ```compile_fail,E0616
/// fn field(grant: &halvo_route::leave::LeaveGrant) {
///     let () = grant._sealed;
/// }
/// ```
///
/// and cannot name the ruling that a test build of this crate alone has:
///
/// ```compile_fail,E0599
/// let ruling = halvo_route::ruling::Ruling::test_hosted(&["127.0.0.1:1"]);
/// ```
///
/// ```compile_fail,E0599
/// let ruling = halvo_route::ruling::Ruling::TestHosted(Vec::new());
/// ```
///
/// It can move a grant, print it, and ask `grant`, which refuses under the release ruling:
///
/// ```
/// use halvo_route::leave::{grant, Consent, LeaveCause, LeaveGrant, PartKind};
/// use halvo_route::tier::Tier;
/// fn once(grant: LeaveGrant) -> LeaveGrant {
///     grant
/// }
/// fn look(grant: &LeaveGrant) -> String {
///     format!("{grant:?}")
/// }
/// let ruling = halvo_route::release::ruling();
/// let answer = grant(
///     &ruling,
///     Tier::Frontier,
///     Tier::Frontier,
///     &[PartKind::Text],
///     "127.0.0.1:1",
///     Consent::Recorded,
/// )
/// .map(once);
/// assert_eq!(answer.as_ref().map(look).err(), Some(&LeaveCause::Ruling));
/// ```
#[derive(Debug)]
pub struct LeaveGrant {
    _sealed: (),
}

/// The one maker of a `LeaveGrant`. The first rule that fits: the tier `on_device` gives `local`,
/// because nothing leaves and no grant is needed; the ruling `None` gives `ruling`. Only a test
/// build of this crate, under its test ruling, tests on: a tier above `ceiling` gives `ceiling`; a
/// part that is not `text` gives `part`; an origin that is not among `ruling.origins()`, by equal
/// bytes, gives `origin`; the consent `missing` gives `consent`; else the grant. Every other build
/// makes no grant.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn grant(
    ruling: &Ruling,
    tier: Tier,
    ceiling: Tier,
    parts: &[PartKind],
    origin: &str,
    consent: Consent,
) -> Result<LeaveGrant, LeaveCause> {
    if tier == Tier::OnDevice {
        return Err(LeaveCause::Local);
    }
    match ruling {
        Ruling::None => Err(LeaveCause::Ruling),
        #[cfg(test)]
        Ruling::TestHosted(_) => {
            if tier > ceiling {
                Err(LeaveCause::Ceiling)
            } else if parts.iter().any(|part| *part != PartKind::Text) {
                Err(LeaveCause::Part)
            } else if !ruling.origins().iter().any(|allowed| allowed == origin) {
                Err(LeaveCause::Origin)
            } else if consent == Consent::Missing {
                Err(LeaveCause::Consent)
            } else {
                Ok(LeaveGrant { _sealed: () })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release;

    const ORIGIN: &str = "127.0.0.1:1";
    const HOSTED: [Tier; 2] = [Tier::LowCostHosted, Tier::Frontier];
    const NOT_TEXT: [PartKind; 4] = [
        PartKind::Image,
        PartKind::FileRef,
        PartKind::Url,
        PartKind::StoredConversation,
    ];

    fn open() -> Ruling {
        Ruling::test_hosted(&[ORIGIN])
    }

    fn answer(
        ruling: &Ruling,
        tier: Tier,
        ceiling: Tier,
        parts: &[PartKind],
        origin: &str,
        consent: Consent,
    ) -> Result<(), LeaveCause> {
        grant(ruling, tier, ceiling, parts, origin, consent).map(|_| ())
    }

    fn hosted(
        tier: Tier,
        parts: &[PartKind],
        origin: &str,
        consent: Consent,
    ) -> Result<(), LeaveCause> {
        answer(&open(), tier, Tier::Frontier, parts, origin, consent)
    }

    #[test]
    fn refusal_leave_ruling() {
        for tier in HOSTED {
            for ceiling in [Tier::OnDevice, Tier::LowCostHosted, Tier::Frontier] {
                assert_eq!(
                    answer(
                        &release::ruling(),
                        tier,
                        ceiling,
                        &[PartKind::Image],
                        "example.invalid:443",
                        Consent::Missing,
                    ),
                    Err(LeaveCause::Ruling),
                    "{tier:?} {ceiling:?}"
                );
            }
            assert_eq!(
                answer(
                    &release::ruling(),
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    ORIGIN,
                    Consent::Recorded,
                ),
                Err(LeaveCause::Ruling),
                "{tier:?}"
            );
            assert_eq!(
                hosted(tier, &[PartKind::Text], ORIGIN, Consent::Recorded),
                Ok(())
            );
        }
    }

    #[test]
    fn refusal_leave_local() {
        for ruling in [release::ruling(), open(), Ruling::test_hosted(&[])] {
            for ceiling in Tier::ALL {
                for consent in Consent::ALL {
                    for parts in [&[PartKind::Text][..], &[PartKind::Image], &[]] {
                        for origin in [ORIGIN, "example.invalid:443"] {
                            assert_eq!(
                                answer(&ruling, Tier::OnDevice, ceiling, parts, origin, consent),
                                Err(LeaveCause::Local),
                                "{ruling:?} {ceiling:?} {consent:?} {parts:?} {origin}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn refusal_leave_ceiling() {
        for (tier, ceiling) in [
            (Tier::LowCostHosted, Tier::OnDevice),
            (Tier::Frontier, Tier::OnDevice),
            (Tier::Frontier, Tier::LowCostHosted),
        ] {
            assert_eq!(
                answer(
                    &open(),
                    tier,
                    ceiling,
                    &[PartKind::Text],
                    ORIGIN,
                    Consent::Recorded
                ),
                Err(LeaveCause::Ceiling),
                "{tier:?} {ceiling:?}"
            );
        }
        for (tier, ceiling) in [
            (Tier::LowCostHosted, Tier::LowCostHosted),
            (Tier::LowCostHosted, Tier::Frontier),
            (Tier::Frontier, Tier::Frontier),
        ] {
            assert_eq!(
                answer(
                    &open(),
                    tier,
                    ceiling,
                    &[PartKind::Text],
                    ORIGIN,
                    Consent::Recorded
                ),
                Ok(()),
                "{tier:?} {ceiling:?}"
            );
        }
    }

    #[test]
    fn refusal_leave_part() {
        for tier in HOSTED {
            for kind in NOT_TEXT {
                for parts in [
                    vec![kind],
                    vec![PartKind::Text, kind],
                    vec![kind, PartKind::Text],
                    vec![PartKind::Text, PartKind::Text, kind],
                ] {
                    assert_eq!(
                        hosted(tier, &parts, ORIGIN, Consent::Recorded),
                        Err(LeaveCause::Part),
                        "{tier:?} {parts:?}"
                    );
                }
            }
            assert_eq!(
                hosted(tier, &PartKind::ALL, ORIGIN, Consent::Recorded),
                Err(LeaveCause::Part)
            );
            for parts in [
                &[PartKind::Text][..],
                &[PartKind::Text, PartKind::Text],
                &[],
            ] {
                assert_eq!(
                    hosted(tier, parts, ORIGIN, Consent::Recorded),
                    Ok(()),
                    "{parts:?}"
                );
            }
        }
    }

    #[test]
    fn refusal_leave_origin() {
        let two = Ruling::test_hosted(&["https://a.example", ORIGIN]);
        for tier in HOSTED {
            for origin in [
                "127.0.0.1:2",
                "127.0.0.1",
                "127.0.0.1:1 ",
                " 127.0.0.1:1",
                "127.0.0.1:10",
                "27.0.0.1:1",
                "example.invalid:443",
                "",
            ] {
                assert_eq!(
                    hosted(tier, &[PartKind::Text], origin, Consent::Recorded),
                    Err(LeaveCause::Origin),
                    "{tier:?} {origin:?}"
                );
            }
            for origin in ["https://a.example", ORIGIN] {
                assert_eq!(
                    answer(
                        &two,
                        tier,
                        Tier::Frontier,
                        &[PartKind::Text],
                        origin,
                        Consent::Recorded
                    ),
                    Ok(()),
                    "{origin}"
                );
            }
            for origin in [
                "https://A.example",
                "https://a.example/",
                "https://a.exampl",
            ] {
                assert_eq!(
                    answer(
                        &two,
                        tier,
                        Tier::Frontier,
                        &[PartKind::Text],
                        origin,
                        Consent::Recorded
                    ),
                    Err(LeaveCause::Origin),
                    "{origin}"
                );
            }
            assert_eq!(
                answer(
                    &Ruling::test_hosted(&[]),
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    ORIGIN,
                    Consent::Recorded,
                ),
                Err(LeaveCause::Origin)
            );
        }
    }

    #[test]
    fn refusal_leave_consent() {
        for tier in HOSTED {
            assert_eq!(
                hosted(tier, &[PartKind::Text], ORIGIN, Consent::Missing),
                Err(LeaveCause::Consent),
                "{tier:?}"
            );
            assert_eq!(
                hosted(tier, &[PartKind::Text], ORIGIN, Consent::Recorded),
                Ok(())
            );
        }
    }

    #[test]
    fn leave_rules_in_order() {
        let none = release::ruling();
        for tier in HOSTED {
            let below = Tier::ALL[tier as usize - 1];
            let cases = [
                (
                    &open(),
                    tier,
                    below,
                    PartKind::Image,
                    "x",
                    Consent::Missing,
                    LeaveCause::Ceiling,
                ),
                (
                    &open(),
                    tier,
                    tier,
                    PartKind::Image,
                    "x",
                    Consent::Missing,
                    LeaveCause::Part,
                ),
                (
                    &open(),
                    tier,
                    tier,
                    PartKind::Text,
                    "x",
                    Consent::Missing,
                    LeaveCause::Origin,
                ),
                (
                    &open(),
                    tier,
                    tier,
                    PartKind::Text,
                    ORIGIN,
                    Consent::Missing,
                    LeaveCause::Consent,
                ),
                (
                    &none,
                    tier,
                    below,
                    PartKind::Image,
                    "x",
                    Consent::Missing,
                    LeaveCause::Ruling,
                ),
            ];
            for (ruling, tier, ceiling, part, origin, consent, cause) in cases {
                assert_eq!(
                    answer(ruling, tier, ceiling, &[part], origin, consent),
                    Err(cause),
                    "{ruling:?} {tier:?} {ceiling:?} {part:?} {origin} {consent:?}"
                );
            }
        }
    }

    #[test]
    fn grant_is_the_only_maker() {
        let code: String = include_str!("leave.rs")
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .expect("the code before the tests")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(code.matches("_sealed").count(), 2, "{code}");
        let body = code
            .split("\npub fn grant(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("the body of grant");
        assert_eq!(body.matches("LeaveGrant { _sealed: () }").count(), 1);
        assert!(!code.contains("unsafe"));
    }

    #[test]
    fn leave_closed_lists() {
        let words = |list: &[&str]| list.join(",");
        assert_eq!(
            words(&PartKind::ALL.map(PartKind::as_str)),
            "text,image,file_ref,url,stored_conversation"
        );
        assert_eq!(
            words(&Consent::ALL.map(Consent::as_str)),
            "recorded,missing"
        );
        assert_eq!(
            words(&LeaveCause::ALL.map(LeaveCause::as_str)),
            "local,ruling,ceiling,part,origin,consent"
        );
    }
}
