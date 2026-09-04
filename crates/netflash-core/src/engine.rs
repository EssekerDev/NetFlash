//! Clock-driven host that glues buffer, scorer, display hysteresis, and scheduler.
//!
//! No I/O. Tests and `netflash-app` inject [`ProbeSample`]s and advance time.

use crate::buffer::SampleBuffer;
use crate::color::{color_for_score, Srgb8};
use crate::config::EngineConfig;
use crate::display::DisplayState;
use crate::sample::{is_wan_success, ProbeSample};
use crate::scheduler::{Scheduler, SchedulerMode};
use crate::score::{evaluate, Band, Quality};

/// Pure engine. Drive it with [`Engine::ingest_round`] and [`Engine::advance_to`].
#[derive(Debug, Clone)]
pub struct Engine {
    cfg: EngineConfig,
    now_ms: u64,
    buffer: SampleBuffer,
    display: DisplayState,
    scheduler: Scheduler,
    paused: bool,
    quality: Quality,
}

/// Immutable view for the tray / tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineSnapshot {
    /// Engine clock.
    pub now_ms: u64,
    /// Latest scored window (truth).
    pub quality: Quality,
    /// Painted score after hysteresis + ease.
    pub displayed_score: f64,
    /// Named band of the painted score.
    pub displayed_band: Band,
    /// Color of the painted score.
    pub displayed_color: Srgb8,
    /// Probe cadence mode.
    pub scheduler: SchedulerMode,
    /// When true, the scheduler will not fire.
    pub paused: bool,
}

impl Engine {
    /// New engine at t=0, painted violet, scheduler bursting (cold start).
    pub fn new(cfg: EngineConfig) -> Self {
        Self {
            scheduler: Scheduler::new(0),
            cfg,
            now_ms: 0,
            buffer: SampleBuffer::new(),
            display: DisplayState::new(),
            paused: false,
            quality: Quality::dead(),
        }
    }

    /// Live config.
    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }

    /// Current engine clock, milliseconds.
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Probe scheduler (spacing / in-flight).
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Pause probing. Display freezes on its last paint; truth still updates if
    /// samples are ingested (tests), but the app should stop ingesting too.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Advance the clock, prune, rescore, ease the display.
    ///
    /// Use this for animation frames. Do **not** use it to timestamp a new
    /// probe round — [`ingest_round`] jumps the clock without treating the gap
    /// as display time.
    pub fn advance_to(&mut self, now_ms: u64) {
        let now_ms = now_ms.max(self.now_ms);
        let dt = now_ms.saturating_sub(self.now_ms);
        self.now_ms = now_ms;
        self.prune();
        self.quality = evaluate(&self.buffer, now_ms, &self.cfg, self.scheduler.mode());
        self.display.step(now_ms, dt, &self.quality, &self.cfg);
    }

    /// Ingest a parallel probe round. Clock jumps to the latest sample time
    /// without easing across the gap (the gap is “we were waiting”, not a frame).
    pub fn ingest_round(&mut self, samples: impl IntoIterator<Item = ProbeSample>) {
        let samples: Vec<_> = samples.into_iter().collect();
        if samples.is_empty() {
            return;
        }
        let at = samples.iter().map(|s| s.at_ms).max().unwrap_or(self.now_ms);
        if at > self.now_ms {
            self.now_ms = at;
            self.prune();
        }
        let mut any = false;
        for s in samples {
            if is_wan_success(&s) {
                any = true;
            }
            self.buffer.push(s);
        }
        self.quality = evaluate(&self.buffer, self.now_ms, &self.cfg, self.scheduler.mode());
        self.display.on_round(self.now_ms, any, &self.cfg);
        if !self.paused {
            self.scheduler.on_round(self.now_ms, any, &self.cfg);
        }
        self.display.step(self.now_ms, 0, &self.quality, &self.cfg);
    }

    /// OS path change: drop stale samples and burst immediately.
    pub fn path_changed(&mut self) {
        let cutoff = self.now_ms.saturating_sub(self.cfg.stale_ms);
        self.buffer.drop_older_than(cutoff);
        self.scheduler.interrupt(self.now_ms);
        self.quality = evaluate(&self.buffer, self.now_ms, &self.cfg, self.scheduler.mode());
        self.display.step(self.now_ms, 0, &self.quality, &self.cfg);
    }

    /// Sleep-wake: same as path change (stale samples lie).
    pub fn resume_from_sleep(&mut self) {
        self.path_changed();
    }

    /// Current snapshot for UI.
    pub fn snapshot(&self) -> EngineSnapshot {
        let displayed = self.display.displayed();
        EngineSnapshot {
            now_ms: self.now_ms,
            quality: self.quality.clone(),
            displayed_score: displayed,
            displayed_band: Band::from_score(displayed),
            displayed_color: color_for_score(displayed),
            scheduler: self.scheduler.mode(),
            paused: self.paused,
        }
    }

    /// Record that the host launched a probe round now.
    pub fn mark_probed(&mut self) {
        self.scheduler.mark_probed(self.now_ms);
    }

    /// Whether the host should fire a probe round at the current clock.
    pub fn should_probe(&self) -> bool {
        self.scheduler
            .should_probe(self.now_ms, &self.cfg, self.paused)
    }

    fn prune(&mut self) {
        let max_age = self.cfg.flap_window_ms.max(self.cfg.quality_window_ms);
        self.buffer.prune(self.now_ms, max_age);
    }
}
