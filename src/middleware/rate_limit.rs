//! Per-IP fixed-window rate limiting.
//!
//! Each client IP may make RATE_LIMIT_MAX_REQUESTS requests (default 1000) per
//! RATE_LIMIT_WINDOW_SECS (default 900). Over the limit, requests get 429 with
//! Retry-After; every response carries RateLimit-Limit / -Remaining / -Reset.
//! /api/health is never limited. (An open dashboard polls about 100 times in
//! 15 minutes, so the default leaves room for several tabs and queries.) Behind a reverse proxy (Render, nginx) set
//! TRUST_PROXY=1 so the client IP comes from X-Forwarded-For; otherwise the
//! socket address is used, since the header can be forged by anyone.

use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::{HeaderName, HeaderValue},
    Error, HttpResponse,
};
use std::collections::HashMap;
use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Fixed-window counter per client key. Pure apart from the clock passed in,
/// so it can be tested directly.
pub struct FixedWindow {
    pub max: u32,
    pub window_secs: u64,
    counts: HashMap<String, (u64, u32)>, // key -> (window start, count)
    last_sweep: u64,
}

/// Outcome of one request: allowed with this many left, or refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed { remaining: u32, reset_in: u64 },
    Limited { retry_after: u64 },
}

impl FixedWindow {
    pub fn new(max: u32, window_secs: u64) -> Self {
        Self { max: max.max(1), window_secs: window_secs.max(1), counts: HashMap::new(), last_sweep: 0 }
    }

    pub fn check(&mut self, key: &str, now: u64) -> Decision {
        // Drop finished windows now and then so idle IPs don't pile up.
        if now.saturating_sub(self.last_sweep) >= self.window_secs {
            let window = self.window_secs;
            self.counts.retain(|_, (start, _)| now < *start + window);
            self.last_sweep = now;
        }
        let entry = self.counts.entry(key.to_string()).or_insert((now, 0));
        if now >= entry.0 + self.window_secs {
            *entry = (now, 0);
        }
        let reset_in = entry.0 + self.window_secs - now;
        if entry.1 >= self.max {
            return Decision::Limited { retry_after: reset_in };
        }
        entry.1 += 1;
        Decision::Allowed { remaining: self.max - entry.1, reset_in }
    }
}

fn limiter() -> &'static Mutex<FixedWindow> {
    static LIMITER: OnceLock<Mutex<FixedWindow>> = OnceLock::new();
    LIMITER.get_or_init(|| {
        let env = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Mutex::new(FixedWindow::new(
            env("RATE_LIMIT_MAX_REQUESTS", 1000) as u32,
            env("RATE_LIMIT_WINDOW_SECS", 900),
        ))
    })
}

fn client_key(req: &ServiceRequest) -> String {
    let trust_proxy = std::env::var("TRUST_PROXY").map(|v| v == "1" || v == "true").unwrap_or(false);
    let info = req.connection_info();
    let addr = if trust_proxy { info.realip_remote_addr() } else { info.peer_addr() };
    addr.unwrap_or("unknown").to_string()
}

pub struct RateLimitMiddleware;

impl<S, B> Transform<S, ServiceRequest> for RateLimitMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimitMiddlewareService { service }))
    }
}

pub struct RateLimitMiddlewareService<S> {
    service: S,
}

fn set_header<B>(res: &mut ServiceResponse<B>, name: &'static str, value: u64) {
    if let Ok(v) = HeaderValue::from_str(&value.to_string()) {
        res.headers_mut().insert(HeaderName::from_static(name), v);
    }
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        if req.path() == "/api/health" {
            let fut = self.service.call(req);
            return Box::pin(async move { Ok(fut.await?.map_into_left_body()) });
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let key = client_key(&req);
        let (decision, max) = {
            let mut l = limiter().lock().unwrap_or_else(|p| p.into_inner());
            (l.check(&key, now), l.max)
        };

        match decision {
            Decision::Limited { retry_after } => {
                let response = HttpResponse::TooManyRequests()
                    .insert_header(("Retry-After", retry_after.to_string()))
                    .json(serde_json::json!({
                        "success": false,
                        "message": "Too many requests, please try again later"
                    }));
                let mut res = req.into_response(response).map_into_right_body();
                set_header(&mut res, "ratelimit-limit", max as u64);
                set_header(&mut res, "ratelimit-remaining", 0);
                set_header(&mut res, "ratelimit-reset", retry_after);
                Box::pin(async move { Ok(res) })
            }
            Decision::Allowed { remaining, reset_in } => {
                let fut = self.service.call(req);
                Box::pin(async move {
                    let mut res = fut.await?.map_into_left_body();
                    set_header(&mut res, "ratelimit-limit", max as u64);
                    set_header(&mut res, "ratelimit-remaining", remaining as u64);
                    set_header(&mut res, "ratelimit-reset", reset_in);
                    Ok(res)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_per_key_and_resets_after_the_window() {
        let mut w = FixedWindow::new(2, 60);
        assert_eq!(w.check("a", 1000), Decision::Allowed { remaining: 1, reset_in: 60 });
        assert_eq!(w.check("a", 1010), Decision::Allowed { remaining: 0, reset_in: 50 });
        assert_eq!(w.check("a", 1020), Decision::Limited { retry_after: 40 });
        // Another client has its own budget.
        assert_eq!(w.check("b", 1020), Decision::Allowed { remaining: 1, reset_in: 60 });
        // A new window starts once the old one ends.
        assert_eq!(w.check("a", 1060), Decision::Allowed { remaining: 1, reset_in: 60 });
    }

    #[test]
    fn sweeps_idle_clients() {
        let mut w = FixedWindow::new(5, 10);
        for i in 0..100 {
            w.check(&format!("ip{i}"), 0);
        }
        w.check("late", 100);
        assert_eq!(w.counts.len(), 1);
    }
}
