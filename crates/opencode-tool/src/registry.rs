//! Thread-safe tool registry.

use crate::common::Ctx;
use crate::types::{Tool, ToolCall, ToolDefinition, ToolError, ToolResult};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

/// Registry of all available tools, keyed by name.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Create a registry pre-populated with all built-in tools for the given context.
    #[must_use]
    pub fn with_builtins(ctx: Ctx) -> Self {
        let reg = Self::new();
        let tools = crate::tools::all(ctx);
        let mut map = reg
            .tools
            .try_write()
            .expect("no contention during construction");
        for tool in tools {
            map.insert(tool.name().to_string(), tool);
        }
        drop(map);
        reg
    }

    /// Register a tool.
    pub async fn register(&self, tool: Arc<dyn Tool>) {
        self.tools
            .write()
            .await
            .insert(tool.name().to_string(), tool);
    }

    /// Retrieve a tool by name.
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    /// Retrieve all registered tool definitions sorted by tool name.
    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        let mut defs: Vec<ToolDefinition> = tools
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    /// Retrieve one tool definition by tool name.
    pub async fn definition(&self, name: &str) -> Option<ToolDefinition> {
        let tool = self.get(name).await?;
        Some(ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            input_schema: tool.input_schema(),
        })
    }

    /// Invoke a tool by name.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NotFound`] if the tool is not registered.
    pub async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let tool = self
            .get(&call.name)
            .await
            .ok_or_else(|| ToolError::NotFound(format!("tool '{}' not registered", call.name)))?;
        tool.invoke(call).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Tool, ToolCall, ToolError, ToolPolicy, ToolResult};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
        }
        async fn invoke(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::ok(
                call.id,
                "echo".into(),
                call.args.to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn register_and_invoke() {
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).await;
        let call = ToolCall {
            id: "1".into(),
            name: "echo".into(),
            args: serde_json::json!({"x": 1}),
        };
        let res = reg.invoke(call).await.unwrap();
        assert!(!res.is_err);
        assert!(res.output.contains("1"));
    }

    #[tokio::test]
    async fn invoke_missing_returns_not_found() {
        let reg = ToolRegistry::new();
        let call = ToolCall {
            id: "1".into(),
            name: "nope".into(),
            args: serde_json::json!({}),
        };
        let err = reg.invoke(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_returns_none_for_missing() {
        let reg = ToolRegistry::new();
        assert!(reg.get("nope").await.is_none());
    }

    #[tokio::test]
    async fn with_builtins_registers_all_six() {
        use std::path::PathBuf;
        let ctx = Ctx::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/out"),
            "/bin/sh".into(),
            5_000,
        );
        let reg = ToolRegistry::with_builtins(ctx);
        for name in &["read", "list", "glob", "grep", "write", "bash"] {
            assert!(reg.get(name).await.is_some(), "missing tool: {name}");
        }
    }

    #[tokio::test]
    async fn definitions_expose_schema_and_descriptions() {
        use std::path::PathBuf;

        let ctx = Ctx::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/out"),
            "/bin/sh".into(),
            5_000,
        );
        let reg = ToolRegistry::with_builtins(ctx);

        let defs = reg.definitions().await;
        assert_eq!(defs.len(), 6);
        assert!(defs.iter().any(|d| d.name == "bash"));
        assert!(defs.iter().all(|d| !d.description.is_empty()));
        assert!(defs.iter().all(|d| d.input_schema.is_object()));
    }

    #[tokio::test]
    async fn definition_returns_single_tool_metadata() {
        use std::path::PathBuf;

        let ctx = Ctx::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/out"),
            "/bin/sh".into(),
            5_000,
        );
        let reg = ToolRegistry::with_builtins(ctx);

        let bash = reg.definition("bash").await.expect("bash definition");
        assert_eq!(bash.name, "bash");
        assert!(bash.input_schema["properties"]["command"].is_object());
        assert!(reg.definition("nope").await.is_none());
    }
}
