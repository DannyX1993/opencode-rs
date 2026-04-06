//! Thread-safe tool registry.

use crate::types::{Tool, ToolCall, ToolError, ToolResult};
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
            Ok(ToolResult {
                call_id: call.id,
                ok: true,
                content: call.args.to_string(),
            })
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
        assert!(res.ok);
        assert!(res.content.contains("1"));
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
}
