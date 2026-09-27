use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLossReason {
    AuthorityRevoked,
    Unauthorized,
    SessionNotFound,
    SessionConflict,
    ConsecutiveTimeouts(u32),
    LeaseExpired,
    Other(String),
}

impl std::fmt::Display for SessionLossReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthorityRevoked => write!(f, "AUTHORITY_REVOKED"),
            Self::Unauthorized => write!(f, "UNAUTHORIZED"),
            Self::SessionNotFound => write!(f, "SESSION_NOT_FOUND"),
            Self::SessionConflict => write!(f, "SESSION_CONFLICT"),
            Self::ConsecutiveTimeouts(n) => write!(f, "CONSECUTIVE_TIMEOUTS({n})"),
            Self::LeaseExpired => write!(f, "LEASE_EXPIRED"),
            Self::Other(s) => write!(f, "OTHER({s})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatDisposition {
    Continue,
    RetryTransient(u32),
    SessionLost(SessionLossReason),
}

/// Classify heartbeat error and determine whether to retry or tear down session.
pub fn classify_heartbeat_error(
    err_str: &str,
    consecutive_failures: u32,
    time_since_last_success: Duration,
    lease_duration: Duration,
) -> HeartbeatDisposition {
    let lower = err_str.to_lowercase();

    // Terminal errors: immediate teardown
    if lower.contains("401") || lower.contains("unauthorized") {
        return HeartbeatDisposition::SessionLost(SessionLossReason::Unauthorized);
    }
    if lower.contains("authority_revoked") || lower.contains("revoked") {
        return HeartbeatDisposition::SessionLost(SessionLossReason::AuthorityRevoked);
    }
    if lower.contains("404") || lower.contains("sessionnotfound") || lower.contains("not found") {
        return HeartbeatDisposition::SessionLost(SessionLossReason::SessionNotFound);
    }
    if lower.contains("409") || lower.contains("conflict") {
        return HeartbeatDisposition::SessionLost(SessionLossReason::SessionConflict);
    }
    if time_since_last_success >= lease_duration {
        return HeartbeatDisposition::SessionLost(SessionLossReason::LeaseExpired);
    }

    // Transient network errors
    if consecutive_failures >= 3 {
        HeartbeatDisposition::SessionLost(SessionLossReason::ConsecutiveTimeouts(consecutive_failures))
    } else {
        HeartbeatDisposition::RetryTransient(consecutive_failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_authority_revoked_tears_down_immediately() {
        let disp = classify_heartbeat_error(
            "heartbeat failed with status 403: AUTHORITY_REVOKED",
            1,
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(disp, HeartbeatDisposition::SessionLost(SessionLossReason::AuthorityRevoked));
    }

    #[test]
    fn test_heartbeat_404_tears_down() {
        let disp = classify_heartbeat_error(
            "heartbeat failed with status 404: SessionNotFound",
            1,
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(disp, HeartbeatDisposition::SessionLost(SessionLossReason::SessionNotFound));
    }

    #[test]
    fn test_heartbeat_401_unauthorized_tears_down() {
        let disp = classify_heartbeat_error(
            "heartbeat failed with status 401 Unauthorized",
            1,
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(disp, HeartbeatDisposition::SessionLost(SessionLossReason::Unauthorized));
    }

    #[test]
    fn test_three_transient_timeouts_before_lease_mark_lost() {
        let timeout_err = "heartbeat request failed: connection timed out";
        // 1st failure: transient retry
        let d1 = classify_heartbeat_error(timeout_err, 1, Duration::from_secs(4), Duration::from_secs(30));
        assert_eq!(d1, HeartbeatDisposition::RetryTransient(1));

        // 2nd failure: transient retry
        let d2 = classify_heartbeat_error(timeout_err, 2, Duration::from_secs(8), Duration::from_secs(30));
        assert_eq!(d2, HeartbeatDisposition::RetryTransient(2));

        // 3rd failure: session lost
        let d3 = classify_heartbeat_error(timeout_err, 3, Duration::from_secs(12), Duration::from_secs(30));
        assert_eq!(d3, HeartbeatDisposition::SessionLost(SessionLossReason::ConsecutiveTimeouts(3)));
    }

    #[test]
    fn test_lease_expiry_forces_session_lost() {
        let transient_err = "heartbeat request failed: broken pipe";
        // Only 1 failure, but time since success >= 30s lease
        let disp = classify_heartbeat_error(transient_err, 1, Duration::from_secs(31), Duration::from_secs(30));
        assert_eq!(disp, HeartbeatDisposition::SessionLost(SessionLossReason::LeaseExpired));
    }
}
