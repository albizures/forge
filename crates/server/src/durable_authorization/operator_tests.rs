use super::InspectionComponents;
use std::path::PathBuf;

#[test]
fn component_selection_is_explicit_complete_and_absolute_without_discovery() {
    assert_eq!(InspectionComponents::select(None, None).unwrap(), None);
    for (node, inspector) in [
        (Some("/missing-node"), None),
        (None, Some("/missing-inspector")),
        (Some("node"), Some("/matched/inspect-effects.ts")),
        (Some("/explicit/node"), Some("inspect-effects.ts")),
        (Some(""), Some("")),
    ] {
        assert!(
            InspectionComponents::select(node.map(PathBuf::from), inspector.map(PathBuf::from))
                .is_err()
        );
    }
    let explicit = InspectionComponents::select(
        Some("/explicit/node".into()),
        Some("/matched/inspect-effects.ts".into()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(explicit.node, PathBuf::from("/explicit/node"));
    assert_eq!(
        explicit.inspector,
        PathBuf::from("/matched/inspect-effects.ts")
    );
}
