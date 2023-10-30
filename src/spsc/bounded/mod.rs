use crate::alloc::{alloc, dealloc};
use crate::sync::atomic::Ordering::AcqRel;
use crate::util::marker::PhantomUnsync;
use std::ptr::NonNull;

mod error;
#[doc(inline)]
pub use error::{RecvError, SendError, TryRecvError, TrySendError};

mod inner;
use inner::Inner;

/// Creates a SPSC channel with storage for at least `min_capacity` elements.
///
/// # Panics
///
/// The function panics if it can't allocate the memory needed for the channel.
pub fn channel<T>(min_capacity: usize) -> (Sender<T>, Receiver<T>) {
    let capacity = min_capacity
        .checked_next_power_of_two()
        .expect("capacity overflow"); /*from std::Vec: https://doc.rust-lang.org/src/alloc/raw_vec.rs.html*/

    let inner = Inner::<T>::new(capacity);
    //order is important: Inner is RAII, but NonNull isn't.
    let inner = {
        /*SAFETY: deallocated in either Sender's or Receiver's Drop*/
        let inner_uninit = NonNull::new(unsafe { alloc(Inner::<T>::LAYOUT) as *mut Inner<T> })
            .expect("failed to allocate memory for the shared state");
        /*SAFETY: this is a safe way to write to _uninitialised memory_.*/
        unsafe { inner_uninit.as_ptr().write(inner) };
        inner_uninit
    };
    unsafe { inner.as_ptr().write(Inner::new(capacity)) };
    (
        Sender {
            inner: inner,
            _unsync: PhantomUnsync {},
        },
        Receiver {
            inner: inner,
            _unsync: PhantomUnsync {},
        },
    )
}

/// The sending endpoint of a [`channel`].
///
/// Data can be sent using the [`try_send`](Sender::try_send) method.
pub struct Sender<T> {
    inner: NonNull<Inner<T>>,
    _unsync: PhantomUnsync,
}

/// The receiving endpoint of a [`channel`].
///
/// Data can be received using the [`try_recv`](Receiver::try_recv) method.
pub struct Receiver<T> {
    inner: NonNull<Inner<T>>,
    _unsync: PhantomUnsync,
}

impl<T> Sender<T> {
    /// Tries to send a value through this [`channel`] without blocking.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.inner_ref().try_send(item)
    }

    /// Sends a value through this [`channel`].
    ///
    /// If the [`channel`] is full, blocks and waits for the [`Receiver`].
    /// Returns a [`SendError`] if the [`Receiver`] is disconnected.
    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.inner_ref().send(item)
    }

    /// Checks if the [`channel`]'s [`Receiver`] is still connected.
    #[inline]
    pub fn receiver_connected(&self) -> bool {
        self.inner_ref().peer_connected()
    }

    fn inner_ref(&self) -> &Inner<T> {
        /*SAFETY:
         *This type and Sender are responsible for inner's lifetime.
         */
        unsafe { self.inner.as_ref() }
    }
}

impl<T> Receiver<T> {
    /// Tries to return a pending value without blocking.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner_ref().try_recv()
    }

    /// Reads a value from the [`channel`].
    ///
    /// If the [`channel`] is empty, blocks and waits for the [`Sender`].
    /// Returns a [`RecvError`] if the [`Sender`] is disconnected.
    #[inline]
    pub fn recv(&self) -> Result<T, RecvError> {
        self.inner_ref().recv()
    }

    /// Checks if the [`channel`]'s [`Sender`] is still connected.
    ///
    /// # Note
    ///
    /// The [`try_recv`](Receiver::try_recv) method returns [`TryRecvError::Disconnected`]
    /// only after consuming all previously sent data, even if the
    /// [`Sender`] isn't connected. This method, on the other hand,
    /// doesn't take pending data into account.
    #[inline]
    pub fn sender_connected(&self) -> bool {
        self.inner_ref().peer_connected()
    }

    fn inner_ref(&self) -> &Inner<T> {
        /*SAFETY:
         *This type and Receiver are responsible for inner's lifetime.
         */
        unsafe { self.inner.as_ref() }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        //this protocol is described at the declaration of 'drop_count'
        loop {
            match self.inner_ref().shared.drop_count.fetch_add(1, AcqRel) {
                0 => self.inner_ref().wake_receiver(),
                1 => break,
                2 => {
                    break unsafe {
                        self.inner.as_ptr().drop_in_place();
                        dealloc(self.inner.as_ptr() as *mut u8, Inner::<T>::LAYOUT)
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        //this protocol is described at the declaration of 'drop_count'
        loop {
            match self.inner_ref().shared.drop_count.fetch_add(1, AcqRel) {
                0 => self.inner_ref().wake_receiver(),
                1 => break,
                2 => {
                    break unsafe {
                        self.inner.as_ptr().drop_in_place();
                        dealloc(self.inner.as_ptr() as *mut u8, Inner::<T>::LAYOUT)
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

#[cfg(test)]
mod tests;
