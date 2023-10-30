cfg_loom! {
    pub(crate) use loom::sync::*;
}

cfg_not_loom! {
    pub(crate) use std::sync::*;
}
