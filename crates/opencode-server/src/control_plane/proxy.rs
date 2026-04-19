//! HTTP forwarding implementation for `RoutingDecision::Forward`.

use std::time::Duration;
use std::time::Instant;

use axum::{
    body::{self, Body},
    http::{HeaderMap, HeaderName, HeaderValue, Request, Response},
};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::state::ProxyPolicy;

use super::{
    RemoteTarget, WorkspaceSelector, error::ControlPlaneError, observability::ControlPlaneMetrics,
};

const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
];

const SELECTOR_HEADER: &str = "x-opencode-workspace";
const FORWARDED_SELECTOR_HEADER: &str = "x-opencode-forwarded-workspace-selector";
const FORWARDED_SOURCE_HEADER: &str = "x-opencode-forwarded-workspace-source";
const FORWARDED_TARGET_HEADER: &str = "x-opencode-forwarded-workspace-instance";

/// Proxy service used by control-plane middleware to forward HTTP requests.
#[derive(Clone)]
pub struct HttpProxyService {
    client: reqwest::Client,
    policy: ProxyPolicy,
}

impl HttpProxyService {
    /// Build a proxy using an explicit transport client and retry policy.
    #[must_use]
    pub fn new(client: reqwest::Client, policy: ProxyPolicy) -> Self {
        Self { client, policy }
    }

    /// Build a proxy with a default reqwest client.
    #[must_use]
    pub fn with_policy(policy: ProxyPolicy) -> Self {
        Self {
            client: reqwest::Client::new(),
            policy,
        }
    }

    /// Forward one HTTP request to the resolved remote target.
    ///
    /// Method, path/query, and body are preserved. Hop-by-hop/internal headers
    /// are stripped so forwarded traffic is proxy-safe.
    pub async fn forward(
        &self,
        request: Request<Body>,
        target: &RemoteTarget,
        selector: &WorkspaceSelector,
        metrics: &ControlPlaneMetrics,
    ) -> Result<Response<Body>, ControlPlaneError> {
        let started_at = Instant::now();
        let (parts, body) = request.into_parts();
        let url = upstream_url(&target.base_url, &parts.uri)?;
        let target_url = url.to_string();
        let request_headers = build_upstream_headers(&parts.headers, target, selector)?;
        let method =
            reqwest::Method::from_bytes(parts.method.as_str().as_bytes()).map_err(|err| {
                ControlPlaneError::Internal {
                    message: format!("invalid HTTP method for forwarding: {err}"),
                }
            })?;
        let payload =
            body::to_bytes(body, usize::MAX)
                .await
                .map_err(|err| ControlPlaneError::Internal {
                    message: format!("failed to read request body before forwarding: {err}"),
                })?;

        let mut last_error: Option<reqwest::Error> = None;
        let mut attempts = 0_u32;
        // `max_retries` means extra attempts after the first try, so the loop
        // allows `1 + max_retries` total executions.
        while attempts <= self.policy.max_retries {
            attempts += 1;
            metrics.record_forward_attempt(attempts, &target_url);
            let request_builder = self
                .client
                .request(method.clone(), url.clone())
                .headers(request_headers.clone())
                .body(payload.clone())
                .timeout(self.policy.timeout);

            match request_builder.send().await {
                Ok(response) => {
                    info!(
                        selector = %selector.raw,
                        selector_source = %selector.source.as_str(),
                        target_url = %target_url,
                        attempts,
                        latency_ms = started_at.elapsed().as_millis(),
                        upstream_status = response.status().as_u16(),
                        "control-plane proxy forward succeeded"
                    );
                    return response_to_axum(response).await;
                }
                Err(err) if err.is_timeout() && attempts <= self.policy.max_retries => {
                    metrics.record_forward_retry(attempts, "timeout");
                    sleep(backoff_for_attempt(self.policy.backoff, attempts)).await;
                    last_error = Some(err);
                }
                Err(err) if err.is_timeout() => {
                    metrics.record_timeout_failure(attempts);
                    warn!(
                        selector = %selector.raw,
                        selector_source = %selector.source.as_str(),
                        target_url = %target_url,
                        attempts,
                        latency_ms = started_at.elapsed().as_millis(),
                        failure_cause = "timeout",
                        "control-plane proxy forward timed out"
                    );
                    return Err(ControlPlaneError::UpstreamTimeout { attempts });
                }
                Err(err) if attempts <= self.policy.max_retries => {
                    metrics.record_forward_retry(attempts, &err.to_string());
                    sleep(backoff_for_attempt(self.policy.backoff, attempts)).await;
                    last_error = Some(err);
                }
                Err(err) => {
                    warn!(
                        selector = %selector.raw,
                        selector_source = %selector.source.as_str(),
                        target_url = %target_url,
                        attempts,
                        latency_ms = started_at.elapsed().as_millis(),
                        failure_cause = %err,
                        "control-plane proxy forward failed"
                    );
                    return Err(ControlPlaneError::UpstreamFailure {
                        message: err.to_string(),
                    });
                }
            }
        }

        match last_error {
            Some(err) if err.is_timeout() => Err(ControlPlaneError::UpstreamTimeout { attempts }),
            Some(err) => Err(ControlPlaneError::UpstreamFailure {
                message: err.to_string(),
            }),
            None => Err(ControlPlaneError::Internal {
                message: "proxy retry loop exited without response".into(),
            }),
        }
    }
}

