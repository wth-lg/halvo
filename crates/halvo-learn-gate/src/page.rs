/// A role of a control on a page (contracts.md L9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Link,
    Textbox,
    Checkbox,
    Combobox,
    Text,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Button,
        Role::Link,
        Role::Textbox,
        Role::Checkbox,
        Role::Combobox,
        Role::Text,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Link => "link",
            Role::Textbox => "textbox",
            Role::Checkbox => "checkbox",
            Role::Combobox => "combobox",
            Role::Text => "text",
        }
    }

    pub fn read(text: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.as_str() == text)
    }
}

/// One control of a page: `id` is the harness's identity of it, which no locator reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub id: String,
    pub role: Role,
    pub name: String,
    pub test_id: String,
    pub passes: Vec<u32>,
}

/// A way to find a control: by its test id, or by its role with its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Locator {
    TestId(String),
    Role(Role, String),
}

impl Locator {
    pub fn role_of(control: &Control) -> Locator {
        Locator::Role(control.role, control.name.clone())
    }

    fn matches(&self, control: &Control) -> bool {
        match self {
            Locator::TestId(test_id) => &control.test_id == test_id,
            Locator::Role(role, name) => control.role == *role && &control.name == name,
        }
    }

    /// The one control of the page that the locator matches, or none when it matches none or several (L9.2).
    pub fn resolve<'p>(&self, page: &'p [Control]) -> Option<&'p Control> {
        let mut matched = None;
        for control in page {
            if self.matches(control) {
                if matched.is_some() {
                    return None;
                }
                matched = Some(control);
            }
        }
        matched
    }
}

/// The two compiled locators of a step made from the page of the first occurrence: the intended control's test
/// id, then its role with its name (L9.2).
pub fn compiled_locators(control: &Control) -> [Locator; 2] {
    [
        Locator::TestId(control.test_id.clone()),
        Locator::role_of(control),
    ]
}

pub fn control<'p>(page: &'p [Control], id: &str) -> Option<&'p Control> {
    page.iter().find(|c| c.id == id)
}

/// What one step did on one occurrence (L9.2), as the harness counts it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepPlay {
    /// The step failed once with the cause `timeout` before any locator was tried.
    pub timeout: bool,
    /// A served locator led the step to a control on which its condition held.
    pub ok: bool,
    /// The served locators did not: the step failed with `step_failed` and the repairer was asked.
    pub failed: bool,
    /// The step acted at least once on a control that is not its intended control.
    pub wrong: bool,
    /// The repaired control on which the condition held, when the step was recovered.
    pub recovered: Option<Control>,
}

impl StepPlay {
    pub fn done(&self) -> bool {
        self.ok || self.recovered.is_some()
    }
}

/// Plays a step of number `n` aimed at `intended` with the served locators, tried in order, on the page, and then
/// with the controls that the repairer names, in order, when the step failed.
pub fn play_step(
    page: &[Control],
    n: u32,
    intended: &str,
    flaky: bool,
    served: &[Locator],
    repair: &[String],
) -> StepPlay {
    let holds = |c: &Control| c.id == intended || c.passes.contains(&n);
    let mut play = StepPlay {
        timeout: flaky,
        ..StepPlay::default()
    };
    if let Some(acted) = served.iter().find_map(|l| l.resolve(page)) {
        play.wrong = acted.id != intended;
        if holds(acted) {
            play.ok = true;
            return play;
        }
    }
    play.failed = true;
    for id in repair {
        let Some(acted) = control(page, id) else {
            continue;
        };
        play.wrong |= acted.id != intended;
        if holds(acted) {
            play.recovered = Some(acted.clone());
            break;
        }
    }
    play
}
