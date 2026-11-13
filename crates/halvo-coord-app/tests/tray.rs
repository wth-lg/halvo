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
        .with(|p| p.set_clock(T0 + Limits::PRODUCT.arm_ms, DAY));
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
