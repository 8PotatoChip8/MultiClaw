use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

/// First 429 sets delay to at least this value.
const INITIAL_BACKOFF_MS: u64 = 1000;
/// Maximum inter-request delay.
const MAX_DELAY_MS: u64 = 30_000;
/// After this many consecutive successes, reduce delay by 25%.
const SUCCESS_REDUCTION_INTERVAL: u64 = 5;

/// Adaptive rate limiter using AIMD (Additive Increase, Multiplicative Decrease).
///
/// Starts fully permissive (zero delay). When a 429 is observed, introduces and
/// doubles an inter-request delay. When requests succeed, gradually reduces the
/// delay back toward zero. This discovers the actual rate limit reactively without
/// needing to know it in advance.
#[derive(Clone)]
pub struct AdaptiveRateLimiter {
    inner: Arc<Mutex<State>>,
}

struct State {
    /// Current minimum delay between requests in milliseconds. 0 = no throttling.
    delay_ms: u64,
    /// When the last request was dispatched.
    last_request: Option<Instant>,
    /// Consecutive successes since last 429.
    consecutive_successes: u64,
    /// Total 429s observed (for logging).
    total_429s: u64,
}

impl AdaptiveRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                delay_ms: 0,
                last_request: None,
                consecutive_successes: 0,
                total_429s: 0,
            })),
        }
    }

    /// Wait until it's safe to send the next request.
    /// Call this BEFORE making an HTTP request.
    pub async fn wait_for_permit(&self) {
        let sleep_duration = {
            let mut state = self.inner.lock().await;
            if state.delay_ms == 0 {
                state.last_request = Some(Instant::now());
                return;
            }

            let required_delay = Duration::from_millis(state.delay_ms);
            let sleep_for = if let Some(last) = state.last_request {
                let elapsed = last.elapsed();
                if elapsed >= required_delay {
                    Duration::ZERO
                } else {
                    required_delay - elapsed
                }
            } else {
                Duration::ZERO
            };

            state.last_request = Some(Instant::now() + sleep_for);
            sleep_for
        };

        if !sleep_duration.is_zero() {
            tracing::debug!("Rate limiter: waiting {:?} before next request", sleep_duration);
            tokio::time::sleep(sleep_duration).await;
        }
    }

    /// Report that a request succeeded (non-429).
    pub async fn record_success(&self) {
        let mut state = self.inner.lock().await;
        if state.delay_ms == 0 {
            return;
        }

        state.consecutive_successes += 1;

        if state.consecutive_successes % SUCCESS_REDUCTION_INTERVAL == 0 {
            let old = state.delay_ms;
            state.delay_ms = state.delay_ms * 3 / 4; // Reduce by 25%
            if state.delay_ms < 50 {
                state.delay_ms = 0;
                tracing::info!(
                    "Rate limiter deactivated: back to full speed (after {} consecutive successes)",
                    state.consecutive_successes
                );
            } else {
                tracing::info!(
                    "Rate limiter relaxed: {}ms -> {}ms ({} consecutive successes)",
                    old, state.delay_ms, state.consecutive_successes
                );
            }
        }
    }

    /// Report that a 429 was received.
    pub async fn record_rate_limited(&self) {
        let mut state = self.inner.lock().await;
        state.total_429s += 1;
        state.consecutive_successes = 0;

        let old = state.delay_ms;
        if state.delay_ms == 0 {
            state.delay_ms = INITIAL_BACKOFF_MS;
        } else {
            state.delay_ms = (state.delay_ms * 2).min(MAX_DELAY_MS);
        }

        tracing::warn!(
            "Rate limited by upstream (429 #{}) — global delay: {}ms -> {}ms",
            state.total_429s, old, state.delay_ms
        );
    }
}
