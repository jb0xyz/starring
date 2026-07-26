use std::fs;
use std::path::PathBuf;

#[test]
fn runtime_panel_evidence_remains_pure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    for forbidden in ["sqlx", "twilight", "tokio", "rusqlite", "postgres"] {
        assert!(!manifest.contains(forbidden));
        assert!(!source.contains(forbidden));
    }
}
