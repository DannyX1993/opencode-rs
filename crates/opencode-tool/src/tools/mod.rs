//! Built-in tool implementations for opencode.

pub mod bash;
pub mod glob;
pub mod grep;
pub mod ls;
pub mod read;
pub mod write;

use crate::common::Ctx;
use crate::types::Tool;
use std::sync::Arc;

/// Construct all built-in tools with the given execution context.
pub fn all(ctx: Ctx) -> Vec<Arc<dyn Tool>> {
    let ctx = Arc::new(ctx);
    vec![
        Arc::new(read::ReadTool { ctx: ctx.clone() }),
        Arc::new(ls::LsTool { ctx: ctx.clone() }),
        Arc::new(glob::GlobTool { ctx: ctx.clone() }),
        Arc::new(grep::GrepTool { ctx: ctx.clone() }),
        Arc::new(write::WriteTool { ctx: ctx.clone() }),
        Arc::new(bash::BashTool { ctx: ctx.clone() }),
    ]
}
