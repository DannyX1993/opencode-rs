//! Canonical project/worktree repository foundation contracts.

use crate::id::ProjectId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Worktree-local state facts discovered by repository probing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeState {
    /// Checked out branch when available.
    #[serde(default)]
    pub branch: Option<String>,
    /// Checked out HEAD object id when available.
    #[serde(default)]
    pub head_oid: Option<String>,
    /// Dirty marker if known.
    #[serde(default)]
    pub is_dirty: Option<bool>,
}

/// Repository-wide durable state discovered by repository probing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryState {
    /// Default branch if the VCS exposes one.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// Repository HEAD object id if known.
    #[serde(default)]
    pub head_oid: Option<String>,
}

/// Future sync anchor used by apply/revert orchestration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncBasis {
    /// Current HEAD object id.
    #[serde(default)]
    pub head_oid: Option<String>,
    /// Optional merge/rebase base object id.
    #[serde(default)]
    pub base_oid: Option<String>,
    /// Dirty marker if known.
    #[serde(default)]
    pub is_dirty: Option<bool>,
}

/// Normalized project-foundation output from repository inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFoundationRecord {
    /// Project identity owning this foundation state.
    pub project_id: ProjectId,
    /// Canonical worktree root if it can be resolved.
    #[serde(default)]
    pub canonical_worktree: Option<String>,
    /// Canonical repository root if it can be resolved.
    #[serde(default)]
    pub repository_root: Option<String>,
    /// Version-control system kind (e.g. `git`) if known.
    #[serde(default)]
    pub vcs_kind: Option<String>,
    /// Worktree-local checkout state.
    #[serde(default)]
    pub worktree_state: WorktreeState,
    /// Repository-wide durable facts.
    #[serde(default)]
    pub repository_state: RepositoryState,
    /// Sync anchor data for later orchestration layers.
    #[serde(default)]
    pub sync_basis: Option<SyncBasis>,
}

/// Errors returned by [`RepositoryProbe`] implementations.
#[derive(Debug, Error)]
pub enum ProjectProbeError {
    /// Probe backend failed.
    #[error("repository probe failed: {0}")]
    Probe(String),
}

/// Backend-agnostic seam for project repository/worktree inspection.
#[async_trait]
pub trait RepositoryProbe: Send + Sync {
    /// Inspect a worktree path and return normalized project-foundation state.
    async fn inspect(
        &self,
        project_id: ProjectId,
        worktree: &Path,
    ) -> Result<ProjectFoundationRecord, ProjectProbeError>;
}
