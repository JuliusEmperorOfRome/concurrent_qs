// loom integration
#[doc(hidden)]
macro_rules! cfg_loom {
    ($($item:item)*) => {
        $(
            #[cfg(loom)]
            $item
        )*
    };
}
#[doc(hidden)]
macro_rules! cfg_not_loom {
    ($($item:item)*) => {
        $(
            #[cfg(not(loom))]
            $item
        )*
    };
}
#[doc(hidden)]
mod cell;
#[doc(hidden)]
mod sync;
#[doc(hidden)]
mod thread;
//loom is now intergrated

/// A module containing flavors of Single Producer Single Consumer queues.
#[deny(missing_docs)]
pub mod spsc;

mod util;
