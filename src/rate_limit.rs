//! Global sliding-window rate limiting for outgoing `JOIN` and `PART` commands.
//!
//! # Twitch IRC rate limits overview
//!
//! Twitch enforces rate limits across all connections belonging to a user or IP, so pacing `JOIN`
//! and `PART` commands only per connection is insufficient when multiple connections initialize or
//! rebalance at once.
//!
//! ## Published authenticated limits
//! From Twitch's Chat & IRC documentation
//! (<https://dev.twitch.tv/docs/chat/>, <https://dev.twitch.tv/docs/chat/irc/>):
//! - **Normal authenticated accounts:**
//!   - **Join attempts:** `20` per `10s` per user.
//!   - **Authentication attempts (IRC):** `20` per `10s` per user.
//!   - **Concurrent joined channels:** `100` channels per user account (exemptions apply for
//!     broadcaster/moderator rooms, broadcaster-authorized `channel:bot` EventSub subscriptions
//!     with an App Access Token, and Shared Chat).
//! - **Verified bot accounts:**
//!   - **Join attempts:** `2,000` per `10s` per user.
//!   - **Authentication attempts (IRC):** `200` per `10s` per user.
//!
//! ## Empirically measured anonymous (`justinfan`) limits
//! Measured empirically against `irc.chat.twitch.tv:6697` (TLS) using anonymous `justinfan` logins:
//! - **`JOIN` rate & burst limit:** **`1,000` `JOIN`s per `10s` per IP** in a rolling ~10s window,
//!   shared across *all* anonymous connections and even across different `justinfan<N>` nicks from
//!   the same IP. Excess `JOIN`s are silently dropped without any `NOTICE`, error, or disconnect
//!   (tested up to `2,000` on 1 connection and `3,000` across `25` connections in `<2 ms`, each
//!   confirming exactly `1,000`).
//!   - **Per-connection processing cadence:** `JOIN` confirmations return linearly at `~5.5–6.1 ms`
//!     per channel (`~170–185 JOIN/s`) on a single connection and scale horizontally across
//!     connections (`~900 JOIN/s` aggregate across `5` connections) until hitting the `1,000 / 10s`
//!     per-IP ceiling.
//!   - **Duplicates & non-existent channels:** Duplicate `JOIN`s echo and consume `JOIN` budget.
//!     Non-existent channels receive no echo or error, but still cost `~5.5 ms` of server-side
//!     `JOIN` processing before subsequent `JOIN`s are handled. Comma-separated `JOIN #a,#b,...`
//!     commands are supported and execute at the same per-channel cadence.
//!   - **Concurrent channels:** No `100`-channel cap was enforced on anonymous `justinfan` sessions
//!     (`763` unique channels confirmed on a single connection; `503` offline channels remained
//!     connected and responsive over a 90s soak).
//! - **Authentication / connect rate:** **`100` logins per `10s` per IP** (identical for repeated
//!   `justinfan12345` and randomized `justinfan<N>` nicks). TCP+TLS continues connecting cleanly,
//!   while attempt `#101+` receives `:tmi.twitch.tv NOTICE * :Login unsuccessful` without `001`/`376`
//!   until the 10s window turns over.
//! - **`PART` rate & isolation:** `PART` uses a separate bucket from `JOIN` (`20/20` joined `PART`s
//!   confirmed immediately even directly after exhausting the `1,000 JOIN / 10s` quota). Echoes for
//!   `200` joined `PART`s completed in `178 ms` (`~0.89 ms/PART`); `PART`s for unjoined channels are
//!   silently ignored.
//! - **Per-IP edge flood breaker:** Bursts up to `25` concurrent connections / `3,000 JOIN`s did
//!   not sever sockets, whereas large unpaced client-wide bursts across `~1,700–2,350` sockets (tens
//!   of thousands of `JOIN`/`PART` commands in `<1 s`) trip an IP-level edge flood breaker that
//!   simultaneously closes all open connections (EOF) and rejects new logins for `5–10s`.

use std::collections::VecDeque;
use std::fmt;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant, sleep_until};

#[derive(Debug)]
struct SlidingWindowInner {
    max_commands: usize,
    window: Duration,
    min_interval: Duration,
    state: Mutex<VecDeque<Instant>>,
}

/// Async FIFO sliding-window rate limiter shared across all connections of a
/// [`TwitchIRCClient`](crate::TwitchIRCClient) (or multiple clients sharing the same IP/account).
///
/// Built-in presets reflect Twitch's published limits for authenticated/verified accounts and
/// empirically measured limits for anonymous (`justinfan`) connections:
/// - [`RateLimiter::twitch_anonymous_join`]
/// - [`RateLimiter::twitch_authenticated_join`]
/// - [`RateLimiter::twitch_verified_bot_join`]
/// - [`RateLimiter::twitch_part`]
/// - [`RateLimiter::unlimited`]
pub struct RateLimiter {
    inner: Option<SlidingWindowInner>,
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            None => f
                .debug_struct("RateLimiter")
                .field("unlimited", &true)
                .finish(),
            Some(inner) => f
                .debug_struct("RateLimiter")
                .field("max_commands", &inner.max_commands)
                .field("window", &inner.window)
                .field("min_interval", &inner.min_interval)
                .finish(),
        }
    }
}

