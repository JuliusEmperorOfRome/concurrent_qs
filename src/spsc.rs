use core::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::Arc;

pub struct Queue<T: Send, const N: usize> {
    reader: ReaderData,
    writer: WriterData,
    elems: [UnsafeCell<MaybeUninit<T>>; N],
}

pub struct Producer<T: Send, const N: usize> {
    q: Arc<Queue<T, N>>,
}

pub struct Consumer<T: Send, const N: usize> {
    q: Arc<Queue<T, N>>,
}
pub struct RefProducer<'q, T: Send, const N: usize> {
    q: &'q Queue<T, N>,
}
pub struct RefConsumer<'q, T: Send, const N: usize> {
    q: &'q Queue<T, N>,
}

impl<T: Send, const N: usize> Producer<T, N> {
    pub fn push(&self, item: T) -> Result<(), T> {
        self.q.push_inner(item)
    }
}

impl<T: Send, const N: usize> Consumer<T, N> {
    pub fn pop(&self) -> Option<T> {
        self.q.pop_inner()
    }
}

impl<'q, T: Send, const N: usize> RefProducer<'q, T, N> {
    pub fn push(&self, item: T) -> Result<(), T> {
        self.q.push_inner(item)
    }
}

impl<'q, T: Send, const N: usize> RefConsumer<'q, T, N> {
    pub fn pop(&self) -> Option<T> {
        self.q.pop_inner()
    }
}

impl<T: Send, const N: usize> Queue<T, N> {
    const _N_IS_POW2_CHECK: () = assert!(N.count_ones() == 1);
    const INIT: UnsafeCell<MaybeUninit<T>> = UnsafeCell::new(MaybeUninit::uninit());

    pub const fn new() -> Self {
        Self {
            reader: ReaderData {
                head: AtomicUsize::new(0),
                tail_cache: Cell::new(0),
            },
            writer: WriterData {
                tail: AtomicUsize::new(0),
                head_cache: Cell::new(0),
            },
            elems: [Self::INIT; N],
        }
    }

    pub fn push(&mut self, item: T) -> Result<(), T> {
        self.push_inner(item)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.pop_inner()
    }

    pub fn ref_split(&mut self) -> (RefProducer<'_, T, N>, RefConsumer<'_, T, N>) {
        (RefProducer { q: self }, RefConsumer { q: self })
    }

    pub fn split(self) -> (Producer<T, N>, Consumer<T, N>) {
        let arc_self = Arc::new(self);
        let _ = &arc_self;
        (
            Producer {
                q: arc_self.clone(),
            },
            Consumer { q: arc_self },
        )
    }

    fn push_inner(&self, item: T) -> Result<(), T> {
        /*safety:
            Only push_inner mutates tail, and since this
            is an SPSC, only one thread is allowed to call it.
        */
        let tail = unsafe { self.writer.tail.as_ptr().read() };

        /*
        since N is a power of two (checked at compile time) and indices are always
        used mod N, doing arithmetic mod *higher power of two* is perfectly fine
        */
        if tail == self.writer.head_cache.get().wrapping_add(N) {
            self.writer.head_cache.set(self.reader.head.load(Acquire));
            if tail == self.writer.head_cache.get().wrapping_add(N) {
                return Err(item);
            }
        }
        /* safety:
            get_unchecked(tail % N): elems.len() == N
            get().write(...):
                it was either uninit from construction or
                moved from by reader, checked by the if blocks above
        */
        unsafe {
            self.elems
                .get_unchecked(tail % N)
                .get()
                .write(MaybeUninit::new(item))
        };
        self.writer.tail.store(tail.wrapping_add(1), Release);
        Ok(())
    }

    fn pop_inner(&self) -> Option<T> {
        /*safety:
            Only pop_inner mutates tail, and since this
            is an SPSC, only one thread is allowed to call it.
        */
        let head = unsafe { self.reader.head.as_ptr().read() };

        if head == self.reader.tail_cache.get() {
            self.reader.tail_cache.set(self.writer.tail.load(Acquire));
            if head == self.reader.tail_cache.get() {
                return None;
            }
        }
        /* safety:
            get_unchecked(head % N): elems.len() == N
            get().read().assume_init():
                the if blocks above check that the
                writer has init'ed the data
        */
        let item = unsafe {
            self.elems
                .get_unchecked(head % N)
                .get()
                .read()
                .assume_init()
        };
        /*
        since N is a power of two (checked at compile time) and indices are always
        used mod N, doing arithmetic mod *higher power of two* is perfectly fine
        */
        self.reader.head.store(head.wrapping_add(1), Release);
        Some(item)
    }
}

unsafe impl<T: Send, const N: usize> Send for Queue<T, N> {}
unsafe impl<T: Send, const N: usize> Send for Producer<T, N> {}
unsafe impl<T: Send, const N: usize> Send for Consumer<T, N> {}
unsafe impl<'q, T: Send, const N: usize> Send for RefProducer<'q, T, N> {}
unsafe impl<'q, T: Send, const N: usize> Send for RefConsumer<'q, T, N> {}

impl<T: Send, const N: usize> Drop for Queue<T, N> {
    fn drop(&mut self) {
        /*safety:
            We're being dropped, so if head/tail are being written
            to concurrently, it's use after drop or the queue is
            being dropped multiple times.
            Therefore these reads don't need atomicity at all.
        */
        let (mut head, tail) = unsafe {
            (
                self.reader.head.as_ptr().read(),
                self.writer.tail.as_ptr().read(),
            )
        };
        while head != tail {
            /*safety:
                get_unchecked_mut(head % N): elems.len() == N
                get_mut().as_mut_ptr().drop_in_place():
                    all initialised elements are between head and tail
            */
            unsafe {
                self.elems
                    .get_unchecked_mut(head % N)
                    .get_mut()
                    .as_mut_ptr()
                    .drop_in_place()
            }
            /*
            since N is a power of two (checked at compile time) and indices are always
            used mod N, doing arithmetic mod *higher power of two* is perfectly fine
            */
            head = head.wrapping_add(1);
        }
    }
}

#[repr(align(64))]
struct ReaderData {
    head: AtomicUsize,
    tail_cache: Cell<usize>,
}

#[repr(align(64))]
struct WriterData {
    tail: AtomicUsize,
    head_cache: Cell<usize>,
}
