//! Live WAN probes. Timeouts are data. Never log full URLs.
//!
//! A round returns as soon as **one** WAN voter succeeds. Waiting for a slow
//! sibling (600 ms timeout) would let the engine's 800 ms dead-man fire while
//! the laptop actually has internet — a false violet.

use std::pin::pin;
use std::time::{Duration, Instant};

use hickory_resolver::config::ResolverOpts;
use hickory_resolver::TokioAsyncResolver;
use netflash_core::{is_wan_success, ProbeOutcome, ProbeSample};
use reqwest::header::{CACHE_CONTROL, CONNECTION};
use reqwest::redirect::Policy;
use reqwest::Client;
use tokio::time::timeout;

/// Hard cap per probe so the dead-man budget stays meaningful.
const PROBE_TIMEOUT: Duration = Duration::from_millis(600);

const HTTP_GOOGLE: (&str, &str) = ("http-gstatic", "https://www.gstatic.com/generate_204");
const HTTP_CF: (&str, &str) = ("http-cloudflare", "https://cp.cloudflare.com/generate_204");
const DNS_NAME: (&str, &str) = ("dns-one", "one.one.one.one");

/// Shared HTTP + DNS clients.
pub struct Prober {
    http: Client,
    dns: TokioAsyncResolver,
    start: Instant,
}

