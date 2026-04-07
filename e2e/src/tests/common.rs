use std::path::PathBuf;
use std::process::Command;

#[allow(dead_code)]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| {
            let manifest_path = path.join("Cargo.toml");
            manifest_path.is_file()
                && std::fs::read_to_string(&manifest_path)
                    .map(|contents| {
                        contents.contains("[workspace]") && contents.contains("members")
                    })
                    .unwrap_or(false)
        })
        .map(std::path::Path::to_path_buf)
        .expect("failed to locate workspace root containing Cargo workspace manifest")
}

#[allow(dead_code)]
pub fn checked_in_runtime_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| {
            path.join("config.yaml").is_file()
                && path.join("extensions_config.json").is_file()
                && path.join("governance").is_dir()
        })
        .map(std::path::Path::to_path_buf)
        .expect("failed to locate repository root containing checked-in runtime fixtures")
}

#[allow(dead_code)]
pub fn run_workspace_test(package: &str, test_name: &str) {
    let output = Command::new("cargo")
        .args(["test", "-p", package, test_name, "--", "--exact", "--nocapture"])
        .current_dir(workspace_root())
        .output()
        .expect("cargo test should start");

    assert!(
        output.status.success(),
        "cargo test -p {package} {test_name} failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
