//! Offline engine fixtures.
//!
//! These tests never touch the network. They drive [`Engine`] with a fake clock
//! and synthetic [`ProbeSample`]s.

use netflash_core::{
    color_for_score, Band, Engine, EngineConfig, ProbeSample, Reason, SchedulerMode,
};

fn good_round(at_ms: u64, rtt_ms: u32) -> Vec<ProbeSample> {
    vec![
        ProbeSample::http_ok(at_ms, "http-a", rtt_ms),
        ProbeSample::http_ok(at_ms, "http-b", rtt_ms),
        ProbeSample::dns_ok(at_ms, "dns-a", rtt_ms / 2),
    ]
}

fn fail_round(at_ms: u64) -> Vec<ProbeSample> {
    vec![
        ProbeSample::http_timeout(at_ms, "http-a"),
        ProbeSample::http_timeout(at_ms, "http-b"),
        ProbeSample::dns_timeout(at_ms, "dns-a"),
    ]
}

fn engine() -> Engine {
    Engine::new(EngineConfig::default())
}

#[test]
fn tunnel_8s_goes_none_despite_last_good_rtt() {
    let mut eng = engine();
    for t in [0_u64, 300, 600, 900] {
        eng.ingest_round(good_round(t, 25));
    }
    let up = eng.snapshot();
    assert!(up.quality.wan_reachable);
    assert!(up.quality.rtt_p50_ms.unwrap() <= 30);
    assert_ne!(up.quality.band, Band::None);

    // Tunnel starts at t=1000. Last success is 900; dead-man fires at 1700.
    for t in (1000..=9000).step_by(300) {
        eng.ingest_round(fail_round(t));
    }
    eng.advance_to(9000);
    let down = eng.snapshot();
    assert!(!down.quality.wan_reachable);
    assert_eq!(down.quality.score, 0.0);
    assert_eq!(down.quality.band, Band::None);
    assert!(down.quality.reasons.contains(&Reason::DeadMan));
}

#[test]
fn dead_man_ignores_stale_good_rtt() {
    let mut eng = engine();
    eng.ingest_round(good_round(0, 25));
    assert_eq!(eng.snapshot().quality.rtt_p50_ms, Some(25));
    eng.advance_to(801);
    let snap = eng.snapshot();
    assert_eq!(snap.quality.score, 0.0);
    assert!(!snap.quality.wan_reachable);
    assert_eq!(snap.quality.band, Band::None);
}

#[test]
fn flap_50_percent_caps_at_medium_never_ultra() {
    let mut eng = engine();
    for i in 0..40 {
        let t = i * 400;
        if i % 2 == 0 {
            eng.ingest_round(good_round(t, 20));
        } else {
            eng.ingest_round(fail_round(t));
        }
    }
    // Land on a success round so dead-man is not the story.
    eng.ingest_round(good_round(40 * 400, 20));
    let snap = eng.snapshot();
    assert!(snap.quality.wan_reachable, "last round succeeded");
    assert!(
        snap.quality.flap_edges > 4,
        "expected many edges, got {}",
        snap.quality.flap_edges
    );
    assert!(
        snap.quality.score <= 0.54 + 1e-9,
        "flap cap is 0.54, got {}",
        snap.quality.score
    );
    assert_ne!(snap.quality.band, Band::Ultra);
    assert!(snap.quality.reasons.contains(&Reason::Flap));
}

#[test]
fn icmp_blocked_http_ok_is_not_none() {
    let mut eng = engine();
    let t = 0;
    eng.ingest_round([
        ProbeSample::http_ok(t, "http-a", 40),
        ProbeSample::http_ok(t, "http-b", 42),
        ProbeSample::dns_ok(t, "dns-a", 15),
        ProbeSample::icmp_timeout(t, "icmp-a"),
    ]);
    let snap = eng.snapshot();
    assert!(snap.quality.wan_reachable);
    assert_ne!(snap.quality.band, Band::None);
}

#[test]
fn http_ok_dns_fail_is_not_none_and_dns_pulls_down() {
    let mut both = engine();
    both.ingest_round(good_round(0, 30));
    let both_score = both.snapshot().quality.score;

    let mut dns_fail = engine();
    dns_fail.ingest_round([
        ProbeSample::http_ok(0, "http-a", 30),
        ProbeSample::http_ok(0, "http-b", 30),
        ProbeSample::dns_timeout(0, "dns-a"),
    ]);
    let snap = dns_fail.snapshot();
    assert!(snap.quality.wan_reachable);
    assert_ne!(snap.quality.band, Band::None);
    assert!(
        snap.quality.score < both_score,
        "DNS fail should pull score down ({} vs both-ok {both_score})",
        snap.quality.score
    );
    assert!(snap.quality.reasons.contains(&Reason::Dns));
}

#[test]
fn captive_302_is_none_or_bad_never_green() {
    let mut eng = engine();
    eng.ingest_round([
        ProbeSample::http_captive(0, "http-a", 302),
        ProbeSample::http_captive(0, "http-b", 302),
    ]);
    let snap = eng.snapshot();
    assert!(snap.quality.captive);
    assert!(!snap.quality.wan_reachable);
    assert!(snap.quality.score <= 0.15);
    assert!(matches!(snap.quality.band, Band::None | Band::Bad));
    assert_ne!(snap.quality.band, Band::Ok);
    assert_ne!(snap.quality.band, Band::Ultra);
    assert!(snap.quality.reasons.contains(&Reason::Captive));
}

