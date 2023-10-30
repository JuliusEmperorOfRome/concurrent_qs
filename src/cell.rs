cfg_loom! {
    pub(crate) use loom::cell::UnsafeCell;
}

cfg_not_loom! {
    pub(crate) struct UnsafeCell<T>(std::cell::UnsafeCell<T>);
    #[allow(dead_code)]
    impl<T> UnsafeCell<T> {
        pub(crate) const fn new(data: T) -> Self {
            Self(std::cell::UnsafeCell::new(data))
        }

        #[inline(always)]
        pub(crate) fn with<R>(&self, f: impl FnOnce(*const T) -> R) -> R {
            f(self.0.get())
        }

        #[inline(always)]
        pub(crate) fn with_mut<R>(&self, f: impl FnOnce(*mut T) -> R) -> R{
            f(self.0.get())
        }
    }
}
