//! Stage A: the category of an episode by the words of its titles and of its sites, and, for an episode that no
//! rule names by words, Filling in a spreadsheet by the app ids and the input classes of its steps. Its rules are
//! the table `rule` and the fallback `spreadsheet_by_input` below; it reads no file and no model.

use halvo_schema::{Category, InputClass, ObservedStep, SeedCategory};

use crate::episode::Stage;

/// Stage A. Its word rules read the masked title and the site of a step; its fallback reads the app id, the input
/// class and the site of a step, and the masked title, which tells the episode's own window and, in a window with
/// no site, the words of a file's name. It never reads the text of a step.
///
/// The rule of a category names an episode when the words of one step's title and site's domain hold a topic word
/// of that rule, and the words of the same step or of another step of the episode hold a done word of it. Of the
/// categories whose rules name the episode, the one whose rule has its words in the most steps names it (a step
/// counts when it holds a topic word or a done word of that rule), and of two with equal counts the earlier one of
/// `Category::ALL`. An episode that no rule names is `Category::SpreadsheetFill` where the fallback
/// `spreadsheet_by_input` names it, and `Category::Other` where it does not.
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

/// The spreadsheet programs of the fallback, each by its app id as the store holds it: the bundle id on macOS, and
/// on Windows the file name of the executable, which `AppId::from_capture` cuts from the captured path. A step's
/// app is one of them when it equals an entry without regard to ASCII case, because a Windows path may write the
/// file name in either case. An entry is added as one more line here, with a row in the unit tests below that
/// names an episode of that app by its key records.
const SPREADSHEET_APPS: [&str; 3] = [
    "com.microsoft.Excel",
    "com.apple.iWork.Numbers",
    "EXCEL.EXE",
];

/// The words of a spreadsheet file's name in a title with which the fallback reads a window outside
/// `SPREADSHEET_APPS` (its reading I): the extensions alone, and only in the title of a window with no site, since
/// a page's title may hold `sheet` with no spreadsheet behind it.
const FILE_WORDS: [&str; 4] = ["xlsx", "xls", "ods", "csv"];

/// The fewest key records, of any class, with which the fallback names an episode (its reading I): the
/// coordinator's default of 2026-11-23, recorded for the owner.
const KEYS_MIN: usize = 5;

/// The fewest click records with which the fallback names an episode in a program of `SPREADSHEET_APPS` that
/// holds fewer than `KEYS_MIN` key records (its reading P): the coordinator's default of 2026-11-23, recorded for
/// the owner.
const CLICKS_MIN: usize = 10;

/// The condition of the fallback's reading P on the counts of an episode in a program of `SPREADSHEET_APPS`: fewer
/// than `KEYS_MIN` key records, and at least `CLICKS_MIN` click records. It stands in this one place, so that a
/// turn of what reading P asks of the key records is a turn of this line alone.
const fn read_by_clicks(keys: usize, clicks: usize) -> bool {
    keys < KEYS_MIN && clicks >= CLICKS_MIN
}

/// Whether an input class is the class of a key record. A key record whose class the store does not hold makes a
/// step with no input class, so it counts as no key record.
const fn is_key(class: InputClass) -> bool {
    match class {
        InputClass::Printable
        | InputClass::Editing
        | InputClass::Navigation
        | InputClass::Enter
        | InputClass::Tab
        | InputClass::Shortcut
        | InputClass::Other => true,
        InputClass::Click | InputClass::Scroll | InputClass::Drag | InputClass::Move => false,
    }
}

/// Whether `step` is of the window of `own` as the episode step keys a window: the same app and, where `own` has a
/// site, the same site, else no site and the same masked title. The app of a step is the app of its own record.
fn same_window(own: &ObservedStep, step: &ObservedStep) -> bool {
    own.app == step.app && own.site == step.site && (own.site.is_some() || own.title == step.title)
}

