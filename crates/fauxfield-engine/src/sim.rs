//! Network and error simulation (SPEC §5.5, §5.6): profiles, fault dice, rate limiting, the
//! throttled / aborting response body and the connection wrapper that can send a TCP RST.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fauxfield_model::{Network, RateLimit};
use hyper::body::{Body, Frame, SizeHint};
use hyper_util::rt::TokioIo;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::time::Sleep;

/// Workspace-wide runtime network profile (SPEC §5.6). Overrides every server's `[network]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkProfile {
    #[default]
    None,
    Fast3g,
    Slow3g,
    FlakyWifi,
    Offline,
}

pub const PROFILES: &[NetworkProfile] = &[
    NetworkProfile::None,
    NetworkProfile::Fast3g,
    NetworkProfile::Slow3g,
    NetworkProfile::FlakyWifi,
    NetworkProfile::Offline,
];

impl NetworkProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fast3g => "Fast 3G",
            Self::Slow3g => "Slow 3G",
            Self::FlakyWifi => "Flaky Wi-Fi",
            Self::Offline => "Offline",
        }
    }
    /// CLI / MCP spelling.
    pub fn slug(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fast3g => "fast-3g",
            Self::Slow3g => "slow-3g",
            Self::FlakyWifi => "flaky-wifi",
            Self::Offline => "offline",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s
            .to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        Some(match k.as_str() {
            "none" | "off" => Self::None,
            "fast3g" => Self::Fast3g,
            "slow3g" => Self::Slow3g,
            "flakywifi" => Self::FlakyWifi,
            "offline" => Self::Offline,
            _ => return None,
        })
    }
    /// The network table a profile stands in for, or `None` when the server's own applies.
    pub fn network(self) -> Option<Network> {
        let n = |latency_ms, jitter_ms, bandwidth_kbps| Network {
            latency_ms,
            jitter_ms,
            bandwidth_kbps,
        };
        match self {
            Self::None | Self::Offline => None,
            Self::Fast3g => Some(n(150, 0, 1600)),
            Self::Slow3g => Some(n(400, 100, 400)),
            Self::FlakyWifi => Some(n(30, 100, 5000)),
        }
    }
    /// One-line summary for pickers, e.g. `400 ms ±100, 400 kbps`.
    pub fn describe(self) -> String {
        match self {
            Self::None => "no throttling".into(),
            Self::Offline => "refuse connections".into(),
            p => {
                let n = p.network().unwrap_or_default();
                let mut s = format!("{} ms", n.latency_ms);
                if n.jitter_ms > 0 {
                    s += &format!(" ±{}", n.jitter_ms);
                }
                s += &format!(", {} kbps", n.bandwidth_kbps);
                if p.drop_rate() > 0.0 {
                    s += &format!(", {:.0} % dropped", p.drop_rate() * 100.0);
                }
                s
            }
        }
    }
    /// Probability that a request is dropped with a connection reset.
    pub fn drop_rate(self) -> f64 {
        if self == Self::FlakyWifi { 0.05 } else { 0.0 }
    }
}

/// An injected failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Timeout,
    Reset,
    Close,
    Empty,
    Malformed,
    SlowBody,
    /// Chaos only: a plain 500 problem+json.
    ServerError,
}

impl Fault {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "timeout" => Self::Timeout,
            "reset" => Self::Reset,
            "close" => Self::Close,
            "empty" => Self::Empty,
            "malformed" => Self::Malformed,
            "slow_body" => Self::SlowBody,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Reset => "reset",
            Self::Close => "close",
            Self::Empty => "empty",
            Self::Malformed => "malformed",
            Self::SlowBody => "slow_body",
            Self::ServerError => "500",
        }
    }
    /// The client never sees a status line.
    pub fn no_status(self) -> bool {
        matches!(self, Self::Timeout | Self::Reset)
    }
}

const CHAOS_KINDS: &[Fault] = &[
    Fault::Reset,
    Fault::Close,
    Fault::Empty,
    Fault::Malformed,
    Fault::ServerError,
];

/// A random chaos fault.
pub fn chaos_fault() -> Fault {
    CHAOS_KINDS[rand::rng().random_range(0..CHAOS_KINDS.len())]
}

/// True with probability `p` (clamped to 0..=1).
pub fn roll(p: f64) -> bool {
    if p >= 1.0 {
        true
    } else if p <= 0.0 {
        false
    } else {
        rand::rng().random::<f64>() < p
    }
}

/// Uniform sample in `min..=max`.
pub fn sample(min: u64, max: u64) -> u64 {
    if max <= min {
        min
    } else {
        rand::rng().random_range(min..=max)
    }
}

