use std::{path::Path, process::Command};

#[test]
fn runs_without_game_repository_and_reports_failed_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = Command::new(env!("CARGO_BIN_EXE_engine-debug-cli"))
        .current_dir(&root)
        .args([
            "--definition",
            "examples/minimal.json",
            "--event",
            "sample",
            "--commands",
            "examples/minimal.commands",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("OK line 8"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_engine-debug-cli"))
        .current_dir(&root)
        .args(["--definition", "examples/minimal.json", "--event", "sample"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"# comment\nchoose-id absent\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("line 2 [choose-id absent] event=sample"),
        "{error}"
    );
}
