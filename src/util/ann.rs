//This type will be used for `spsc::unbounded::channel`.
#![allow(dead_code)]
use crate::sync::atomic::{AtomicPtr, Ordering};
use std::fmt::Debug;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr::NonNull;

pub(crate) struct AtomicNonNull<T>(AtomicPtr<T>);

impl<T> AtomicNonNull<T> {
    /// Creates a new `AtomicNonNull`.
    pub(crate) fn new(p: NonNull<T>) -> Self {
        Self(AtomicPtr::new(p.as_ptr()))
    }

    /// Loads without any synchronisation.
    ///
    /// # Safety
    ///
    /// It's undefined behaviour if any atomic operations are performed on `self`.
    /// That includes loads, stores, and RMW operations of any [`Ordering`](std::sync::atomic::Ordering).
    #[inline]
    pub(crate) unsafe fn unsync_load(&self) -> NonNull<T> {
        #[cfg(feature = "loom")]
        // The safety of this call is guaranteed by the caller.
        let ptr = unsafe { self.0.unsync_load() };
        #[cfg(not(feature = "loom"))]
        // The safety of this call is guaranteed by the caller.
        let ptr = unsafe { self.0.as_ptr().read() };
        // SAFETY: the API only accepts and gives access to NonNull<T>, so ptr isn't null.
        unsafe { NonNull::new_unchecked(ptr) }
    }

    pub(crate) fn with_mut<R>(&mut self, f: impl FnOnce(&mut NonNull<T>) -> R) -> R {
        // SAFETY:
        // - the layout of both AtomicPtr<T> and NonNull<T> are the same as *mut.
        // - the API only accepts and gives access to NonNull<T>, so self.0 isn't null.
        #[cfg(not(feature = "loom"))]
        return f(unsafe { std::mem::transmute(self.0.get_mut()) });
        #[cfg(feature = "loom")]
        return self.0.with_mut(|me| unsafe { f(std::mem::transmute(me)) });
    }

    pub(crate) fn load(&self, ord: Ordering) -> NonNull<T> {
        let ptr = self.0.load(ord);
        // SAFETY: the API only accepts and gives access to NonNull<T>, so ptr isn't null.
        unsafe { NonNull::new_unchecked(ptr) }
    }

    pub(crate) fn store(&self, val: NonNull<T>, ord: Ordering) {
        self.0.store(val.as_ptr(), ord)
    }

    pub(crate) fn swap(&self, val: NonNull<T>, ord: Ordering) -> NonNull<T> {
        let ptr = self.0.swap(val.as_ptr(), ord);
        // SAFETY: the API only accepts and gives access to NonNull<T>, so ptr isn't null.
        unsafe { NonNull::new_unchecked(ptr) }
    }

    pub(crate) fn compare_exchange(
        &self,
        current: NonNull<T>,
        new: NonNull<T>,
        success: Ordering,
        failure: Ordering,
    ) -> Result<NonNull<T>, NonNull<T>> {
        match self
            .0
            .compare_exchange(current.as_ptr(), new.as_ptr(), success, failure)
        {
            // SAFETY: the API only accepts and gives access to NonNull<T>, so ptr isn't null.
            Ok(ptr) => Ok(unsafe { NonNull::new_unchecked(ptr) }),
            Err(ptr) => Err(unsafe { NonNull::new_unchecked(ptr) }),
        }
    }

    pub(crate) fn compare_exchange_weak(
        &self,
        current: NonNull<T>,
        new: NonNull<T>,
        success: Ordering,
        failure: Ordering,
    ) -> Result<NonNull<T>, NonNull<T>> {
        match self
            .0
            .compare_exchange_weak(current.as_ptr(), new.as_ptr(), success, failure)
        {
            // SAFETY: the API only accepts and gives access to NonNull<T>, so ptr isn't null.
            Ok(ptr) => Ok(unsafe { NonNull::new_unchecked(ptr) }),
            Err(ptr) => Err(unsafe { NonNull::new_unchecked(ptr) }),
        }
    }
}

//impl the same things as loom AtomicPtr<T>, except Default
unsafe impl<T> Send for AtomicNonNull<T> {}
unsafe impl<T> Sync for AtomicNonNull<T> {}
impl<T> RefUnwindSafe for AtomicNonNull<T> {}
impl<T> UnwindSafe for AtomicNonNull<T> {}
impl<T> Unpin for AtomicNonNull<T> {}
impl<T> Debug for AtomicNonNull<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}
impl<T> From<NonNull<T>> for AtomicNonNull<T> {
    fn from(value: NonNull<T>) -> Self {
        Self::new(value)
    }
}
