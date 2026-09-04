//! Probe samples are *data*. A timeout is a fact, not an exception.

/// Stable id of a probe target (e.g. `"http-google-204"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetId(pub String);

impl TargetId {
    /// Create a target id from any string-like value.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TargetId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for TargetId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// How the probe was performed. ICMP never votes for WAN reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    /// Tiny HTTP(S) connectivity check (204 / short body).
    Http,
    /// Direct DNS lookup with its own timeout.
    Dns,
    /// Optional ICMP echo. Informational only — never a WAN voter.
    Icmp,
}

/// Outcome of a single probe attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Reached the target. `rtt_ms` is milliseconds (TTFB or resolve time).
    Success {
        /// Round-trip / TTFB in milliseconds.
        rtt_ms: u32,
    },
    /// Hard timeout (no useful response inside the probe budget).
    Timeout,
    /// Transport/TLS/DNS-servfail style failure (not a captive portal).
    TransportFail,
    /// HTTP looks like a captive portal (redirect/HTML login), not WAN.
    Captive {
        /// HTTP status that triggered classification.
        http_status: u16,
    },
}

impl ProbeOutcome {
    /// True when the probe got a genuine success (not captive).
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// RTT when successful.
    pub fn rtt_ms(self) -> Option<u32> {
        match self {
            Self::Success { rtt_ms } => Some(rtt_ms),
            _ => None,
        }
    }
}

/// One finished probe, timestamped with the engine clock (`ms` from an arbitrary epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSample {
    /// Engine clock millisecond when the probe completed.
    pub at_ms: u64,
    /// Which target was probed.
    pub target: TargetId,
    /// Protocol used.
    pub kind: ProbeKind,
    /// What happened.
    pub outcome: ProbeOutcome,
}

impl ProbeSample {
    /// HTTP connectivity success.
    pub fn http_ok(at_ms: u64, target: impl Into<TargetId>, rtt_ms: u32) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Http,
            outcome: ProbeOutcome::Success { rtt_ms },
        }
    }

    /// HTTP timeout.
    pub fn http_timeout(at_ms: u64, target: impl Into<TargetId>) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Http,
            outcome: ProbeOutcome::Timeout,
        }
    }

    /// HTTP transport failure.
    pub fn http_fail(at_ms: u64, target: impl Into<TargetId>) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Http,
            outcome: ProbeOutcome::TransportFail,
        }
    }

    /// Captive-portal HTTP response.
    pub fn http_captive(at_ms: u64, target: impl Into<TargetId>, http_status: u16) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Http,
            outcome: ProbeOutcome::Captive { http_status },
        }
    }

    /// DNS success.
    pub fn dns_ok(at_ms: u64, target: impl Into<TargetId>, rtt_ms: u32) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Dns,
            outcome: ProbeOutcome::Success { rtt_ms },
        }
    }

    /// DNS timeout.
    pub fn dns_timeout(at_ms: u64, target: impl Into<TargetId>) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Dns,
            outcome: ProbeOutcome::Timeout,
        }
    }

    /// DNS transport / empty-answer failure (not a timeout).
    pub fn dns_fail(at_ms: u64, target: impl Into<TargetId>) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Dns,
            outcome: ProbeOutcome::TransportFail,
        }
    }

    /// ICMP timeout (must not, by itself, paint the path dead).
    pub fn icmp_timeout(at_ms: u64, target: impl Into<TargetId>) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Icmp,
            outcome: ProbeOutcome::Timeout,
        }
    }

    /// ICMP success (still not a WAN voter).
    pub fn icmp_ok(at_ms: u64, target: impl Into<TargetId>, rtt_ms: u32) -> Self {
        Self {
            at_ms,
            target: target.into(),
            kind: ProbeKind::Icmp,
            outcome: ProbeOutcome::Success { rtt_ms },
        }
    }
}

/// HTTP and DNS may vote for WAN. ICMP must not.
pub fn is_wan_voter(kind: ProbeKind) -> bool {
    matches!(kind, ProbeKind::Http | ProbeKind::Dns)
}

/// True when this sample is evidence the public internet answered.
pub fn is_wan_success(sample: &ProbeSample) -> bool {
    is_wan_voter(sample.kind) && sample.outcome.is_success()
}

/// True when this sample is a captive-portal classification.
pub fn is_captive(sample: &ProbeSample) -> bool {
    matches!(sample.outcome, ProbeOutcome::Captive { .. })
}
