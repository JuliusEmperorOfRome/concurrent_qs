use crate::util::CacheAligned;
use std::cell::{Cell, UnsafeCell};
use std::default::Default;
use std::error;
use std::fmt;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use std::usize;

/// An enumeration listing the failure modes of the [`try_send`](Sender::try_send) method.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum TrySendError<T> {
    /// The data couldn't be sent on the [`channel`] because the internal buffer was already full.
    Full(T),
    /// The [`Receiver`] bound to the [`channel`] disconnected and any further sends will not succeed.
    Disconnected(T),
}

/// An enumeration listing the failure modes of the [`try_recv`](Receiver::try_recv) method.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    /// No data was received from the [`channel`] because the internal buffer was empty.
    Empty,
    /// The [`Sender`] bound to the [`channel`] disconnected and all previously sent data was already received.
    Disconnected,
}

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
            inner: InnerHolder(inner),
        },
        Receiver {
            inner: InnerHolder(inner),
        },
    )
}

/// The sending endpoint of a [`channel`].
///
/// Data can be sent using the [`try_send`](Sender::try_send) method.
pub struct Sender<T> {
    inner: InnerHolder<T>,
}

/// The receiving endpoint of a [`channel`].
///
/// Data can be received using the [`try_recv`](Receiver::try_recv) method.
pub struct Receiver<T> {
    inner: InnerHolder<T>,
}

impl<T> Sender<T> {
    /// Tries to send a value through this channel without blocking.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.inner.try_send(item)
    }

    /// Checks if the [`channel`]'s receiver is still connected.
    #[inline]
    pub fn receiver_connected(&self) -> bool {
        self.inner.peer_connected()
    }
}

impl<T> Receiver<T> {
    /// Tries to return a pending value without blocking.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner.try_recv()
    }

    /// Checks if the [`channel`]'s sender is still connected.
    ///
    /// # Note
    ///
    /// The [`try_recv`](Receiver::try_recv) method returns [`TryRecvError::Disconnected`]
    /// only after consuming all previously sent data, even if the
    /// [`Sender`] isn't connected. This method, on the other hand,
    /// doesn't take pending data into account.
    #[inline]
    pub fn sender_connected(&self) -> bool {
        self.inner.peer_connected()
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

struct Inner<T> {
    sender: CacheAligned<SenderData>,
    receiver: CacheAligned<ReceiverData>,
    shared: SharedData<T>,
}

impl<T> Inner<T> {
    fn new(capacity: usize) -> Self {
        // should already be ensured in channel()
        debug_assert!(capacity.is_power_of_two(), "capacity wasn't a power of two");

        Self {
            sender: CacheAligned::default(),
            receiver: CacheAligned::default(),
            shared: SharedData {
                buffer: {
                    let mut vec = Vec::with_capacity(capacity);
                    /*SAFETY:
                     *elements are MaybeUninit, so uninitialised
                     *data is a valid value for them.
                     */
                    unsafe { vec.set_len(capacity) };
                    vec.into_boxed_slice()
                },
                drop_count: AtomicUsize::default(),
            },
        }
    }

    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        if self.shared.drop_count.load(Relaxed) != 0 {
            return Err(TrySendError::Disconnected(item));
        }

        /*SAFETY:
         *tail is only modified by try_send and this is
         *an SPSC, so no other thread is modifying it.
         */
        let tail = unsafe { self.sender.tail.as_ptr().read() };

        let cap = self.shared.buffer.len();

        if tail == self.sender.head_cache.get().wrapping_add(cap) {
            self.sender.head_cache.set(self.receiver.head.load(Acquire));

            if tail == self.sender.head_cache.get().wrapping_add(cap) {
                return Err(TrySendError::Full(item));
            }
        }

        unsafe {
            /*SAFETY:
             *cap( = self.shared.buffer.len()) is a power of two,
             *so <tail & (cap - 1)> is in [0, cap) and
             *the get_unchecked call is valid.
             */
            let slot = self.shared.buffer.get_unchecked(tail & (cap - 1));
            /*SAFETY:
             *receiver only reads values past self.reader.head
             *and the if block above checks for this.
             */
            let slot = slot.get();
            /*SAFETY:
             *this doesn't overwrite valid <T>s because it's either
             *uninit from Self::new() or already taken out by reader.
             */
            (slot as *mut T).write(item);
        }

        self.sender.tail.store(tail.wrapping_add(1), Release);
        Ok(())
    }

    fn try_recv(&self) -> Result<T, TryRecvError> {
        use TryRecvError::*;
        /*SAFETY:
         *head is only modified by try_recv and this is
         *an SPSC, so no other thread is modifying it.
         */
        let head = unsafe { self.receiver.head.as_ptr().read() };

        if head == self.receiver.tail_cache.get() {
            self.receiver.tail_cache.set(self.sender.tail.load(Acquire));
            if head == self.receiver.tail_cache.get() {
                // Let the receiver consume all the messages after sender disconnects.
                if self.shared.drop_count.load(Relaxed) != 0 {
                    return Err(Disconnected);
                }
                return Err(Empty);
            }
        }

        let buffer = &self.shared.buffer;
        let item = unsafe {
            /*SAFETY:
             *buffer.len() is a power of two,
             *so <tail & buffer.len()> is in [0, buffer.len()) and
             *the get_unchecked call is valid.
             */
            let slot = buffer.get_unchecked(head & (buffer.len() - 1));
            let slot = slot.get();
            slot.read().assume_init()
        };

        self.receiver.head.store(head.wrapping_add(1), Release);
        Ok(item)
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        //head points to the first not read element
        //tail points after the last written element
        let (mut head, tail) = unsafe {
            /*SAFETY:
             *this object is being destroyed so we
             *have exclusive access to these atomics.
             */
            (
                self.receiver.head.as_ptr().read(),
                self.sender.tail.as_ptr().read(),
            )
        };

        let mask = self.shared.buffer.len() - 1;

        while head != tail {
            /*SAFETY:
             *self.shared.buffer.len() is a power of 2, so <head & mask>
             *is in [0, self.shared.buffer.len()) and get_unchecked_mut is valid.
             */
            let slot = unsafe { self.shared.buffer.get_unchecked_mut(head & mask) };
            /*SAFETY:
             *all elements in [head, tail) have been sent, but not received.
             */
            unsafe { slot.get_mut().assume_init_drop() };
            head = head.wrapping_add(1);
        }
    }
}

