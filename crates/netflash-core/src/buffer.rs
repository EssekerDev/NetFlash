//! Ring of timestamped samples. Pruning is explicit so tests can freeze time.

use std::collections::VecDeque;

use crate::sample::ProbeSample;

/// In-memory sample ring. Oldest-first.
#[derive(Debug, Clone, Default)]
pub struct SampleBuffer {
    samples: VecDeque<ProbeSample>,
}

impl SampleBuffer {
    /// Empty buffer.
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
        }
    }

    /// Append a sample. Does not prune; the engine prunes on clock advance.
    pub fn push(&mut self, sample: ProbeSample) {
        self.samples.push_back(sample);
    }

    /// Drop samples strictly older than `now_ms - max_age_ms`.
    pub fn prune(&mut self, now_ms: u64, max_age_ms: u64) {
        let cutoff = now_ms.saturating_sub(max_age_ms);
        self.drop_older_than(cutoff);
    }

    /// Drop samples with `at_ms < cutoff_ms`.
    pub fn drop_older_than(&mut self, cutoff_ms: u64) {
        while self.samples.front().is_some_and(|s| s.at_ms < cutoff_ms) {
            self.samples.pop_front();
        }
    }

    /// All samples currently retained.
    pub fn iter(&self) -> impl Iterator<Item = &ProbeSample> {
        self.samples.iter()
    }

    /// Samples in `(now - window, now]` (inclusive on the right).
    pub fn in_window(&self, now_ms: u64, window_ms: u64) -> impl Iterator<Item = &ProbeSample> {
        let cutoff = now_ms.saturating_sub(window_ms);
        self.samples
            .iter()
            .filter(move |s| s.at_ms >= cutoff && s.at_ms <= now_ms)
    }
}
