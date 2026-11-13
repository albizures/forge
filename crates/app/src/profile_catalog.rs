//! Filesystem catalog discovery does not execute extensions or connect servers.
use super::agent_profiles::{ProfileDocument, ResourceReference};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Canonical selected file and its narrow SHA-256 coverage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectResource {
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}

/// A selected MCP entry. Configuration is persisted, not reread from ambient settings.
/// Debug intentionally omits launch configuration, which may contain credentials.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpResource {
    pub name: String,
    pub path: PathBuf,
    pub configuration: serde_json::Value,
    pub sha256: String,
}
impl std::fmt::Debug for McpResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpResource")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Shared immutable boundary for preview, registration, preflight and runtime loading.
/// It freezes policy, not dependencies, services, credentials or runtime versions.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResolvedLaunchPolicy {
    pub version: u32,
    pub runtime: String,
    pub model: String,
    pub tools: Vec<String>,
    pub behavior: String,
    pub skills: Vec<DirectResource>,
    pub extensions: Vec<DirectResource>,
    pub mcp_servers: Vec<McpResource>,
}
impl std::fmt::Debug for ResolvedLaunchPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedLaunchPolicy")
            .field("version", &self.version)
            .field("runtime", &self.runtime)
            .field("skills", &self.skills)
            .field("extensions", &self.extensions)
            .field("mcp_servers", &self.mcp_servers)
            .finish_non_exhaustive()
    }
}
impl ResolvedLaunchPolicy {
    /// Recheck only selected entry files and frozen MCP configuration, never profile source
    /// Markdown or supporting files. Existing Assignment operations must not call this.
    /// # Errors
    /// Missing or changed direct inputs reject a new launch or supported reload.
    pub fn validate_direct_inputs(&self) -> Result<(), String> {
        super::agent_profiles::validate_header(
            self.version,
            &self.runtime,
            &self.model,
            &self.tools,
        )?;
        for entry in self.skills.iter().chain(&self.extensions) {
            if !entry.path.is_absolute()
                || entry.name.trim().is_empty()
                || !entry
                    .path
                    .canonicalize()
                    .is_ok_and(|path| path == entry.path)
            {
                return Err("selected direct input identity is not canonical".into());
            }
            if hash(&fs::read(&entry.path).map_err(|_| "selected direct input unavailable")?)
                != entry.sha256
            {
                return Err("selected direct input drifted".into());
            }
        }
        for entry in &self.mcp_servers {
            if !entry.path.is_absolute()
                || entry.name.trim().is_empty()
                || !entry.configuration.is_object()
            {
                return Err("invalid persisted MCP identity or configuration".into());
            }
            if hash(
                &serde_json::to_vec(&entry.configuration)
                    .map_err(|_| "invalid MCP configuration")?,
            ) != entry.sha256
            {
                return Err("persisted MCP configuration drifted".into());
            }
        }
        Ok(())
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Unified operator and Workspace membership; paths disambiguate but never add members.
#[derive(Default)]
pub struct ResourceCatalog {
    skills: Vec<DirectResource>,
    extensions: Vec<DirectResource>,
    mcp_servers: Vec<McpResource>,
}
impl ResourceCatalog {
    /// Discover operator and Workspace `.agents/skills`, `.pi/skills`,
    /// `.pi/extensions`, and `.pi/mcp.json`. Both roots must be explicit absolute paths.
    /// The operator root is the home/catalog root, not an execution directory.
    /// # Errors
    /// Malformed or unreadable installed catalogs fail closed.
    pub fn discover(operator: &Path, workspace: &Path) -> Result<Self, String> {
        let mut catalog = Self::default();
        for root in [operator, workspace] {
            if !root.is_absolute() {
                return Err("catalog roots must be absolute".into());
            }
            for directory in [".agents/skills", ".pi/skills", ".pi/agent/skills"] {
                discover_files(
                    &root.join(directory),
                    true,
                    &mut catalog.skills,
                    &mut Vec::new(),
                )?;
            }
            for directory in [".pi/extensions", ".pi/agent/extensions"] {
                discover_files(
                    &root.join(directory),
                    false,
                    &mut catalog.extensions,
                    &mut Vec::new(),
                )?;
            }
            for file in [".pi/mcp.json", ".pi/agent/mcp.json"] {
                let path = root.join(file);
                if !path.exists() {
                    continue;
                }
                let path = path.canonicalize().map_err(|_| "MCP catalog unavailable")?;
                let config: serde_json::Value = serde_json::from_slice(
                    &fs::read(&path).map_err(|_| "MCP catalog unavailable")?,
                )
                .map_err(|_| "invalid MCP catalog")?;
                let servers = config
                    .get("mcpServers")
                    .and_then(serde_json::Value::as_object)
                    .ok_or("invalid MCP server catalog")?;
                for (name, configuration) in servers {
                    if name.trim().is_empty() || !configuration.is_object() {
                        return Err("invalid MCP server entry".into());
                    }
                    let entry = McpResource {
                        name: name.clone(),
                        path: path.clone(),
                        configuration: configuration.clone(),
                        sha256: hash(
                            &serde_json::to_vec(configuration)
                                .map_err(|_| "invalid MCP configuration")?,
                        ),
                    };
                    if !catalog.mcp_servers.contains(&entry) {
                        catalog.mcp_servers.push(entry);
                    }
                }
            }
        }
        Ok(catalog)
    }

    /// Resolve a Markdown profile beneath the selected Workspace's .agents/profiles,
    /// not the client cwd. Resource references remain relative to the document.
    /// # Errors
    /// Rejects missing, ambiguous, or nonmember references without substitution.
    pub fn resolve(
        &self,
        workspace: &Path,
        document: impl AsRef<Path>,
    ) -> Result<ResolvedLaunchPolicy, String> {
        if !workspace.is_absolute() {
            return Err("Workspace root must be absolute".into());
        }
        let workspace = workspace
            .canonicalize()
            .map_err(|_| "Workspace root unavailable")?;
        let profiles = workspace.join(".agents/profiles");
        let canonical_profiles = profiles
            .canonicalize()
            .map_err(|_| "profile directory unavailable")?;
        // Do not let the designated directory (or its .agents parent) redirect
        // elsewhere, even inside the Workspace: that could broaden containment
        // to the entire Workspace or a skill directory.
        if canonical_profiles != profiles {
            return Err("profile directory must not redirect outside .agents/profiles".into());
        }
        let path = workspace
            .join(document)
            .canonicalize()
            .map_err(|_| "profile document unavailable")?;
        if !path.starts_with(&canonical_profiles)
            || path.extension().is_none_or(|extension| extension != "md")
            || !path.is_file()
        {
            return Err("expected a Markdown file within .agents/profiles".into());
        }
        let document = ProfileDocument::parse(
            &fs::read_to_string(&path).map_err(|_| "profile document unavailable")?,
        )?;
        let base = path.parent().ok_or("missing document directory")?;
        let resolve_files = |references: &[ResourceReference],
                             entries: &[DirectResource]|
         -> Result<Vec<DirectResource>, String> {
            let mut selected = Vec::new();
            for reference in references {
                let matches: Vec<_> = entries
                    .iter()
                    .filter(|entry| matches_reference(reference, base, &entry.name, &entry.path))
                    .collect();
                if matches.len() != 1 {
                    return Err("missing or ambiguous catalog resource".into());
                }
                if !selected.contains(matches[0]) {
                    selected.push(matches[0].clone());
                }
            }
            Ok(selected)
        };
        let mut mcp_servers = Vec::new();
        for reference in &document.mcp_servers {
            let matches: Vec<_> = self
                .mcp_servers
                .iter()
                .filter(|entry| matches_reference(reference, base, &entry.name, &entry.path))
                .collect();
            if matches.len() != 1 {
                return Err("missing or ambiguous MCP catalog resource".into());
            }
            if !mcp_servers.contains(matches[0]) {
                mcp_servers.push(matches[0].clone());
            }
        }
        let policy = ResolvedLaunchPolicy {
            version: document.version,
            runtime: document.runtime,
            model: document.model,
            tools: document.tools,
            behavior: document.behavior,
            skills: resolve_files(&document.skills, &self.skills)?,
            extensions: resolve_files(&document.extensions, &self.extensions)?,
            mcp_servers,
        };
        policy.validate_direct_inputs()?;
        Ok(policy)
    }
}
fn matches_reference(reference: &ResourceReference, base: &Path, name: &str, path: &Path) -> bool {
    match reference {
        ResourceReference::Name { name: value } => value == name,
        ResourceReference::Path { path: value } => {
            let mut candidate = base.join(value);
            if candidate.is_dir()
                && let Some(entry_name) = path.file_name()
            {
                candidate = candidate.join(entry_name);
            }
            candidate
                .canonicalize()
                .is_ok_and(|canonical| canonical == path)
        }
    }
}
fn discover_files(
    directory: &Path,
    skills: bool,
    entries: &mut Vec<DirectResource>,
    visited: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    let directory = directory
        .canonicalize()
        .map_err(|_| "catalog directory unavailable")?;
    if visited.contains(&directory) {
        return Ok(());
    }
    visited.push(directory.clone());
    for child in fs::read_dir(directory).map_err(|_| "catalog directory unavailable")? {
        let mut path = child.map_err(|_| "catalog entry unavailable")?.path();
        if path.is_dir() {
            if skills {
                discover_files(&path, skills, entries, visited)?;
                continue;
            }
            let Some(entry) = ["index.ts", "index.js", "index.mjs"]
                .iter()
                .map(|name| path.join(name))
                .find(|entry| entry.is_file())
            else {
                continue;
            };
            path = entry;
        }
        if (skills && path.file_name().is_some_and(|name| name == "SKILL.md"))
            || (!skills
                && path
                    .extension()
                    .is_some_and(|ext| ext == "ts" || ext == "js" || ext == "mjs"))
        {
            let path = path
                .canonicalize()
                .map_err(|_| "catalog entry unavailable")?;
            if entries.iter().any(|entry| entry.path == path) {
                continue;
            }
            let bytes = fs::read(&path).map_err(|_| "catalog entry unavailable")?;
            let name = if skills {
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| "invalid skill catalog entry")?;
                let yaml = text
                    .strip_prefix("---\n")
                    .and_then(|text| text.split_once("\n---"))
                    .ok_or("invalid skill frontmatter")?
                    .0;
                let metadata: serde_yaml::Value =
                    serde_yaml::from_str(yaml).map_err(|_| "invalid skill frontmatter")?;
                metadata
                    .get("name")
                    .and_then(serde_yaml::Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or("missing skill name")?
                    .to_string()
            } else {
                let name = if path.file_stem().is_some_and(|name| name == "index") {
                    path.parent().and_then(Path::file_name)
                } else {
                    path.file_stem()
                };
                name.and_then(|name| name.to_str())
                    .ok_or("invalid extension name")?
                    .to_string()
            };
            entries.push(DirectResource {
                name,
                path,
                sha256: hash(&bytes),
            });
        }
    }
    Ok(())
}
