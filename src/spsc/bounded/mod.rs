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

    let inner = NonNull::from(Box::leak(Box::new(Inner::<T>::new(capacity))));
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
        //The other endpoint hasn't dropped yet, wake it if it's sleeping.
        if self.inner_ref().shared.drop_count.fetch_add(1, AcqRel) == 0 {
        }
        //The other endpoint has already been dropped, we have to deallocate inner.
        else {
            drop(unsafe {
                /*SAFETY:
                 *inner is created with a Box in channel().
                 */
                Box::from_raw(self.inner.as_ptr())
            });
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        //The other endpoint hasn't dropped yet, wake it if it's sleeping.
        if self.inner_ref().shared.drop_count.fetch_add(1, AcqRel) == 0 {
        }
        //The other endpoint has already been dropped, we have to deallocate inner.
        else {
            drop(unsafe {
                /*SAFETY:
                 *inner is created with a Box in channel().
                 */
                Box::from_raw(self.inner.as_ptr())
            });
        }
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    cfg_not_loom! {
    #[test]
    fn st_insert_remove() {
        let (src, sink) = channel::<i32>(4);

        assert_eq!(src.try_send(1), Ok(()));
        assert_eq!(src.try_send(2), Ok(()));
        assert_eq!(src.try_send(3), Ok(()));
        assert_eq!(src.try_send(4), Ok(()));
        assert_eq!(src.try_send(5), Err(TrySendError::Full(5)));

        assert_eq!(sink.try_recv(), Ok(1));
        assert_eq!(sink.try_recv(), Ok(2));
        assert_eq!(sink.try_recv(), Ok(3));
        assert_eq!(sink.try_recv(), Ok(4));
        assert_eq!(sink.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn st_insert_remove_blocking() {
        let (src, sink) = channel::<i32>(4);

        assert_eq!(src.send(1), Ok(()));
        assert_eq!(src.send(2), Ok(()));
        assert_eq!(src.send(3), Ok(()));
        assert_eq!(src.send(4), Ok(()));

        assert_eq!(sink.recv(), Ok(1));
        assert_eq!(sink.recv(), Ok(2));
        assert_eq!(sink.recv(), Ok(3));
        assert_eq!(sink.recv(), Ok(4));
    }

    #[test]
    fn st_sender_disconnect() {
        let (src, sink) = channel::<i32>(0);
        drop(src);
        assert_eq!(sink.try_recv(), Err(TryRecvError::Disconnected));
    }
    #[test]
    fn st_receiver_disconnect() {
        let (src, sink) = channel::<i32>(0);
        drop(sink);
        assert_eq!(src.try_send(1), Err(TrySendError::Disconnected(1)));
    }
    }

    cfg_loom! {
    use loom::model::model;
    use loom::thread;

    #[test]
    fn mt_insert_remove() {
        model(|| {
            // small capacity to speed up loom testing.
            let (src, sink) = channel(2);

            thread::spawn(move || {
                for i in 0..4 {
                    loop {
                        match src.try_send(i) {
                            Ok(_) => break,
                            Err(TrySendError::Full(_)) => thread::yield_now(),
                            Err(TrySendError::Disconnected(at)) => {
                                panic!("Receiver dropped before end of data.(at {at})")
                            }
                        }
                    }
                }
                println!("finished sending");
            });

            for i in 0..4 {
                loop {
                    match sink.try_recv() {
                        Ok(ret) => {
                            assert_eq!(ret, i,"Data should be received in the same order as it was sent.");
                            break;
                        },
                        Err(TryRecvError::Empty) => thread::yield_now(),
                        Err(TryRecvError::Disconnected) => {
                            panic!("Sender dropped before sending all data.(at {i})")
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn mt_sender_disconnect() {
        model(|| {
            let (src, sink) = channel::<i32>(1);//minimize the time loom takes.
            thread::spawn(|| drop(src));
            loop {
                match sink.try_recv() {
                    Ok(_) => panic!("No data was sent, but some was received."),
                    Err(TryRecvError::Empty) => thread::yield_now(),
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        });
    }

    #[test]
    fn mt_receiver_disconnect() {
        model(|| {
            let (src, sink) = channel::<i32>(1);//minimize the time loom takes.
            thread::spawn(|| drop(sink));
            loop {
                match src.try_send(0) {
                    Ok(_) => thread::yield_now(),
                    Err(TrySendError::Full(x)) => {
                        assert_eq!(x, 0);
                        thread::yield_now();
                    },
                    Err(TrySendError::Disconnected(x)) => break assert_eq!(x, 0),
                }
            }
        });
    }
    #[test]
    fn mt_insert_remove_blocking() {
        model(|| {
            let (src, sink) = channel::<i16>(3);
            thread::spawn(move || {
                //five to test wrap-around
                for i in 0..5 {
                    src.send(i).expect("Receiver dropped before receiving all data.");
                }
            });

            for i in 0..5 {
                assert_eq!(i, sink.recv().expect("Sender dropped before sending all data."), "Data should be received in the same order as it was sent.");
            }
        });
    }
    }
}
