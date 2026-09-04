//! Pure WAN-quality engine for NetFlash.
//!
//! This crate has **no I/O and no GUI**. Tests drive it with a fake clock and
//! synthetic [`ProbeSample`]s; `netflash-app` supplies live probes.
//!
//! Invariants:
//! - Gateway / OS "connected" is not proof of internet.
//! - ICMP is never a WAN voter.
//! - Dead-man timeout beats a stale good RTT.
//! - Recovery is conservative; outage is urgent.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod buffer;
mod color;
mod config;
mod display;
mod engine;
mod sample;
mod scheduler;
mod score;

pub use color::{color_for_score, Srgb8, BAND_STOP_COLORS};
pub use config::EngineConfig;
pub use display::DisplayState;
pub use engine::{Engine, EngineSnapshot};
pub use sample::{is_wan_success, ProbeKind, ProbeOutcome, ProbeSample, TargetId};
pub use scheduler::{Scheduler, SchedulerMode};
pub use score::{percentile, Band, Quality, Reason};

impl EngineSnapshot {
    /// One-line English tooltip.
    pub fn tooltip(&self) -> String {
        tooltip(self)
    }
}

fn tooltip(snap: &EngineSnapshot) -> String {
    if snap.paused {
        return "NetFlash · Paused".to_owned();
    }
    if snap.displayed_band == Band::None {
        return "NetFlash · No connection".to_owned();
    }
    let band = match snap.displayed_band {
        Band::None => "No connection",
        Band::Bad => "Bad",
        Band::Medium => "Medium",
        Band::Ok => "OK",
        Band::Ultra => "Ultra",
    };
    let rtt = snap
        .quality
        .rtt_p50_ms
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "—".to_owned());
    let loss_pct = (snap.quality.loss * 100.0).round() as i32;
    format!("NetFlash · {band} · {rtt} · {loss_pct}% loss")
}
