use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RateLimiter {
    rps: u32,
    state: std::sync::Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn from_env() -> Option<Self> {
        let rps = std::env::var("RATE_LIMIT_RPS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&n| n > 0)?;

        Some(Self {
            rps,
            state: std::sync::Arc::new(Mutex::new(State {
                tokens: rps as f64,
                last: Instant::now(),
            })),
        })
    }

    pub async fn check(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(state.last);
        state.last = now;

        let refill = (elapsed.as_secs_f64() * self.rps as f64).min(self.rps as f64);
        state.tokens = (state.tokens + refill).min(self.rps as f64);

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            return Ok(());
        }

        let wait = Duration::from_secs_f64((1.0 - state.tokens) / self.rps as f64);
        Err(format!(
            "rate limit exceeded (RATE_LIMIT_RPS={}): try again in ~{}ms",
            self.rps,
            wait.as_millis()
        ))
    }
}

