//! Closed, Workspace-rooted Agent Profile authoring and resolved launch policy.
pub use crate::profile_catalog::{
    DirectResource, McpResource, ResolvedLaunchPolicy, ResourceCatalog,
};
use serde::{Deserialize, Serialize};

/// Pi reasoning effort requested by an immutable Agent Profile revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

impl ThinkingLevel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

pub(crate) fn validate_header(
    version: u32,
    runtime: &str,
    model: &str,
    tools: &[String],
) -> Result<(), String> {
    if version != 1 || runtime != "herdr/pi" {
        return Err("unsupported Agent Profile version or runtime".into());
    }
    if !model
        .split_once('/')
        .is_some_and(|(provider, name)| !provider.is_empty() && !name.is_empty())
        || model.chars().any(|c| c.is_whitespace() || c == '*')
    {
        return Err("model must be an exact provider-qualified identifier".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for tool in tools {
        if !["read", "bash", "edit", "write", "grep", "find", "ls"].contains(&tool.as_str())
            || !seen.insert(tool)
        {
            return Err("tools must be a unique explicit built-in allowlist".into());
        }
    }
    Ok(())
}

/// Author-authored policy, excluding Forge-owned lifecycle controls.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProfileDocument {
    pub version: u32,
    pub runtime: String,
    pub model: String,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<String>,
    #[serde(default)]
    pub skills: Vec<ResourceReference>,
    #[serde(default)]
    pub extensions: Vec<ResourceReference>,
    #[serde(default)]
    pub mcp_servers: Vec<ResourceReference>,
    #[serde(skip)]
    pub behavior: String,
}

/// Exactly one catalog name or document-relative path, never an additive glob.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResourceReference {
    Name { name: String },
    Path { path: String },
}

impl ProfileDocument {
    /// Parse strict v1 YAML frontmatter and retain behavioral Markdown verbatim.
    ///
    /// # Errors
    /// Rejects malformed or unsupported policy without interpreting behavior.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.split_inclusive('\n');
        if lines.next().map(str::trim_end) != Some("---") {
            return Err("expected YAML frontmatter".into());
        }
        let mut yaml = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line.trim_end() == "---" {
                closed = true;
                break;
            }
            yaml.push_str(line);
        }
        if !closed {
            return Err("unterminated YAML frontmatter".into());
        }
        // Validate separately so every missing, null or malformed choice has an
        // actionable diagnostic without echoing potentially sensitive YAML values.
        let fields: serde_yaml::Value = serde_yaml::from_str(&yaml)
            .map_err(|_| "invalid Agent Profile frontmatter".to_string())?;
        if fields
            .get("thinkingLevel")
            .is_none_or(|value| serde_yaml::from_value::<ThinkingLevel>(value.clone()).is_err())
        {
            return Err("thinkingLevel is required and must be one of: off, minimal, low, medium, high, xhigh".into());
        }
        let mut document: Self = serde_yaml::from_str(&yaml)
            .map_err(|_| "invalid Agent Profile frontmatter".to_string())?;
        validate_header(
            document.version,
            &document.runtime,
            &document.model,
            &document.tools,
        )?;
        for reference in document
            .skills
            .iter()
            .chain(&document.extensions)
            .chain(&document.mcp_servers)
        {
            let (ResourceReference::Name { name: value } | ResourceReference::Path { path: value }) =
                reference;
            if value.trim().is_empty() {
                return Err("resource references must be nonblank".into());
            }
        }
        document.behavior = lines.collect();
        Ok(document)
    }
}
