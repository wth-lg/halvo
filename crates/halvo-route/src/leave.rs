//! What may leave the machine, contract T9.2: the grant without which no byte of a model request
//! leaves, and `grant`, the one code that makes one.

use std::fmt;

use crate::ruling::{HostRow, Ruling};
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

/// The leave for one model request to leave the machine, which names the row of the ruling that it
/// was made for. It has private fields, no public maker but `grant`, and neither `Clone` nor
/// `Copy`, so one grant serves one request. One field is of a private type of no size that is
/// neither `Clone` nor `Copy` either, so no code can copy a grant from a borrowed one by a struct
/// update. `grant` makes one only under a hosted ruling, for the origin of its row for the tier,
/// and never under the release ruling of main, which is `ruling=none` over a table with no row.
///
/// This file is pinned whole, its test module included, as lib.rs, ruling.rs and tier.rs are: the
/// test `pinned_files_hold_their_hashes` of tests/no_grant_in_release.rs holds its SHA-256. A pin
/// stops no change. It makes each change of the file come with a new hash line in the same commit,
/// where the reviewer of that commit reads it.
///
/// A caller outside this module cannot write one:
///
/// ```compile_fail,E0451
/// let grant = halvo_route::leave::LeaveGrant { _sealed: todo!(), row: todo!() };
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
/// and, in every build, cannot call the reader of a ruling line with a table, which is private to
/// the crate, or name the payload of a hosted ruling, which stands in a private module:
///
/// ```compile_fail,E0624
/// let ruling = halvo_route::ruling::Ruling::read_with_table("ruling=hosted", "");
/// ```
///
/// ```compile_fail,E0603
/// fn rows(rows: halvo_route::ruling::payload::HostRows) {}
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
    _sealed: Seal,
    row: HostRow,
}

impl LeaveGrant {
    /// The row that the grant was made for: its tier, its origin, its model word and its adapter
    /// word.
    pub fn row(&self) -> &HostRow {
        &self.row
    }
}

struct Seal;

impl fmt::Debug for Seal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Seal")
    }
}

