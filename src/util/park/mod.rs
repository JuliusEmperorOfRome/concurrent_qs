#[cfg(all(loom, not(park)))]
mod loom;
#[cfg(all(loom, not(park)))]
pub(crate) use loom::Parker;

#[cfg(any(not(loom), park))]
mod real;
#[cfg(any(not(loom), park))]
pub(crate) use real::Parker;

#[cfg(test)]
mod tests;