impl RateLimiter {
    /// Creates a strict sliding-window rate limiter allowing at most `max_commands` permits within
    /// any rolling `window`.
    ///
    /// If `window` is zero, returns [`RateLimiter::unlimited`].
    ///
    /// # Panics
    ///
    /// Panics if `max_commands == 0`.
    #[must_use]
    pub fn new(max_commands: usize, window: Duration) -> Self {
        assert!(max_commands > 0, "max_commands must be at least 1");
        if window.is_zero() {
            return Self::unlimited();
        }
        RateLimiter {
            inner: Some(SlidingWindowInner {
                max_commands,
                window,
                min_interval: Duration::ZERO,
                state: Mutex::new(VecDeque::with_capacity(max_commands.min(1024))),
            }),
        }
    }

    /// Enforces a minimum spacing between consecutive permits granted by this limiter to smooth
    /// micro-bursts across many concurrent connections.
    ///
    /// Has no effect on [`RateLimiter::unlimited`].
    #[must_use]
    pub fn with_min_interval(mut self, min_interval: Duration) -> Self {
        if let Some(ref mut inner) = self.inner {
            inner.min_interval = min_interval;
        }
        self
    }

    /// Creates a disabled rate limiter whose [`RateLimiter::acquire`] completes immediately.
    #[must_use]
    pub fn unlimited() -> Self {
        RateLimiter { inner: None }
    }

    /// Default `JOIN` limiter for **anonymous (`justinfan`)** clients on a single IP: `900` `JOIN`s
    /// per `10.5s` with a `5 ms` minimum spacing between commands.
    ///
    /// Leaves 10% budget headroom and `500 ms` window margin under the empirically measured
    /// `1,000 JOINs / 10s` per-IP limit, matching Twitch's ~5.5 ms/JOIN single-connection cadence
    /// while pacing bursts across all open connections.
    #[must_use]
    pub fn twitch_anonymous_join() -> Self {
        Self::new(900, Duration::from_millis(10_500)).with_min_interval(Duration::from_millis(5))
    }

    /// Default `JOIN` limiter for **normal authenticated** Twitch accounts: `20` `JOIN`s per `10.5s`.
    ///
    /// Matches Twitch's published `20 join attempts per 10 seconds per user` limit plus `500 ms`
    /// margin for RTT/clock jitter.
    #[must_use]
    pub fn twitch_authenticated_join() -> Self {
        Self::new(20, Duration::from_millis(10_500))
    }

    /// Preset `JOIN` limiter for **verified bot** accounts: `2,000` `JOIN`s per `10.5s` with a
    /// `2 ms` minimum spacing between commands.
    ///
    /// Matches Twitch's published `2,000 join attempts per 10 seconds per user` limit plus `500 ms`
    /// margin for RTT/clock jitter while smoothing multi-connection micro-bursts.
    #[must_use]
    pub fn twitch_verified_bot_join() -> Self {
        Self::new(2_000, Duration::from_millis(10_500)).with_min_interval(Duration::from_millis(2))
    }

    /// Default `PART` limiter shared across the client: `500` `PART`s per `10s` with a `2 ms`
    /// minimum spacing.
    ///
    /// `PART` uses a separate server-side bucket from `JOIN` (`200` `PART`s complete in ~178 ms
    /// even after exhausting `JOIN`), so this preset keeps normal channel pruning fast while
    /// preventing tens of thousands of simultaneous `PART`s from tripping Twitch's edge flood
    /// breaker.
    #[must_use]
    pub fn twitch_part() -> Self {
        Self::new(500, Duration::from_secs(10)).with_min_interval(Duration::from_millis(2))
    }

    /// Returns the maximum number of permits allowed per rolling window, or `None` if unlimited.
    #[must_use]
    pub fn max_commands(&self) -> Option<usize> {
        self.inner.as_ref().map(|i| i.max_commands)
    }

    /// Returns the rolling window duration (`Duration::ZERO` if unlimited).
    #[must_use]
    pub fn window(&self) -> Duration {
        self.inner.as_ref().map_or(Duration::ZERO, |i| i.window)
    }

    /// Returns the minimum spacing enforced between consecutive permits (`Duration::ZERO` if none).
    #[must_use]
    pub fn min_interval(&self) -> Duration {
        self.inner
            .as_ref()
            .map_or(Duration::ZERO, |i| i.min_interval)
    }

