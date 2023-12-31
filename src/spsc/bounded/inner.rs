use crate::alloc::Layout;
use crate::cell::UnsafeCell;
use crate::error::{RecvError, SendError, TryRecvError, TrySendError};
use crate::sync::atomic::AtomicUsize;
use crate::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use crate::util::cache::CacheAligned;
use crate::util::park::Parker;
use std::cell::Cell; //There's only a Sender exclusive cell and a Receiver exclusive cell.
use std::default::Default;
use std::mem::MaybeUninit;

#[repr(C)]
pub(crate) struct Inner<T> {
    sender: CacheAligned<SenderData>,
    receiver: CacheAligned<ReceiverData>,
    pub(super) shared: SharedData<T>,
}

impl<T> Inner<T> {
    pub(super) const LAYOUT: Layout = Layout::new::<Inner<T>>();

    pub(super) fn new(capacity: usize) -> Self {
        // should already be ensured in channel()
        debug_assert!(capacity.is_power_of_two(), "capacity wasn't a power of two");
        #[cfg(not(feature = "loom"))]
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
        #[cfg(feature = "loom")]
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

    pub(super) fn send(&self, item: T) -> Result<(), SendError<T>> {
        let mut resend = match self.try_send(item) {
            Ok(_) => return Ok(()),
            Err(TrySendError::Disconnected(ret)) => return Err(SendError(ret)),
            Err(TrySendError::Full(ret)) => ret,
        };
        loop {
            //SAFETY: park can't be called by different threads, since Sender is !Sync.
            unsafe {
                self.receiver.send_park.park();
            }

            match self.try_send(resend) {
                Ok(_) => break Ok(()),
                Err(TrySendError::Disconnected(ret)) => break Err(SendError(ret)),
                Err(TrySendError::Full(ret)) => resend = ret,
            }
        }
    }

    pub(super) fn recv(&self) -> Result<T, RecvError> {
        match self.try_recv() {
            Ok(ret) => return Ok(ret),
            Err(TryRecvError::Disconnected) => return Err(RecvError {}),
            Err(TryRecvError::Empty) => {}
        };
        loop {
            //SAFETY: park can't be called by different threads, since Receiver is !Sync.
            unsafe {
                self.sender.recv_park.park();
            }

            match self.try_recv() {
                Ok(ret) => return Ok(ret),
                Err(TryRecvError::Disconnected) => return Err(RecvError {}),
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    pub(super) fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        if self.shared.drop_count.load(Relaxed) != 0 {
            return Err(TrySendError::Disconnected(item));
        }

        /*SAFETY:
         *tail is only modified by try_send and this is
         *an SPSC, so no other thread is modifying it.
         */
        #[cfg(not(feature = "loom"))]
        let tail = unsafe { self.sender.tail.as_ptr().read() };
        #[cfg(feature = "loom")]
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

    pub(super) fn try_recv(&self) -> Result<T, TryRecvError> {
        use TryRecvError::*;
        /*SAFETY:
         *head is only modified by try_recv and this is
         *an SPSC, so no other thread is modifying it.
         */
        #[cfg(not(feature = "loom"))]
        let head = unsafe { self.receiver.head.as_ptr().read() };
        #[cfg(feature = "loom")]
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

    pub(super) fn peer_connected(&self) -> bool {
        self.shared.drop_count.load(Acquire) == 0
    }

    #[inline]
    pub(super) fn wake_receiver(&self) {
        self.sender.recv_park.unpark();
    }

    #[inline]
    pub(super) fn wake_sender(&self) {
        self.receiver.send_park.unpark();
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
        #[cfg(not(feature = "loom"))]
        let (mut head, tail) = unsafe {
            (
                self.receiver.head.as_ptr().read(),
                self.sender.tail.as_ptr().read(),
            )
        };
        #[cfg(feature = "loom")]
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
    recv_park: Parker,
}

struct ReceiverData {
    head: AtomicUsize,
    tail_cache: Cell<usize>,
    send_park: Parker,
}

pub(super) struct SharedData<T> {
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    /*
    starts off as 0, incremented when entering Sender/Receiver drop.
    match 'previous value' {
        0 => {
            Now the channel is disconnected. We try to wake the other end point.
            If the other end point was asleep, it will detect the disconnect and unblock.
            Then, we increment 'drop_count' again and repeat this decision tree with the
            new 'previous value'.
            This is done so that the inner state can't be deallocated while we're waking
            the other thread, but the disconnect has to be discoverable.
        }
        1 => just fall off drop.
        2 => deallocate the inner state.
    }
    */
    pub(super) drop_count: AtomicUsize,
}

impl Default for SenderData {
    #[inline(always)]
    fn default() -> Self {
        Self {
            tail: AtomicUsize::default(),
            head_cache: Cell::default(),
            recv_park: Parker::new(),
        }
    }
}

impl Default for ReceiverData {
    #[inline(always)]
    fn default() -> Self {
        Self {
            head: AtomicUsize::default(),
            tail_cache: Cell::default(),
            send_park: Parker::new(),
        }
    }
}
