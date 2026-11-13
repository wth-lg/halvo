mod support;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Action, Class, Limits, RunEvent};
use halvo_coord_app::{Attention, Hub};
use support::{agent, ask, started, world, Counts, Dialogs, DAY, T0};

#[test]
fn the_count_of_agents_under_needs_you_reaches_the_trays_sink_at_each_change() {
    let counts = Counts::default();
    let dialogs = Dialogs::default();
    let hub = Hub::new(
        world(2, Limits::PRODUCT),
        Limits::PRODUCT,
        counts.sink(),
        dialogs.boxed(),
    );
    let c = hub.coordinator();
    c.tick();
    assert_eq!(
        counts.all().last(),
        Some(&0),
        "two agents, none needs the person"
    );
    c.control(&agent(0), Action::RunNow).unwrap();
    let run = started(&hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    c.on_run(&run, ask(&run, 0, Class::Payment)).unwrap();
    assert_eq!(counts.all().last(), Some(&1));
    let item = c.snapshot().items[0].item().clone();
    c.on_screen(&item);
    c.ports()
        .inner()
        .with(|p| p.set_now(T0 + Limits::PRODUCT.arm_ms));
    c.tick();
    assert_eq!(c.answer_native(&item, ConfirmAnswer::Allow), Ok(()));
    assert_eq!(counts.all().last(), Some(&0));
}

#[test]
fn the_tray_shows_the_count_as_i10_3_asks_on_each_system() {
    assert_eq!(
        Attention::of(0),
        Attention {
            count: 0,
            variant: false,
            tooltip: None,
            title: None,
        }
    );
    let one = Attention::of(1);
    assert!(
        one.variant,
        "one agent that needs the person shows the variant"
    );
    assert_eq!(one.tooltip.as_deref(), Some("Agents that need you: 1"));
    let three = Attention::of(3);
    assert!(three.variant);
    assert_eq!(three.tooltip.as_deref(), Some("Agents that need you: 3"));
    let title = if cfg!(target_os = "macos") {
        Some("3")
    } else {
        None
    };
    assert_eq!(three.title.as_deref(), title);
}

#[test]
fn an_agent_under_problem_does_not_count_for_the_tray() {
    let counts = Counts::default();
    let hub = Hub::new(
        support::world_ending(
            &[
                halvo_coord::types::RunEnd::Completed,
                halvo_coord::types::RunEnd::Failed,
            ],
            Limits::PRODUCT,
        ),
        Limits::PRODUCT,
        counts.sink(),
        Dialogs::default().boxed(),
    );
    hub.coordinator().ports().inner().with(|p| {
        p.events.borrow_mut().push(halvo_coord::types::Event {
            workflow: support::flow(1),
            agent: agent(1),
            seq: std::num::NonZeroU32::new(3).unwrap(),
            day: DAY - 1,
            at: 0,
            what: halvo_coord::types::What::HoldSet {
                cause: halvo_coord::types::HoldSetCause::Failures,
            },
        })
    });
    let c = hub.coordinator();
    c.tick();
    let status = |n: usize| {
        c.snapshot()
            .rows
            .iter()
            .find(|r| r.agent == agent(n))
            .map(|r| r.status)
    };
    assert_eq!(status(1), Some(halvo_coord::types::Status::Problem));
    assert_eq!(counts.all().last(), Some(&0));
    c.control(&agent(0), Action::RunNow).unwrap();
    let run = started(&hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    c.on_run(&run, ask(&run, 0, Class::Payment)).unwrap();
    assert_eq!(status(0), Some(halvo_coord::types::Status::NeedsYou));
    assert_eq!(
        counts.all().last(),
        Some(&1),
        "one under needs_you, one under problem"
    );
}
