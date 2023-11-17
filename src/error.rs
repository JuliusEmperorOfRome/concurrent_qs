use std::error::Error;
use std::fmt;

/// An enumeration listing the failure modes of `try_send` method of a `bounded::Sender`.
///
/// The available `bounded::Sender`s are
/// - [spsc::bounded::Sender](crate::spsc::bounded::Sender)
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum TrySendError<T> {
    /// The data couldn't be sent on the `bounded::channel`
    /// because it was already full.
    ///
    /// Contains the data that failed to send.
    Full(T),
    /// The `bounded::Receiver` connected to the `bounded::channel`
    /// disconnected and any further sends will not succeed.
    ///
    /// Contains the data that failed to send.
    Disconnected(T),
}

/// An enumeration listing the failure modes of the `try_recv` method of a `Receiver`.
///
/// The available `Receiver`s are:
/// - [spsc::bounded::Receiver](crate::spsc::bounded::Receiver)
/// - [spsc::unbounded::Receiver](crate::spsc::unbounded::Receiver)
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    /// No data was received from the `channel` because it was empty.
    Empty,
    /// The `Sender` bound to the `channel` disconnected
    /// and all previously sent data was already received.
    Disconnected,
}

/// Error for the `send` method of a `Sender`.
///
/// This error is returned when the `Receiver` connected
/// to the `channel` has disconnected. Contains the data
/// that failed to send.
///
/// The available `Sender`s are:
/// - [spsc::bounded::Sender](crate::spsc::bounded::Sender)
/// - [spsc::unbounded::Sender](crate::spsc::unbounded::Sender)
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct SendError<T>(pub T);

/// Error for the `recv` method of a `Receiver`.
///
/// This error is returned when the `Sender` connected to
/// the `channel` has disconnected.
///
/// The available `Receiver`s are:
/// - [spsc::bounded::Receiver](crate::spsc::bounded::Receiver)
/// - [spsc::unbounded::Receiver](crate::spsc::unbounded::Receiver)
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
