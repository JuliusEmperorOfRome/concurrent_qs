/// A bounded lock-free Single Producer Single Consumer queue.
///
/// A fixed capacity queue for sending from a producer thread to a consumer thread.
/// Only one thread may send and only one may receive at any given time.
///
/// # Example
///
/// ```
/// use concurrent_qs::spsc::bounded::{self, TryRecvError};
/// use std::thread;
/// fn main() {
///     let (src, sink) = bounded::channel::<&'static str>(8);
///
///     thread::spawn(move || {
///         src.try_send("H").unwrap();
///         src.try_send("E").unwrap();
///         src.try_send("L").unwrap();
///         src.try_send("L").unwrap();
///         src.try_send("O").unwrap();
///         drop(src);
///     });
///     let mut str = String::new();
///     loop {
///         match sink.try_recv() {
///             Ok(s) => str.push_str(s),
///             Err(TryRecvError::Empty) => {/*sophisticated back-off policy*/},
///             _ => break,
///         }
///     }
///
///     assert_eq!(str, "HELLO");
/// }
/// ```
///
/// # Cross-platform notes
///
/// This implementation depends only on pointer-sized atomics.
#[cfg(target_has_atomic = "ptr")]
pub mod bounded;