#[test]
fn one_dead_target_plus_one_live_target_is_not_none() {
    let mut eng = engine();
    eng.ingest_round([
        ProbeSample::http_timeout(0, "http-a"),
        ProbeSample::http_ok(0, "http-b", 45),
        ProbeSample::dns_ok(0, "dns-a", 12),
    ]);
    let snap = eng.snapshot();
    assert!(snap.quality.wan_reachable);
    assert_ne!(snap.quality.band, Band::None);
}

#[test]
fn one_success_after_tunnel_does_not_leave_displayed_none() {
    let mut eng = engine();
    eng.ingest_round(good_round(0, 30));
    for t in (1000..=2500).step_by(300) {
        eng.ingest_round(fail_round(t));
    }
    assert_eq!(eng.snapshot().quality.band, Band::None);

    eng.ingest_round(good_round(3000, 30));
    let snap = eng.snapshot();
    assert!(
        snap.quality.wan_reachable,
        "truth may already be up after one success"
    );
    assert_eq!(
        snap.displayed_band,
        Band::None,
        "paint must stay violet until K successes + hold"
    );
    assert!(snap.tooltip().contains("No connection"));
}

#[test]
fn recovery_requires_k_successes_and_hold() {
    let mut cfg = EngineConfig::default();
    cfg.reduced_motion = true;
    let mut eng = Engine::new(cfg);
    // Start from down.
    eng.ingest_round(fail_round(0));
    eng.advance_to(800);
    assert_eq!(eng.snapshot().displayed_band, Band::None);

    // Three successes but only 600 ms after the first — hold is 1200 ms.
    for (i, t) in [2000_u64, 2300, 2600].into_iter().enumerate() {
        eng.ingest_round(good_round(t, 30));
        if i < 2 {
            assert_eq!(eng.snapshot().displayed_band, Band::None);
        }
    }
    assert_eq!(
        eng.snapshot().displayed_band,
        Band::None,
        "hold not elapsed yet"
    );

    // Keep succeeding until hold (first recovery success at 2000 → 3200).
    eng.ingest_round(good_round(3200, 30));
    let snap = eng.snapshot();
    assert!(snap.quality.wan_reachable);
    assert_ne!(snap.displayed_band, Band::None);
    assert!(snap.displayed_score > 0.0);
}

#[test]
fn color_stops_and_violet_red_not_green() {
    let violet = color_for_score(0.0);
    assert_eq!((violet.r, violet.g, violet.b), (0x7C, 0x3A, 0xED));
    let blue = color_for_score(1.0);
    assert_eq!((blue.r, blue.g, blue.b), (0x3B, 0x82, 0xF6));
    let mid = color_for_score(0.04);
    assert!(mid.r > mid.g, "violet→red must not pass through green");
}

#[test]
fn miss_in_stable_bursts_immediately() {
    let mut eng = engine();
    // 10 s of clean successes → scheduler should go Stable (burst_hold is 8 s).
    for t in (0..=10_000).step_by(300) {
        eng.ingest_round(good_round(t, 30));
        eng.mark_probed();
    }
    assert_eq!(eng.snapshot().scheduler, SchedulerMode::Stable);
    assert!(!eng.should_probe());

    eng.ingest_round(fail_round(10_300));
    assert_eq!(eng.snapshot().scheduler, SchedulerMode::Uncertain);
    assert!(
        eng.should_probe(),
        "a miss in stable is an interrupt, not a 1.5 s wait"
    );
}

#[test]
fn stable_1_5s_gap_is_not_a_false_outage() {
    let mut cfg = EngineConfig::default();
    cfg.reduced_motion = true;
    let mut eng = Engine::new(cfg);
    for t in (0..=10_000).step_by(300) {
        eng.ingest_round(good_round(t, 30));
        eng.mark_probed();
    }
    assert_eq!(eng.snapshot().scheduler, SchedulerMode::Stable);
    assert!(eng.snapshot().quality.wan_reachable);
    assert_ne!(eng.snapshot().displayed_band, Band::None);

    // Frugal interval is 1.5 s; dead-man is 800 ms. The gap must not go violet.
    eng.advance_to(10_000 + 800);
    let snap = eng.snapshot();
    assert!(
        snap.quality.wan_reachable,
        "stable must survive dead_ms without a new probe"
    );
    assert_ne!(snap.displayed_band, Band::None);
}

#[test]
fn path_change_bursts_immediately() {
    let mut eng = engine();
    for t in (0..=10_000).step_by(300) {
        eng.ingest_round(good_round(t, 30));
        eng.mark_probed();
    }
    assert_eq!(eng.snapshot().scheduler, SchedulerMode::Stable);
    eng.path_changed();
    assert_ne!(eng.snapshot().scheduler, SchedulerMode::Stable);
    assert!(eng.should_probe());
}

#[test]
fn pause_blocks_should_probe() {
    let mut eng = engine();
    assert!(eng.should_probe());
    eng.set_paused(true);
    assert!(!eng.should_probe());
    assert!(eng.snapshot().tooltip().contains("Paused"));
}
