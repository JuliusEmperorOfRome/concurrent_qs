use loom::sync::Notify;

/// loom mock implementation of [`Parker`](crate::util::park::real::Parker)
pub(crate) struct Parker(Notify);

impl Parker {
    pub(crate) fn new() -> Self {
        Self(Notify::new())
    }

    pub(crate) unsafe fn park(&self) {
        self.0.wait();
    }

    pub(crate) fn unpark(&self) {
        self.0.notify();
    }
}
