//! Shared immutable Revision Identity for Forge executables.

use serde::Serialize;

/// The management protocol implemented by this build.
///
/// # Panics
///
/// Panics only when an invalid protocol revision bypassed build-script validation.
#[must_use]
pub fn management_protocol_revision() -> u32 {
    env!("FORGE_MANAGEMENT_PROTOCOL_REVISION")
        .parse()
        .expect("build script validates FORGE_MANAGEMENT_PROTOCOL_REVISION")
}

/// Immutable build facts emitted by both Forge executables.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BuildInfo {
    /// Full source commit used to build this executable.
    pub commit: &'static str,
    /// Whether the source checkout had uncommitted changes when built.
    pub dirty: bool,
    /// Monotonic revision of the Forge management protocol.
    pub management_protocol_revision: u32,
    /// Build-provided version metadata.
    pub version: &'static str,
}

/// Returns the Revision Identity compiled into this build.
#[must_use]
pub fn current() -> BuildInfo {
    BuildInfo {
        commit: env!("FORGE_BUILD_COMMIT"),
        dirty: matches!(env!("FORGE_BUILD_DIRTY"), "true"),
        management_protocol_revision: management_protocol_revision(),
        version: env!("FORGE_BUILD_VERSION"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_has_the_stable_machine_readable_schema() {
        let json = serde_json::to_value(current()).unwrap();
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["commit", "dirty", "management_protocol_revision", "version",]
        );
        assert!(json["commit"].is_string());
        assert!(json["dirty"].is_boolean());
        assert!(json["management_protocol_revision"].is_u64());
        assert!(json["version"].is_string());
    }
}
