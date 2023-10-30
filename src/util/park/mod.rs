#[cfg(all(feature = "hl-loom", not(feature = "full-loom")))]
mod loom;
#[cfg(all(feature = "hl-loom", not(feature = "full-loom")))]
pub(crate) use loom::Parker;

#[cfg(any(not(feature = "hl-loom"), feature = "full-loom"))]
mod real;
#[cfg(any(not(feature = "hl-loom"), feature = "full-loom"))]
pub(crate) use real::Parker;

#[cfg(test)]
mod tests;
