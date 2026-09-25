// Tests were written by AI (Claude Opus 5), not reviewed by Author

use super::*;

#[test]
fn aws_wins_when_it_asks_for_slower_polling_than_configured() {
    assert_eq!(
        poll_interval(Duration::from_secs(5), Duration::from_secs(30)),
        Duration::from_secs(30)
    );
}

#[test]
fn the_configured_interval_wins_when_it_is_the_slower_of_the_two() {
    assert_eq!(
        poll_interval(Duration::from_secs(30), Duration::from_secs(5)),
        Duration::from_secs(30)
    );
}

#[test]
fn a_zero_from_either_side_never_yields_a_sleepless_poll() {
    assert_eq!(
        poll_interval(Duration::ZERO, Duration::ZERO),
        Duration::from_secs(1)
    );
}

#[test]
fn the_default_interval_clears_the_floor_untouched() {
    assert_eq!(
        poll_interval(DEFAULT_CREATE_TOKEN_RETRY_INTERVAL, Duration::ZERO),
        DEFAULT_CREATE_TOKEN_RETRY_INTERVAL
    );
}
