//! Pure scoring: a sample window + config + now → [`Quality`].
//!
//! Dead-man is a *hard gate*. A 25 ms RTT from 2 s ago must not keep the score
//! off zero after `dead_ms` of silence.

use crate::buffer::SampleBuffer;
use crate::config::EngineConfig;
use crate::sample::{is_captive, is_wan_success, ProbeKind, ProbeSample};
use crate::scheduler::SchedulerMode;

/// Named quality band. Labels only — pixels interpolate the raw score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Band {
    /// No WAN (violet).
    None,
    /// Bad (red).
    Bad,
    /// Medium (orange).
    Medium,
    /// Ok (green).
    Ok,
    /// Ultra (blue).
    Ultra,
}

impl Band {
    /// Map a score in `[0, 1]` to a named band.
    pub fn from_score(score: f64) -> Self {
        let s = score.clamp(0.0, 1.0);
        if s < 0.08 {
            Self::None
        } else if s < 0.28 {
            Self::Bad
        } else if s < 0.55 {
            Self::Medium
        } else if s < 0.82 {
            Self::Ok
        } else {
            Self::Ultra
        }
    }
}

/// Why the score is not ultra. Ordered roughly by user-priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No WAN success inside `dead_ms`.
    DeadMan,
    /// Captive portal, not the public internet.
    Captive,
    /// Too many reachability edges — intermittent, not ultra.
    Flap,
    /// Packet/request loss in the quality window.
    Loss,
    /// High RTT.
    Latency,
    /// DNS failing while HTTP may still work.
    Dns,
    /// Unstable RTT.
    Jitter,
}

/// Scored view of the current window. Pure data.
#[derive(Debug, Clone, PartialEq)]
pub struct Quality {
    /// Composite score in `[0, 1]`.
    pub score: f64,
    /// Named band of `score`.
    pub band: Band,
    /// True iff a WAN voter succeeded within `dead_ms`.
    pub wan_reachable: bool,
    /// True iff we only see captive HTTP (no connectivity-ok).
    pub captive: bool,
    /// p50 RTT of successful WAN voters, milliseconds.
    pub rtt_p50_ms: Option<u32>,
    /// p95 RTT of successful WAN voters, milliseconds.
    pub rtt_p95_ms: Option<u32>,
    /// HTTP failure ratio in the quality window, `0..=1`.
    pub loss: f64,
    /// Mean absolute successive difference of success RTTs, milliseconds.
    pub jitter_ms: f64,
    /// DNS success ratio in the quality window (`1.0` if no DNS samples).
    pub dns_success_ratio: f64,
    /// Reachability edges in the flap window.
    pub flap_edges: u32,
    /// Active reasons, highest priority first.
    pub reasons: Vec<Reason>,
}

impl Quality {
    /// Hard-down snapshot.
    pub fn dead() -> Self {
        Self {
            score: 0.0,
            band: Band::None,
            wan_reachable: false,
            captive: false,
            rtt_p50_ms: None,
            rtt_p95_ms: None,
            loss: 1.0,
            jitter_ms: 0.0,
            dns_success_ratio: 0.0,
            flap_edges: 0,
            reasons: vec![Reason::DeadMan],
        }
    }
}

/// Nearest-rank percentile. `sorted` must be non-decreasing. `p` in `[0, 1]`.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let p = p.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    Some(sorted[idx.min(sorted.len() - 1)])
}

/// Piecewise-linear map. `points` are `(x, y)` sorted by `x`.
pub fn piecewise_linear(x: f64, points: &[(f64, f64)]) -> f64 {
    assert!(
        !points.is_empty(),
        "piecewise_linear needs at least one point"
    );
    if x <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x <= x1 {
            if (x1 - x0).abs() < f64::EPSILON {
                return y1;
            }
            let t = (x - x0) / (x1 - x0);
            return y0 + t * (y1 - y0);
        }
    }
    points[points.len() - 1].1
}

fn latency_term(ms: f64) -> f64 {
    piecewise_linear(
        ms,
        &[
            (25.0, 1.00),
            (80.0, 0.70),
            (150.0, 0.45),
            (300.0, 0.20),
            (800.0, 0.05),
        ],
    )
}

fn loss_term(loss: f64) -> f64 {
    piecewise_linear(
        loss,
        &[(0.00, 1.00), (0.05, 0.50), (0.15, 0.15), (0.30, 0.00)],
    )
}