/// The fallback behind the word rules, which `StageA` asks only for an episode that no rule names: whether the
/// episode is the filling in of a spreadsheet by what the store holds of the steps of its own window. An episode
/// takes the key of its first record (`cut` in the episode step), so its own window is that of its first step,
/// and a step of another window that the episode took in counts for nothing here. It names the episode where
/// (I) the own window's app is one of `SPREADSHEET_APPS`, or the own window has no site and holds one of
/// `FILE_WORDS` in its title, and at least `KEYS_MIN` steps of the own window are key records; or where (P) the own
/// window's app is one of `SPREADSHEET_APPS` and the counts of its steps meet `read_by_clicks`: fewer than
/// `KEYS_MIN` key records and at least `CLICKS_MIN` click records. It reads no text.
fn spreadsheet_by_input(steps: &[ObservedStep]) -> bool {
    let Some(own) = steps.first() else {
        return false;
    };
    let in_table = own.app.as_ref().is_some_and(|app| {
        SPREADSHEET_APPS
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(app.value.as_str()))
    });
    let file = own.site.is_none() && holds_one_of(own, &FILE_WORDS);
    let classes: Vec<InputClass> = steps
        .iter()
        .filter(|step| same_window(own, step))
        .filter_map(|step| step.input.as_ref().map(|input| input.value))
        .collect();
    let keys = classes.iter().filter(|&&class| is_key(class)).count();
    let clicks = classes
        .iter()
        .filter(|&&class| class == InputClass::Click)
        .count();
    let by_keys = (in_table || file) && keys >= KEYS_MIN;
    let by_clicks = in_table && read_by_clicks(keys, clicks);
    by_keys || by_clicks
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
        if named == Category::Other && spreadsheet_by_input(steps) {
            return Category::SpreadsheetFill;
        }
        named
    }
}

#[cfg(test)]
mod tests {
    use halvo_schema::{
        AddressFieldLayer, AppId, FloorLayer, InputClass, SiteDomain, Tagged, Text, TextLayer,
    };

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

    /// The key classes, which the key records of `work` take in turn.
    const KEY_CLASSES: [InputClass; 7] = [
        InputClass::Printable,
        InputClass::Editing,
        InputClass::Navigation,
        InputClass::Enter,
        InputClass::Tab,
        InputClass::Shortcut,
        InputClass::Other,
    ];

    /// A step of the app `app` in a window titled `title`, with the input class `input`, as the episode step makes
    /// it from a title, key or pointer record: no site and no text.
    fn app_step(app: &str, title: &str, input: Option<InputClass>) -> ObservedStep {
        ObservedStep {
            app: Some(Tagged {
                value: AppId::parse(app).unwrap(),
                layer: FloorLayer::Floor,
            }),
            title: Some(Tagged {
                value: Text::parse(title).unwrap(),
                layer: FloorLayer::Floor,
            }),
            input: input.map(|value| Tagged {
                value,
                layer: FloorLayer::Floor,
            }),
            site: None,
            text: None,
        }
    }

    /// An episode in one window of `app` titled `title`: the step of its title record, then `keys` key records of
    /// the key classes in turn, then `clicks` click records.
    fn work(app: &str, title: &str, keys: usize, clicks: usize) -> Vec<ObservedStep> {
        let mut steps = vec![app_step(app, title, None)];
        steps.extend((0..keys).map(|n| app_step(app, title, Some(KEY_CLASSES[n % 7]))));
        steps.extend((0..clicks).map(|_| app_step(app, title, Some(InputClass::Click))));
        steps
    }

    #[test]
    fn a_numbers_window_with_five_keys_or_more_is_a_spreadsheet() {
        // Numbers and Excel for Mac title a workbook by its name, which holds no word of a rule here. A title that
        // a word rule names keeps its category in the same window.
        let numbers = "com.apple.iWork.Numbers";
        assert_eq!(
            StageA.classify(&work(numbers, "Budget", 6, 0)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work(numbers, "Budget", 5, 0)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work(numbers, "Budget", 4, 0)),
            Category::Other
        );
        let excel = "com.microsoft.Excel";
        assert_eq!(
            StageA.classify(&work(excel, "Budget", 6, 0)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work(excel, "Budget", 4, 0)),
            Category::Other
        );
        assert_eq!(
            StageA.classify(&work(numbers, "Lease renewal signed", 6, 0)),
            Category::LeaseRenewalOrChange
        );
    }

