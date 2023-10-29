#![allow(unused_imports)]
cfg_loom! {
    pub(crate) use loom::sync::*;
    pub(crate) use std::sync::TryLockError;
}

cfg_not_loom! {
    pub(crate) use std::sync::*;

}
