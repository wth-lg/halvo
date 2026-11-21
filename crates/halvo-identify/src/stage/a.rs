//! Stage A: the category of an episode by the words of its titles and of its sites. Its rules are the table
//! `rule` below; it reads no file and no model.

use halvo_schema::{Category, ObservedStep, SeedCategory};

use crate::episode::Stage;

/// Stage A. It reads the masked title and the site of a step, and never its app, its input or its text.
///
/// A step holds the rule of a category when the words of its title and of its site's domain hold one topic word
/// and one done word of that rule. The category whose rule holds in the most steps of the episode names it, and
/// of two categories with equal counts the earlier one of `Category::ALL`. An episode in which no rule holds is
/// `Category::Other`.
pub struct StageA;

/// The words that name one seed category. A word of the task's topic alone names nothing: the rule holds in a
/// step that also holds a word of the task done.
struct Rule {
    /// The words of the task's topic. The rule needs one of them, or none where the list is empty.
    topic: &'static [&'static str],
    /// The words of the task done: its act as a title names it (`paid`, `payment`, `pay`), or a thing that only
    /// the finished task leaves (`driver`, `outcome`). The rule needs one of them. A word that also names what a
    /// person only looks at (`addendum`, `screening`, `sign` as in a sign-in) is none of them.
    done: &'static [&'static str],
}

/// The rule table: the rule of each seed category, in words of the task as a person's own titles hold them. No
/// rule holds the name or the domain of a service.
const fn rule(category: SeedCategory) -> Rule {
    match category {
        SeedCategory::UtilityBillPayment => Rule {
            topic: &[
                "bill",
                "bills",
                "invoice",
                "invoices",
                "electricity",
                "gas",
                "water",
                "broadband",
                "energy",
            ],
            done: &["pay", "paid", "payment"],
        },
        SeedCategory::TenantRent => Rule {
            topic: &["rent"],
            done: &["pay", "paid", "payment"],
        },
        SeedCategory::LeaseRenewalOrChange => Rule {
            topic: &["lease", "tenancy"],
            done: &[
                "renew", "renewed", "renewal", "extended", "amended", "signed", "export",
                "exported",
            ],
        },
        SeedCategory::TenantApplicationReview => Rule {
            topic: &["tenant", "tenants", "applicant", "applicants"],
            done: &[
                "approved", "declined", "rejected", "reviewed", "screened", "outcome",
            ],
        },
        SeedCategory::Email => Rule {
            topic: &["mail", "email", "inbox", "mailbox", "message", "messages"],
            done: &["sent", "replied", "forwarded", "archived"],
        },
        SeedCategory::SpreadsheetFill => Rule {
            topic: &[
                "spreadsheet",
                "spreadsheets",
                "sheet",
                "sheets",
                "ods",
                "xlsx",
                "xls",
                "csv",
            ],
            done: &["fill", "filled", "enter", "entered", "saved", "updated"],
        },
        SeedCategory::ShopOrder => Rule {
            topic: &["order"],
            done: &["placed", "confirmed"],
        },
        SeedCategory::CalendarCleanup => Rule {
            topic: &[
                "calendar", "planner", "event", "events", "meeting", "meetings",
            ],
            done: &["removed", "deleted", "cancelled", "canceled", "merged"],
        },
        SeedCategory::RideOrder => Rule {
            topic: &["ride", "rides", "trip", "trips", "taxi", "cab"],
            done: &["booked", "confirmed", "requested", "pickup", "driver"],
        },
        SeedCategory::DailyChartCheck => Rule {
            topic: &[],
            done: &["chart", "charts", "trend", "trends", "graph"],
        },
    }
}

/// The words of a text: its longest runs of letters and digits.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

impl Rule {
    /// Whether the title of the step or the labels of its site's domain hold, as whole words and without regard
    /// to case, what the rule needs.
    fn holds(&self, step: &ObservedStep) -> bool {
        let title = step.title.as_ref().map_or("", |t| t.value.as_str());
        let site = step.site.as_ref().map_or("", |s| s.value.as_str());
        let one_of = |list: &[&str]| {
            words(title)
                .chain(words(site))
                .any(|word| list.iter().any(|w| w.eq_ignore_ascii_case(word)))
        };
        (self.topic.is_empty() || one_of(self.topic)) && one_of(self.done)
    }
}

