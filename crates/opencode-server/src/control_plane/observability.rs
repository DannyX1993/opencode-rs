//! Control-plane observability hooks.
//!
//! This slice intentionally uses lightweight in-process counters so we can
//! emit deterministic metrics-like signals before introducing a dedicated
//! metrics backend. The API is designed as a seam: call sites stay stable while
//! backend wiring evolves later.

use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

use axum::http::Method;
use tracing::{debug, info, warn};

use super::{RemoteTarget, WorkspaceSelector};

/// In-memory control-plane counters used as metric hooks.
#[derive(Debug, Default)]
pub struct ControlPlaneMetrics {
    decisions_local: AtomicU64,
    decisions_forward: AtomicU64,
    forward_attempts: AtomicU64,
    forward_retries: AtomicU64,
    forward_timeout_failures: AtomicU64,
}

impl ControlPlaneMetrics {
    /// Record a selector-resolved request that stayed local.
    pub fn record_local_decision(&self, selector: &WorkspaceSelector, method: &Method, path: &str) {
        self.decisions_local.fetch_add(1, Ordering::Relaxed);
        info!(
            selector = %selector.raw,
            selector_source = %selector.source.as_str(),
            route_decision = "local",
            method = %method,
            path,
            "control-plane decision"
        );
    }

    /// Record a selector-resolved request that will be forwarded.
    pub fn record_forward_decision(
        &self,
        selector: &WorkspaceSelector,
        method: &Method,
        path: &str,
        target: &RemoteTarget,
    ) {
        self.decisions_forward.fetch_add(1, Ordering::Relaxed);
        info!(
            selector = %selector.raw,
            selector_source = %selector.source.as_str(),
            route_decision = "forward",
            method = %method,
            path,
            workspace_id = %target.workspace_id,
            target_instance = %target.instance_id,
            target_base_url = %target.base_url,
            "control-plane decision"
        );
    }

    /// Record one upstream proxy attempt.
    pub fn record_forward_attempt(&self, attempt: u32, target_url: &str) {
        self.forward_attempts.fetch_add(1, Ordering::Relaxed);
        debug!(attempt, target_url, "control-plane proxy attempt");
    }

    /// Record one retry after an upstream failure.
    pub fn record_forward_retry(&self, attempt: u32, cause: &str) {
        self.forward_retries.fetch_add(1, Ordering::Relaxed);
        warn!(attempt, failure_cause = cause, "control-plane proxy retry");
    }

    /// Record final timeout exhaustion after retries.
    pub fn record_timeout_failure(&self, attempts: u32) {
        self.forward_timeout_failures
            .fetch_add(1, Ordering::Relaxed);
        warn!(attempts, "control-plane proxy timeout exhausted retries");
    }

    /// Number of local routing decisions observed.
    #[must_use]
    pub fn decisions_local(&self) -> u64 {
        self.decisions_local.load(Ordering::Relaxed)
    }

    /// Number of forward routing decisions observed.
    #[must_use]
    pub fn decisions_forward(&self) -> u64 {
        self.decisions_forward.load(Ordering::Relaxed)
    }

    /// Number of upstream attempt executions observed.
    #[must_use]
    pub fn forward_attempts(&self) -> u64 {
        self.forward_attempts.load(Ordering::Relaxed)
    }

    /// Number of retries observed after non-success attempts.
    #[must_use]
    pub fn forward_retries(&self) -> u64 {
        self.forward_retries.load(Ordering::Relaxed)
    }

    /// Number of final timeout failures observed.
    #[must_use]
    pub fn forward_timeout_failures(&self) -> u64 {
        self.forward_timeout_failures.load(Ordering::Relaxed)
    }
}

