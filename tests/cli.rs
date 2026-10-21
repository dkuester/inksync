use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_nickelinksync"))
    .args(args)
    .output()
    .expect("failed to run nickelinksync")
}

#[test]
fn version_prints_crate_version() {
  let output = run(&["--version"]);
  assert!(output.status.success());
  assert_eq!(
    String::from_utf8_lossy(&output.stdout).trim(),
    format!("nickelinksync {}", env!("CARGO_PKG_VERSION"))
  );
}

#[test]
fn subcommands_are_not_implemented_yet() {
  for command in ["status", "push", "pull"] {
    let output = run(&[command]);
    assert!(!output.status.success(), "`{command}` should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      stderr.contains("not implemented yet"),
      "`{command}` printed: {stderr}"
    );
  }
}