/// Detect WebSocket upgrade intent in an HTTP request.
#[must_use]
pub fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    let has_upgrade = headers
        .get("upgrade")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    let has_connection_upgrade = headers
        .get("connection")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false);

    has_upgrade && has_connection_upgrade
}

fn backoff_for_attempt(base: Duration, attempt: u32) -> Duration {
    let multiplier = 2_u32.saturating_pow(attempt.saturating_sub(1));
    base.saturating_mul(multiplier)
}

fn upstream_url(
    base_url: &str,
    request_uri: &axum::http::Uri,
) -> Result<reqwest::Url, ControlPlaneError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|err| ControlPlaneError::Internal {
        message: format!("invalid remote base_url '{base_url}': {err}"),
    })?;
    url.set_path(request_uri.path());
    url.set_query(request_uri.query());
    Ok(url)
}

fn build_upstream_headers(
    source: &HeaderMap,
    target: &RemoteTarget,
    selector: &WorkspaceSelector,
) -> Result<reqwest::header::HeaderMap, ControlPlaneError> {
    let mut out = reqwest::header::HeaderMap::new();

    for (name, value) in source {
        // Strip both hop-by-hop headers and the original selector header.
        // We forward selector context via explicit `x-opencode-forwarded-*`
        // headers to avoid ambiguous precedence on downstream hops.
        if should_strip_header(name.as_str()) || name.as_str().eq_ignore_ascii_case(SELECTOR_HEADER)
        {
            continue;
        }
        out.insert(name, value.clone());
    }

    out.insert(
        HeaderName::from_static(FORWARDED_SELECTOR_HEADER),
        HeaderValue::from_str(&selector.raw).map_err(|err| ControlPlaneError::Internal {
            message: format!("invalid selector header value: {err}"),
        })?,
    );
    out.insert(
        HeaderName::from_static(FORWARDED_SOURCE_HEADER),
        HeaderValue::from_static(selector.source.as_str()),
    );
    out.insert(
        HeaderName::from_static(FORWARDED_TARGET_HEADER),
        HeaderValue::from_str(&target.instance_id).map_err(|err| ControlPlaneError::Internal {
            message: format!("invalid target instance header value: {err}"),
        })?,
    );

    Ok(out)
}

