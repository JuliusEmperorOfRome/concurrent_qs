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
        /* Potential deadlock:
         * 1. parking thread transitions from EMPTY to PARKED
         * 2. unpark call swaps state to NOTIFIED and finds PARKED
         *   and calls condvar.notify_one(), which doesn't go through.
         * 3. parking thread goes to park_slow and goes to sleep.
         *
         * Resolved by checking
         */
        // Do NOTIFIED=>EMPTY or EMPTY=>PARKED
        match self.state.fetch_add(1, Acquire) {
            NOTIFIED => return,
            EMPTY => self.park_slow(),
            _ => panic!("Invalid call to Parker::park."),
        }
    }

    #[inline(never)]
    fn park_slow(&self) {
        //thread::park doesn't transmit panics, so we ignore poison.
        let mut m = match self.mutex.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        loop {
            if self
                .state
                .compare_exchange(NOTIFIED, EMPTY, Acquire, Acquire)
                .is_ok()
            {
                return; //got our notification.
            } else {
                //spurious wake-up.
            }

            m = match self.condvar.wait(m) {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }

    pub(crate) fn unpark(&self) {
        if self.state.swap(NOTIFIED, Release) == PARKED {
            /*
             * Potential deadlock:
             *  1. parked thread wakes up and has the mutex.
             *  2. parked thread checks state, finds PARKED.
             *  3. unpark call swaps state to NOTIFIED, finds PARKED and
             *    calls condvar.notify_one(). Since the condvar isn't
             *    waiting yet, the notification does nothing.
             *  4. parked thread sleeps on the condvar in the NOTIFIED state.
             *  5. all later unpark calls find the NOTIFIED state and
             *    don't try to wake the parked thread.
             *
             * To avoid this, we take the lock after writing NOTIFIED,
             * but before calling notify_one.
             */
            /* Second potential deadlock:
             * 1. parking thread enters park(), transitions from EMPTY to PARKED
             * 2. unpark call swaps state to NOTIFIED and finds PARKED,
             *   locks and drops the mutex, and calls condvar.notify_one(), which
             *   does nothing.
             * 3. parking thread goes to park_slow and goes to sleep.
             *
             * Resolved in park_slow by checking the state after locking the mutex,
             * but before going to sleep.
             *
             */
            drop(self.mutex.lock());
            self.condvar.notify_one();
        }
    }
}

unsafe impl Send for Parker {}
unsafe impl Sync for Parker {}
