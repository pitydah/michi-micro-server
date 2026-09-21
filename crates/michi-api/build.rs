fn main() {
    println!("cargo:rerun-if-env-changed=MICHI_BUILD_COMMIT");
    if let Ok(commit) = std::env::var("MICHI_BUILD_COMMIT") {
        let trimmed = commit.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=MICHI_BUILD_COMMIT={trimmed}");
            return;
        }
    }

    // Inspect git paths using rev-parse --git-path to support Git worktrees and common dirs
    let resolve_git_path = |subpath: &str| -> Option<std::path::PathBuf> {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", subpath])
            .output()
            .ok()?;
        if out.status.success() {
            let s = String::from_utf8(out.stdout).ok()?;
            let p = std::path::PathBuf::from(s.trim());
            let p = if p.is_relative() {
                std::env::current_dir().unwrap_or_default().join(p)
            } else {
                p
            };
            Some(p)
        } else {
            None
        }
    };

    if let Some(git_dir) = resolve_git_path("") {
        if git_dir.exists() {
            println!("cargo:rerun-if-changed={}", git_dir.display());
        }
    }

    if let Some(head_path) = resolve_git_path("HEAD") {
        if head_path.exists() {
            println!("cargo:rerun-if-changed={}", head_path.display());
            if let Ok(content) = std::fs::read_to_string(&head_path) {
                let trimmed = content.trim();
                if let Some(ref_path) = trimmed.strip_prefix("ref:") {
                    if let Some(resolved_ref) = resolve_git_path(ref_path.trim()) {
                        println!("cargo:rerun-if-changed={}", resolved_ref.display());
                    }
                }
            }
        }
    }

    if let Some(packed_refs) = resolve_git_path("packed-refs") {
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
