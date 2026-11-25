use super::*;

/// The probe key of the slow manifest listing of `folder`, apart from the folder's own.
pub(super) fn manifest_key(folder: &Path) -> PathBuf {
    let mut key = folder.as_os_str().to_owned();
    key.push("\u{0}manifests");
    PathBuf::from(key)
}

/// A stalled write check must leave the configured folder's read probes available.
pub(super) fn candidate_key(folder: &Path) -> PathBuf {
    let mut key = folder.as_os_str().to_owned();
    // NUL keeps this key distinct from filesystem paths and manifest probe keys.
    key.push("\u{0}candidate");
    PathBuf::from(key)
}

/// Runs backup folder checks with a deadline. A drive that is asleep or a share that is offline
/// can block filesystem calls for minutes. The work runs on its own thread; past the deadline
/// the caller stops waiting and reports the folder as unavailable (a call can wait for another
/// healthy one on the same folder and then run, so at most twice the timeout). A thread stuck in
/// the operating system can't be cancelled, so while one is stuck on a folder, later calls on
/// that folder answer at once instead of starting (and leaking) another.
pub(super) struct FolderProbe {
    pub(super) probes: Arc<Probes>,
    pub(super) next_id: AtomicU64,
    pub(super) timeout: Duration,
}

/// The probes that are running or stuck, by folder.
pub(super) type Probes = (Mutex<HashMap<PathBuf, Probe>>, Condvar);

pub(super) struct Probe {
    pub(super) id: u64,
    pub(super) stuck: bool,
}

/// Frees the folder's entry when the worker ends, however it ends (a panic included).
pub(super) struct FreeProbe {
    probes: Arc<Probes>,
    path: PathBuf,
    id: u64,
}

impl Drop for FreeProbe {
    fn drop(&mut self) {
        let (mutex, wake) = &*self.probes;
        let mut probes = lock(mutex);
        if probes
            .get(&self.path)
            .is_some_and(|probe| probe.id == self.id)
        {
            probes.remove(&self.path);
        }
        wake.notify_all();
    }
}

impl FolderProbe {
    pub(super) fn with_timeout(timeout: Duration) -> Self {
        Self {
            probes: Arc::default(),
            next_id: AtomicU64::new(1),
            timeout,
        }
    }

    /// The result of `work` on `path`, or `None` when it didn't finish in time (or an earlier
    /// call on the same path is still stuck).
    pub(super) fn run<T: Send + 'static>(
        &self,
        path: &Path,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        self.run_for(path, self.timeout, work)
    }

    /// [`Self::run`] with its own deadline, for work that is slow when healthy (reading many
    /// manifests, which a cloud-synced folder may have to download first).
    pub(super) fn run_for<T: Send + 'static>(
        &self,
        path: &Path,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if !self.claim(path, id, deadline) {
            return None;
        }
        let (done, result) = mpsc::channel();
        let free = FreeProbe {
            probes: self.probes.clone(),
            path: path.to_path_buf(),
            id,
        };
        let spawned = std::thread::Builder::new().spawn(move || {
            // Held for the whole run, so a panic in `work` still frees the entry.
            let free = free;
            let value = work();
            // Free the folder before the caller can see the result: otherwise a caller that
            // answers at once may still find the entry occupied (and a test of that flaked).
            drop(free);
            let _ = done.send(value);
        });
        if spawned.is_err() {
            // The closure (and the guard in it) was dropped: the entry is already free.
            return None;
        }
        match result.recv_timeout(timeout) {
            Ok(value) => Some(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.timeout_probe(path, id);
                None
            }
            // The work panicked; the guard freed the entry.
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
        }
    }

    fn claim(&self, path: &Path, id: u64, deadline: Instant) -> bool {
        let (mutex, wake) = &*self.probes;
        let mut probes = lock(mutex);
        loop {
            match probes.get(path) {
                None => {
                    probes.insert(path.to_path_buf(), Probe { id, stuck: false });
                    break;
                }
                Some(other) if other.stuck => return false,
                // A healthy probe of another call on this folder: wait for it.
                Some(_) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return false;
                    }
                    probes = wake
                        .wait_timeout(probes, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
        true
    }

    fn timeout_probe(&self, path: &Path, id: u64) {
        let (mutex, wake) = &*self.probes;
        let mut probes = lock(mutex);
        if let Some(probe) = probes.get_mut(path).filter(|probe| probe.id == id) {
            probe.stuck = true;
        }
        wake.notify_all();
    }

    /// Test helper: the folder is stuck as if a read had hung, with no clock involved.
    #[cfg(test)]
    pub(super) fn mark_stuck(&self, path: &Path) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.probes.0).insert(path.to_path_buf(), Probe { id, stuck: true });
    }

    /// [`inspect_old_folder`] within the deadline; `None` when it timed out. The same folder
    /// is answered without touching the disk.
    pub(super) fn inspect(&self, old: &Path, current: &Path) -> Option<OldFolder> {
        if old == current {
            return Some(OldFolder::Current);
        }
        let (path, current) = (old.to_path_buf(), current.to_path_buf());
        self.run(old, move || inspect_old_folder(&path, &current))
    }
}

#[cfg(test)]
mod tests;
