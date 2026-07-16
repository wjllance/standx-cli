use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::Path;

const DEFAULT_MAKER_LOCK: &str = "/run/lock/standx-maker-live.lock";
const DEFAULT_AB_GUARD_LOCK: &str = "/run/lock/standx-maker-stage2-ab.lock";

/// Process-lifetime locks for live maker activity.
///
/// A normal live run owns both locks. The A/B orchestrator owns the guard lock
/// across arm transitions and marks its child, which then owns only the maker
/// lock. This closes the transition gap without weakening per-process mutual
/// exclusion. Locks are operational coordination, not an authorization gate.
pub(super) struct LiveProcessLock {
    _maker: File,
    _ab_guard: Option<File>,
}

impl LiveProcessLock {
    pub(super) fn acquire() -> Result<Self> {
        let maker_path = std::env::var("STANDX_MAKER_LOCK_PATH")
            .unwrap_or_else(|_| DEFAULT_MAKER_LOCK.to_string());
        let ab_guard_path = std::env::var("STANDX_STAGE2_AB_LOCK_PATH")
            .unwrap_or_else(|_| DEFAULT_AB_GUARD_LOCK.to_string());
        let orchestrated = std::env::var("STANDX_STAGE2_AB_MEMBER").ok().as_deref() == Some("1");

        let ab_guard = if orchestrated {
            None
        } else {
            Some(lock(&ab_guard_path, "stage-2 A/B orchestrator")?)
        };
        let maker = lock(&maker_path, "another live maker")?;
        Ok(Self {
            _maker: maker,
            _ab_guard: ab_guard,
        })
    }
}

fn lock(path: &str, owner: &str) -> Result<File> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create live lock directory {}", parent.display())
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open live lock {}", path.display()))?;
    #[cfg(unix)]
    {
        // SAFETY: flock only observes the valid fd owned by `file`; the file is
        // retained in LiveProcessLock for the full live process lifetime.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err(anyhow::anyhow!(
                "live maker lock {} is held by {}; refusing concurrent execution",
                path.display(),
                owner
            ));
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let _ = (file, owner);
        Err(anyhow::anyhow!(
            "live maker process locking is unavailable on this platform; refusing live execution"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn exclusive_lock_rejects_a_second_live_owner_until_release() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("maker.lock");
        let path = path.to_str().unwrap();
        let first = lock(path, "first maker").unwrap();
        assert!(lock(path, "second maker").is_err());
        drop(first);
        assert!(lock(path, "replacement maker").is_ok());
    }
}
