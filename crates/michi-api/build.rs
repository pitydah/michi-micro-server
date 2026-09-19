fn main() {
    println!("cargo:rerun-if-env-changed=MICHI_BUILD_COMMIT");
    if let Ok(commit) = std::env::var("MICHI_BUILD_COMMIT") {
        let trimmed = commit.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=MICHI_BUILD_COMMIT={trimmed}");
            return;
        }
    }
    // Attempt git rev-parse HEAD if running inside a git checkout
    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        });
    if let Some(c) = git_commit {
        if !c.is_empty() {
            println!("cargo:rustc-env=MICHI_BUILD_COMMIT={c}");
        }
    }
}
