use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

/// A fixed-window rate limiter, keyed by an arbitrary string (an IP, a
/// username, whatever the caller wants to bucket by). Not distributed and
/// not persisted across restarts - a self-hosted instance's own process
/// memory is all this needs to blunt casual brute-forcing, not a
/// determined attacker with a botnet. A fixed window is simpler than a
/// sliding one and good enough for that: the worst case is a burst right
/// at the window boundary, not an unbounded one.
pub struct RateLimiter {
    max_attempts: u32,
    window: Duration,
    buckets: Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    pub fn new(max_attempts: u32, window: Duration) -> Self {
        Self {
            max_attempts,
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Records an attempt for `key` and reports whether it's still within
    /// the limit. Locking a plain `Mutex` for this is fine at self-hosted
    /// scale - the critical section is a hash map lookup, not I/O.
    pub fn check(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();

        let entry = buckets.entry(key.to_string()).or_insert((0, now));

        if now.duration_since(entry.1) > self.window {
            *entry = (1, now);
            return true;
        }

        if entry.0 >= self.max_attempts {
            return false;
        }

        entry.0 += 1;
        true
    }

    /// Drops buckets whose window has already elapsed, so a long-running
    /// process doesn't accumulate one entry per IP or username it's ever
    /// seen. Called periodically from `run_rate_limit_cleanup`, not on
    /// every `check` - the map only grows by one entry per distinct key
    /// per window anyway, which is already bounded at any real traffic
    /// level this app runs at.
    fn cleanup(&self) {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        buckets.retain(|_, (_, started)| now.duration_since(*started) <= self.window);
    }
}

/// Rate limiters for the handful of endpoints worth protecting against
/// brute-forcing or casual abuse - login attempts, TOTP codes (only a
/// million possible six-digit codes, so this matters more than the
/// password does), registrations, and uploads.
pub struct RateLimiters {
    pub login: RateLimiter,
    pub totp: RateLimiter,
    pub register: RateLimiter,
    pub upload: RateLimiter,
}

impl Default for RateLimiters {
    fn default() -> Self {
        Self {
            login: RateLimiter::new(10, Duration::from_secs(5 * 60)),
            totp: RateLimiter::new(10, Duration::from_secs(5 * 60)),
            register: RateLimiter::new(5, Duration::from_secs(60 * 60)),
            upload: RateLimiter::new(60, Duration::from_secs(60)),
        }
    }
}

impl RateLimiters {
    fn cleanup(&self) {
        self.login.cleanup();
        self.totp.cleanup();
        self.register.cleanup();
        self.upload.cleanup();
    }
}

/// Runs forever as a background task, same shape as the other sweeps in
/// this crate - keeps the rate limiters' own memory bounded instead of
/// growing for as long as the process stays up.
pub async fn run_rate_limit_cleanup(limiters: std::sync::Arc<RateLimiters>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(600));
    loop {
        ticker.tick().await;
        limiters.cleanup();
    }
}

/// The client's own address, as best as this request's headers can say -
/// `X-Forwarded-For`'s first entry (set by a reverse proxy, which is how
/// this app is meant to be deployed - see the README's Docker section),
/// falling back to `X-Real-IP`, and finally a shared placeholder for a
/// bare, non-proxied deployment. That fallback means every direct
/// connection shares one rate-limit bucket rather than being limited
/// individually - an acceptable gap for a setup this app doesn't actually
/// recommend running that way.
pub fn client_ip(headers: &HeaderMap) -> String {
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(first) = forwarded.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
    {
        if !real_ip.is_empty() {
            return real_ip.to_string();
        }
    }

    "unknown".to_string()
}
