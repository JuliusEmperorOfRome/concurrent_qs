use crate::util::marker::PhantomUnsync;

use std::{fmt::Debug, ops::Deref};

mod inner;

pub use crate::error::{RecvError, SendError, TryRecvError};

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (h1, h2) = inner::Inner::<T>::allocate();
    (Sender(h1, PhantomUnsync {}), Receiver(h2, PhantomUnsync {}))
}

pub struct Sender<T>(inner::InnerHolder<T>, PhantomUnsync);
pub struct Receiver<T>(inner::InnerHolder<T>, PhantomUnsync);

impl<T> Sender<T> {
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item)
    }
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, RecvError> {
        self.0.recv()
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }
}

unsafe impl<T: Send> Send for Sender<T> {}
unsafe impl<T: Send> Send for Receiver<T> {}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        use crate::sync::atomic::Ordering::AcqRel;
        self.0.drop_count.fetch_add(1, AcqRel);
        self.0.unpark_receiver();
        // InnerHolder does the rest
    }
}

impl<T> Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::unbounded::Sender<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.0.deref() as *const _
        )
    }
}

impl<T> Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "spsc::unbounded::Receiver<{}> {{ channel: {:p} }}",
            std::any::type_name::<T>(),
            self.0.deref() as *const _
        )
    }
}

#[cfg(test)]
mod tests;
