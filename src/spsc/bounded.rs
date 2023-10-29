use crate::cell::{Cell, UnsafeCell};
use crate::sync::atomic::AtomicUsize;
use crate::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use crate::util::CacheAligned;
use std::default::Default;
use std::error;
use std::fmt;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

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

/// Error for the [`send`](Sender::send) method.
///
/// This error is returned when the [`Receiver`] bound to the [`channel`]
/// has disconnected. The `T` is the item that failed to send.
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct SendError<T>(pub T);

/// Error for the [`recv`](Receiver::recv) method.
///
/// This error is returned when the [`Sender`] bound to the [`channel`]
/// has disconnected.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct RecvError {}

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
    (Sender { inner: inner }, Receiver { inner: inner })
}

/// The sending endpoint of a [`channel`].
///
/// Data can be sent using the [`try_send`](Sender::try_send) method.
pub struct Sender<T> {
    inner: NonNull<Inner<T>>,
}

/// The receiving endpoint of a [`channel`].
///
/// Data can be received using the [`try_recv`](Receiver::try_recv) method.
pub struct Receiver<T> {
    inner: NonNull<Inner<T>>,
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

#[repr(C)]
struct Inner<T> {
    sender: CacheAligned<SenderData>,
    receiver: CacheAligned<ReceiverData>,
    shared: SharedData<T>,
}

impl<T> Inner<T> {
    fn new(capacity: usize) -> Self {
        // should already be ensured in channel()
        debug_assert!(capacity.is_power_of_two(), "capacity wasn't a power of two");
        #[cfg(not(loom))]
        let buffer = {
            let mut vec = Vec::with_capacity(capacity);
            /*SAFETY:
             *elements are MaybeUninit, so uninitialised
             *data is a valid value for them.
             */
            unsafe { vec.set_len(capacity) };
            vec.into_boxed_slice()
        };
        /*
        !!!IMPORTANT!!!

        In loom, UnsafeCell::new(MaybeUninit::uninit()) isn't uninitialised memory.
        It initialises extra fields used for keeping track of accesses to the cell.

        !!!DO NOT DELETE THE CODE BELOW!!!
        */
        #[cfg(loom)]
        let buffer = (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect::<Box<[UnsafeCell<MaybeUninit<T>>]>>();
        Self {
            sender: CacheAligned::default(),
            receiver: CacheAligned::default(),
            shared: SharedData {
                buffer: buffer,
                drop_count: AtomicUsize::default(),
            },
        }
    }

    fn send(&self, item: T) -> Result<(), SendError<T>> {
        let mut resend = match self.try_send(item) {
            Ok(_) => return Ok(()),
            Err(TrySendError::Disconnected(ret)) => return Err(SendError(ret)),
            Err(TrySendError::Full(ret)) => ret,
        };
        loop {
            #[cfg(loom)]
            loom::thread::yield_now();
            //TODO: implement sleeping/waking
            match self.try_send(resend) {
                Ok(_) => break Ok(()),
                Err(TrySendError::Disconnected(ret)) => break Err(SendError(ret)),
                Err(TrySendError::Full(ret)) => resend = ret,
            }
        }
    }

    fn recv(&self) -> Result<T, RecvError> {
        match self.try_recv() {
            Ok(ret) => return Ok(ret),
            Err(TryRecvError::Disconnected) => return Err(RecvError {}),
            Err(TryRecvError::Empty) => {}
        };
        loop {
            #[cfg(loom)]
            loom::thread::yield_now();
            //TODO: implement sleeping/waking
            match self.try_recv() {
                Ok(ret) => return Ok(ret),
                Err(TryRecvError::Disconnected) => return Err(RecvError {}),
                Err(TryRecvError::Empty) => {}
            }
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
        #[cfg(not(loom))]
        let tail = unsafe { self.sender.tail.as_ptr().read() };
        #[cfg(loom)]
        let tail = unsafe { self.sender.tail.unsync_load() };

        let cap = self.shared.buffer.len();

        if tail == self.sender.head_cache.get().wrapping_add(cap) {
            self.sender.head_cache.set(self.receiver.head.load(Acquire));

            if tail == self.sender.head_cache.get().wrapping_add(cap) {
                self.wake_receiver();
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
            slot.with_mut(|ptr| {
                /*SAFETY:
                 *this doesn't overwrite valid <T>s because it's either
                 *uninit from Self::new() or already taken out by reader.
                 */
                (ptr as *mut T).write(item)
            });
        }
        self.sender.tail.store(tail.wrapping_add(1), Release);
        self.wake_receiver();
        Ok(())
    }

    fn try_recv(&self) -> Result<T, TryRecvError> {
        use TryRecvError::*;
        /*SAFETY:
         *head is only modified by try_recv and this is
         *an SPSC, so no other thread is modifying it.
         */
        #[cfg(not(loom))]
        let head = unsafe { self.receiver.head.as_ptr().read() };
        #[cfg(loom)]
        let head = unsafe { self.receiver.head.unsync_load() };

        if head == self.receiver.tail_cache.get() {
            self.receiver.tail_cache.set(self.sender.tail.load(Acquire));
            if head == self.receiver.tail_cache.get() {
                // Let the receiver consume all the messages after sender disconnects.
                if self.shared.drop_count.load(Acquire) != 0 {
                    self.receiver.tail_cache.set(self.sender.tail.load(Relaxed));
                    if head == self.receiver.tail_cache.get() {
                        return Err(Disconnected);
                    }
                }
                self.wake_sender();
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
            /*SAFETY:
             *everything before tail has been written to by the sender.
             */
            slot.with_mut(|ptr| (ptr as *mut T).read())
        };

        self.receiver.head.store(head.wrapping_add(1), Release);
        self.wake_sender();
        Ok(item)
    }

    fn peer_connected(&self) -> bool {
        self.shared.drop_count.load(Acquire) == 0
    }

    #[inline]
    fn wake_receiver(&self) {
        //TODO: implement sleeping/waking
    }

    #[inline]
    fn wake_sender(&self) {
        //TODO: implement sleeping/waking
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        //head points to the first not read element
        //tail points after the last written element
        /*SAFETY:
         *this object is being destroyed so we
         *have exclusive access to these atomics.
         */
        #[cfg(not(loom))]
        let (mut head, tail) = unsafe {
            (
                self.receiver.head.as_ptr().read(),
                self.sender.tail.as_ptr().read(),
            )
        };
        #[cfg(loom)]
        let (mut head, tail) = unsafe {
            (
                self.receiver.head.unsync_load(),
                self.sender.tail.unsync_load(),
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
            unsafe { slot.with_mut(|ptr| std::ptr::drop_in_place(ptr)) };
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
impl<T> error::Error for SendError<T> {}
impl error::Error for RecvError {}

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

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("writing to a disconnected queue")
    }
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("reading from a disconnected queue")
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

impl<T> fmt::Debug for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendError(..)")
    }
}

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
