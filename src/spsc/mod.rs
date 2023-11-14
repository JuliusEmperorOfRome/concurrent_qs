/// A bounded lock-free Single Producer Single Consumer queue.
///
/// A fixed capacity queue for sending from a producer thread to a
/// consumer thread. Only one thread may send and only one may receive
/// at any given time. It is lock-free as long as only [`try_send`](bounded::Sender::try_send)
/// and [`try_recv`](bounded::Receiver::try_recv) are used.
///
/// # Examples
/// ```
/// use concurrent_qs::spsc::bounded;
/// use std::thread;
///
/// fn main() {
///     let (src, sink) = bounded::channel::<&'static str>(4);
///
///     thread::spawn(move || {
///         src.send("H").unwrap();
///         src.send("E").unwrap();
///         src.send("L").unwrap();
///         src.send("L").unwrap();
///         src.send("O").unwrap();
///     });
///
///     let mut str = String::new();
///     while let Ok(s) = sink.recv() {
///         str.push_str(s);
///     }
///
///     assert_eq!(str, "HELLO");
/// }
/// ```
///
/// This can also be done without blocking.
///
/// ```
/// use concurrent_qs::error::TryRecvError;
/// use concurrent_qs::spsc::bounded;
/// use std::thread;
///
/// fn main() {
///     let (src, sink) = bounded::channel::<&'static str>(8);
///
///     thread::spawn(move || {
///         // In this example the queue never fills up, and therefore try_send
///         // never fails. Normally, you would want to check the result.
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
pub mod bounded;

#[cfg(feature = "unstable")]
#[allow(missing_docs)]
pub mod unbounded;