fn jitter_term(masd_ms: f64) -> f64 {
    piecewise_linear(
        masd_ms,
        &[
            (10.0, 1.00),
            (30.0, 0.70),
            (60.0, 0.40),
            (120.0, 0.15),
            (200.0, 0.05),
        ],
    )
}

fn last_wan_success_ms<'a>(samples: impl Iterator<Item = &'a ProbeSample>) -> Option<u64> {
    samples.filter(|s| is_wan_success(s)).map(|s| s.at_ms).max()
}

fn flap_edges(samples: &[&ProbeSample]) -> u32 {
    let mut times: Vec<u64> = samples.iter().map(|s| s.at_ms).collect();
    times.sort_unstable();
    times.dedup();
    let mut prev: Option<bool> = None;
    let mut edges = 0u32;
    for t in times {
        let up = samples.iter().any(|s| s.at_ms == t && is_wan_success(s));
        if let Some(p) = prev {
            if p != up {
                edges += 1;
            }
        }
        prev = Some(up);
    }
    edges
}

/// Dead-man window. Burst/down must trip in `dead_ms`. Stable uses the
/// frugal interval plus `dead_ms` so a 1.5 s gap is not a false outage.
pub fn reachability_timeout_ms(mode: SchedulerMode, cfg: &EngineConfig) -> u64 {
    match mode {
        SchedulerMode::Stable => cfg.stable_spacing_ms.saturating_add(cfg.dead_ms),
        SchedulerMode::Uncertain | SchedulerMode::Down | SchedulerMode::Recovering => cfg.dead_ms,
    }
}

/// Score the buffer at `now_ms`. Pure.
pub fn evaluate(
    buffer: &SampleBuffer,
    now_ms: u64,
    cfg: &EngineConfig,
    mode: SchedulerMode,
) -> Quality {
    let last_ok = last_wan_success_ms(buffer.iter().filter(|s| s.at_ms <= now_ms));
    let reach_ms = reachability_timeout_ms(mode, cfg);
    let wan_reachable = last_ok
        .map(|t| now_ms.saturating_sub(t) <= reach_ms)
        .unwrap_or(false);

    let window: Vec<&ProbeSample> = buffer.in_window(now_ms, cfg.quality_window_ms).collect();
    let flap_window: Vec<&ProbeSample> = buffer.in_window(now_ms, cfg.flap_window_ms).collect();
    let edges = flap_edges(&flap_window);

    let http: Vec<&&ProbeSample> = window
        .iter()
        .filter(|s| s.kind == ProbeKind::Http)
        .collect();
    let http_ok = http.iter().filter(|s| s.outcome.is_success()).count();
    let had_http_ok = http_ok > 0;
    let had_captive = window.iter().any(|s| is_captive(s));
    let captive = had_captive && !had_http_ok;

    if captive {
        return Quality {
            score: cfg.captive_score.clamp(0.0, 0.15),
            band: Band::from_score(cfg.captive_score),
            wan_reachable: false,
            captive: true,
            rtt_p50_ms: None,
            rtt_p95_ms: None,
            loss: 1.0,
            jitter_ms: 0.0,
            dns_success_ratio: dns_ratio(&window),
            flap_edges: edges,
            reasons: vec![Reason::Captive],
        };
    }

    if !wan_reachable {
        let mut q = Quality::dead();
        q.flap_edges = edges;
        q.loss = http_loss(&http);
        q.dns_success_ratio = dns_ratio(&window);
        return q;
    }

    let mut success_rtts: Vec<f64> = window
        .iter()
        .filter(|s| is_wan_success(s))
        .filter_map(|s| s.outcome.rtt_ms().map(f64::from))
        .collect();
    success_rtts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let p50 = percentile(&success_rtts, 0.50);
    let p95 = percentile(&success_rtts, 0.95);
    let latency_score = match (p50, p95) {
        (Some(a), Some(b)) => 0.65 * latency_term(a) + 0.35 * latency_term(b),
        (Some(a), None) => latency_term(a),
        _ => 0.5,
    };

    let loss = http_loss(&http);
    let loss_score = if http.is_empty() {
        1.0
    } else {
        loss_term(loss)
    };

    let mut ordered_rtts: Vec<(u64, f64)> = window
        .iter()
        .filter(|s| is_wan_success(s))
        .filter_map(|s| s.outcome.rtt_ms().map(|ms| (s.at_ms, f64::from(ms))))
        .collect();
    ordered_rtts.sort_by_key(|(t, _)| *t);
    let rtt_seq: Vec<f64> = ordered_rtts.into_iter().map(|(_, r)| r).collect();
    let jitter_ms = mean_abs_successive_diff(&rtt_seq);
    let jitter_score = if rtt_seq.len() < 2 {
        1.0
    } else {
        jitter_term(jitter_ms)
    };

    let dns_success_ratio = dns_ratio(&window);
    let dns_score = dns_success_ratio;

    let wsum = cfg.weight_latency + cfg.weight_loss + cfg.weight_jitter + cfg.weight_dns;
    let wsum = if wsum <= f64::EPSILON { 1.0 } else { wsum };
    let mut score = (cfg.weight_latency * latency_score
        + cfg.weight_loss * loss_score
        + cfg.weight_jitter * jitter_score
        + cfg.weight_dns * dns_score)
        / wsum;
    score = score.clamp(0.0, 1.0);

    let flapping = edges > cfg.flap_edge_threshold;
    if flapping {
        score = score.min(cfg.flap_score_cap);
    }

    let mut reasons = Vec::new();
    if flapping {
        reasons.push(Reason::Flap);
    }
    if loss >= 0.05 && !http.is_empty() {
        reasons.push(Reason::Loss);
    }
    if p50.is_some_and(|ms| ms >= 80.0) {
        reasons.push(Reason::Latency);
    }
    if dns_success_ratio < 1.0 && window.iter().any(|s| s.kind == ProbeKind::Dns) {
        reasons.push(Reason::Dns);
    }
    if rtt_seq.len() >= 2 && jitter_ms >= 30.0 {
        reasons.push(Reason::Jitter);
    }

    Quality {
        score,
        band: Band::from_score(score),
        wan_reachable: true,
        captive: false,
        rtt_p50_ms: p50.map(|ms| ms.round() as u32),
        rtt_p95_ms: p95.map(|ms| ms.round() as u32),
        loss,
        jitter_ms,
        dns_success_ratio,
        flap_edges: edges,
        reasons,
    }
}