impl Stage for StageA {
    fn classify(&self, steps: &[ObservedStep]) -> Category {
        let mut named = Category::Other;
        let mut most = 0;
        for &category in Category::ALL {
            let Some(seed) = category.seed() else {
                continue;
            };
            let rule = rule(seed);
            let count = steps.iter().filter(|step| rule.holds(step)).count();
            if count > most {
                named = category;
                most = count;
            }
        }
        named
    }
}

#[cfg(test)]
mod tests {
    use halvo_schema::{AddressFieldLayer, FloorLayer, SiteDomain, Tagged, Text, TextLayer};

    use super::*;

    fn step(title: &str, site: Option<&str>, text: Option<&str>) -> ObservedStep {
        ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse(title).unwrap(),
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: site.map(|domain| Tagged {
                value: SiteDomain::parse(domain).unwrap(),
                layer: AddressFieldLayer::AddressField,
            }),
            text: text.map(|text| Tagged {
                value: Text::parse(text).unwrap(),
                layer: TextLayer::AccessibilityText,
            }),
        }
    }

    #[test]
    fn rules_name_each_category() {
        // The second row holds its rule words in capitals, and `Waterfall` in it is no `water`; the fifth row
        // holds its topic word, `mail`, in the site alone.
        let rows: [(&str, &str, Option<&str>, Category); 12] = [
            (
                "Water bill paid",
                "tidewell-supply.example",
                None,
                Category::UtilityBillPayment,
            ),
            (
                "RENT PAID - Waterfall Court",
                "ashgrove-homes.example",
                None,
                Category::TenantRent,
            ),
            (
                "Lease renewal signed",
                "fernbrook-estates.example",
                None,
                Category::LeaseRenewalOrChange,
            ),
            (
                "Tenant application declined: Mill Lane",
                "keyturn-lets.example",
                None,
                Category::TenantApplicationReview,
            ),
            (
                "Answer to the plumber sent",
                "quillpost-mail.example",
                None,
                Category::Email,
            ),
            (
                "Spreadsheet saved: household budget",
                "tallybook.example",
                None,
                Category::SpreadsheetFill,
            ),
            (
                "Your order has been placed",
                "larchmarket.example",
                None,
                Category::ShopOrder,
            ),
            (
                "Three old events deleted",
                "daybook-planner.example",
                None,
                Category::CalendarCleanup,
            ),
            (
                "Taxi booked for 7 am",
                "hopalong-cars.example",
                None,
                Category::RideOrder,
            ),
            (
                "Blood pressure trend",
                "vitalnote.example",
                None,
                Category::DailyChartCheck,
            ),
            (
                "Five day forecast",
                "fairsky.example",
                None,
                Category::Other,
            ),
            (
                "Home",
                "larchmarket.example",
                Some("Order placed"),
                Category::Other,
            ),
        ];
        let named: Vec<Category> = rows.iter().map(|row| row.3).collect();
        assert_eq!(&named[..11], Category::ALL, "one row a category, in order");
        for (title, site, text, category) in rows {
            assert_eq!(
                StageA.classify(&[step(title, Some(site), text)]),
                category,
                "{title} at {site}"
            );
        }
    }

    #[test]
    fn tie_goes_by_steps_then_order() {
        // `Mail folder Bills` holds topic words alone, so no rule; the second title holds the rule of
        // utility_bill_payment and the rule of email, the third the rule of email alone.
        let both = "Gas bill paid - message archived";
        let mail = [
            step("Mail folder Bills", None, None),
            step(both, None, None),
            step("Inbox - one answer sent", None, None),
        ];
        assert_eq!(StageA.classify(&mail), Category::Email);
        assert_eq!(
            StageA.classify(&[step(both, None, None)]),
            Category::UtilityBillPayment
        );
    }
}