    #[test]
    fn excel_on_windows_with_ten_clicks_and_fewer_than_five_keys_is_a_spreadsheet() {
        // On Windows a key in Excel's grid may leave no record, so clicks stand for the work where fewer than five
        // key records stand, as where one key went into a dialog's field. The app id is compared without regard to
        // case.
        let title = "Budget.xlsx - Excel";
        assert_eq!(
            StageA.classify(&work("EXCEL.EXE", title, 0, 12)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work("EXCEL.EXE", title, 0, 10)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work("EXCEL.EXE", title, 0, 9)),
            Category::Other
        );
        assert_eq!(
            StageA.classify(&work("EXCEL.EXE", title, 1, 12)),
            Category::SpreadsheetFill
        );
        assert_eq!(
            StageA.classify(&work("excel.exe", title, 0, 12)),
            Category::SpreadsheetFill
        );
    }

    #[test]
    fn a_file_word_in_a_title_with_no_site_and_five_keys_is_a_spreadsheet() {
        // LibreOffice runs Calc and Writer under one app id, which is outside the table, so its window comes in by
        // the file word of its title. The same title on a step with a site, a title with `sheet` in place of a
        // file word, and clicks alone outside the table name nothing.
        let office = "org.libreoffice.script";
        let title = "budget.ods - LibreOffice Calc";
        assert_eq!(
            StageA.classify(&work(office, title, 6, 0)),
            Category::SpreadsheetFill
        );
        assert_eq!(StageA.classify(&work(office, title, 4, 0)), Category::Other);
        assert_eq!(
            StageA.classify(&work(office, title, 0, 12)),
            Category::Other
        );
        let mut on_a_site = work(office, title, 6, 0);
        for step in &mut on_a_site {
            step.site = Some(Tagged {
                value: SiteDomain::parse("tallybook.example").unwrap(),
                layer: AddressFieldLayer::AddressField,
            });
        }
        assert_eq!(StageA.classify(&on_a_site), Category::Other);
        assert_eq!(
            StageA.classify(&work("com.apple.Preview", "Cheat sheet", 6, 0)),
            Category::Other
        );
    }

    #[test]
    fn libreoffice_writer_with_six_keys_is_no_spreadsheet() {
        let office = "org.libreoffice.script";
        assert_eq!(
            StageA.classify(&work(office, "notes.odt - LibreOffice Writer", 6, 0)),
            Category::Other
        );
    }

    #[test]
    fn a_window_that_flickered_past_an_episode_counts_nothing_for_it() {
        // An episode of a ride page holds three key records of its own and a stretch of a workbook window with no
        // site and two key records, joined because the page came back within 20 s: no file word of the page's
        // own window and five key records in the episode name nothing. A Numbers episode with four key records of
        // its own and two typed into a window that flickered past is no spreadsheet either.
        let chromium = "org.chromium.Chromium";
        let page = "Where to - Swiftfare Rides";
        let on_the_page = |input| {
            let mut step = app_step(chromium, page, input);
            step.site = Some(Tagged {
                value: SiteDomain::parse("swiftfare-rides.example").unwrap(),
                layer: AddressFieldLayer::AddressField,
            });
            step
        };
        let mut ride = vec![on_the_page(None)];
        ride.extend((0..3).map(|n| on_the_page(Some(KEY_CLASSES[n]))));
        ride.extend(work(
            "org.libreoffice.script",
            "reading-list.ods - LibreOffice Calc",
            2,
            0,
        ));
        ride.push(on_the_page(None));
        assert_eq!(StageA.classify(&ride), Category::Other);

        let mut budget = work("com.apple.iWork.Numbers", "Budget", 4, 0);
        budget.extend(work("com.apple.TextEdit", "notes.txt", 2, 0));
        budget.push(app_step("com.apple.iWork.Numbers", "Budget", None));
        assert_eq!(StageA.classify(&budget), Category::Other);
    }
}
