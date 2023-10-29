use std::error::Error;
use std::fmt;

/// An enumeration listing the failure modes of the [`try_send`](super::Sender::try_send) method.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum TrySendError<T> {
    /// The data couldn't be sent on the [`channel`](super::channel)
    /// because the internal buffer was already full.
    Full(T),
    /// The [`Receiver`](super::Receiver) bound to the [`channel`](super::channel)
    /// disconnected and any further sends will not succeed.
    Disconnected(T),
}

/// An enumeration listing the failure modes of the [`try_recv`](super::Receiver::try_recv) method.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    /// No data was received from the [`channel`](super::channel)
    /// because the internal buffer was empty.
    Empty,
    /// The [`Sender`](super::Sender) bound to the [`channel`](super::channel)
    /// disconnected and all previously sent data was already received.
    Disconnected,
}

/// Error for the [`send`](super::Sender::send) method.
///
/// This error is returned when the [`Receiver`](super::Receiver) bound
/// to the [`channel`](super::channel) has disconnected. The `T` inside
/// is the item that failed to send.
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct SendError<T>(pub T);

/// Error for the [`recv`](super::Receiver::recv) method.
///
/// This error is returned when the [`Sender`](super::Sender) bound
/// to the [`channel`](super::channel) has disconnected.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct RecvError {}

impl<T> Error for TrySendError<T> {}
impl Error for TryRecvError {}
impl<T> Error for SendError<T> {}
impl Error for RecvError {}

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
