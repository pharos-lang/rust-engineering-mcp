//! The one host clock the transport adapter hands to the project registry.
use rust_engineering_domain::{Clock, UnixSeconds};
use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock seconds since the Unix epoch. A host clock set before the epoch
/// reads as zero rather than failing a tool call: freshness is evidence the
/// registry assesses, never an authorization this adapter decides.
pub(super) struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wall_clock_advances_with_the_host() {
        let before = WallClock.now();
        assert!(before.0 > 1_700_000_000);
        assert!(WallClock.now().0 >= before.0);
    }
}
