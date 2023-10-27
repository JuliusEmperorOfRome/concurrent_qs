use crate::util::CacheAligned;
use std::cell::{Cell, UnsafeCell};
use std::default::Default;
use std::error;
use std::fmt;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};
use std::usize;

/*
TODO - document
*/

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum TrySendError<T> {
    Full(T),
    Disconnected(T),
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Disconnected,
}

pub fn channel<T>(min_capacity: usize) -> (Sender<T>, Receiver<T>) {
    let capacity = min_capacity
        .checked_next_power_of_two()
        .expect("capacity overflow"); /*from std::Vec: https://doc.rust-lang.org/src/alloc/raw_vec.rs.html*/

    let inner = NonNull::from(Box::leak(Box::new(Inner::<T>::new(capacity))));
    (Sender { inner: inner }, Receiver { inner: inner })
}
pub struct Sender<T> {
    inner: NonNull<Inner<T>>,
}

pub struct Receiver<T> {
    inner: NonNull<Inner<T>>,
}

impl<T> Sender<T> {
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        /*SAFETY:
         *inner is only dropped when this sender and the coresponding receiver
         */
        unsafe { self.inner.as_ref() }.try_send(item)
    }
}

impl<T> Receiver<T> {
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        unsafe { self.inner.as_ref() }.try_recv()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        /*SAFETY:
         *this method and Receiver<T>::drop are responsible for the lifetime of inner*/
        let inner = unsafe { self.inner.as_ref() };

        /*fetch_add is only called twice for any Inner, so if the
         *prev value is 1, the other end point has already dropped.
         */
        if inner.shared.drop_count.fetch_add(1, SeqCst) == 1 {
            /*SAFETY:
             *self.inner is made from a Box::leak(...)
             */
            drop(unsafe { Box::from_raw(self.inner.as_ptr()) });
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        /*SAFETY:
         *this method and Sender<T>::drop are responsible for the lifetime of inner*/
        let inner = unsafe { self.inner.as_ref() };

        /*fetch_add is only called twice for any Inner, so if the
         *prev value is 1, the other end point has already dropped.
         *SeqCst is most likely not necessary, but this is cold code anyways.
         */
        if inner.shared.drop_count.fetch_add(1, SeqCst) == 1 {
            /*SAFETY:
             *self.inner is made from a Box::leak(...)
             */
            drop(unsafe { Box::from_raw(self.inner.as_ptr()) });
        }
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
        use TrySendError::*;
        if self.shared.drop_count.load(Relaxed) != 0 {
            return Err(Disconnected(item));
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
                return Err(Full(item));
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
        if self.shared.drop_count.load(Relaxed) != 0 {
            return Err(Disconnected);
        }
        /*SAFETY:
         *head is only modified by try_recv and this is
         *an SPSC, so no other thread is modifying it.
         */
        let head = unsafe { self.receiver.head.as_ptr().read() };

        if head == self.receiver.tail_cache.get() {
            self.receiver.tail_cache.set(self.sender.tail.load(Acquire));
            if head == self.receiver.tail_cache.get() {
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
        use TrySendError::*;
        match *self {
            Full(_) => f.write_str("writing to a full queue"),
            Disconnected(_) => f.write_str("writing to a disconnected queue"),
        }
    }
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TryRecvError::*;
        match *self {
            Empty => f.write_str("reading from an empty queue"),
            Disconnected => f.write_str("reading from a disconnected queue"),
        }
    }
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TrySendError::*;
        match *self {
            Full(_) => "Full(..)".fmt(f),
            Disconnected(_) => "Disconnected(..)".fmt(f),
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
