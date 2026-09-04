//! Frugal-with-burst cadence. A miss in stable is an *interrupt*, not “see you in 1.5 s”.

use crate::config::EngineConfig;

/// Probe-loop mode. Drives spacing and in-flight count, not the painted color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchedulerMode {
    /// Path looks good; cheap interval.
    Stable,
    /// First miss, path change, or cold start — burst.
    Uncertain,
    /// Dead-man: no WAN success inside `dead_ms`.
    Down,
    /// WAN samples succeeding again; keep bursting until `burst_hold_ms` clean.
    Recovering,
}

/// Stateful probe cadence.
#[derive(Debug, Clone)]
pub struct Scheduler {
    mode: SchedulerMode,
    last_probe_ms: Option<u64>,
    last_success_ms: Option<u64>,
    clean_since_ms: Option<u64>,
    started_ms: u64,
}

impl Scheduler {
    /// Cold start in `Uncertain` so the first probes are a burst, not a 1.5 s wait.
    pub fn new(started_ms: u64) -> Self {
        Self {
            mode: SchedulerMode::Uncertain,
            last_probe_ms: None,
            last_success_ms: None,
            clean_since_ms: None,
            started_ms,
        }
    }

    /// Current mode.
    pub fn mode(&self) -> SchedulerMode {
        self.mode
    }

    /// Delay until the next fire, milliseconds.
    pub fn spacing_ms(&self, cfg: &EngineConfig) -> u64 {
        match self.mode {
            SchedulerMode::Stable => cfg.stable_spacing_ms,
            SchedulerMode::Uncertain | SchedulerMode::Recovering => cfg.burst_spacing_ms,
            SchedulerMode::Down => cfg.down_spacing_ms,
        }
    }

    /// How many probes to launch in parallel this round.
    pub fn in_flight(&self, cfg: &EngineConfig) -> u8 {
        match self.mode {
            SchedulerMode::Stable => cfg.stable_in_flight,
            SchedulerMode::Uncertain | SchedulerMode::Recovering => cfg.burst_in_flight,
            SchedulerMode::Down => cfg.down_in_flight,
        }
    }

    /// Whether a probe round should start *now*.
    pub fn should_probe(&self, now_ms: u64, cfg: &EngineConfig, paused: bool) -> bool {
        if paused {
            return false;
        }
        match self.last_probe_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= self.spacing_ms(cfg),
        }
    }

    /// Record that a round was launched at `now_ms`.
    pub fn mark_probed(&mut self, now_ms: u64) {
        self.last_probe_ms = Some(now_ms);
    }

    /// Path change, sleep-wake, or equivalent: burst immediately.
    pub fn interrupt(&mut self, _now_ms: u64) {
        if self.mode == SchedulerMode::Stable {
            self.mode = SchedulerMode::Uncertain;
        }
        if self.mode == SchedulerMode::Down {
            self.mode = SchedulerMode::Uncertain;
        }
        self.clean_since_ms = None;
        self.last_probe_ms = None;
    }

    /// Consume a completed round.
    pub fn on_round(&mut self, now_ms: u64, wan_success: bool, cfg: &EngineConfig) {
        if wan_success {
            self.last_success_ms = Some(now_ms);
            if self.clean_since_ms.is_none() {
                self.clean_since_ms = Some(now_ms);
            }
            match self.mode {
                SchedulerMode::Down => {
                    self.mode = SchedulerMode::Recovering;
                    self.clean_since_ms = Some(now_ms);
                }
                SchedulerMode::Uncertain | SchedulerMode::Recovering => {
                    if let Some(clean) = self.clean_since_ms {
                        if now_ms.saturating_sub(clean) >= cfg.burst_hold_ms {
                            self.mode = SchedulerMode::Stable;
                        }
                    }
                }
                SchedulerMode::Stable => {}
            }
            return;
        }

        self.clean_since_ms = None;
        if self.mode == SchedulerMode::Stable {
            self.mode = SchedulerMode::Uncertain;
            self.last_probe_ms = None;
        }
        let dead = match self.last_success_ms {
            Some(t) => now_ms.saturating_sub(t) >= cfg.dead_ms,
            None => now_ms.saturating_sub(self.started_ms) >= cfg.dead_ms,
        };
        if dead {
            self.mode = SchedulerMode::Down;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miss_in_stable_is_an_interrupt() {
        let cfg = EngineConfig::default();
        let mut s = Scheduler::new(0);
        s.mode = SchedulerMode::Stable;
        s.last_probe_ms = Some(0);
        s.last_success_ms = Some(0);
        s.clean_since_ms = Some(0);
        s.on_round(100, false, &cfg);
        assert_eq!(s.mode(), SchedulerMode::Uncertain);
        assert!(s.should_probe(100, &cfg, false));
        assert_eq!(s.spacing_ms(&cfg), cfg.burst_spacing_ms);
    }

    #[test]
    fn pause_blocks_probes() {
        let cfg = EngineConfig::default();
        let s = Scheduler::new(0);
        assert!(!s.should_probe(0, &cfg, true));
    }
}
