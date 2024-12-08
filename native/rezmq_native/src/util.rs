use std::ptr::NonNull;

#[derive(Debug)]
pub struct SyncNonNull<T>(pub NonNull<T>);

unsafe impl<T> Send for SyncNonNull<T> {}
unsafe impl<T> Sync for SyncNonNull<T> {}

impl<T> Clone for SyncNonNull<T> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<T> Copy for SyncNonNull<T> {}
