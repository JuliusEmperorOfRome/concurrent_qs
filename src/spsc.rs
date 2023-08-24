#![warn(missing_docs)]

use core::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::Arc;

/// A single producer single consumer bounded FIFO queue.
///
/// ***IMPORTANT***: N must be a power of two.
pub struct Queue<T: Send, const N: usize>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    reader: ReaderData,
    writer: WriterData,
    elems: [UnsafeCell<MaybeUninit<T>>; N],
}

/// Producer handle. May only be used from one thread.
pub struct Producer<T: Send, const N: usize>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    q: Arc<Queue<T, N>>,
}

/// Consumer handle. May only be used from one thread.
pub struct Consumer<T: Send, const N: usize>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    q: Arc<Queue<T, N>>,
}

/// Producer handle that holds a simple reference to a queue with no reference counting. May only be used from one thread.
pub struct RefProducer<'q, T: Send, const N: usize>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    q: &'q Queue<T, N>,
}

/// Consumer handle that holds a simple reference to a queue with no reference counting. May only be used from one thread.
pub struct RefConsumer<'q, T: Send, const N: usize>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    q: &'q Queue<T, N>,
}

impl<T: Send, const N: usize> Producer<T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    /// Pushes `item` onto the queue or
    /// returns [`Err`] with `item` inside if the queue is full.
    pub fn push(&self, item: T) -> Result<(), T> {
        self.q.push(item)
    }
}

impl<T: Send, const N: usize> Consumer<T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    /// Pops an item from the queue or returns [`None`] if the queue is empty.
    pub fn pop(&self) -> Option<T> {
        self.q.pop()
    }
}

impl<'q, T: Send, const N: usize> RefProducer<'q, T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    /// Pushes `item` onto the queue or
    /// returns [`Err`] with `item` inside if the queue is full.
    pub fn push(&self, item: T) -> Result<(), T> {
        self.q.push(item)
    }
}

impl<'q, T: Send, const N: usize> RefConsumer<'q, T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    /// Pops an item from the queue or returns [`None`] if the queue is empty
    pub fn pop(&self) -> Option<T> {
        self.q.pop()
    }
}

