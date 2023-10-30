use crate::sync::atomic::{
    AtomicUsize,
    Ordering::{Acquire, Release},
};
use crate::sync::{Condvar, Mutex};

/// park/unpark equivalent, except can be embedded in objects.
///
/// based on https://doc.rust-lang.org/src/std/sys_common/thread_parking/futex.rs.html
/*If we're unlucky enough to have a Parker split across
 *cachelines, it's important to have state at the top.*/
#[repr(C)]
pub(crate) struct Parker {
    state: AtomicUsize,
    condvar: Condvar,
    mutex: Mutex<()>,
}

const NOTIFIED: usize = 0;
const EMPTY: usize = 1;
const PARKED: usize = 2;

impl Parker {
    #[cfg(not(loom))]
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicUsize::new(EMPTY),
            condvar: Condvar::new(),
            mutex: Mutex::new(()),
        }
    }
    #[cfg(loom)]
    pub(crate) fn new() -> Self {
        Self {
            state: AtomicUsize::new(EMPTY),
            condvar: Condvar::new(),
            mutex: Mutex::new(()),
        }
    }

    /// SAFETY: this method can't _EVER_ be called concurrently.
    #[inline(always)]
    pub(crate) unsafe fn park(&self) {
        let prev_state = self.state.fetch_add(1, Acquire);
        debug_assert!(
            prev_state == NOTIFIED || prev_state == EMPTY,
            "multiple threads parked in one parker."
        );
        if prev_state == NOTIFIED {
            return; //already had a token
        }

        self.park_slow();
    }

    #[inline(never)]
    fn park_slow(&self) {
        //thread::park doesn't transmit panics, so we ignore poison.
        let mut m = match self.mutex.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        loop {
            m = match self.condvar.wait(m) {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };

            if self
                .state
                .compare_exchange(NOTIFIED, EMPTY, Acquire, Acquire)
                .is_ok()
            {
                return; //got our notification.
            } else {
                //spurious wake-up.
            }
        }
    }

    pub(crate) fn unpark(&self) {
        if self.state.swap(NOTIFIED, Release) == PARKED {
            self.condvar.notify_one();
        }
    }
}

unsafe impl Send for Parker {}
unsafe impl Sync for Parker {}

#[cfg(any(all(test, not(loom)), all(test, loom, park)))]
mod tests {
    use super::Parker;
    cfg_not_loom! {

    #[test]
    fn test_two_threads() {
        static PARKER: Parker = Parker::new();
        std::thread::spawn(|| PARKER.unpark());
        unsafe { PARKER.park() };
    }

    #[test]
    fn test_one_thread() {
        let parker = Parker::new();
        parker.unpark();
        unsafe { parker.park() };
    }

    }

    cfg_loom! {

    #[test]
    fn test_two_threads() {
        loom::model::model(|| {
            static PARKER: Parker = Parker::new();
            std::thread::spawn(|| PARKER.unpark());
            unsafe { PARKER.park() };
        });
    }

    #[cfg(all(test, loom))]
    #[test]
    fn test_one_thread() {
        loom::model::model(|| {
            let parker = Parker::new();
            parker.unpark()
        });
    }

    }
}
