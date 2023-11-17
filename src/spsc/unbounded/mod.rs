use crate::util::marker::PhantomUnsync;

use std::{fmt::Debug, ops::Deref};

mod inner;

pub use crate::error::{RecvError, SendError, TryRecvError};

/// Creates an SPSC channel with unbounded capacity.
///
/// It should be noted that an unbounded channel is bounded by system memory.
/// If items are received slower than they are sent, [`send`](Sender::send)
/// will eventually consume all available memory and panic.
///
/// # Panics
///
/// This function panics if it can't allocate the inner state of the channel.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (h1, h2) = inner::Inner::<T>::allocate();
    (Sender(h1, PhantomUnsync {}), Receiver(h2, PhantomUnsync {}))
}

/// The sending endpoint of a [`channel`].
///
/// Data can be sent using the [`send`](Sender::send) method.
pub struct Sender<T>(inner::InnerHolder<T>, PhantomUnsync);

/// The receiving endpoint of a [`channel`].
///
/// Data can be received using the [`try_recv`](Receiver::try_recv)
/// and [`recv`](Receiver::recv) methods.
pub struct Receiver<T>(inner::InnerHolder<T>, PhantomUnsync);

impl<T> Sender<T> {
    /// Sends a value through this [`channel`].
    ///
    /// It does not block by itself, but it potentially allocates
    /// memory, so [`send`] is **not** real-time safe by any means.
    ///
    /// # Panics
    ///
    /// This function may panic if no more memory is available.
    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item)
    }

    /// Checks if the [`channel`]'s [`Receiver`] is still connected.
    #[inline]
    pub fn receiver_connected(&self) -> bool {
        self.0.peer_connected()
    }
}

impl<T> Receiver<T> {
    /// Reads a value from the [`channel`].
    ///
    /// If the [`channel`] is empty, blocks and waits for the [`Sender`].
    ///
    /// # Note
    ///
    /// [`RecvError`] is only returned after consuming all sent data. To
    /// avoid this, use [`sender_connected`](Receiver::sender_connected).
    pub fn recv(&self) -> Result<T, RecvError> {
        self.0.recv()
    }

    /// Tries to return a pending value.
    ///
    /// # Note
    ///
    /// Returns [`TrySendError::Disconnected`] only after consuming all
    /// sent data. To avoid this, use [`sender_connected`](Receiver::sender_connected).
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
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
    pub fn sender_connected(&self) -> bool {
        self.0.peer_connected()
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        use crate::sync::atomic::Ordering::AcqRel;
        self.0.drop_count.fetch_add(1, AcqRel);
        self.0.unpark_receiver();
        // InnerHolder does the rest
    }
}

impl<T> Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::unbounded::Sender<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.0.deref() as *const _
        )
    }
}

impl<T> Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::unbounded::Receiver<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.0.deref() as *const _
        )
    }
}

#[cfg(test)]
mod tests;
