use std::env;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("git must be available while building design-harness-cli");
    assert!(output.status.success(), "git source attestation failed");
    String::from_utf8(output.stdout)
        .expect("git source attestation must be UTF-8")
        .trim()
        .to_string()
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set");
    let root = Path::new(&manifest).join("../..");
    let commit = git(&root, &["rev-parse", "HEAD"]);
    let status = git(
        &root,
        &["status", "--porcelain", "--untracked-files=normal"],
    );
    assert!(
        matches!(commit.len(), 40 | 64)
            && commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "git source commit must be an exact lowercase hexadecimal identity"
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/HEAD").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join(".git/index").display()
    );
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=STARRING_BUILD_SOURCE_COMMIT={commit}");
    println!(
        "cargo:rustc-env=STARRING_BUILD_SOURCE_DIRTY={}",
        !status.is_empty()
    );
}
