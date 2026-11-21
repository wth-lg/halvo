//! Stage A: the category of an episode by the words of its titles and of its sites. Its rules are the table
//! `rule` below; it reads no file and no model.

use halvo_schema::{Category, ObservedStep, SeedCategory};

use crate::episode::Stage;

/// Stage A. It reads the masked title and the site of a step, and never its app, its input or its text.
///
/// The rule of a category names an episode when the words of one step's title and site's domain hold a topic word
/// of that rule, and the words of the same step or of another step of the episode hold a done word of it. Of the
/// categories whose rules name the episode, the one whose rule has its words in the most steps names it (a step
/// counts when it holds a topic word or a done word of that rule), and of two with equal counts the earlier one of
/// `Category::ALL`. An episode that no rule names is `Category::Other`.
pub struct StageA;

/// The words that name one seed category. A word of the task's topic alone names nothing: the rule names an
/// episode in which a step also holds a word of the task done. That step may be another one than the step of the
/// topic, because the page that shows a task done often does not repeat its topic.
struct Rule {
    /// The words of the task's topic. The rule needs one of them in a step of the episode, or none where the list
    /// is empty.
    topic: &'static [&'static str],
    /// The words of the task done: its act as a title names it (`paid`, `payment`, `pay`), or a thing that only
    /// the finished task leaves (`driver`, `outcome`). The rule needs one of them in a step of the episode. A word
    /// that also names what a person only looks at (`addendum`, `screening`, `sign` as in a sign-in) is none of
    /// them.
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

/// Whether the title of the step or the labels of its site's domain hold one word of the list, as a whole word and
/// without regard to case.
fn holds_one_of(step: &ObservedStep, list: &[&str]) -> bool {
    let title = step.title.as_ref().map_or("", |t| t.value.as_str());
    let site = step.site.as_ref().map_or("", |s| s.value.as_str());
    words(title)
        .chain(words(site))
        .any(|word| list.iter().any(|w| w.eq_ignore_ascii_case(word)))
}

impl Rule {
    /// The number of steps of the episode that hold a topic word or a done word of the rule, or 0 where the rule
    /// does not name the episode: where no step holds a done word, or no step a topic word while the topic list is
    /// not empty.
    fn steps(&self, steps: &[ObservedStep]) -> usize {
        let mut topic = self.topic.is_empty();
        let mut done = false;
        let mut count = 0;
        for step in steps {
            let has_topic = holds_one_of(step, self.topic);
            let has_done = holds_one_of(step, self.done);
            topic |= has_topic;
            done |= has_done;
            if has_topic || has_done {
                count += 1;
            }
        }
        if topic && done {
            count
        } else {
            0
        }
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
            let count = rule(seed).steps(steps);
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
    fn a_topic_and_its_done_word_in_two_steps_name_the_episode() {
        // The bill's page holds the topic words `gas` and `bill` and no done word; the page after the payment
        // holds the done word `payment` and no topic word.
        let episode = [
            step(
                "Your gas bill for March",
                Some("quayside-power.example"),
                None,
            ),
            step(
                "Payment received - thank you",
                Some("quayside-power.example"),
                None,
            ),
        ];
        assert_eq!(StageA.classify(&episode), Category::UtilityBillPayment);
        assert_eq!(StageA.classify(&episode[..1]), Category::Other);
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
