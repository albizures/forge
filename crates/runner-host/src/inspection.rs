//! Exclusive read-only inspection, not execution authority or process-death proof.
use crate::{directory, invalid, protected_file};
use fs2::FileExt;
use std::{
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// Holds the same kernel lock used by supervisors and their workers. Keep this
/// value alive throughout inspection and the dependent Forge commit. No runtime
/// file is created, repaired or opened writable. Escaped external children are
/// not fenced by this lock; it is NOT verified retirement or effect resolution.
#[must_use = "dropping the inspection guard releases exclusive host ownership"]
pub struct ExclusiveInspection {
    path: PathBuf,
    directory: (u64, u64),
    lock: fs::File,
}

impl ExclusiveInspection {
    /// Acquire an EXISTING protected ownership lock without waiting. A missing
    /// lock is missing evidence, never permission to manufacture a fresh one.
    /// # Errors
    /// Rejects unsafe/missing paths, a live lock owner or changed file identity.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let metadata = directory(path)?;
        // protected_file uses O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC and checks
        // regular-file, owner, single-link and exact 0600 permissions. Linux
        // flock supports an exclusive lock on a read-only descriptor.
        let lock = protected_file(&path.join("owner.lock"))?;
        lock.try_lock_exclusive()?;
        let guard = Self {
            path: path.to_owned(),
            directory: (metadata.dev(), metadata.ino()),
            lock,
        };
        guard.verify()?;
        Ok(guard)
    }

    /// Revalidate protected path identity before accepting dependent evidence.
    /// This cannot exclude arbitrary malicious same-user filesystem writers.
    /// # Errors
    /// Rejects replaced directories/locks or unsafe metadata. Failure retains
    /// the old descriptor's lock until this guard is dropped; it repairs nothing.
    pub fn verify(&self) -> io::Result<()> {
        let metadata = directory(&self.path)?;
        if (metadata.dev(), metadata.ino()) != self.directory {
            return Err(invalid());
        }
        let current = protected_file(&self.path.join("owner.lock"))?;
        let current = current.metadata()?;
        let retained = self.lock.metadata()?;
        if current.dev() != retained.dev() || current.ino() != retained.ino() {
            return Err(invalid());
        }
        Ok(())
    }
}

impl Drop for ExclusiveInspection {
    fn drop(&mut self) {
        // CLOEXEC does not prevent a concurrent fork from briefly retaining the
        // same open description before exec. Closing only our descriptor would
        // then leave a dropped read-only guard's flock behind. Explicit unlock
        // ends this guard's lifetime, not a supervisor's shared execution lock.
        if FileExt::unlock(&self.lock).is_err() {
            // Closing remains the fallback. Any inherited lock stays fail-closed;
            // do not infer cleanup or repair paths from a release failure.
            eprintln!("runner-inspection-unlock-unavailable; ownership may remain retained");
        }
    }
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
