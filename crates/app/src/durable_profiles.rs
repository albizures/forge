//! Explicit initial durable compatibility policy; never used for historical cleanup.
use crate::agent_profiles::{ResolvedLaunchPolicy, ThinkingLevel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read};

/// Bound on all behavioral/context UTF-8 bytes delivered for a new launch.
pub const MAX_CONTEXT_BYTES: usize = 256 * 1024;

pub(crate) fn validate_configuration(
    model: &str,
    thinking: Option<ThinkingLevel>,
    tools: &[String],
    unsupported_resources: bool,
) -> Result<(), String> {
    if model != "openai/gpt-4.1" || thinking != Some(ThinkingLevel::Off) {
        return Err("durable execution requires openai/gpt-4.1 and thinkingLevel off".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    if tools.iter().any(|tool| {
        !["read", "write", "edit", "bash"].contains(&tool.as_str()) || !seen.insert(tool)
    }) {
        return Err("durable tools must be a unique subset of read, write, edit, bash".into());
    }
    if unsupported_resources {
        return Err("durable extensions and MCP servers are unsupported".into());
    }
    Ok(())
}

/// Approved content and provenance only: no provider credentials or capabilities.
/// Repository instructions are loaded only when explicitly selected as resources.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub instructions: String,
    pub resources: Vec<crate::agent_profiles::DirectResource>,
}

/// Forge-owned resolved runner input, not a capability or scheduling permission.
/// Credentials are deliberately absent. Deliver only through trusted host setup.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionConfiguration {
    pub version: u32,
    pub model: String,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<String>,
    pub context: ContextSnapshot,
    pub cwd: std::path::PathBuf,
}

impl std::fmt::Debug for ExecutionConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionConfiguration")
            .field("version", &self.version)
            .field("model", &self.model)
            .field("tools", &self.tools)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ContextSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextSnapshot")
            .field("resource_count", &self.resources.len())
            .finish_non_exhaustive()
    }
}

impl crate::AgentProfile {
    /// Resolve only an explicitly compatible immutable Task profile. Historical
    /// operations must not use this new-launch-only source validation.
    /// # Errors
    /// Rejects incompatible backend, Record Type, model identity or source inputs.
    pub fn durable_execution_configuration(
        &self,
        cwd: &std::path::Path,
    ) -> Result<ExecutionConfiguration, String> {
        if self.record_type != crate::RecordType::Task
            || self.provider != "pi-durable"
            || self.agent_kind != "pi"
        {
            return Err("an explicit compatible pi-durable Task revision is required".into());
        }
        let policy = self
            .launch_policy
            .as_ref()
            .ok_or("registered durable policy required")?;
        if policy.model != self.model {
            return Err("durable profile model identity mismatch".into());
        }
        policy.durable_execution_configuration(cwd)
    }
}

impl ResolvedLaunchPolicy {
    /// Resolve a new-launch configuration from approved inputs and an explicitly
    /// supplied canonical execution directory. The caller must bind this to the
    /// Assignment's worktree; a valid path alone is not Assignment authority.
    /// # Errors
    /// Rejects incompatible configuration, unavailable sources and invalid cwd.
    pub fn durable_execution_configuration(
        &self,
        cwd: &std::path::Path,
    ) -> Result<ExecutionConfiguration, String> {
        if !cwd.is_absolute() || !cwd.is_dir() || !cwd.canonicalize().is_ok_and(|path| path == cwd)
        {
            return Err(
                "durable execution requires an explicit canonical working directory".into(),
            );
        }
        Ok(ExecutionConfiguration {
            version: 1,
            model: self.model.clone(),
            thinking_level: ThinkingLevel::Off,
            tools: self.tools.clone(),
            context: self.durable_context()?,
            cwd: cwd.to_path_buf(),
        })
    }

    /// Validate compatibility without consulting mutable source files.
    /// # Errors
    /// Rejects unsupported or incomplete durable policy.
    pub fn validate_durable_configuration(&self) -> Result<(), String> {
        if self.version != 1 || self.runtime != "pi-durable" {
            return Err("explicit pi-durable profile version 1 required".into());
        }
        validate_configuration(
            &self.model,
            self.thinking_level,
            &self.tools,
            !self.extensions.is_empty() || !self.mcp_servers.is_empty(),
        )?;
        if self.behavior.len() > MAX_CONTEXT_BYTES {
            return Err("durable context exceeds 256 KiB".into());
        }
        if self.skills.iter().any(|entry| {
            !entry.path.is_absolute()
                || entry.path.extension().is_none_or(|ext| ext != "md")
                || entry.name.trim().is_empty()
        }) {
            return Err("durable resources must be explicit canonical Markdown files".into());
        }
        Ok(())
    }

    /// Read bounded approved Markdown and verify the exact bytes rendered as context.
    /// No ancestor AGENTS, Pi settings, executable skills or supporting files are loaded.
    /// # Errors
    /// Source loss, drift, invalid UTF-8 and excess context reject new launches.
    pub fn durable_context(&self) -> Result<ContextSnapshot, String> {
        self.validate_durable_configuration()?;
        let mut instructions = self.behavior.clone();
        for entry in &self.skills {
            if !entry
                .path
                .canonicalize()
                .is_ok_and(|path| path == entry.path)
            {
                return Err("durable resource identity unavailable or redirected".into());
            }
            if !std::fs::symlink_metadata(&entry.path)
                .is_ok_and(|metadata| metadata.file_type().is_file())
            {
                return Err("durable resource must be a regular Markdown file".into());
            }
            let mut bytes = Vec::new();
            File::open(&entry.path)
                .map_err(|_| "durable resource unavailable")?
                .take((MAX_CONTEXT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "durable resource unreadable")?;
            if bytes.len() > MAX_CONTEXT_BYTES {
                return Err("durable context exceeds 256 KiB".into());
            }
            if format!("{:x}", Sha256::digest(&bytes)) != entry.sha256 {
                return Err("durable resource drifted; register a new revision".into());
            }
            let text = String::from_utf8(bytes).map_err(|_| "durable resource must be UTF-8")?;
            // Names/paths are provenance, not injected instruction headings.
            instructions.push_str("\n\n");
            instructions.push_str(&text);
            if instructions.len() > MAX_CONTEXT_BYTES {
                return Err("durable context exceeds 256 KiB".into());
            }
        }
        Ok(ContextSnapshot {
            instructions,
            resources: self.skills.clone(),
        })
    }
}