impl<T: Send, const N: usize> Queue<T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
    const INIT: UnsafeCell<MaybeUninit<T>> = UnsafeCell::new(MaybeUninit::uninit());

    /// Creates an empty [`Queue`].
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

    /// Creates an empty [`Queue`] and splits it into [`Producer`] and [`Consumer`]
    pub fn new_split() -> (Producer<T, N>, Consumer<T, N>) {
        #[cfg(not(feature = "no_new_uninit"))]
        let q = unsafe {
            use std::ptr::addr_of_mut;

            let q_uninit = Arc::<Queue<T, N>>::new_uninit();
            let ptr = q_uninit.as_ptr() as *mut Queue<T, N>;

            addr_of_mut!((*ptr).reader).write(ReaderData {
                head: AtomicUsize::default(),
                tail_cache: Cell::default(),
            });

            addr_of_mut!((*ptr).writer).write(WriterData {
                tail: AtomicUsize::default(),
                head_cache: Cell::default(),
            });

            /* there is no need to init q.elems since its a slice of MaybeUninits
             */

            q_uninit.assume_init()
        };
        #[cfg(feature = "no_new_uninit")]
        let q = Arc::new(Self::new());
        (Producer { q: q.clone() }, Consumer { q: q })
    }

    /// Splits the queue into [`RefProducer`] and [`RefConsumer`] endpoints.
    ///
    /// # Safety
    ///
    /// `ref_split` doesn't consume the queue, which
    /// allows obtaining multiple producers/consumers,
    /// but there can only be one thread that owns producers
    /// and one that owns consumers.
    pub unsafe fn ref_split(&mut self) -> (RefProducer<'_, T, N>, RefConsumer<'_, T, N>) {
        (RefProducer { q: self }, RefConsumer { q: self })
    }

    /// Splits the queue into [`Producer`] and [`Consumer`] endpoints.
    ///
    /// It's never better than [`new_split`] and oftentimes worse.
    #[deprecated(
        since = "0.1.1",
        note = "There is no reason to use this method over Queue::new_split()"
    )]
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

    fn push(&self, item: T) -> Result<(), T> {
        /*safety:
            Only push mutates tail, and since this
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

    fn pop(&self) -> Option<T> {
        /*safety:
            Only pop mutates tail, and since this
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

unsafe impl<T: Send, const N: usize> Send for Queue<T, N> where
    [(); (N.count_ones() == 1) as usize - 1]:
{
}
unsafe impl<T: Send, const N: usize> Send for Producer<T, N> where
    [(); (N.count_ones() == 1) as usize - 1]:
{
}
unsafe impl<T: Send, const N: usize> Send for Consumer<T, N> where
    [(); (N.count_ones() == 1) as usize - 1]:
{
}
unsafe impl<'q, T: Send, const N: usize> Send for RefProducer<'q, T, N> where
    [(); (N.count_ones() == 1) as usize - 1]:
{
}
unsafe impl<'q, T: Send, const N: usize> Send for RefConsumer<'q, T, N> where
    [(); (N.count_ones() == 1) as usize - 1]:
{
}

impl<T: Send, const N: usize> Drop for Queue<T, N>
where
    [(); (N.count_ones() == 1) as usize - 1]:,
{
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

#[cfg(test)]
mod tests {
    use super::Queue;
    use std::thread;

    #[test]
    fn mt_ref_test() {
        static mut Q: Queue<i32, 4> = Queue::new();
        let (src, sink) = unsafe { Q.ref_split() };

        let thread = thread::spawn(move || {
            for i in 0..256 {
                loop {
                    if let Some(item) = sink.pop() {
                        assert_eq!(item, i);
                        break;
                    }
                    thread::yield_now();
                }
            }
        });

        for i in 0..256 {
            while let Err(_) = src.push(i) {
                thread::yield_now();
            }
        }

        thread.join().expect("Failed to join thread.");
    }

    #[test]
    fn mt_arc_test() {
        let (src, sink) = Queue::<i32, 4>::new_split();

        let thread = thread::spawn(move || {
            for i in 0..256 {
                loop {
                    if let Some(item) = sink.pop() {
                        assert_eq!(item, i);
                        break;
                    }
                    std::thread::yield_now();
                }
            }
        });

        for i in 0..256 {
            while let Err(_) = src.push(i) {
                std::thread::yield_now();
            }
        }

        thread.join().expect("Failed to join thread.");
    }

    #[test]
    fn st_ref_test() {
        let mut q = Queue::<i32, 4>::new();
        let (src, sink) = unsafe { q.ref_split() };

        assert_eq!(sink.pop(), None);

        assert_eq!(src.push(1), Ok(()));
        assert_eq!(src.push(2), Ok(()));
        assert_eq!(src.push(3), Ok(()));
        assert_eq!(src.push(4), Ok(()));
        assert_eq!(src.push(5), Err(5));

        assert_eq!(sink.pop(), Some(1));
        assert_eq!(sink.pop(), Some(2));
        assert_eq!(sink.pop(), Some(3));
        assert_eq!(sink.pop(), Some(4));
        assert_eq!(sink.pop(), None);
    }

    #[test]
    fn st_arc_test() {
        let (src, sink) = Queue::<i32, 4>::new_split();

        assert_eq!(sink.pop(), None);

        assert_eq!(src.push(1), Ok(()));
        assert_eq!(src.push(2), Ok(()));
        assert_eq!(src.push(3), Ok(()));
        assert_eq!(src.push(4), Ok(()));
        assert_eq!(src.push(5), Err(5));

        assert_eq!(sink.pop(), Some(1));
        assert_eq!(sink.pop(), Some(2));
        assert_eq!(sink.pop(), Some(3));
        assert_eq!(sink.pop(), Some(4));
        assert_eq!(sink.pop(), None);
    }
}