fn http_loss(http: &[&&ProbeSample]) -> f64 {
    if http.is_empty() {
        return 0.0;
    }
    let fails = http.iter().filter(|s| !s.outcome.is_success()).count();
    fails as f64 / http.len() as f64
}

fn dns_ratio(window: &[&ProbeSample]) -> f64 {
    let dns: Vec<&&ProbeSample> = window.iter().filter(|s| s.kind == ProbeKind::Dns).collect();
    if dns.is_empty() {
        return 1.0;
    }
    let ok = dns.iter().filter(|s| s.outcome.is_success()).count();
    ok as f64 / dns.len() as f64
}

fn mean_abs_successive_diff(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    for w in xs.windows(2) {
        sum += (w[1] - w[0]).abs();
    }
    sum / (xs.len() - 1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_boundaries() {
        assert_eq!(Band::from_score(0.0), Band::None);
        assert_eq!(Band::from_score(0.079), Band::None);
        assert_eq!(Band::from_score(0.08), Band::Bad);
        assert_eq!(Band::from_score(0.27), Band::Bad);
        assert_eq!(Band::from_score(0.28), Band::Medium);
        assert_eq!(Band::from_score(0.54), Band::Medium);
        assert_eq!(Band::from_score(0.55), Band::Ok);
        assert_eq!(Band::from_score(0.81), Band::Ok);
        assert_eq!(Band::from_score(0.82), Band::Ultra);
        assert_eq!(Band::from_score(1.0), Band::Ultra);
    }

    #[test]
    fn latency_map_anchors() {
        assert!((latency_term(25.0) - 1.0).abs() < 1e-9);
        assert!((latency_term(80.0) - 0.70).abs() < 1e-9);
        assert!((latency_term(10.0) - 1.0).abs() < 1e-9);
        assert!((latency_term(800.0) - 0.05).abs() < 1e-9);
        assert!(latency_term(900.0) <= 0.05 + 1e-9);
    }

    #[test]
    fn percentile_empty_and_singleton() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[42.0], 0.95), Some(42.0));
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.0), Some(1.0));
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 1.0), Some(4.0));
    }
}
