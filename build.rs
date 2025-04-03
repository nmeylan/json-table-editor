use std::process::Command;

fn main() {
    // Get the current git revision
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .expect("Failed to execute git command");

    let git_hash = String::from_utf8(output.stdout)
        .unwrap_or_default()
        .trim()
        .to_string();

    // Set the version as an environment variable
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // Optional: re-run this script only if the `.git` directory changes
    println!("cargo:rerun-if-changed=.git");
}
