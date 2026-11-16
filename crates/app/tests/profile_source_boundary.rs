use forge_app::agent_profiles::ResourceCatalog;
use std::fs;

const DOCUMENT: &str = "---\nversion: 1\nruntime: herdr/pi\nmodel: openai/gpt-5\nthinkingLevel: low\ntools: []\nskills: []\n---\nRole instructions";

#[test]
fn accepts_standalone_nested_markdown_and_only_explicit_skills() {
    let root = tempfile::tempdir().unwrap();
    let profiles = root.path().join(".agents/profiles/reviews");
    fs::create_dir_all(&profiles).unwrap();
    fs::write(profiles.join("security.md"), DOCUMENT).unwrap();
    // Adjacency does not implicitly select a skill.
    fs::write(
        profiles.join("SKILL.md"),
        "---\nname: nearby\ndescription: nearby\n---\nSkill",
    )
    .unwrap();
    let catalog = ResourceCatalog::discover(root.path(), root.path()).unwrap();
    for path in [
        profiles.join("security.md"),
        std::path::PathBuf::from(".agents/profiles/reviews/security.md"),
    ] {
        let policy = catalog.resolve(root.path(), path).unwrap();
        assert!(policy.skills.is_empty());
        assert_eq!(policy.behavior, "Role instructions");
    }
}

#[test]
fn rejects_legacy_sources_outside_sources_non_markdown_and_directories() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".agents/profiles/directory.md")).unwrap();
    fs::create_dir_all(root.path().join(".agents/skills/legacy")).unwrap();
    fs::write(
        root.path().join(".agents/skills/legacy/SKILL.md"),
        "---\nname: legacy\ndescription: legacy\n---\nSkill",
    )
    .unwrap();
    for path in [
        ".agents/skills/legacy/forge-agent.md",
        "outside.md",
        ".agents/profiles/config.yaml",
        ".agents/profiles-sibling.md",
    ] {
        fs::write(root.path().join(path), DOCUMENT).unwrap();
    }
    let catalog = ResourceCatalog::discover(root.path(), root.path()).unwrap();
    for path in [
        ".agents/skills/legacy/forge-agent.md",
        "outside.md",
        ".agents/profiles/config.yaml",
        ".agents/profiles-sibling.md",
        ".agents/profiles/../../outside.md",
        ".agents/profiles/directory.md",
        ".agents/profiles/missing.md",
    ] {
        assert!(catalog.resolve(root.path(), path).is_err(), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn rejects_file_directory_and_profiles_root_symlink_escapes() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".agents/profiles")).unwrap();
    fs::write(outside.path().join("external.md"), DOCUMENT).unwrap();
    symlink(
        outside.path().join("external.md"),
        root.path().join(".agents/profiles/escape.md"),
    )
    .unwrap();
    symlink(outside.path(), root.path().join(".agents/profiles/escape")).unwrap();
    let catalog = ResourceCatalog::discover(root.path(), root.path()).unwrap();
    for path in [
        ".agents/profiles/escape.md",
        ".agents/profiles/escape/external.md",
    ] {
        assert!(catalog.resolve(root.path(), path).is_err(), "{path}");
    }
    // A symlink within the source tree is fine when its target stays inside.
    fs::write(root.path().join(".agents/profiles/internal.md"), DOCUMENT).unwrap();
    symlink("internal.md", root.path().join(".agents/profiles/alias.md")).unwrap();
    assert!(
        catalog
            .resolve(root.path(), ".agents/profiles/alias.md")
            .is_ok()
    );
    // Redirecting the profiles root to the Workspace itself must not make every
    // Markdown document in the Workspace an eligible source.
    let redirected = tempfile::tempdir().unwrap();
    fs::create_dir_all(redirected.path().join(".agents")).unwrap();
    fs::write(redirected.path().join("external.md"), DOCUMENT).unwrap();
    symlink(
        redirected.path(),
        redirected.path().join(".agents/profiles"),
    )
    .unwrap();
    assert!(
        catalog
            .resolve(redirected.path(), ".agents/profiles/external.md")
            .is_err()
    );
    let other = tempfile::tempdir().unwrap();
    fs::create_dir_all(other.path().join(".agents")).unwrap();
    symlink(outside.path(), other.path().join(".agents/profiles")).unwrap();
    assert!(
        catalog
            .resolve(other.path(), ".agents/profiles/external.md")
            .is_err()
    );
    let parent = tempfile::tempdir().unwrap();
    fs::create_dir_all(outside.path().join("profiles")).unwrap();
    fs::write(outside.path().join("profiles/external.md"), DOCUMENT).unwrap();
    symlink(outside.path(), parent.path().join(".agents")).unwrap();
    assert!(
        catalog
            .resolve(parent.path(), ".agents/profiles/external.md")
            .is_err()
    );
}
