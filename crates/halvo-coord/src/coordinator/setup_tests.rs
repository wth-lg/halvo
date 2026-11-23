use crate::items::{compose_setup, params, setup_on, SetupParam};
use crate::types::{AgentId, CandidateFrom, Category, ItemId, Limits, System};

/// S0 of the ruling of r4 U3.22 (the coordinator's log line 18737), to the letter.
const S0: &str = "Answer each question, then choose Use these answers.";

fn item() -> ItemId {
    ItemId::parse(&format!("{:032x}", 0x1e)).unwrap()
}

fn agent() -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa1)).unwrap()
}

/// The questions of `category`, each with one candidate where `lists`, else each a typed line.
fn asked(category: Category, lists: bool) -> Vec<SetupParam> {
    params(category)
        .iter()
        .map(|p| SetupParam {
            param: *p,
            candidates: if lists {
                vec![(
                    vec![format!("A value of {}", p.name())],
                    CandidateFrom::Records,
                )]
            } else {
                Vec::new()
            },
        })
        .collect()
}

/// r4 U3.22a sentence 1: above the questions of EVERY set-up item stands S0, in place of the
/// text that asked to choose one answer: the item of a category whose questions are lists and the
/// item of the spreadsheet agent, whose four questions are typed lines, each hold it byte for byte.
#[test]
fn u322a_s0_stands_above_the_questions_of_a_set_up_with_lists_and_of_the_spreadsheet_agent() {
    for (category, lists) in [
        (Category::UtilityBillPayment, true),
        (Category::SpreadsheetFill, false),
    ] {
        let item = compose_setup(
            &item(),
            &agent(),
            "An agent",
            &asked(category, lists),
            false,
            &Limits::PRODUCT,
        )
        .unwrap();
        assert_eq!(item.text(), S0, "{}", category.name());
    }
}

/// The hints of the ruling of r4 U3.22, to the letter, in the order of the spreadsheet agent's
/// questions (Which spreadsheet file, Which sheet, Which cells, Where the data comes from).
const HINTS: [(System, [&str; 4]); 2] = [
    (
        System::Mac,
        [
            "Type the full path of an Excel workbook (.xlsx), for example /Users/ana/Documents/Budget.xlsx.",
            "Type the sheet's name as its tab shows it, for example Sheet1.",
            "Type the cells to fill as A2:C11: the top left cell, a colon, the bottom right cell. The data must have exactly that many rows and columns.",
            "Type the full path of a text file in the workbook's folder, for example /Users/ana/Documents/receipts.csv. Each line is one row, with commas, semicolons or tabs between its values.",
        ],
    ),
    (
        System::Windows,
        [
            "Type the full path of an Excel workbook (.xlsx), for example C:\\Users\\ana\\Documents\\Budget.xlsx.",
            "Type the sheet's name as its tab shows it, for example Sheet1.",
            "Type the cells to fill as A2:C11: the top left cell, a colon, the bottom right cell. The data must have exactly that many rows and columns.",
            "Type the full path of a text file in the workbook's folder, for example C:\\Users\\ana\\Documents\\receipts.csv. Each line is one row, with commas, semicolons or tabs between its values.",
        ],
    ),
];

/// r4 U3.22a sentence 2: the item's reply holds ONE new key `hint` for each question, beside
/// `full` and `too_long`: on each system the four questions of the spreadsheet agent hint what to
/// type, byte for byte, with an example of that system where the answer is a path, and a question
/// with a list holds the key with the empty text.
#[test]
fn u322a_the_spreadsheet_agents_four_questions_hint_what_to_type_on_each_system_and_a_list_hints_nothing(
) {
    for (system, want) in HINTS {
        let sheet = setup_on(
            system,
            &item(),
            &agent(),
            "An agent",
            &asked(Category::SpreadsheetFill, false),
            None,
            &Limits::PRODUCT,
        )
        .unwrap();
        let hints: Vec<&str> = sheet.params().iter().map(|p| p.hint.as_str()).collect();
        assert_eq!(hints, want, "{system:?}");
        let listed = setup_on(
            system,
            &item(),
            &agent(),
            "An agent",
            &asked(Category::UtilityBillPayment, true),
            None,
            &Limits::PRODUCT,
        )
        .unwrap();
        let reply = serde_json::to_value(&listed).unwrap();
        for p in reply["params"].as_array().unwrap() {
            assert_eq!(p["hint"], "", "{system:?} {}", p["name"]);
        }
    }
}
