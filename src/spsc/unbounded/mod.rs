mod inner;
pub use crate::error::{RecvError, SendError, TryRecvError};

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (h1, h2) = inner::Inner::<T>::allocate();
    (Sender(h1), Receiver(h2))
}

pub struct Sender<T>(inner::InnerHolder<T>);
pub struct Receiver<T>(inner::InnerHolder<T>);

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

#[cfg(test)]
mod tests;
