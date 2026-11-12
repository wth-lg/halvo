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

#[test]
fn every_one_of_the_48_bytes_is_filled() {
    let mut seen = [false; 48];
    for _ in 0..16 {
        let mut bytes = [0; 48];
        SystemRandom.fill(&mut bytes).expect("random bytes");
        for (at, byte) in bytes.iter().enumerate() {
            seen[at] |= *byte != 0;
        }
    }
    let unfilled: Vec<usize> = (0..48).filter(|at| !seen[*at]).collect();
    assert!(unfilled.is_empty(), "never filled: {unfilled:?}");
}
