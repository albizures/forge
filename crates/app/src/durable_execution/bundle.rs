//! Fixed layout and pins of a matched durable-runner bundle (T15/T24).
//!
//! Forgeup installs and attests a bundle; the daemon derives every execution
//! component from an attested bundle root using only these fixed locations.
//! Neither side consults `PATH`, npm or a manifest-selected path.

/// Bundle manifest file name at the bundle root.
pub const MANIFEST: &str = "forge-runner-bundle.json";
/// The pinned Node executable.
pub const NODE: &str = "node/node";
/// The Linux single-owner supervisor (`forge-runner-host`), schema 2 and later.
pub const HOST: &str = "host/forge-runner-host";
/// The shipped production Check runner entry (`runCheckRunner`, no substitution).
pub const ENTRY: &str = "runner/src/check-entry.ts";
/// The matched read-only interrupted-effect inspector.
pub const EFFECT_INSPECTOR: &str = "runner/src/inspect-effects.ts";
/// The matched read-only recovery inspector.
pub const RECOVERY_INSPECTOR: &str = "runner/src/inspect-recovery.ts";
/// The matched read-only answer inspector.
pub const ANSWER_INSPECTOR: &str = "runner/src/inspect-answer.ts";
/// The locked JavaScript dependency closure.
pub const LOCK: &str = "runner/package-lock.json";
/// The Node version every matched runner of this Forge build is pinned to
/// (`runner/package.json` `engines.node`).
pub const PINNED_NODE_VERSION: &str = "24.18.0";

#[cfg(test)]
mod tests {
    #[test]
    fn pinned_node_version_matches_the_runner_package() {
        let package = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runner/package.json"),
        )
        .unwrap();
        let package: serde_json::Value = serde_json::from_str(&package).unwrap();
        assert_eq!(package["engines"]["node"], super::PINNED_NODE_VERSION);
    }
}
