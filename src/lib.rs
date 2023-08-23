#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

/// A bounded lock-free SPSC(single producer, single consumer) FIFO queue.
///
/// # EXAMPLES
///
/// Can be used completely on the stack:
/// ```
/// use concurrent_qs::spsc::Queue;
///
/// let mut q = Queue::<i32, 2>::new();
/// // safety: only one thread has a producer and only
/// //         one thread has a consumer -> we're safe.
/// let (source, sink) = unsafe { q.ref_split() };
///
/// // queue has space
/// assert_eq!(source.push(1), Ok(()));
/// assert_eq!(source.push(2), Ok(()));
/// // queue is full
/// assert_eq!(source.push(3), Err(3));
///
/// // queue has items
/// assert_eq!(sink.pop(), Some(1));
/// assert_eq!(sink.pop(), Some(2));
/// // no more items in queue
/// assert_eq!(sink.pop(), None);
/// ```
///
/// Can also be used in a safe manner:
/// ```
/// use concurrent_qs::spsc::Queue;
/// use std::thread;
///
/// let (source, sink) = Queue::<i32, 8>::new().split();
///
/// let join = thread::spawn(move || {
///     {
///         // for T: Copy
///         let to_push = 42;
///         while let Err(_) = source.push(to_push) {
///             // optional, but recommended
///             thread::yield_now();
///         }
///     }
///     {
///         // for any T
///         let mut to_push = source.push(42);
///         while let Err(item) = to_push {
///             // once again, optional yield
///             thread::yield_now();
///             to_push = source.push(item);
///         }
///     }
/// });
/// ```
pub mod spsc;

#[cfg(test)]
mod tests {
    use super::spsc::Queue;
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
        let (src, sink) = Queue::<i32, 4>::new().split();

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
        let (src, sink) = Queue::<i32, 4>::new().split();

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
