//! Display hysteresis: truth (`Quality`) vs what we paint (`displayed` score).
//!
//! Down is urgent (target 0 on the next step). Up needs K successes and a hold.

use crate::config::EngineConfig;
use crate::score::Quality;

/// Painted score state. The tray eases toward `displayed`, not raw quality.
#[derive(Debug, Clone)]
pub struct DisplayState {
    displayed: f64,
    declared_up: bool,
    consecutive_ok: u32,
    recovery_started_ms: Option<u64>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayState {
    /// Start painted at 0 (violet) — never assume WAN on boot.
    pub fn new() -> Self {
        Self {
            displayed: 0.0,
            declared_up: false,
            consecutive_ok: 0,
            recovery_started_ms: None,
        }
    }

    /// Current painted score in `[0, 1]`.
    pub fn displayed(&self) -> f64 {
        self.displayed
    }

    /// True once recovery hysteresis has released the paint from violet.
    pub fn declared_up(&self) -> bool {
        self.declared_up
    }

    /// Observe a probe *round* (parallel samples that share a timestamp).
    pub fn on_round(&mut self, now_ms: u64, wan_success: bool, cfg: &EngineConfig) {
        if !wan_success {
            self.consecutive_ok = 0;
            self.recovery_started_ms = None;
            self.declared_up = false;
            return;
        }
        self.consecutive_ok = self.consecutive_ok.saturating_add(1);
        if self.recovery_started_ms.is_none() {
            self.recovery_started_ms = Some(now_ms);
        }
        self.try_promote(now_ms, cfg);
    }

    fn try_promote(&mut self, now_ms: u64, cfg: &EngineConfig) {
        if self.declared_up {
            return;
        }
        let Some(started) = self.recovery_started_ms else {
            return;
        };
        if self.consecutive_ok >= cfg.recovery_successes
            && now_ms.saturating_sub(started) >= cfg.recovery_hold_ms
        {
            self.declared_up = true;
        }
    }

    /// Ease toward the target. `dt_ms` is the frame delta.
    ///
    /// If WAN is dead, the *target* is 0 immediately (dead-man), even mid-ease.
    pub fn step(&mut self, now_ms: u64, dt_ms: u64, quality: &Quality, cfg: &EngineConfig) {
        if !quality.wan_reachable {
            self.declared_up = false;
            self.consecutive_ok = 0;
            self.recovery_started_ms = None;
        } else {
            self.try_promote(now_ms, cfg);
        }

        let target = if quality.wan_reachable && self.declared_up {
            quality.score
        } else {
            0.0
        };

        if cfg.reduced_motion {
            self.displayed = target;
            return;
        }

        let tau = cfg.ease_tau_ms.max(1.0);
        let dt = dt_ms as f64;
        let alpha = 1.0 - (-dt / tau).exp();
        self.displayed += (target - self.displayed) * alpha;
        if (self.displayed - target).abs() < 0.002 {
            self.displayed = target;
        }
        self.displayed = self.displayed.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Quality;

    fn up_quality(score: f64) -> Quality {
        Quality {
            score,
            wan_reachable: true,
            ..Quality::dead()
        }
    }

    #[test]
    fn one_success_does_not_leave_violet() {
        let cfg = EngineConfig::default();
        let mut d = DisplayState::new();
        d.on_round(0, true, &cfg);
        d.step(0, 0, &up_quality(0.9), &cfg);
        assert!(!d.declared_up());
        assert!(d.displayed() < 0.01);
    }

    #[test]
    fn reduced_motion_snaps() {
        let mut cfg = EngineConfig::default();
        cfg.reduced_motion = true;
        cfg.recovery_successes = 1;
        cfg.recovery_hold_ms = 0;
        let mut d = DisplayState::new();
        d.on_round(0, true, &cfg);
        let q = up_quality(0.7);
        d.step(0, 0, &q, &cfg);
        assert!((d.displayed() - 0.7).abs() < 1e-9);
    }
}
