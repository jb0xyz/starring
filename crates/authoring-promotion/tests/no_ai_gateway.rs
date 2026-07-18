use std::fs;
use std::path::{Path, PathBuf};

fn regular_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to read an entry under {}: {error}",
                directory.display()
            )
        });
        let file_type = entry.file_type().unwrap_or_else(|error| {
            panic!("failed to inspect {}: {error}", entry.path().display())
        });
        let path = entry.path();
        if file_type.is_dir() {
            regular_rust_files(&path, files);
        } else if file_type.is_file() && path.extension().is_some_and(|value| value == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn regular_source_files_do_not_reference_ai_gateway() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    regular_rust_files(&source, &mut files);
    files.sort();
    assert!(!files.is_empty());

    for file in files {
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display()));
        assert!(
            !content.contains("ai_gateway") && !content.contains("ai-gateway"),
            "AI gateway reference in {}",
            file.display()
        );
    }
}
