//! Shared locking for service state that remains usable after a worker panics.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