/// The one maker of a `LeaveGrant`, with every rule compiled in every build. The first rule that
/// fits: the tier `on_device` gives `local`, because nothing leaves and no grant is needed; the
/// ruling `None`, and a hosted ruling that holds no row for the tier, give `ruling`; a tier above
/// `ceiling` gives `ceiling`; a part that is not `text` gives `part`; an origin that is not, by
/// equal bytes, the origin of the ruling's row for that tier gives `origin`; the consent `missing`
/// gives `consent`; else the grant, which holds a copy of that row. Under the release ruling of
/// main, `ruling=none` over a table with no row, no build makes a grant.
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
    let Some(row) = ruling.row(tier) else {
        return Err(LeaveCause::Ruling);
    };
    if tier > ceiling {
        Err(LeaveCause::Ceiling)
    } else if parts.iter().any(|part| *part != PartKind::Text) {
        Err(LeaveCause::Part)
    } else if row.origin() != origin {
        Err(LeaveCause::Origin)
    } else if consent == Consent::Missing {
        Err(LeaveCause::Consent)
    } else {
        Ok(LeaveGrant {
            _sealed: Seal,
            row: row.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release;

    const LOW: &str = "https://low.invalid";
    const TOP: &str = "https://top.invalid";
    const TABLE: &str = concat!(
        "tier\torigin\tmodel\tadapter\tsource\tdate\n",
        "low_cost_hosted\thttps://low.invalid\tlow-model\tadapter-a\tfixture:test\t2026-11-22\n",
        "frontier\thttps://top.invalid\ttop-model\tadapter-b\tfixture:test\t2026-11-22\n",
    );
    const HOSTED: [Tier; 2] = [Tier::LowCostHosted, Tier::Frontier];
    const NOT_TEXT: [PartKind; 4] = [
        PartKind::Image,
        PartKind::FileRef,
        PartKind::Url,
        PartKind::StoredConversation,
    ];

    fn hosted_by(table: &str) -> Ruling {
        Ruling::read_with_table("ruling=hosted", table)
    }

    fn open() -> Ruling {
        hosted_by(TABLE)
    }

    /// The hosted ruling of `TABLE` without the row of `tier`.
    fn without(tier: Tier) -> Ruling {
        let row = TABLE
            .lines()
            .find(|line| line.starts_with(&format!("{}\t", tier.as_str())))
            .expect("a row of the tier");
        let ruling = hosted_by(&TABLE.replace(&format!("{row}\n"), ""));
        assert_eq!(ruling.rows().len(), 1, "{tier:?}");
        ruling
    }

    fn origin(tier: Tier) -> &'static str {
        match tier {
            Tier::LowCostHosted => LOW,
            _ => TOP,
        }
    }

    fn other(tier: Tier) -> Tier {
        match tier {
            Tier::LowCostHosted => Tier::Frontier,
            _ => Tier::LowCostHosted,
        }
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
            for ruling in [release::ruling(), without(tier)] {
                assert_eq!(
                    answer(
                        &ruling,
                        tier,
                        Tier::Frontier,
                        &[PartKind::Text],
                        origin(tier),
                        Consent::Recorded,
                    ),
                    Err(LeaveCause::Ruling),
                    "{tier:?} {ruling:?}"
                );
            }
            assert_eq!(
                answer(
                    &without(other(tier)),
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    origin(tier),
                    Consent::Recorded,
                ),
                Ok(())
            );
            assert_eq!(
                hosted(tier, &[PartKind::Text], origin(tier), Consent::Recorded),
                Ok(())
            );
        }
    }

    #[test]
    fn refusal_leave_local() {
        for ruling in [release::ruling(), open(), without(Tier::Frontier)] {
            for ceiling in Tier::ALL {
                for consent in Consent::ALL {
                    for parts in [&[PartKind::Text][..], &[PartKind::Image], &[]] {
                        for origin in [LOW, TOP, "example.invalid:443"] {
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
                    origin(tier),
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
                    origin(tier),
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
                        hosted(tier, &parts, origin(tier), Consent::Recorded),
                        Err(LeaveCause::Part),
                        "{tier:?} {parts:?}"
                    );
                }
            }
            assert_eq!(
                hosted(tier, &PartKind::ALL, origin(tier), Consent::Recorded),
                Err(LeaveCause::Part)
            );
            for parts in [
                &[PartKind::Text][..],
                &[PartKind::Text, PartKind::Text],
                &[],
            ] {
                assert_eq!(
                    hosted(tier, parts, origin(tier), Consent::Recorded),
                    Ok(()),
                    "{parts:?}"
                );
            }
        }
    }

    #[test]
    fn refusal_leave_origin() {
        for tier in HOSTED {
            let own = origin(tier);
            let near = [
                origin(other(tier)).to_owned(),
                format!("{own}/"),
                format!("{own}:443"),
                format!("{own} "),
                format!(" {own}"),
                format!("{own}."),
                own.to_uppercase(),
                own.replacen("https://", "http://", 1),
                own.replacen("https://", "", 1),
                own[..own.len() - 1].to_owned(),
                "127.0.0.1:1".to_owned(),
                "http://127.0.0.1:1".to_owned(),
                "example.invalid:443".to_owned(),
                String::new(),
            ];
            for asked in &near {
                assert_eq!(
                    hosted(tier, &[PartKind::Text], asked, Consent::Recorded),
                    Err(LeaveCause::Origin),
                    "{tier:?} {asked:?}"
                );
            }
            assert_eq!(
                hosted(tier, &[PartKind::Text], own, Consent::Recorded),
                Ok(()),
                "{tier:?}"
            );
        }
        let shared = hosted_by(&TABLE.replace(TOP, LOW));
        for tier in HOSTED {
            assert_eq!(
                answer(
                    &shared,
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    LOW,
                    Consent::Recorded
                ),
                Ok(()),
                "{tier:?}"
            );
            assert_eq!(
                answer(
                    &shared,
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    TOP,
                    Consent::Recorded
                ),
                Err(LeaveCause::Origin),
                "{tier:?}"
            );
        }
    }

    #[test]
    fn a_grant_names_the_row_of_its_origin() {
        let ruling = open();
        for (tier, model, adapter) in [
            (Tier::LowCostHosted, "low-model", "adapter-a"),
            (Tier::Frontier, "top-model", "adapter-b"),
        ] {
            assert_eq!(
                answer(
                    &ruling,
                    tier,
                    Tier::Frontier,
                    &[PartKind::Text],
                    origin(other(tier)),
                    Consent::Recorded
                ),
                Err(LeaveCause::Origin),
                "{tier:?}"
            );
            let made = grant(
                &ruling,
                tier,
                Tier::Frontier,
                &[PartKind::Text],
                origin(tier),
                Consent::Recorded,
            )
            .expect("a grant for the origin of the row of its tier");
            let row = made.row();
            assert_eq!(
                (row.tier(), row.origin(), row.model(), row.adapter()),
                (tier, origin(tier), model, adapter)
            );
            assert_eq!(Some(row), ruling.row(tier));
        }
    }

    #[test]
    fn refusal_leave_consent() {
        for tier in HOSTED {
            assert_eq!(
                hosted(tier, &[PartKind::Text], origin(tier), Consent::Missing),
                Err(LeaveCause::Consent),
                "{tier:?}"
            );
            assert_eq!(
                hosted(tier, &[], origin(tier), Consent::Missing),
                Err(LeaveCause::Consent),
                "{tier:?}"
            );
            assert_eq!(
                hosted(tier, &[PartKind::Text], origin(tier), Consent::Recorded),
                Ok(())
            );
        }
    }

    #[test]
    fn leave_rules_in_order() {
        let none = release::ruling();
        for tier in HOSTED {
            let below = Tier::ALL[tier as usize - 1];
            let missing_row = without(tier);
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
                    origin(tier),
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
                (
                    &missing_row,
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
        let field = ["_", "sealed"].concat();
        let include_word = ["incl", "ude"].concat();
        let files = crate_sources();
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert!(
            names.contains(&"leave.rs") && names.contains(&"lib.rs"),
            "{names:?}"
        );
        for (name, text) in &files {
            for attribute in attributes(text) {
                assert!(
                    !words(&attribute).any(|word| word == "path"),
                    "{name}: #[{attribute}]"
                );
            }
            assert!(!words(text).any(|word| word == include_word), "{name}");
            if name != "leave.rs" {
                assert!(!text.contains(&field), "{name} names the private field");
            }
        }
    }

    #[test]
    fn a_grant_is_its_row_and_a_seal_of_no_size() {
        assert_eq!(std::mem::size_of::<Seal>(), 0);
        assert_eq!(
            std::mem::size_of::<LeaveGrant>(),
            std::mem::size_of::<HostRow>()
        );
    }

    fn crate_sources() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut dirs = vec![root.clone()];
        let mut files = Vec::new();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("a directory under src") {
                let file = entry.expect("an entry under src").path();
                if file.is_dir() {
                    dirs.push(file);
                } else if file.extension().is_some_and(|ext| ext == "rs") {
                    let name = file.strip_prefix(&root).expect("under src");
                    let text = std::fs::read_to_string(&file).expect("a source file");
                    files.push((name.to_string_lossy().replace('\\', "/"), text));
                }
            }
        }
        files.sort();
        files
    }

    fn words(text: &str) -> impl Iterator<Item = &str> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| !word.is_empty())
    }

    fn attributes(text: &str) -> Vec<String> {
        let packed: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        let mut found = Vec::new();
        for start in 0..packed.len() {
            let open = match (packed[start], packed.get(start + 1), packed.get(start + 2)) {
                ('#', Some('['), _) => start + 1,
                ('#', Some('!'), Some('[')) => start + 2,
                _ => continue,
            };
            let mut depth = 0;
            for (at, c) in packed.iter().enumerate().skip(open) {
                match c {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            found.push(packed[open + 1..at].iter().collect());
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        found
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