struct SenderData {
    tail: AtomicUsize,
    head_cache: Cell<usize>,
}

struct ReceiverData {
    head: AtomicUsize,
    tail_cache: Cell<usize>,
}

struct SharedData<T> {
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    drop_count: AtomicUsize, /*dropped endpoint count*/
}

struct InnerHolder<T>(NonNull<Inner<T>>);

impl<T> InnerHolder<T> {
    fn peer_connected(&self) -> bool {
        self.deref().shared.drop_count.load(Relaxed) == 0
    }
}

impl<T> Deref for InnerHolder<T> {
    type Target = Inner<T>;

    fn deref(&self) -> &Self::Target {
        /*SAFETY:
         *This is the type that tracks the lifetime
         *of the Inner pointed to by self.0.
         */
        unsafe { self.0.as_ref() }
    }
}

impl<T> DerefMut for InnerHolder<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        /*SAFETY:
         *This is the type that tracks the lifetime
         *of the Inner pointed to by self.0.
         */
        unsafe { self.0.as_mut() }
    }
}

impl<T> Drop for InnerHolder<T> {
    fn drop(&mut self) {
        /*There are 2 holders for any Inner, so if the previously
         *dropped holder count was 1, the other was already dropped.*/
        if self.deref().shared.drop_count.fetch_add(1, AcqRel) == 1 {
            drop(unsafe { Box::from_raw(self.deref_mut()) });
        }
    }
}

impl Default for SenderData {
    #[inline(always)]
    fn default() -> Self {
        Self {
            tail: AtomicUsize::default(),
            head_cache: Cell::default(),
        }
    }
}

impl Default for ReceiverData {
    #[inline(always)]
    fn default() -> Self {
        Self {
            head: AtomicUsize::default(),
            tail_cache: Cell::default(),
        }
    }
}

impl<T> error::Error for TrySendError<T> {}
impl error::Error for TryRecvError {}

impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            TrySendError::Full(_) => f.write_str("writing to a full queue"),
            TrySendError::Disconnected(_) => f.write_str("writing to a disconnected queue"),
        }
    }
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TryRecvError::Empty => f.write_str("reading from an empty queue"),
            TryRecvError::Disconnected => f.write_str("reading from a disconnected queue"),
        }
    }
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TrySendError::Full(_) => "Full(..)".fmt(f),
            TrySendError::Disconnected(_) => "Disconnected(..)".fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /*
    TODO - further testing
    */
}
