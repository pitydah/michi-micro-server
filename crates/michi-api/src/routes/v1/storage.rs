use crate::AppState;
use axum::{extract::State, Json};

#[cfg(unix)]
fn statvfs_free_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    #[repr(C)]
    struct Statvfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        _rest: [u64; 10],
    }
    extern "C" {
        fn statvfs(path: *const i8, buf: *mut Statvfs) -> i32;
    }
    let path_c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: Statvfs = unsafe { std::mem::zeroed() };
    if unsafe { statvfs(path_c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(stat.f_frsize * stat.f_bavail)
}

fn free_bytes(p: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        if let Some(bytes) = statvfs_free_bytes(p) {
            return Some(bytes);
        }
    }
    None
}

/// Get real disk usage for a directory (recursive file sizes)
fn dir_size(p: &std::path::Path) -> u64 {
    let mut total = 0u64;
    fn walk(dir: &std::path::Path, total: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        walk(&entry.path(), total);
                    } else {
                        *total += meta.len();
                    }
                }
            }
        }
    }
    walk(p, &mut total);
    total
}

pub async fn storage_health_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let data_dir = &state.config.config_path;
    let cache_dir = &state.config.cache_path;

    let data_path = data_dir.clone();
    let cache_path = cache_dir.clone();

    let (config_free, config_used, cache_free, cache_used) =
        tokio::task::spawn_blocking(move || {
            (
                free_bytes(&data_path),
                dir_size(&data_path),
                free_bytes(&cache_path),
                dir_size(&cache_path),
            )
        })
        .await
        .unwrap_or((None, 0, None, 0));

    let warn_threshold: u64 = 1_000_000_000; // 1GB
    let crit_threshold: u64 = 100_000_000; // 100MB

    let config_free = config_free.unwrap_or(0);
    let cache_free = cache_free.unwrap_or(0);

    let config_status = if config_free < crit_threshold {
        "critical"
    } else if config_free < warn_threshold {
        "warning"
    } else {
        "ok"
    };

    let cache_status = if cache_free < crit_threshold {
        "critical"
    } else if cache_free < warn_threshold {
        "warning"
    } else {
        "ok"
    };

    Json(serde_json::json!({
        "status": if config_status == "ok" && cache_status == "ok" { "ok" } else { "warning" },
        "config": {
            "path": data_dir.display().to_string(),
            "free_bytes": config_free,
            "used_bytes": config_used,
            "status": config_status
        },
        "cache": {
            "path": cache_dir.display().to_string(),
            "free_bytes": cache_free,
            "used_bytes": cache_used,
            "status": cache_status
        },
        "thresholds": {
            "warning_bytes": warn_threshold,
            "critical_bytes": crit_threshold
        }
    }))
}
