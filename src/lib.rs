#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
// loom integration
#[doc(hidden)]
macro_rules! cfg_loom {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "loom")]
            $item
        )*
    };
}
#[doc(hidden)]
macro_rules! cfg_not_loom {
    ($($item:item)*) => {
        $(
            #[cfg(not(feature = "loom"))]
            $item
        )*
    };
}

#[doc(hidden)]
mod alloc;
#[doc(hidden)]
mod cell;
#[doc(hidden)]
mod sync;
#[doc(hidden)]
mod thread;
//loom integration finished.

/// A module containing the error types used by the library.
pub mod error;

/// A module containing flavors of Single Producer Single Consumer queues.
pub mod spsc;

mod util;
