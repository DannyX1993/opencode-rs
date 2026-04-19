//! Workspace selector resolution from request query/header input.

use axum::http::{HeaderMap, Uri};

use super::{SelectorSource, WorkspaceSelector, error::ControlPlaneError};

/// Query parameter name for explicit workspace selection.
pub const WORKSPACE_SELECTOR_QUERY: &str = "workspace";
/// Header used as a fallback selector source when query is absent.
pub const WORKSPACE_SELECTOR_HEADER: &str = "x-opencode-workspace";

/// Resolve selector precedence: query first, then header.
///
/// This keeps routing behavior deterministic when both selector sources are
/// present and mirrors the contract defined in the control-plane spec.
pub fn resolve_selector(
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<Option<WorkspaceSelector>, ControlPlaneError> {
    if let Some(raw) = query_selector(uri)? {
        return Ok(Some(WorkspaceSelector {
            raw,
            source: SelectorSource::Query,
        }));
    }

    if let Some(raw) = header_selector(headers)? {
        return Ok(Some(WorkspaceSelector {
            raw,
            source: SelectorSource::Header,
        }));
    }

    Ok(None)
}

fn query_selector(uri: &Uri) -> Result<Option<String>, ControlPlaneError> {
    let Some(query) = uri.query() else {
        return Ok(None);
    };

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key != WORKSPACE_SELECTOR_QUERY {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(ControlPlaneError::SelectorMalformed {
                message: "workspace query parameter must be non-empty".into(),
            });
        }
        return Ok(Some(value.to_string()));
    }
    Ok(None)
}

fn header_selector(headers: &HeaderMap) -> Result<Option<String>, ControlPlaneError> {
    let Some(value) = headers.get(WORKSPACE_SELECTOR_HEADER) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ControlPlaneError::SelectorMalformed {
            message: "workspace selector header must be valid UTF-8".into(),
        })?
        .trim()
        .to_string();

    if value.is_empty() {
        return Err(ControlPlaneError::SelectorMalformed {
            message: "workspace selector header must be non-empty".into(),
        });
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, Uri};

    #[test]
    fn query_selector_takes_precedence_over_header() {
        let uri: Uri = "/api/v1/sessions/abc?workspace=from-query".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            WORKSPACE_SELECTOR_HEADER,
            HeaderValue::from_static("from-header"),
        );

        let selector = resolve_selector(&uri, &headers)
            .expect("resolution should succeed")
            .expect("selector should exist");
        assert_eq!(selector.raw, "from-query");
        assert_eq!(selector.source, SelectorSource::Query);
    }

    #[test]
    fn header_selector_used_when_query_is_missing() {
        let uri: Uri = "/api/v1/sessions/abc".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            WORKSPACE_SELECTOR_HEADER,
            HeaderValue::from_static("from-header"),
        );

        let selector = resolve_selector(&uri, &headers)
            .expect("resolution should succeed")
            .expect("selector should exist");
        assert_eq!(selector.raw, "from-header");
        assert_eq!(selector.source, SelectorSource::Header);
    }

    #[test]
    fn empty_query_selector_returns_bad_request_error() {
        let uri: Uri = "/api/v1/sessions/abc?workspace=".parse().unwrap();
        let headers = HeaderMap::new();
        let err = resolve_selector(&uri, &headers).expect_err("empty selector must fail");
        assert!(matches!(err, ControlPlaneError::SelectorMalformed { .. }));
    }
}
