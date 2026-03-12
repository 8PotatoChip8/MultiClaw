use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio::time::{Duration, Instant};

/// Maximum inter-request delay during backoff.
const MAX_DELAY_MS: u64 = 30_000;
/// First 429 sets delay to at least this value.
const INITIAL_BACKOFF_MS: u64 = 1000;
/// After this many consecutive successes, reduce delay by 25%.
const SUCCESS_REDUCTION_INTERVAL: u64 = 5;

/// Concurrency-aware rate limiter using a semaphore + AIMD backoff.
///
/// Allows up to `max_concurrent` requests to Ollama simultaneously.
/// When a 429 is received, introduces and doubles an inter-request delay.
/// When requests succeed, gradually reduces the delay back toward zero.
///
/// The concurrency limit can be adjusted at runtime via `set_max_concurrent()`
/// (used by the startup probe to auto-discover the Ollama tier limit).
#[derive(Clone)]
pub struct ConcurrentRateLimiter {
    semaphore: Arc<RwLock<Arc<Semaphore>>>,
    max_concurrent: Arc<RwLock<usize>>,
    backoff: Arc<Mutex<BackoffState>>,
}

struct BackoffState {
    /// Current delay (milliseconds) applied after acquiring the semaphore.
    /// 0 = no throttling beyond the concurrency limit.
    delay_ms: u64,
    /// When the last request was dispatched (for spacing under backoff).
    last_request: Option<Instant>,
    /// Consecutive successes since last 429.
    consecutive_successes: u64,
    /// Total 429s observed (for logging).
    total_429s: u64,
}

impl ConcurrentRateLimiter {
    /// Create a new limiter allowing `max_concurrent` simultaneous requests.
    /// Setting `max_concurrent` to 1 restores the old serial behavior.
    pub fn new(max_concurrent: usize) -> Self {
        tracing::info!(
            "ConcurrentRateLimiter initialized: max_concurrent={}",
            max_concurrent
        );
        Self {
            semaphore: Arc::new(RwLock::new(Arc::new(Semaphore::new(max_concurrent)))),
            max_concurrent: Arc::new(RwLock::new(max_concurrent)),
            backoff: Arc::new(Mutex::new(BackoffState {
                delay_ms: 0,
                last_request: None,
                consecutive_successes: 0,
                total_429s: 0,
            })),
        }
    }

    /// Adjust the concurrency limit at runtime. Replaces the inner semaphore.
    /// Safe to call during active traffic: `acquire()` clones the semaphore Arc,
    /// so in-flight requests hold the old semaphore and finish normally.
    /// New requests immediately see the updated limit.
    pub async fn set_max_concurrent(&self, new_max: usize) {
        let mut sem_guard = self.semaphore.write().await;
        let mut max_guard = self.max_concurrent.write().await;
        let old = *max_guard;
        *sem_guard = Arc::new(Semaphore::new(new_max));
        *max_guard = new_max;
        tracing::info!(
            "ConcurrentRateLimiter adjusted: {} -> {} concurrent permits",
            old, new_max
        );
    }

    /// Get the current concurrency limit.
    pub async fn get_max_concurrent(&self) -> usize {
        *self.max_concurrent.read().await
    }

    /// Acquire a permit to make an Ollama request.
    ///
    /// Blocks if all `max_concurrent` slots are in use. If the limiter is in
    /// backoff mode (due to recent 429s), applies additional delay after
    /// acquiring the semaphore permit.
    ///
    /// The returned guard must be held for the duration of the request.
    /// It is released automatically when dropped.
    pub async fn acquire(&self) -> ConcurrencyGuard {
        // Clone the current semaphore Arc (cheap — just a ref count bump).
        // This ensures we acquire from the semaphore that was active at call time,
        // even if set_max_concurrent() replaces it concurrently.
        let sem = self.semaphore.read().await.clone();

        // Wait for a concurrency slot
        let permit = sem.acquire_owned().await
            .expect("ConcurrentRateLimiter semaphore closed unexpectedly");

        // Apply backoff delay if active (protects against 429 storms)
        let sleep_duration = {
            let mut state = self.backoff.lock().await;
            if state.delay_ms == 0 {
                state.last_request = Some(Instant::now());
                Duration::ZERO
            } else {
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
            }
        };

        if !sleep_duration.is_zero() {
            tracing::debug!("Rate limiter: backoff delay {:?} before request", sleep_duration);
            tokio::time::sleep(sleep_duration).await;
        }

        ConcurrencyGuard { _permit: permit }
    }

    /// Report that a request succeeded (non-429).
    pub async fn record_success(&self) {
        let mut state = self.backoff.lock().await;
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
        let mut state = self.backoff.lock().await;
        state.total_429s += 1;
        state.consecutive_successes = 0;

        let old = state.delay_ms;
        if state.delay_ms == 0 {
            state.delay_ms = INITIAL_BACKOFF_MS;
        } else {
            state.delay_ms = (state.delay_ms * 2).min(MAX_DELAY_MS);
        }

        tracing::warn!(
            "Rate limited by upstream (429 #{}) — backoff delay: {}ms -> {}ms",
            state.total_429s, old, state.delay_ms
        );
    }
}

/// RAII guard that holds a concurrency permit. Drop to release.
pub struct ConcurrencyGuard {
    _permit: OwnedSemaphorePermit,
}
