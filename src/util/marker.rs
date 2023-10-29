use std::marker::PhantomData;

pub(crate) type PhantomUnsync = PhantomData<std::cell::Cell<()>>;
#[allow(dead_code)]
pub(crate) type PhantomUnsend = PhantomData<std::sync::MutexGuard<'static, ()>>;
