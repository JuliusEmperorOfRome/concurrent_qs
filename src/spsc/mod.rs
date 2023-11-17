/// A bounded lock-free Single Producer Single Consumer queue.
/// Enabled by the `spsc-bounded` feature.
///
/// A fixed capacity queue for sending from a producer thread to a
/// consumer thread. Only one thread may send and only one may receive
/// at any given time. It is lock-free as long as only [`try_send`](bounded::Sender::try_send)
/// and [`try_recv`](bounded::Receiver::try_recv) are used.
///
/// # Examples
///
/// ```rust
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
#[cfg(any(doc, feature = "spsc-bounded"))]
pub mod bounded;

/// An unbounded lock-free Single Producer Single Consumer queue.
/// Enabled by the `spsc-unbounded` feature.
///
/// An unbounded queue for sending from a producer thread to a
/// consumer thread. Only one thread may send and only one may
/// receive at any given time.
///
/// # Examples
///
/// ```rust
/// use concurrent_qs::spsc::unbounded;
/// use std::thread;
///
/// fn main() {
///     let (src, sink) = unbounded::channel::<&'static str>();
///
///     thread::spawn(move || {
///         src.send("One").unwrap();
///         src.send("Two").unwrap();
///         src.send("Three").unwrap();
///     });
///     let mut str = String::new();
///     while let Ok(s) = sink.recv() {
///         str.push_str(s);
///     }
///
///     assert_eq!(str, "OneTwoThree");
/// }
/// ```
#[cfg(any(doc, feature = "spsc-unbounded"))]
pub mod unbounded;
