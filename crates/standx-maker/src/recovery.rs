//! Deterministic retry policy for live transport recovery.
//!
//! Transport availability is not itself a trading-safety invariant. The CLI
//! keeps the maker frozen while it retries; only cleanup, reconciliation, or
//! other proof-of-safety failures are terminal.

/// Keep repeated transport failures from producing a tight reconnect loop.
pub const MAX_RECOVERY_RETRY_BACKOFF_SECS: u64 = 60;

/// Return the delay before the next bounded reconnect round.
///
/// `failed_rounds` is one-based. The delay doubles for each failed round and
/// caps at [`MAX_RECOVERY_RETRY_BACKOFF_SECS`]. A zero base remains zero so the
/// pure policy is total; live CLI validation requires a positive base whenever
/// reconnect is enabled.
pub fn recovery_retry_delay_secs(base_secs: u64, failed_rounds: u32) -> u64 {
    let exponent = failed_rounds.saturating_sub(1).min(5);
    base_secs
        .saturating_mul(1_u64 << exponent)
        .min(MAX_RECOVERY_RETRY_BACKOFF_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_is_bounded_exponential_backoff() {
        let delays = (1..=8)
            .map(|round| recovery_retry_delay_secs(2, round))
            .collect::<Vec<_>>();

        assert_eq!(delays, vec![2, 4, 8, 16, 32, 60, 60, 60]);
    }

    #[test]
    fn retry_delay_saturates_large_inputs() {
        assert_eq!(recovery_retry_delay_secs(u64::MAX, u32::MAX), 60);
    }

    #[test]
    fn zero_base_has_zero_delay() {
        assert_eq!(recovery_retry_delay_secs(0, 1), 0);
    }
}