async fn response_to_axum(
    response: reqwest::Response,
) -> Result<Response<Body>, ControlPlaneError> {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| ControlPlaneError::UpstreamFailure {
            message: err.to_string(),
        })?;

    let mut builder = Response::builder().status(status);
    if let Some(target) = builder.headers_mut() {
        for (name, value) in headers {
            if let Some(name) = name {
                if should_strip_header(name.as_str()) {
                    continue;
                }
                target.insert(name, value);
            }
        }
    }
    builder
        .body(Body::from(bytes))
        .map_err(|err| ControlPlaneError::Internal {
            message: format!("failed to build proxied response: {err}"),
        })
}

fn should_strip_header(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use super::*;
    use axum::http::{Method, Request, StatusCode};
    use tracing_subscriber::layer::SubscriberExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    use crate::control_plane::{
        SelectorSource, WorkspaceSelector, observability::ControlPlaneMetrics,
    };

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

    #[tokio::test]
    async fn forward_preserves_method_path_query_body_and_status() {
        let upstream = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/api/v1/sessions/abc"))
            .and(query_param("workspace", "wid"))
            .and(header("x-opencode-forwarded-workspace-selector", "wid"))
            .respond_with(ResponseTemplate::new(207).set_body_string("proxied"))
            .mount(&upstream)
            .await;

        let service = HttpProxyService::with_policy(ProxyPolicy::default());
        let metrics = ControlPlaneMetrics::default();
        let request = Request::builder()
            .method(Method::PATCH)
            .uri("/api/v1/sessions/abc?workspace=wid")
            .header("content-type", "application/json")
            .header("x-opencode-workspace", "wid")
            .body(Body::from(r#"{"name":"value"}"#))
            .unwrap();
        let selector = WorkspaceSelector {
            raw: "wid".into(),
            source: SelectorSource::Query,
        };
        let target = RemoteTarget {
            workspace_id: "0195df90-4283-7b90-a362-d47f3076a913".parse().unwrap(),
            instance_id: "cp-remote".into(),
            base_url: upstream.uri(),
        };

        let response = service
            .forward(request, &target, &selector, &metrics)
            .await
            .expect("forward should succeed");
        assert_eq!(response.status(), StatusCode::MULTI_STATUS);
        let body = body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(body, "proxied");
        assert_eq!(metrics.forward_attempts(), 1);
        assert_eq!(metrics.forward_retries(), 0);
    }

    #[tokio::test]
    async fn websocket_upgrade_is_detected() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("Upgrade"));
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        assert!(is_websocket_upgrade(&headers));
    }

    #[tokio::test]
    async fn timeout_maps_to_gateway_timeout_after_retries() {
        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/sessions/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(180))
                    .set_body_string("slow"),
            )
            .mount(&upstream)
            .await;

        let policy = ProxyPolicy::bounded(Duration::from_millis(20), 1, Duration::from_millis(5));
        let service = HttpProxyService::with_policy(policy);
        let metrics = ControlPlaneMetrics::default();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/sessions/abc")
            .body(Body::empty())
            .unwrap();
        let selector = WorkspaceSelector {
            raw: "wid".into(),
            source: SelectorSource::Header,
        };
        let target = RemoteTarget {
            workspace_id: "0195df90-4283-7b90-a362-d47f3076a913".parse().unwrap(),
            instance_id: "cp-remote".into(),
            base_url: upstream.uri(),
        };

        let err = service
            .forward(request, &target, &selector, &metrics)
            .await
            .expect_err("slow upstream should time out");
        assert!(matches!(err, ControlPlaneError::UpstreamTimeout { .. }));
        assert_eq!(metrics.forward_attempts(), 2);
        assert_eq!(metrics.forward_retries(), 1);
        assert_eq!(metrics.forward_timeout_failures(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_logs_include_structured_observability_fields() {
        let captured = SharedLogWriter::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(false)
                .without_time()
                .with_ansi(false)
                .with_writer(captured.clone()),
        );
        let _subscriber = tracing::subscriber::set_default(subscriber);

        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/sessions/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(180))
                    .set_body_string("slow"),
            )
            .mount(&upstream)
            .await;

        let policy = ProxyPolicy::bounded(Duration::from_millis(20), 0, Duration::from_millis(5));
        let service = HttpProxyService::with_policy(policy);
        let metrics = ControlPlaneMetrics::default();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/sessions/abc")
            .body(Body::empty())
            .unwrap();
        let selector = WorkspaceSelector {
            raw: "wid".into(),
            source: SelectorSource::Header,
        };
        let target = RemoteTarget {
            workspace_id: "0195df90-4283-7b90-a362-d47f3076a913".parse().unwrap(),
            instance_id: "cp-remote".into(),
            base_url: upstream.uri(),
        };

        let err = service
            .forward(request, &target, &selector, &metrics)
            .await
            .expect_err("slow upstream should time out");
        assert!(matches!(
            err,
            ControlPlaneError::UpstreamTimeout { attempts: 1 }
        ));

        let logs = captured.contents();
        assert!(logs.contains("\"selector_source\":\"header\""));
        assert!(logs.contains("\"target_url\":"));
        assert!(logs.contains("\"latency_ms\":"));
        assert!(logs.contains("\"failure_cause\":\"timeout\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn success_logs_include_selector_target_and_latency_fields() {
        let captured = SharedLogWriter::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(false)
                .without_time()
                .with_ansi(false)
                .with_writer(captured.clone()),
        );
        let _subscriber = tracing::subscriber::set_default(subscriber);

        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/sessions/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&upstream)
            .await;

        let service = HttpProxyService::with_policy(ProxyPolicy::default());
        let metrics = ControlPlaneMetrics::default();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/sessions/ok")
            .body(Body::empty())
            .unwrap();
        let selector = WorkspaceSelector {
            raw: "wid".into(),
            source: SelectorSource::Query,
        };
        let target = RemoteTarget {
            workspace_id: "0195df90-4283-7b90-a362-d47f3076a913".parse().unwrap(),
            instance_id: "cp-remote".into(),
            base_url: upstream.uri(),
        };

        let response = service
            .forward(request, &target, &selector, &metrics)
            .await
            .expect("forward should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        let logs = captured.contents();
        assert!(logs.contains("\"selector_source\":\"query\""));
        assert!(logs.contains("\"target_url\":"));
        assert!(logs.contains("\"latency_ms\":"));
    }

    #[tokio::test]
    async fn forward_strips_internal_headers_and_preserves_authorization() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/sessions/abc/messages"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&upstream)
            .await;

        let service = HttpProxyService::with_policy(ProxyPolicy::default());
        let metrics = ControlPlaneMetrics::default();
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/sessions/abc/messages")
            .header("authorization", "Bearer token-value")
            .header("x-opencode-workspace", "wid")
            .header("connection", "keep-alive")
            .body(Body::from("{}"))
            .unwrap();
        let selector = WorkspaceSelector {
            raw: "wid".into(),
            source: SelectorSource::Header,
        };
        let target = RemoteTarget {
            workspace_id: "0195df90-4283-7b90-a362-d47f3076a913".parse().unwrap(),
            instance_id: "cp-remote".into(),
            base_url: upstream.uri(),
        };

        let response = service
            .forward(request, &target, &selector, &metrics)
            .await
            .expect("forward should succeed");
        assert_eq!(response.status(), StatusCode::CREATED);

        let requests = upstream.received_requests().await.unwrap();
        let forwarded = requests.first().expect("upstream should receive request");
        assert_eq!(
            forwarded
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer token-value"
        );
        assert!(!forwarded.headers.contains_key("x-opencode-workspace"));
        assert!(!forwarded.headers.contains_key("connection"));
        assert!(
            forwarded
                .headers
                .contains_key("x-opencode-forwarded-workspace-selector")
        );
    }
}
