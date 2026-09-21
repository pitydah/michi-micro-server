fn main() {
    println!("cargo:rerun-if-env-changed=MICHI_BUILD_COMMIT");
    if let Ok(commit) = std::env::var("MICHI_BUILD_COMMIT") {
        let trimmed = commit.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=MICHI_BUILD_COMMIT={trimmed}");
            return;
        }
    }

    // Inspect git directory to emit rerun-if-changed for HEAD, current branch ref, and packed-refs
    let git_dir_res = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| std::path::PathBuf::from(s.trim()))
            } else {
                None
            }
        });

    if let Some(git_dir) = git_dir_res {
        let git_dir = if git_dir.is_relative() {
            std::env::current_dir().unwrap_or_default().join(&git_dir)
        } else {
            git_dir
        };

        let head_path = git_dir.join("HEAD");
        if head_path.exists() {
            println!("cargo:rerun-if-changed={}", head_path.display());
            if let Ok(content) = std::fs::read_to_string(&head_path) {
                let trimmed = content.trim();
                if let Some(ref_path) = trimmed.strip_prefix("ref:") {
                    let ref_target = git_dir.join(ref_path.trim());
                    println!("cargo:rerun-if-changed={}", ref_target.display());
                }
            }
        }

        let packed_refs = git_dir.join("packed-refs");
        if packed_refs.exists() {
            println!("cargo:rerun-if-changed={}", packed_refs.display());
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
