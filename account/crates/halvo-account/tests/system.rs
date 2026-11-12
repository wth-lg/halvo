use halvo_account::flow::Random;
use halvo_account::system::SystemRandom;

#[test]
fn the_system_gives_new_random_bytes_at_each_fill() {
    let mut first = [0; 48];
    let mut second = [0; 48];
    SystemRandom.fill(&mut first).expect("random bytes");
    SystemRandom.fill(&mut second).expect("random bytes");
    assert_ne!(first, second);
    assert_ne!(first, [0; 48]);
    assert_ne!(first[..16], first[16..32]);
}