impl Prober {
    /// Build clients. DNS uses the OS nameservers (what apps see), not a pinned 8.8.8.8.
    pub fn new(start: Instant) -> Result<Self, String> {
        let http = Client::builder()
            .use_rustls_tls()
            .redirect(Policy::none())
            .timeout(PROBE_TIMEOUT)
            .connect_timeout(Duration::from_millis(450))
            .pool_max_idle_per_host(0)
            .tcp_nodelay(true)
            .user_agent(concat!("NetFlash/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;

        let dns = match hickory_resolver::system_conf::read_system_conf() {
            Ok((config, mut opts)) => {
                opts.timeout = Duration::from_millis(500);
                opts.attempts = 1;
                opts.cache_size = 0;
                TokioAsyncResolver::tokio(config, opts)
            }
            Err(_) => {
                let mut opts = ResolverOpts::default();
                opts.timeout = Duration::from_millis(500);
                opts.attempts = 1;
                opts.cache_size = 0;
                TokioAsyncResolver::tokio(hickory_resolver::config::ResolverConfig::default(), opts)
            }
        };

        Ok(Self { http, dns, start })
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Fire `count` diverse probes in parallel. Stop the round on the first WAN success.
    pub async fn round(&self, count: u8) -> Vec<ProbeSample> {
        match count.max(1) {
            1 => vec![self.http_probe(HTTP_GOOGLE.0, HTTP_GOOGLE.1).await],
            2 => self.race_two().await,
            _ => self.race_three().await,
        }
    }

    async fn race_two(&self) -> Vec<ProbeSample> {
        let mut a = pin!(self.http_probe(HTTP_GOOGLE.0, HTTP_GOOGLE.1));
        let mut b = pin!(self.http_probe(HTTP_CF.0, HTTP_CF.1));
        let mut a_done = false;
        let mut b_done = false;
        let mut out = Vec::new();
        loop {
            tokio::select! {
                s = &mut a, if !a_done => {
                    a_done = true;
                    let ok = is_wan_success(&s);
                    out.push(s);
                    if ok || b_done {
                        break;
                    }
                }
                s = &mut b, if !b_done => {
                    b_done = true;
                    let ok = is_wan_success(&s);
                    out.push(s);
                    if ok || a_done {
                        break;
                    }
                }
            }
        }
        out
    }

    async fn race_three(&self) -> Vec<ProbeSample> {
        let mut a = pin!(self.http_probe(HTTP_GOOGLE.0, HTTP_GOOGLE.1));
        let mut b = pin!(self.http_probe(HTTP_CF.0, HTTP_CF.1));
        let mut c = pin!(self.dns_probe(DNS_NAME.0, DNS_NAME.1));
        let mut a_done = false;
        let mut b_done = false;
        let mut c_done = false;
        let mut out = Vec::new();
        loop {
            tokio::select! {
                s = &mut a, if !a_done => {
                    a_done = true;
                    let ok = is_wan_success(&s);
                    out.push(s);
                    if ok || (b_done && c_done) {
                        break;
                    }
                }
                s = &mut b, if !b_done => {
                    b_done = true;
                    let ok = is_wan_success(&s);
                    out.push(s);
                    if ok || (a_done && c_done) {
                        break;
                    }
                }
                s = &mut c, if !c_done => {
                    c_done = true;
                    let ok = is_wan_success(&s);
                    out.push(s);
                    if ok || (a_done && b_done) {
                        break;
                    }
                }
            }
        }
        out
    }

    async fn http_probe(&self, id: &'static str, url: &'static str) -> ProbeSample {
        let t0 = Instant::now();
        let fut = self
            .http
            .get(url)
            .header(CACHE_CONTROL, "no-store")
            .header(CONNECTION, "close")
            .send();
        match timeout(PROBE_TIMEOUT, fut).await {
            Err(_) => ProbeSample::http_timeout(self.now_ms(), id),
            Ok(Err(_)) => {
                if t0.elapsed() >= PROBE_TIMEOUT.saturating_sub(Duration::from_millis(20)) {
                    ProbeSample::http_timeout(self.now_ms(), id)
                } else {
                    ProbeSample::http_fail(self.now_ms(), id)
                }
            }
            Ok(Ok(res)) => {
                let status = res.status().as_u16();
                let ct = res
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_owned();
                let body = match timeout(Duration::from_millis(200), res.bytes()).await {
                    Ok(Ok(b)) => b,
                    _ => Default::default(),
                };
                let body = &body[..body.len().min(2048)];
                match classify_http(status, &ct, body, t0.elapsed()) {
                    ProbeOutcome::Success { rtt_ms } => {
                        ProbeSample::http_ok(self.now_ms(), id, rtt_ms)
                    }
                    ProbeOutcome::Timeout => ProbeSample::http_timeout(self.now_ms(), id),
                    ProbeOutcome::Captive { http_status } => {
                        ProbeSample::http_captive(self.now_ms(), id, http_status)
                    }
                    ProbeOutcome::TransportFail => ProbeSample::http_fail(self.now_ms(), id),
                }
            }
        }
    }

    async fn dns_probe(&self, id: &'static str, name: &'static str) -> ProbeSample {
        let t0 = Instant::now();
        let fut = self.dns.lookup_ip(name);
        match timeout(PROBE_TIMEOUT, fut).await {
            Err(_) => ProbeSample::dns_timeout(self.now_ms(), id),
            Ok(Err(_)) => ProbeSample::dns_fail(self.now_ms(), id),
            Ok(Ok(lookup)) => {
                if lookup.iter().next().is_some() {
                    let rtt_ms = t0.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                    ProbeSample::dns_ok(self.now_ms(), id, rtt_ms)
                } else {
                    ProbeSample::dns_fail(self.now_ms(), id)
                }
            }
        }
    }
}

fn classify_http(status: u16, content_type: &str, body: &[u8], elapsed: Duration) -> ProbeOutcome {
    let rtt_ms = elapsed.as_millis().min(u128::from(u32::MAX)) as u32;
    match status {
        204 | 205 => ProbeOutcome::Success { rtt_ms },
        301 | 302 | 303 | 307 | 308 => ProbeOutcome::Captive {
            http_status: status,
        },
        200 => {
            let ct = content_type.to_ascii_lowercase();
            if ct.contains("text/html") {
                return ProbeOutcome::Captive { http_status: 200 };
            }
            let head = std::str::from_utf8(body).unwrap_or("").trim_start();
            let lower = head.to_ascii_lowercase();
            if lower.starts_with("<!doctype") || lower.starts_with("<html") {
                return ProbeOutcome::Captive { http_status: 200 };
            }
            ProbeOutcome::Success { rtt_ms }
        }
        _ => ProbeOutcome::TransportFail,
    }
}
