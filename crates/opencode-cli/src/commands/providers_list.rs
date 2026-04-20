//! Handler for `providers list` command.

use crate::backend_client::BackendClient;
use opencode_provider::catalog::ProviderListDto;
use serde::Serialize;

/// Scriptable command result payload for providers listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidersListOutcome {
    /// Table/JSON payload for stdout.
    pub stdout: String,
    /// Actionable diagnostic for stderr.
    pub stderr: String,
    /// Process-compatible exit status.
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderRow {
    id: String,
    name: String,
    default_model: String,
    connected: bool,
    models: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProvidersJson {
    providers: Vec<ProviderJsonRow>,
}

#[derive(Debug, Serialize)]
struct ProviderJsonRow {
    id: String,
    name: String,
    default_model: String,
    connected: bool,
    models: Vec<String>,
}

impl ProvidersListOutcome {
    fn success(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn failure(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
        }
    }
}

/// Execute `providers list` and return a deterministic command outcome.
pub async fn run(client: &dyn BackendClient, output: &str) -> ProvidersListOutcome {
    if output != "text" && output != "json" {
        return ProvidersListOutcome::failure(
            format!("error: invalid --output value '{output}': expected 'text' or 'json'\n"),
            2,
        );
    }

    let providers = match client.list_providers().await {
        Ok(value) => value,
        Err(error) => {
            return ProvidersListOutcome::failure(format!("error: {error}\n"), 1);
        }
    };

    let rows = sorted_rows(providers);
    if output == "json" {
        let payload = ProvidersJson {
            providers: rows
                .into_iter()
                .map(|row| ProviderJsonRow {
                    id: row.id,
                    name: row.name,
                    default_model: row.default_model,
                    connected: row.connected,
                    models: row.models,
                })
                .collect(),
        };
        match serde_json::to_string_pretty(&payload) {
            Ok(mut json) => {
                json.push('\n');
                return ProvidersListOutcome::success(json);
            }
            Err(error) => {
                return ProvidersListOutcome::failure(
                    format!("error: failed to encode provider JSON output: {error}\n"),
                    1,
                );
            }
        }
    }

    ProvidersListOutcome::success(format_table(&rows))
}

fn sorted_rows(dto: ProviderListDto) -> Vec<ProviderRow> {
    let mut rows: Vec<ProviderRow> = dto
        .all
        .into_iter()
        .map(|provider| {
            let mut models: Vec<String> = provider.models.into_keys().collect();
            models.sort();
            ProviderRow {
                default_model: dto.default.get(&provider.id).cloned().unwrap_or_default(),
                connected: dto.connected.binary_search(&provider.id).is_ok(),
                id: provider.id,
                name: provider.name,
                models,
            }
        })
        .collect();
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    rows
}

fn format_table(rows: &[ProviderRow]) -> String {
    let mut output = String::from("id\tname\tdefault_model\tconnected\n");
    for row in rows {
        let connected = if row.connected { "yes" } else { "no" };
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.id, row.name, row.default_model, connected
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use opencode_core::id::ProjectId;
    use opencode_provider::catalog::{ProviderInfoDto, ProviderListDto};
    use opencode_session::types::SessionPrompt;
    enum ProvidersResponse {
        Success(ProviderListDto),
        Failure(String),
    }

    struct MockBackendClient {
        providers: ProvidersResponse,
    }

    impl MockBackendClient {
        fn success(dto: ProviderListDto) -> Self {
            Self {
                providers: ProvidersResponse::Success(dto),
            }
        }

        fn failure(message: &str) -> Self {
            Self {
                providers: ProvidersResponse::Failure(message.to_string()),
            }
        }
    }

    #[async_trait]
    impl BackendClient for MockBackendClient {
        async fn list_projects(&self) -> anyhow::Result<Vec<opencode_core::dto::ProjectRow>> {
            Err(anyhow!("not used in providers tests"))
        }

        async fn list_providers(&self) -> anyhow::Result<ProviderListDto> {
            match &self.providers {
                ProvidersResponse::Success(dto) => Ok(dto.clone()),
                ProvidersResponse::Failure(message) => Err(anyhow!(message.clone())),
            }
        }

        async fn list_sessions(
            &self,
            _project_id: ProjectId,
        ) -> anyhow::Result<Vec<opencode_core::dto::SessionRow>> {
            Err(anyhow!("not used in providers tests"))
        }

        async fn create_session(
            &self,
            _project_id: ProjectId,
            _row: opencode_core::dto::SessionRow,
        ) -> anyhow::Result<()> {
            Err(anyhow!("not used in providers tests"))
        }

        async fn prompt_detached(
            &self,
            _prompt: SessionPrompt,
        ) -> anyhow::Result<opencode_core::dto::SessionDetachedPromptDto> {
            Err(anyhow!("not used in providers tests"))
        }
    }

    #[tokio::test]
    async fn table_output_uses_stdout_with_zero_exit() {
        let client = MockBackendClient::success(sample_provider_list());

        let outcome = run(&client, "text").await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());
        assert_eq!(
            outcome.stdout,
            concat!(
                "id\tname\tdefault_model\tconnected\n",
                "anthropic\tAnthropic\tclaude-sonnet-4-5\tyes\n",
                "openai\tOpenAI\tgpt-5\tno\n"
            )
        );
    }

    #[tokio::test]
    async fn json_output_uses_stable_schema() {
        let client = MockBackendClient::success(sample_provider_list());

        let outcome = run(&client, "json").await;

        assert_eq!(outcome.exit_code, 0);
        assert!(outcome.stderr.is_empty());

        let payload: serde_json::Value =
            serde_json::from_str(&outcome.stdout).expect("json output should parse");
        assert_eq!(payload["providers"].as_array().map(Vec::len), Some(2));
        assert_eq!(payload["providers"][0]["id"], "anthropic");
        assert_eq!(
            payload["providers"][0]["default_model"],
            "claude-sonnet-4-5"
        );
        assert_eq!(payload["providers"][0]["connected"], true);
        assert_eq!(
            payload["providers"][0]["models"],
            serde_json::json!(["claude-3-5-haiku", "claude-sonnet-4-5"])
        );
    }

    #[tokio::test]
    async fn backend_error_maps_to_stderr_non_zero_exit() {
        let client = MockBackendClient::failure("backend unavailable");

        let outcome = run(&client, "text").await;

        assert_eq!(outcome.exit_code, 1);
        assert!(outcome.stdout.is_empty());
        assert_eq!(outcome.stderr, "error: backend unavailable\n");
    }

    #[tokio::test]
    async fn invalid_output_maps_to_stderr_non_zero_exit() {
        let client = MockBackendClient::success(sample_provider_list());

        let outcome = run(&client, "yaml").await;

        assert_eq!(outcome.exit_code, 2);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.contains("invalid --output value 'yaml'"));
    }

    fn sample_provider_list() -> ProviderListDto {
        ProviderListDto {
            all: vec![
                ProviderInfoDto {
                    id: "openai".to_string(),
                    name: "OpenAI".to_string(),
                    models: [
                        model_entry("gpt-4o", "GPT-4o"),
                        model_entry("gpt-5", "GPT-5"),
                    ]
                    .into_iter()
                    .collect(),
                },
                ProviderInfoDto {
                    id: "anthropic".to_string(),
                    name: "Anthropic".to_string(),
                    models: [
                        model_entry("claude-sonnet-4-5", "Claude Sonnet 4.5"),
                        model_entry("claude-3-5-haiku", "Claude 3.5 Haiku"),
                    ]
                    .into_iter()
                    .collect(),
                },
            ],
            default: [
                ("anthropic".to_string(), "claude-sonnet-4-5".to_string()),
                ("openai".to_string(), "gpt-5".to_string()),
            ]
            .into_iter()
            .collect(),
            connected: vec!["anthropic".to_string()],
        }
    }

    fn model_entry(id: &str, name: &str) -> (String, opencode_provider::types::ModelInfo) {
        (
            id.to_string(),
            opencode_provider::types::ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
                context_window: 200_000,
                max_output: 8_192,
                vision: true,
            },
        )
    }
}
