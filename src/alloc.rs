cfg_loom! {
    pub(crate) use loom::alloc::*;
}

cfg_not_loom! {
    pub(crate) use std::alloc::*;
}