    /// Waits in strict FIFO order until sending one command is permitted by both the sliding window
    /// cap and `min_interval`, then records this permit.
    ///
    /// Cancel-safe: if the returned future is dropped before completing (for example when a
    /// connection closes or is preempted), no permit is consumed.
    pub async fn acquire(&self) {
        let Some(ref inner) = self.inner else {
            return;
        };

        let mut timestamps = inner.state.lock().await;
        loop {
            let now = Instant::now();
            while let Some(&front) = timestamps.front() {
                if now.saturating_duration_since(front) >= inner.window {
                    timestamps.pop_front();
                } else {
                    break;
                }
            }

            let mut sleep_target: Option<Instant> = None;
            if timestamps.len() >= inner.max_commands
                && let Some(&oldest) = timestamps.front()
            {
                sleep_target = Some(oldest + inner.window);
            }

            if !inner.min_interval.is_zero()
                && let Some(&last) = timestamps.back()
            {
                let next_allowed = last + inner.min_interval;
                sleep_target = Some(match sleep_target {
                    Some(existing) if existing >= next_allowed => existing,
                    _ => next_allowed,
                });
            }

            if let Some(target) = sleep_target
                && target > now
            {
                sleep_until(target).await;
                continue;
            }

            timestamps.push_back(now);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;
    use crate::ClientConfig;
    use crate::login::StaticLoginCredentials;
    use std::sync::Arc;
    use tokio::time::{Duration, Instant};

    #[tokio::test]
    async fn unlimited_grants_immediately() {
        let limiter = RateLimiter::unlimited();
        assert_eq!(limiter.max_commands(), None);
        assert_eq!(limiter.window(), Duration::ZERO);
        assert_eq!(limiter.min_interval(), Duration::ZERO);

        for _ in 0..100 {
            limiter.acquire().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn sliding_window_enforces_exact_cap_and_staggered_refill() {
        let limiter = RateLimiter::new(3, Duration::from_secs(10));
        let start = Instant::now();

        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);

        tokio::time::advance(Duration::from_secs(2)).await;
        limiter.acquire().await;
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(2));

        // 4th must wait for the 1st permit (recorded at t=0) to expire at t=10s.
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(10));

        // 5th must wait for the 2nd permit (recorded at t=2s) to expire at t=12s,
        // and 6th uses the 3rd permit slot that also expired at t=12s.
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(12));
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(12));
    }

    #[tokio::test(start_paused = true)]
    async fn min_interval_smooths_bursts() {
        let limiter = RateLimiter::new(10, Duration::from_secs(10))
            .with_min_interval(Duration::from_millis(25));
        let start = Instant::now();

        for i in 0..4 {
            limiter.acquire().await;
            assert_eq!(start.elapsed(), Duration::from_millis(25 * i));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_waiter_does_not_leak_permit() {
        let limiter = Arc::new(RateLimiter::new(2, Duration::from_secs(10)));
        let start = Instant::now();

        limiter.acquire().await;
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);

        // Start a waiter and drop it before the 10s window expires.
        let waited_limiter = Arc::clone(&limiter);
        let cancelled = tokio::select! {
            () = waited_limiter.acquire() => false,
            () = tokio::time::sleep(Duration::from_secs(3)) => true,
        };
        assert!(cancelled);
        assert_eq!(start.elapsed(), Duration::from_secs(3));

        // Once the original two permits expire at t=10s, two full permits must still be free.
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(10));
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }

    #[test]
    fn client_config_selects_expected_default_limiters_for_anonymous_and_authenticated() {
        let anon_cfg = ClientConfig::default();
        assert_eq!(anon_cfg.join_rate_limiter.max_commands(), Some(900));
        assert_eq!(
            anon_cfg.join_rate_limiter.window(),
            Duration::from_millis(10_500)
        );
        assert_eq!(
            anon_cfg.join_rate_limiter.min_interval(),
            Duration::from_millis(5)
        );
        assert_eq!(anon_cfg.part_rate_limiter.max_commands(), Some(500));

        let auth_cfg = ClientConfig::new_simple(StaticLoginCredentials::new(
            "mybot".to_owned(),
            Some("oauth_token".to_owned()),
        ));
        assert_eq!(auth_cfg.join_rate_limiter.max_commands(), Some(20));
        assert_eq!(
            auth_cfg.join_rate_limiter.window(),
            Duration::from_millis(10_500)
        );
        assert_eq!(auth_cfg.join_rate_limiter.min_interval(), Duration::ZERO);
        assert_eq!(auth_cfg.part_rate_limiter.max_commands(), Some(500));

        let verified_limiter = RateLimiter::twitch_verified_bot_join();
        assert_eq!(verified_limiter.max_commands(), Some(2_000));
        assert_eq!(verified_limiter.window(), Duration::from_millis(10_500));
        assert_eq!(verified_limiter.min_interval(), Duration::from_millis(2));
    }
}