/// Process-wide placeholder metrics registry.
///
/// This is intentionally simple until a backend (OpenTelemetry/Prometheus/etc)
/// is chosen. Callers should depend on this accessor rather than storing global
/// statics themselves.
#[must_use]
pub fn global_metrics() -> &'static ControlPlaneMetrics {
    static METRICS: OnceLock<ControlPlaneMetrics> = OnceLock::new();
    METRICS.get_or_init(ControlPlaneMetrics::default)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use super::*;
    use axum::http::Method;
    use opencode_core::id::WorkspaceId;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::control_plane::{RemoteTarget, SelectorSource, WorkspaceSelector};

    #[derive(Clone, Default)]
    struct SharedLogWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedLogWriter {
        fn contents(&self) -> String {
            let bytes = self
                .bytes
                .lock()
                .expect("log capture lock poisoned")
                .clone();
            String::from_utf8(bytes).expect("captured logs should be valid UTF-8")
        }
    }

    struct SharedWriterGuard {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl io::Write for SharedWriterGuard {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes
                .lock()
                .expect("log capture lock poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedLogWriter {
        type Writer = SharedWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriterGuard {
                bytes: Arc::clone(&self.bytes),
            }
        }
    }

    struct CapturedJsonLogs {
        writer: SharedLogWriter,
    }

    impl CapturedJsonLogs {
        fn contents(&self) -> String {
            self.writer.contents()
        }

        fn install_subscriber(&self) -> tracing::dispatcher::DefaultGuard {
            let subscriber = tracing_subscriber::registry().with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_target(false)
                    .without_time()
                    .with_ansi(false)
                    .with_writer(self.writer.clone()),
            );
            tracing::subscriber::set_default(subscriber)
        }
    }

    fn captured_json_logs() -> CapturedJsonLogs {
        CapturedJsonLogs {
            writer: SharedLogWriter::default(),
        }
    }

    #[test]
    fn counters_increment_for_attempt_retry_and_timeout() {
        let metrics = ControlPlaneMetrics::default();
        metrics.record_forward_attempt(1, "https://upstream.example/path");
        metrics.record_forward_retry(1, "deadline exceeded");
        metrics.record_timeout_failure(2);

        assert_eq!(metrics.forward_attempts(), 1);
        assert_eq!(metrics.forward_retries(), 1);
        assert_eq!(metrics.forward_timeout_failures(), 1);
    }

    #[test]
    fn counters_increment_for_local_and_forward_decisions() {
        let metrics = ControlPlaneMetrics::default();
        let selector = WorkspaceSelector {
            raw: "selector-a".into(),
            source: SelectorSource::Query,
        };
        metrics.record_local_decision(&selector, &Method::GET, "/api/v1/sessions/abc");
        metrics.record_forward_decision(
            &selector,
            &Method::POST,
            "/api/v1/sessions/abc/prompt",
            &RemoteTarget {
                workspace_id: WorkspaceId::new(),
                instance_id: "remote-a".into(),
                base_url: "https://remote-a.example".into(),
            },
        );

        assert_eq!(metrics.decisions_local(), 1);
        assert_eq!(metrics.decisions_forward(), 1);
    }

    #[test]
    fn local_decision_logs_include_selector_source_route_decision_and_path() {
        let captured_logs = captured_json_logs();
        let _subscriber_guard = captured_logs.install_subscriber();

        let metrics = ControlPlaneMetrics::default();
        let selector = WorkspaceSelector {
            raw: "selector-local".into(),
            source: SelectorSource::Header,
        };
        metrics.record_local_decision(&selector, &Method::PATCH, "/api/v1/sessions/local");

        let logs = captured_logs.contents();
        assert!(logs.contains("\"selector_source\":\"header\""));
        assert!(logs.contains("\"route_decision\":\"local\""));
        assert!(logs.contains("\"path\":\"/api/v1/sessions/local\""));
    }

    #[test]
    fn forward_decision_logs_include_target_identity_fields() {
        let captured_logs = captured_json_logs();
        let _subscriber_guard = captured_logs.install_subscriber();

        let metrics = ControlPlaneMetrics::default();
        let selector = WorkspaceSelector {
            raw: "selector-forward".into(),
            source: SelectorSource::Query,
        };
        metrics.record_forward_decision(
            &selector,
            &Method::GET,
            "/api/v1/sessions/forward",
            &RemoteTarget {
                workspace_id: WorkspaceId::new(),
                instance_id: "cp-remote-2".into(),
                base_url: "https://remote-2.example".into(),
            },
        );

        let logs = captured_logs.contents();
        assert!(logs.contains("\"selector_source\":\"query\""));
        assert!(logs.contains("\"route_decision\":\"forward\""));
        assert!(logs.contains("\"target_instance\":\"cp-remote-2\""));
        assert!(logs.contains("\"target_base_url\":\"https://remote-2.example\""));
    }
}
