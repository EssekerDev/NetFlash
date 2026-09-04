//! Engine knobs. Units are milliseconds unless noted.

/// Tunable engine parameters. Weights are ratios that should sum to 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// Dead-man: no WAN success for this long → score 0, regardless of last RTT.
    pub dead_ms: u64,
    /// On path-change / sleep-wake, drop samples older than this.
    pub stale_ms: u64,
    /// Window used for latency / loss / jitter / DNS ratios.
    pub quality_window_ms: u64,
    /// Window used to count reachability edges (flap).
    pub flap_window_ms: u64,
    /// More than this many up/down edges in `flap_window_ms` caps the score.
    pub flap_edge_threshold: u32,
    /// Score ceiling when flapping (top of the medium / orange band, exclusive of ok).
    pub flap_score_cap: f64,
    /// Consecutive successful *rounds* required before leaving violet.
    pub recovery_successes: u32,
    /// Minimum time after the first recovery success before leaving violet.
    pub recovery_hold_ms: u64,
    /// Display ease time-constant in milliseconds (~180–280).
    pub ease_tau_ms: f64,
    /// When true, displayed score snaps to the target (no interpolation).
    pub reduced_motion: bool,
    /// Probe spacing while `SchedulerMode::Stable` (frugal).
    pub stable_spacing_ms: u64,
    /// Probe spacing while uncertain / recovering (burst).
    pub burst_spacing_ms: u64,
    /// Probe spacing while down.
    pub down_spacing_ms: u64,
    /// Clean burst duration before returning to stable spacing.
    pub burst_hold_ms: u64,
    /// Parallel probes while bursting.
    pub burst_in_flight: u8,
    /// Parallel probes while stable (keep this 1 to stay near the data budget).
    pub stable_in_flight: u8,
    /// Parallel probes while down.
    pub down_in_flight: u8,
    /// Weight of the latency term (p50 with p95 penalty).
    pub weight_latency: f64,
    /// Weight of the loss term.
    pub weight_loss: f64,
    /// Weight of the jitter term.
    pub weight_jitter: f64,
    /// Weight of the DNS term.
    pub weight_dns: f64,
    /// Score used when the path is a captive portal (violet/red, never green).
    pub captive_score: f64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            dead_ms: 800,
            stale_ms: 2_000,
            quality_window_ms: 12_000,
            flap_window_ms: 30_000,
            flap_edge_threshold: 4,
            flap_score_cap: 0.54,
            recovery_successes: 3,
            recovery_hold_ms: 1_200,
            ease_tau_ms: 230.0,
            reduced_motion: false,
            stable_spacing_ms: 1_500,
            burst_spacing_ms: 320,
            down_spacing_ms: 500,
            burst_hold_ms: 8_000,
            burst_in_flight: 3,
            stable_in_flight: 1,
            down_in_flight: 2,
            weight_latency: 0.40,
            weight_loss: 0.30,
            weight_jitter: 0.20,
            weight_dns: 0.10,
            captive_score: 0.12,
        }
    }
}

impl EngineConfig {
    /// Rough bytes/hour on the *stable* path: one ~400 B HTTP check per `stable_spacing_ms`
    /// times `stable_in_flight`. Default ≈ 960 KiB/h. Burst is extra and short-lived.
    pub fn estimated_stable_bytes_per_hour(&self) -> u64 {
        let spacing = self.stable_spacing_ms.max(1);
        let rounds = 3_600_000 / spacing;
        rounds * 400 * u64::from(self.stable_in_flight.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_budget_is_under_two_mib_per_hour() {
        let n = EngineConfig::default().estimated_stable_bytes_per_hour();
        assert!(n > 100_000, "budget should be non-trivial, got {n}");
        assert!(
            n < 2 * 1024 * 1024,
            "stable path must not look like a speedtest, got {n}"
        );
    }
}