/// Latency to wait for one request: `latency ± jitter`, never negative.
pub fn latency_ms(n: &Network) -> u64 {
    if n.jitter_ms == 0 {
        return n.latency_ms;
    }
    let lo = n.latency_ms.saturating_sub(n.jitter_ms);
    sample(lo, n.latency_ms + n.jitter_ms)
}

/// Fault rate used when the chaos switch is toggled on without a rate.
pub const DEFAULT_CHAOS_RATE: f64 = 0.2;

/// Default speed for `slow_body` when no bandwidth is configured.
pub const DEFAULT_SLOW_KBPS: u64 = 56;

// ----- rate limiting -----

/// Outcome of one rate-limit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateState {
    pub limit: u64,
    pub remaining: u64,
    /// Seconds until the window resets.
    pub reset_s: u64,
    pub limited: bool,
}

impl RateState {
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut h = vec![
            ("x-ratelimit-limit".into(), self.limit.to_string()),
            ("x-ratelimit-remaining".into(), self.remaining.to_string()),
            ("x-ratelimit-reset".into(), self.reset_s.to_string()),
        ];
        if self.limited {
            h.push(("retry-after".into(), self.reset_s.max(1).to_string()));
        }
        h
    }
}

/// Fixed-window counters per key.
#[derive(Default)]
pub struct Limiter {
    windows: Mutex<HashMap<String, (Instant, u64)>>,
}

impl Limiter {
    pub fn clear(&self) {
        self.windows.lock().unwrap().clear();
    }

    pub fn check(&self, cfg: &RateLimit, key: &str) -> Option<RateState> {
        if !cfg.enabled {
            return None;
        }
        let win = Duration::from_secs(fauxfield_model::parse_window(&cfg.window)?);
        let now = Instant::now();
        let mut m = self.windows.lock().unwrap();
        m.retain(|_, (t, _)| now.duration_since(*t) < win);
        let e = m.entry(key.to_string()).or_insert((now, 0));
        let reset_s = win.saturating_sub(now.duration_since(e.0)).as_secs().max(1);
        if e.1 >= cfg.requests {
            return Some(RateState {
                limit: cfg.requests,
                remaining: 0,
                reset_s,
                limited: true,
            });
        }
        e.1 += 1;
        Some(RateState {
            limit: cfg.requests,
            remaining: cfg.requests - e.1,
            reset_s,
            limited: false,
        })
    }
}

/// The rate-limit key of a request, or `None` when the named header or query is absent
/// (such requests are not counted).
pub fn rate_key(
    cfg: &RateLimit,
    ip: &str,
    headers: &[(String, String)],
    url: &str,
) -> Option<String> {
    if cfg.key == "ip" {
        return Some(ip.to_string());
    }
    if let Some(h) = cfg.key.strip_prefix("header:") {
        return headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(h.trim()))
            .map(|(_, v)| v.clone());
    }
    let q = cfg.key.strip_prefix("query:")?.trim();
    form_urlencoded::parse(url.split_once('?')?.1.as_bytes())
        .find(|(k, _)| k == q)
        .map(|(_, v)| v.into_owned())
}

// ----- response body -----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyMode {
    Full,
    /// Bytes per second.
    Throttle(u64),
    /// Send the headers, then abort the connection.
    Abort,
    /// Declare the full length, send half, then abort.
    Truncate,
}

const TICK: Duration = Duration::from_millis(50);

/// The response body: plain, throttled, or aborting. `on_done` gets the transfer time in ms
/// when the body finishes or is dropped.
pub struct SimBody {
    data: Bytes,
    pos: usize,
    mode: BodyMode,
    sleep: Option<Pin<Box<Sleep>>>,
    started: Instant,
    sent_first: bool,
    on_done: Option<Box<dyn FnOnce(u64) + Send>>,
}

impl SimBody {
    pub fn new(data: Bytes, mode: BodyMode, on_done: Option<Box<dyn FnOnce(u64) + Send>>) -> Self {
        Self {
            data,
            pos: 0,
            mode,
            sleep: None,
            started: Instant::now(),
            sent_first: false,
            on_done,
        }
    }
}

impl SimBody {
    /// Pend briefly so hyper flushes what it has buffered (the headers), then abort.
    fn abort_after_flush(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let s = self
            .sleep
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_millis(30))));
        if s.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        Poll::Ready(Some(Err(abort())))
    }
}

impl Drop for SimBody {
    fn drop(&mut self) {
        if let Some(f) = self.on_done.take() {
            f(self.started.elapsed().as_millis() as u64);
        }
    }
}

fn abort() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "fault injected")
}

