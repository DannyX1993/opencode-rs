//! Route policy table for control-plane forwarding eligibility.

use axum::http::Method;

/// Policy action chosen for a request path + method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    /// Route can participate in selector-based local/forward decisioning.
    Eligible,
    /// Route must always execute in-process.
    LocalOnly,
}

/// Static route policy matcher.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoutePolicy;

impl RoutePolicy {
    /// Classify request routing behavior for the control-plane middleware.
    ///
    /// Parity note:
    /// - `/session/status` remains forward-eligible (remote runtime status check).
    /// - `GET /session/**` is intentionally local-only to match existing TS behavior
    ///   that serves cached/session-read flows in-process for rollback safety.
    /// - non-GET mutable session alias routes (for example `POST /session/:id/abort`)
    ///   remain forward-eligible.
    #[must_use]
    pub fn classify(&self, method: &Method, path: &str) -> PolicyAction {
        let mut segments = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
        if matches!(segments.as_slice(), ["api", "v1", ..]) {
            segments = segments.split_off(2);
        }

        match segments.as_slice() {
            ["projects", _, "sessions"] if method == Method::GET => PolicyAction::Eligible,
            ["projects", _, "sessions"] if method == Method::POST => PolicyAction::Eligible,
            ["sessions", _] if method == Method::GET => PolicyAction::Eligible,
            ["sessions", _] if method == Method::PATCH => PolicyAction::Eligible,
            ["sessions", _, "messages"] if method == Method::GET => PolicyAction::Eligible,
            ["sessions", _, "messages"] if method == Method::POST => PolicyAction::Eligible,
            ["sessions", _, "prompt"] if method == Method::POST => PolicyAction::Eligible,
            ["sessions", _, "cancel"] if method == Method::POST => PolicyAction::Eligible,
            ["session", "status"] if method == Method::GET => PolicyAction::Eligible,
            ["session", ..] if method == Method::GET => PolicyAction::LocalOnly,
            ["session", _, "abort"] if method == Method::POST => PolicyAction::Eligible,
            _ => PolicyAction::LocalOnly,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TS parity fixtures copied from `opencode-ts/packages/opencode/src/server/router.ts`.
    ///
    /// The TS control-plane keeps `GET /session/**` local (except `/session/status`),
    /// while non-GET session endpoints continue to be forward-eligible.
    #[derive(Debug, Clone)]
    struct TsParityFixture {
        method: Method,
        path: &'static str,
        expected: PolicyAction,
    }

    #[test]
    fn session_routes_are_eligible_for_forwarding() {
        let policy = RoutePolicy;
        assert_eq!(
            policy.classify(&Method::POST, "/api/v1/sessions/abc/prompt"),
            PolicyAction::Eligible
        );
        assert_eq!(
            policy.classify(&Method::GET, "/api/v1/projects/pid/sessions"),
            PolicyAction::Eligible
        );
        assert_eq!(
            policy.classify(&Method::GET, "/sessions/abc"),
            PolicyAction::Eligible
        );
    }

    #[test]
    fn workspace_and_config_routes_stay_local_only() {
        let policy = RoutePolicy;
        assert_eq!(
            policy.classify(&Method::GET, "/api/v1/workspaces"),
            PolicyAction::LocalOnly
        );
        assert_eq!(
            policy.classify(&Method::PATCH, "/api/v1/config"),
            PolicyAction::LocalOnly
        );
    }

    #[test]
    fn ts_route_policy_parity_fixtures_match_expected_actions() {
        let policy = RoutePolicy;
        let fixtures = [
            TsParityFixture {
                method: Method::GET,
                path: "/api/v1/session/status",
                expected: PolicyAction::Eligible,
            },
            TsParityFixture {
                method: Method::GET,
                path: "/api/v1/session/abc/status",
                expected: PolicyAction::LocalOnly,
            },
            TsParityFixture {
                method: Method::GET,
                path: "/api/v1/session/abc/message",
                expected: PolicyAction::LocalOnly,
            },
            TsParityFixture {
                method: Method::POST,
                path: "/api/v1/session/abc/abort",
                expected: PolicyAction::Eligible,
            },
        ];

        for fixture in fixtures {
            assert_eq!(
                policy.classify(&fixture.method, fixture.path),
                fixture.expected,
                "parity mismatch for {} {}",
                fixture.method,
                fixture.path
            );
        }
    }
}
