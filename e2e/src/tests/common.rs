use std::path::PathBuf;
use std::process::Command;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e crate should be inside workspace root")
        .to_path_buf()
}

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
