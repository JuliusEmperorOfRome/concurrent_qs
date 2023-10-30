#[cfg(feature = "hl-loom")]
mod loom;
#[cfg(feature = "hl-loom")]
pub(crate) use loom::Parker;

#[cfg(not(feature = "hl-loom"))]
mod real;
#[cfg(not(feature = "hl-loom"))]
#[allow(unused_imports)]
pub(crate) use real::Parker;

#[cfg(test)]
mod tests;
