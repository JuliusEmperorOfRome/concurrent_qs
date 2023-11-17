use crate::alloc::{alloc, dealloc};
use crate::error::{RecvError, SendError, TryRecvError, TrySendError};
use crate::sync::atomic::Ordering::AcqRel;
use crate::util::marker::PhantomUnsync;
use std::ptr::NonNull;

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
/// Data can be sent using the [`try_send`](Sender::try_send)
/// and [`send`](Sender::send) methods.
pub struct Sender<T> {
    inner: NonNull<Inner<T>>,
    _unsync: PhantomUnsync,
}

/// The receiving endpoint of a [`channel`].
///
/// Data can be received using the [`try_recv`](Receiver::try_recv)
/// and [`recv`](Receiver::recv) methods.
pub struct Receiver<T> {
    inner: NonNull<Inner<T>>,
    _unsync: PhantomUnsync,
}

impl<T> Sender<T> {
    /// Tries to send a value through this [`channel`].
    ///
    /// # Notes
    ///
    /// - Will never block as long as [`recv`](Receiver::recv) hasn't been called.
    /// - After every call to [`recv`](Receiver::recv), up to one [`try_send`](Sender::try_send)
    /// call may block for a short period.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.inner_ref().try_send(item)
    }

    /// Sends a value through this [`channel`].
    ///
    /// If the [`channel`] is full, blocks and waits for the [`Receiver`].
    /// Returns a [`SendError`] if the [`Receiver`] is disconnected.
    ///
    /// # Note
    ///
    /// Calling this method may result in a [`try_recv`](Receiver::try_recv)
    /// call blocking for a short period.
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
    /// Tries to return a pending value.
    ///
    /// # Notes
    ///
    /// - Returns [`TryRecvError::Disconnected`] only after consuming all
    /// sent data. To avoid this, use [`sender_connected`](Receiver::sender_connected).
    /// - Will never block as long as [`send`](Sender::send) hasn't been called.
    /// - After every call to [`send`](Sender::send), up to one [`try_recv`](Receiver::try_recv)
    /// call may block for a short period.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner_ref().try_recv()
    }

    /// Reads a value from the [`channel`].
    ///
    /// If the [`channel`] is empty, blocks and waits for the [`Sender`].
    ///
    /// # Notes
    /// - [`RecvError`] is only returned after consuming all sent data. To
    /// avoid this, use [`sender_connected`](Receiver::sender_connected).
    /// - Calling this method may result in a [`try_send`](Sender::try_send)
    /// call blocking for a short period.
    #[inline]
    pub fn recv(&self) -> Result<T, RecvError> {
        self.inner_ref().recv()
    }

    /// Checks if the [`channel`]'s [`Sender`] is still connected.
    ///
    /// # Note
    ///
    /// The [`try_recv`](Receiver::try_recv) and [`recv`](Receiver::recv)
    /// methods return [`TryRecvError::Disconnected`] or [`RecvError`] only
    /// after consuming all previously sent data, even if the [`Sender`] isn't
    /// connected. This method doesn't take pending data into account and can
    /// be used to avoid this behaviour.
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

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::bounded::Sender<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.inner
        )
    }
}
impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::bounded::Receiver<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.inner
        )
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

#[cfg(test)]
mod tests;
