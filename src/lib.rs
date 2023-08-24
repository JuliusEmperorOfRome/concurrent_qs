#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![cfg_attr(not(no_new_uninit), feature(new_uninit))]
/// A bounded lock-free SPSC(single producer, single consumer) FIFO queue.
///
/// # EXAMPLES
///
/// Can be used the simple and safe way:
/// ```
/// use concurrent_qs::spsc::Queue;
/// use std::thread;
///
/// let (source, sink) = Queue::<i32, 8>::new_split();
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
///
/// Can also be used completely on the stack:
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
pub mod spsc;
