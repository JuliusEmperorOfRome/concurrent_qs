#![allow(unused_imports)]
cfg_loom! {
    pub(crate) use loom::thread::*;
}

cfg_not_loom! {
    pub(crate) use std::thread::*;
}