impl Body for SimBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = &mut *self;
        match this.mode {
            BodyMode::Abort => this.abort_after_flush(cx),
            BodyMode::Full => {
                if this.pos >= this.data.len() {
                    return Poll::Ready(None);
                }
                let all = this.data.slice(this.pos..);
                this.pos = this.data.len();
                Poll::Ready(Some(Ok(Frame::data(all))))
            }
            BodyMode::Truncate => {
                let cut = this.data.len() / 2;
                if !this.sent_first && cut > 0 {
                    this.sent_first = true;
                    return Poll::Ready(Some(Ok(Frame::data(this.data.slice(..cut)))));
                }
                this.abort_after_flush(cx)
            }
            BodyMode::Throttle(bps) => {
                if this.pos >= this.data.len() {
                    return Poll::Ready(None);
                }
                if let Some(s) = this.sleep.as_mut() {
                    if s.as_mut().poll(cx).is_pending() {
                        return Poll::Pending;
                    }
                    this.sleep = None;
                }
                let chunk = ((bps as f64 * TICK.as_secs_f64()) as usize).max(1);
                let end = (this.pos + chunk).min(this.data.len());
                let part = this.data.slice(this.pos..end);
                this.pos = end;
                if this.pos < this.data.len() {
                    this.sleep = Some(Box::pin(tokio::time::sleep(TICK)));
                }
                Poll::Ready(Some(Ok(Frame::data(part))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.mode == BodyMode::Full && self.pos >= self.data.len()
    }

    fn size_hint(&self) -> SizeHint {
        // Truncate declares the full length so the client sees a short body.
        SizeHint::with_exact(self.data.len() as u64 - self.pos as u64)
    }
}

// ----- connection wrapper -----

/// A connection that sends a TCP RST on drop when `reset` is set.
pub struct FaultIo {
    inner: TokioIo<TcpStream>,
    pub reset: Arc<AtomicBool>,
}

impl FaultIo {
    pub fn new(stream: TcpStream) -> (Self, Arc<AtomicBool>) {
        let reset = Arc::new(AtomicBool::new(false));
        (
            Self {
                inner: TokioIo::new(stream),
                reset: reset.clone(),
            },
            reset,
        )
    }
}

impl Drop for FaultIo {
    fn drop(&mut self) {
        if self.reset.load(Ordering::SeqCst) {
            #[allow(deprecated)]
            let _ = self.inner.inner().set_linger(Some(Duration::ZERO));
        }
    }
}

impl hyper::rt::Read for FaultIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        hyper::rt::Read::poll_read(Pin::new(&mut self.inner), cx, buf)
    }
}

impl hyper::rt::Write for FaultIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        hyper::rt::Write::poll_write(Pin::new(&mut self.inner), cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        hyper::rt::Write::poll_flush(Pin::new(&mut self.inner), cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        hyper::rt::Write::poll_shutdown(Pin::new(&mut self.inner), cx)
    }
    fn is_write_vectored(&self) -> bool {
        hyper::rt::Write::is_write_vectored(&self.inner)
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        hyper::rt::Write::poll_write_vectored(Pin::new(&mut self.inner), cx, bufs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_parse() {
        assert_eq!(
            NetworkProfile::parse("Slow 3G"),
            Some(NetworkProfile::Slow3g)
        );
        assert_eq!(
            NetworkProfile::parse("flaky-wifi"),
            Some(NetworkProfile::FlakyWifi)
        );
        assert_eq!(NetworkProfile::parse("x"), None);
        for p in PROFILES {
            assert_eq!(NetworkProfile::parse(p.slug()), Some(*p));
        }
    }

    #[test]
    fn limiter_counts_and_keys() {
        let cfg = RateLimit {
            enabled: true,
            requests: 2,
            window: "1m".into(),
            key: "ip".into(),
        };
        let l = Limiter::default();
        assert_eq!(l.check(&cfg, "a").unwrap().remaining, 1);
        assert_eq!(l.check(&cfg, "a").unwrap().remaining, 0);
        assert!(l.check(&cfg, "a").unwrap().limited);
        assert!(!l.check(&cfg, "b").unwrap().limited);
        let off = RateLimit {
            enabled: false,
            ..cfg
        };
        assert!(l.check(&off, "a").is_none());
    }

    #[test]
    fn keys_and_rolls() {
        let mut cfg = RateLimit {
            enabled: true,
            requests: 1,
            window: "1m".into(),
            key: "header:X-Key".into(),
        };
        let h = vec![("x-key".to_string(), "k1".to_string())];
        assert_eq!(rate_key(&cfg, "ip", &h, "/"), Some("k1".into()));
        assert_eq!(rate_key(&cfg, "ip", &[], "/"), None);
        cfg.key = "query:u".into();
        assert_eq!(rate_key(&cfg, "ip", &[], "/p?u=7"), Some("7".into()));
        assert!(roll(1.0) && !roll(0.0));
        assert_eq!(sample(5, 5), 5);
        let n = Network {
            latency_ms: 10,
            jitter_ms: 50,
            bandwidth_kbps: 0,
        };
        for _ in 0..50 {
            assert!(latency_ms(&n) <= 60);
        }
    }
}
